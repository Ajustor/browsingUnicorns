//! User scripts on the desktop: installs in the background, and their part
//! of the "Extensions" view.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use application::UserScriptManager;
use application::ports::{ExtensionError, WebEngine};
use domain::{UserScript, UserScriptId, UserScripts};
use egui::{Button, RichText};
use egui_phosphor::regular as icon;

use super::command::Command;
use super::panel_kit::{card, field, primary_button};

type Installed = Result<UserScript, ExtensionError>;

pub struct UserScriptsRunner {
    manager: UserScriptManager,
    installing: bool,
    /// The last install's outcome, shown under the field.
    pub status: Option<Result<String, String>>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    tx: Sender<Installed>,
    rx: Receiver<Installed>,
}

impl UserScriptsRunner {
    pub fn new(manager: UserScriptManager, repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            manager,
            installing: false,
            status: None,
            repaint: Arc::new(repaint),
            tx,
            rx,
        }
    }

    pub fn scripts(&self) -> &UserScripts {
        self.manager.scripts()
    }

    /// Downloads in the background; [`Self::poll`] finishes.
    pub fn install(&mut self, url: &str) {
        self.installing = true;
        self.status = None;
        let (packages, tx, repaint, url) = (
            self.manager.packages(),
            self.tx.clone(),
            self.repaint.clone(),
            url.trim().to_owned(),
        );
        std::thread::spawn(move || {
            let _ = tx.send(packages.install(&url));
            repaint();
        });
    }

    pub fn poll(&mut self, engine: &mut dyn WebEngine) {
        let Ok(result) = self.rx.try_recv() else {
            return;
        };
        self.installing = false;
        self.status = Some(match result {
            Ok(script) => {
                let name = i18n::user_scripts::installed(&script.meta.name, &script.meta.version);
                self.manager
                    .installed(script, engine)
                    .map(|()| name)
                    .map_err(|e| e.to_string())
            }
            Err(e) => Err(e.to_string()),
        });
    }

    pub fn set_enabled(
        &mut self,
        id: &UserScriptId,
        enabled: bool,
        engine: &mut dyn WebEngine,
    ) -> Result<(), String> {
        self.manager
            .set_enabled(id, enabled, engine)
            .map_err(|e| e.to_string())
    }

    pub fn remove(&mut self, id: &UserScriptId, engine: &mut dyn WebEngine) -> Result<(), String> {
        self.manager.remove(id, engine).map_err(|e| e.to_string())
    }
}

/// The user scripts' part of the Extensions view.
pub fn show(
    ui: &mut egui::Ui,
    runner: &UserScriptsRunner,
    input: &mut String,
    commands: &mut Vec<Command>,
) {
    card(
        ui,
        icon::FILE_CODE,
        i18n::user_scripts::heading(),
        Some(i18n::user_scripts::input_label()),
        |ui| {
            let field = ui.add(field(
                input,
                "https://greasyfork.org/scripts/…/code/….user.js",
            ));
            let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal_wrapped(|ui| {
                let can = !input.trim().is_empty() && !runner.installing;
                if primary_button(
                    ui,
                    format!(
                        "{} {}",
                        icon::DOWNLOAD_SIMPLE,
                        i18n::user_scripts::install()
                    ),
                    can,
                )
                .clicked()
                    || (submitted && can)
                {
                    commands.push(Command::InstallUserScript(std::mem::take(input)));
                }
                if ui
                    .button(format!("{} Greasy Fork", icon::GLOBE))
                    .on_hover_text(i18n::user_scripts::greasy_fork_tip())
                    .clicked()
                    && let Ok(site) = domain::WebAddress::parse("https://greasyfork.org/")
                {
                    commands.push(Command::Open(site, application::Target::NewTab));
                }
                if runner.installing {
                    ui.spinner();
                }
            });
            super::extensions::status(ui, runner.status.as_ref());
            if runner.scripts().is_empty() {
                ui.weak(i18n::user_scripts::none());
                return;
            }
            for s in runner.scripts().items() {
                ui.separator();
                let tip = i18n::user_scripts::on_off_tip();
                if super::extensions::add_on_head(
                    ui,
                    icon::FILE_CODE,
                    &s.meta.name,
                    &s.meta.version,
                    s.enabled,
                    tip,
                ) {
                    commands.push(Command::SetUserScriptEnabled(s.id.clone(), !s.enabled));
                }
                if !s.meta.description.is_empty() {
                    ui.add(
                        egui::Label::new(RichText::new(&s.meta.description).size(12.5).weak())
                            .wrap(),
                    );
                }
                if ui
                    .add(
                        Button::new(
                            RichText::new(format!(
                                "{} {}",
                                icon::TRASH,
                                i18n::user_scripts::remove()
                            ))
                            .color(ui.visuals().error_fg_color),
                        )
                        .small(),
                    )
                    .clicked()
                {
                    commands.push(Command::RemoveUserScript(s.id.clone()));
                }
            }
        },
    );
}
