//! Extensions on the desktop: installs in the background, and the
//! "Extensions" view of the side panel.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use application::ExtensionManager;
use application::Target;
use application::ports::{ExtensionError, ExtensionSource, ExtensionSupport, WebEngine};
use domain::{Extension, ExtensionId, Extensions, WebAddress};
use egui::{Button, RichText};
use egui_phosphor::regular as icon;

use infrastructure::extensions::UpdateClock;

use super::command::Command;
use super::panel_kit::{self, card, field, primary_button};

/// The stores are first asked for updates this long after start-up...
const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(30);
/// ...then looked at again this often (a check runs once a day).
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

type Installed = Result<Extension, ExtensionError>;

pub struct ExtensionsRunner {
    manager: ExtensionManager,
    /// When the stores were last asked for updates; `None`: never (a
    /// private window).
    update_clock: Option<UpdateClock>,
    next_update_check: Instant,
    updates_tx: Sender<Vec<Extension>>,
    updates_rx: Receiver<Vec<Extension>>,
    installing: Option<String>,
    /// The last install's outcome, shown under the field.
    pub status: Option<Result<String, String>>,
    repaint: Arc<dyn Fn() + Send + Sync>,
    tx: Sender<Installed>,
    rx: Receiver<Installed>,
}

/// What the input names: a `.crx` file, an unpacked extension's folder, or
/// a Web Store link or id.
fn source_of(input: &str) -> Option<ExtensionSource> {
    let input = input.trim().trim_matches('"');
    if input.to_ascii_lowercase().ends_with(".crx") {
        return Some(ExtensionSource::File(input.into()));
    }
    if std::path::Path::new(input).is_dir() {
        return Some(ExtensionSource::Folder(input.into()));
    }
    ExtensionId::from_store_input(input).map(ExtensionSource::Store)
}

impl ExtensionsRunner {
    pub fn new(
        manager: ExtensionManager,
        update_clock: Option<UpdateClock>,
        repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        let (updates_tx, updates_rx) = mpsc::channel();
        Self {
            manager,
            update_clock,
            next_update_check: Instant::now() + FIRST_UPDATE_CHECK,
            updates_tx,
            updates_rx,
            installing: None,
            status: None,
            repaint: Arc::new(repaint),
            tx,
            rx,
        }
    }

    pub fn extensions(&self) -> &Extensions {
        self.manager.extensions()
    }

    pub fn is_installing(&self) -> bool {
        self.installing.is_some()
    }

    /// Downloads and unpacks in the background; [`Self::poll`] finishes.
    pub fn install(&mut self, input: &str) {
        let Some(source) = source_of(input) else {
            self.status = Some(Err(i18n::extensions::invalid_input().into()));
            return;
        };
        self.installing = Some(input.trim().to_owned());
        self.status = None;
        let (packages, tx, repaint) = (
            self.manager.packages(),
            self.tx.clone(),
            self.repaint.clone(),
        );
        std::thread::spawn(move || {
            let _ = tx.send(packages.install(&source));
            repaint();
        });
    }

    /// Asks the stores for newer versions of the extensions, once a day,
    /// in the background; [`Self::poll`] records what came.
    fn check_updates(&mut self) {
        let Some(clock) = &self.update_clock else {
            return;
        };
        if Instant::now() < self.next_update_check {
            return;
        }
        self.next_update_check = Instant::now() + UPDATE_CHECK_EVERY;
        if !clock.is_due() || self.manager.extensions().items().is_empty() {
            return;
        }
        clock.mark();
        let (packages, installed) = (self.manager.packages(), self.manager.versions());
        let (tx, repaint) = (self.updates_tx.clone(), self.repaint.clone());
        std::thread::spawn(move || {
            let _ = tx.send(application::extensions::fetch_updates(
                packages.as_ref(),
                &installed,
            ));
            repaint();
        });
    }

    /// Records a finished install or updates, and hands them to the engine.
    pub fn poll(&mut self, engine: &mut dyn WebEngine) {
        self.check_updates();
        if let Ok(updated) = self.updates_rx.try_recv() {
            for extension in updated {
                log::info!(
                    "extensions: {} updated to {}",
                    extension.name,
                    extension.version
                );
                if let Err(e) = self.manager.installed(extension, engine) {
                    log::warn!("extensions: recording an update: {e}");
                }
            }
        }
        let Ok(result) = self.rx.try_recv() else {
            return;
        };
        self.installing = None;
        self.status = Some(match result {
            Ok(extension) => {
                let name = i18n::extensions::installed(&extension.name, &extension.version);
                match self.manager.installed(extension, engine) {
                    Ok(()) => Ok(name),
                    Err(e) => Err(e.to_string()),
                }
            }
            Err(e) => Err(e.to_string()),
        });
    }

    pub fn set_enabled(
        &mut self,
        id: &ExtensionId,
        enabled: bool,
        engine: &mut dyn WebEngine,
    ) -> Result<(), String> {
        self.manager
            .set_enabled(id, enabled, engine)
            .map_err(|e| e.to_string())
    }

