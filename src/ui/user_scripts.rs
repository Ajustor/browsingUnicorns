//! User scripts on the desktop: installs in the background, and their part
//! of the "Extensions" view.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use application::UserScriptManager;
use application::ports::{ExtensionError, WebEngine};
use domain::{UserScript, UserScriptId, UserScripts};
use egui::{Button, RichText, TextEdit};
use egui_phosphor::regular as icon;

use super::command::Command;

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
                let name = format!(
                    "{} {} installed: it runs from the next page load.",
                    script.meta.name, script.meta.version
                );
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
    ui.heading("User scripts");
    ui.label("Address of a .user.js script (Greasy Fork, OpenUserJS, GitHub…):");
    let field = ui.add(
        TextEdit::singleline(input)
            .hint_text("https://greasyfork.org/scripts/…/code/….user.js")
            .desired_width(f32::INFINITY),
    );
    let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        let can = !input.trim().is_empty() && !runner.installing;
        if ui
            .add_enabled(
                can,
                Button::new(format!("{} Install", icon::DOWNLOAD_SIMPLE)),
            )
            .clicked()
            || (submitted && can)
        {
            commands.push(Command::InstallUserScript(std::mem::take(input)));
        }
        if ui
            .add(Button::new(format!("{} Greasy Fork", icon::GLOBE)).frame(false))
            .on_hover_text("Find user scripts on Greasy Fork")
            .clicked()
            && let Ok(site) = domain::WebAddress::parse("https://greasyfork.org/")
        {
            commands.push(Command::Open(site, application::Target::NewTab));
        }
        if runner.installing {
            ui.spinner();
        }
    });
    match &runner.status {
        Some(Ok(text)) => {
            ui.label(text);
        }
        Some(Err(text)) => {
            ui.colored_label(ui.visuals().error_fg_color, text);
        }
        None => {}
    }
    if runner.scripts().is_empty() {
        ui.weak("No user scripts.");
        return;
    }
    for s in runner.scripts().items() {
        ui.horizontal(|ui| {
            let mut enabled = s.enabled;
            if ui.checkbox(&mut enabled, "").changed() {
                commands.push(Command::SetUserScriptEnabled(s.id.clone(), enabled));
            }
            let title = RichText::new(format!("{} {}", s.meta.name, s.meta.version)).strong();
            ui.add(egui::Label::new(if s.enabled { title } else { title.weak() }).truncate());
        });
        if !s.meta.description.is_empty() {
            ui.add(egui::Label::new(RichText::new(&s.meta.description).small().weak()).wrap());
        }
        if ui
            .add(Button::new(format!("{} Remove", icon::TRASH)).small())
            .clicked()
        {
            commands.push(Command::RemoveUserScript(s.id.clone()));
        }
        ui.add_space(6.0);
    }
}