    pub fn remove(&mut self, id: &ExtensionId, engine: &mut dyn WebEngine) -> Result<(), String> {
        self.manager.remove(id, engine).map_err(|e| e.to_string())
    }
}

/// What the view shows, borrowed for one frame.
pub struct ExtensionsView<'a> {
    pub support: ExtensionSupport,
    /// Where the browser serves the extensions' pages (it runs them itself).
    pub origins: Option<infrastructure::extension_server::Origins>,
    pub runner: &'a ExtensionsRunner,
    pub user_scripts: &'a super::user_scripts::UserScriptsRunner,
    /// Which background pages run (to inspect them).
    pub engine: &'a infrastructure::WryEngine,
}

/// What the view keeps between frames.
#[derive(Default)]
pub struct ExtensionsPanel {
    input: String,
    script_input: String,
}

impl ExtensionsPanel {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        view: ExtensionsView<'_>,
        commands: &mut Vec<Command>,
    ) {
        self.extensions(ui, &view, commands);
        super::user_scripts::show(ui, view.user_scripts, &mut self.script_input, commands);
    }

    fn extensions(
        &mut self,
        ui: &mut egui::Ui,
        view: &ExtensionsView<'_>,
        commands: &mut Vec<Command>,
    ) {
        let hint = match view.support {
            ExtensionSupport::None => Some(i18n::extensions::unsupported()),
            ExtensionSupport::Partial => Some(i18n::extensions::partly_supported()),
            ExtensionSupport::Full => None,
        };
        let runner = view.runner;
        card(
            ui,
            icon::PUZZLE_PIECE,
            i18n::extensions::add_title(),
            hint,
            |ui| {
                if view.support == ExtensionSupport::None {
                    return;
                }
                ui.add(
                    egui::Label::new(RichText::new(i18n::extensions::input_label()).size(12.5))
                        .wrap(),
                );
                let field = ui.add(field(
                    &mut self.input,
                    "https://chromewebstore.google.com/detail/…",
                ));
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal_wrapped(|ui| {
                    let can = !self.input.trim().is_empty() && !runner.is_installing();
                    if primary_button(
                        ui,
                        format!("{} {}", icon::DOWNLOAD_SIMPLE, i18n::extensions::install()),
                        can,
                    )
                    .clicked()
                        || (submitted && can)
                    {
                        commands.push(Command::InstallExtension(std::mem::take(&mut self.input)));
                    }
                    if ui
                        .button(format!("{} Web Store", icon::STOREFRONT))
                        .on_hover_text(i18n::extensions::web_store_tip())
                        .clicked()
                        && let Ok(store) =
                            domain::WebAddress::parse("https://chromewebstore.google.com/")
                    {
                        commands.push(Command::Open(store, Target::NewTab));
                    }
                });
                // By hand: a package, or an extension's folder (Chrome's
                // "Load unpacked"; install it again to take changes).
                ui.horizontal_wrapped(|ui| {
                    let idle = !runner.is_installing();
                    if ui
                        .add_enabled(
                            idle,
                            Button::new(format!(
                                "{} {}",
                                icon::FILE_ARCHIVE,
                                i18n::extensions::choose_crx()
                            )),
                        )
                        .on_hover_text(i18n::extensions::choose_crx_tip())
                        .clicked()
                        && let Some(file) = rfd::FileDialog::new()
                            .set_title(i18n::extensions::crx_dialog_title())
                            .add_filter(i18n::extensions::crx_filter(), &["crx"])
                            .pick_file()
                    {
                        commands.push(Command::InstallExtension(file.display().to_string()));
                    }
                    if ui
                        .add_enabled(
                            idle,
                            Button::new(format!(
                                "{} {}",
                                icon::FOLDER_OPEN,
                                i18n::extensions::load_unpacked()
                            )),
                        )
                        .on_hover_text(i18n::extensions::load_unpacked_tip())
                        .clicked()
                        && let Some(folder) = rfd::FileDialog::new()
                            .set_title(i18n::extensions::folder_dialog_title())
                            .pick_folder()
                    {
                        commands.push(Command::InstallExtension(folder.display().to_string()));
                    }
                });
                if let Some(what) = &runner.installing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.add(egui::Label::new(i18n::extensions::installing(what)).truncate());
                    });
                }
                status(ui, runner.status.as_ref());
            },
        );
        if view.support == ExtensionSupport::None {
            return;
        }
        let extensions = runner.extensions();
        let note = if view.origins.is_some() {
            i18n::extensions::note_with_popups()
        } else {
            i18n::extensions::note_in_tabs()
        };
        card(
            ui,
            icon::LIST_CHECKS,
            i18n::extensions::installed_title(),
            (!extensions.is_empty()).then_some(note),
            |ui| {
                if extensions.is_empty() {
                    ui.weak(i18n::extensions::none());
                    return;
                }
                for (i, e) in extensions.items().iter().enumerate() {
                    if i > 0 {
                        ui.separator();
                    }
                    let inspect = view.engine.has_background(&e.id);
                    extension_row(ui, e, view.origins, inspect, commands);
                }
            },
        );
    }
}

/// How the last install went, under the buttons.
pub fn status(ui: &mut egui::Ui, status: Option<&Result<String, String>>) {
    match status {
        Some(Ok(text)) => {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::CHECK_CIRCLE).color(panel_kit::ok_text(ui)));
                ui.add(egui::Label::new(text).wrap());
            });
        }
        Some(Err(text)) => {
            let red = ui.visuals().error_fg_color;
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::WARNING).color(red));
                ui.add(egui::Label::new(RichText::new(text).color(red)).wrap());
            });
        }
        None => {}
    }
}

/// An add-on's head: its badge, name and version, and the switch that
/// turns it on or off. Returns whether the switch was clicked.
pub fn add_on_head(
    ui: &mut egui::Ui,
    glyph: &str,
    name: &str,
    version: &str,
    enabled: bool,
    tip: &str,
) -> bool {
    ui.horizontal(|ui| {
        let fill = if enabled {
            panel_kit::ACCENT
        } else {
            ui.visuals().weak_text_color()
        };
        panel_kit::badge(ui, glyph, 24.0, fill);
        let width = ui.available_width() - 36.0 - 2.0 * ui.spacing().item_spacing.x;
        ui.allocate_ui_with_layout(
            egui::vec2(width, 0.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_width(width);
                let title = RichText::new(name).strong();
                ui.add(egui::Label::new(if enabled { title } else { title.weak() }).truncate());
                if !version.is_empty() {
                    ui.label(RichText::new(version).size(11.5).weak());
                }
            },
        );
        panel_kit::switch(ui, enabled).on_hover_text(tip).clicked()
    })
    .inner
}

fn extension_row(
    ui: &mut egui::Ui,
    e: &Extension,
    origins: Option<infrastructure::extension_server::Origins>,
    inspect: bool,
    commands: &mut Vec<Command>,
) {
    let tip = if e.enabled {
        i18n::extensions::disable_tip()
    } else {
        i18n::extensions::enable_tip()
    };
    if add_on_head(ui, icon::PUZZLE_PIECE, &e.name, &e.version, e.enabled, tip) {
        commands.push(Command::SetExtensionEnabled(e.id.clone(), !e.enabled));
    }
    if !e.description.is_empty() {
        ui.add(egui::Label::new(RichText::new(&e.description).size(12.5).weak()).wrap());
    }
    ui.horizontal_wrapped(|ui| {
        match origins {
            // The browser serves the pages: the popup goes in this panel.
            Some(origins) => {
                if e.popup.is_some() {
                    let response = ui.add_enabled(
                        e.enabled,
                        Button::new(format!(
                            "{} {}",
                            icon::APP_WINDOW,
                            i18n::extensions::popup()
                        ))
                        .small(),
                    );
                    if response.clicked() {
                        commands.push(Command::ToggleExtensionPopup {
                            id: e.id.clone(),
                            anchor: response.rect,
                        });
                    }
                }
                if let Some(options) = &e.options
                    && let Ok(page) = WebAddress::parse(&origins.url(&e.id, options))
                    && ui
                        .add_enabled(
                            e.enabled,
                            Button::new(format!(
                                "{} {}",
                                icon::SLIDERS_HORIZONTAL,
                                i18n::extensions::options()
                            ))
                            .small(),
                        )
                        .clicked()
                {
                    commands.push(Command::Open(page, Target::NewTab));
                }
                if inspect
                    && ui
                        .add(
                            Button::new(format!("{} {}", icon::CODE, i18n::extensions::inspect()))
                                .small(),
                        )
                        .on_hover_text(i18n::extensions::inspect_tip())
                        .clicked()
                {
                    commands.push(Command::InspectExtension(e.id.clone()));
                }
            }
            // WebView2's own: its pages in a tab.
            None => {
                for (label, page) in [
                    (i18n::extensions::popup(), e.popup_page()),
                    (i18n::extensions::options(), e.options_page()),
                ] {
                    if let Some(page) = page
                        && ui
                            .add_enabled(e.enabled, Button::new(label).small())
                            .clicked()
                    {
                        commands.push(Command::Open(page, Target::NewTab));
                    }
                }
            }
        }
        if ui
            .add(
                Button::new(
                    RichText::new(format!("{} {}", icon::TRASH, i18n::extensions::remove()))
                        .color(ui.visuals().error_fg_color),
                )
                .small(),
            )
            .clicked()
        {
            commands.push(Command::RemoveExtension(e.id.clone()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_from_input() {
        let id = "ddkjiahejlhfcafbddmgiahcphecmpfh";
        assert_eq!(
            source_of(&format!("https://chromewebstore.google.com/detail/x/{id}")),
            Some(ExtensionSource::Store(ExtensionId::parse(id).unwrap()))
        );
        assert_eq!(
            source_of("\"C:\\Downloads\\My Ext.CRX\""),
            Some(ExtensionSource::File("C:\\Downloads\\My Ext.CRX".into()))
        );
        assert_eq!(source_of("hello"), None);
        let dir = std::env::temp_dir();
        assert_eq!(
            source_of(&dir.display().to_string()),
            Some(ExtensionSource::Folder(dir))
        );
    }
}
