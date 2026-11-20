//! Extensions on the desktop: installs in the background, and the
//! "Extensions" view of the side panel.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use application::ExtensionManager;
use application::Target;
use application::ports::{ExtensionError, ExtensionSource, ExtensionSupport, WebEngine};
use domain::{Extension, ExtensionId, Extensions, WebAddress};
use egui::{Button, RichText, ScrollArea, TextEdit};
use egui_phosphor::regular as icon;

use super::command::Command;

type Installed = Result<Extension, ExtensionError>;

pub struct ExtensionsRunner {
    manager: ExtensionManager,
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
    pub fn new(manager: ExtensionManager, repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            manager,
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

    /// Records a finished install and hands it to the engine.
    pub fn poll(&mut self, engine: &mut dyn WebEngine) {
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
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                self.extensions(ui, &view, commands);
                ui.separator();
                super::user_scripts::show(ui, view.user_scripts, &mut self.script_input, commands);
            });
    }

    fn extensions(
        &mut self,
        ui: &mut egui::Ui,
        view: &ExtensionsView<'_>,
        commands: &mut Vec<Command>,
    ) {
        ui.heading(i18n::extensions::heading());
        match view.support {
            ExtensionSupport::None => {
                ui.label(i18n::extensions::unsupported());
                return;
            }
            ExtensionSupport::Partial => {
                ui.weak(i18n::extensions::partly_supported());
            }
            ExtensionSupport::Full => {}
        }
        let runner = view.runner;
        ui.label(i18n::extensions::input_label());
        let field = ui.add(
            TextEdit::singleline(&mut self.input)
                .hint_text("https://chromewebstore.google.com/detail/…")
                .desired_width(f32::INFINITY),
        );
        let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            let can = !self.input.trim().is_empty() && !runner.is_installing();
            if ui
                .add_enabled(
                    can,
                    Button::new(format!(
                        "{} {}",
                        icon::DOWNLOAD_SIMPLE,
                        i18n::extensions::install()
                    )),
                )
                .clicked()
                || (submitted && can)
            {
                commands.push(Command::InstallExtension(std::mem::take(&mut self.input)));
            }
            if super::theme::hover_fill(ui, |ui| {
                ui.add(Button::new(format!("{} Web Store", icon::STOREFRONT)).frame(false))
            })
            .on_hover_text(i18n::extensions::web_store_tip())
            .clicked()
                && let Ok(store) = domain::WebAddress::parse("https://chromewebstore.google.com/")
            {
                commands.push(Command::Open(store, Target::NewTab));
            }
        });
        // By hand: a package, or an extension's folder (Chrome's "Load
        // unpacked"; install it again to take changes).
        ui.horizontal(|ui| {
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
        match &runner.status {
            Some(Ok(text)) => {
                ui.label(text);
            }
            Some(Err(text)) => {
                ui.colored_label(ui.visuals().error_fg_color, text);
            }
            None => {}
        }
        ui.separator();
        let extensions = runner.extensions();
        if extensions.is_empty() {
            ui.weak(i18n::extensions::none());
            return;
        }
        for e in extensions.items() {
            let inspect = view.engine.has_background(&e.id);
            extension_row(ui, e, view.origins, inspect, commands);
            ui.add_space(6.0);
        }
        ui.weak(if view.origins.is_some() {
            i18n::extensions::note_with_popups()
        } else {
            i18n::extensions::note_in_tabs()
        });
    }
}

fn extension_row(
    ui: &mut egui::Ui,
    e: &Extension,
    origins: Option<infrastructure::extension_server::Origins>,
    inspect: bool,
    commands: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        let mut enabled = e.enabled;
        if ui
            .checkbox(&mut enabled, "")
            .on_hover_text(if e.enabled {
                i18n::extensions::disable_tip()
            } else {
                i18n::extensions::enable_tip()
            })
            .changed()
        {
            commands.push(Command::SetExtensionEnabled(e.id.clone(), enabled));
        }
        let title = RichText::new(format!("{} {}", e.name, e.version)).strong();
        ui.add(egui::Label::new(if e.enabled { title } else { title.weak() }).truncate());
    });
    if !e.description.is_empty() {
        ui.add(egui::Label::new(RichText::new(&e.description).small().weak()).wrap());
    }
    ui.horizontal(|ui| {
        match origins {
            // The browser serves the pages: the popup goes in this panel.
            Some(origins) => {
                if e.popup.is_some() {
                    let response =
                        ui.add_enabled(e.enabled, Button::new(i18n::extensions::popup()).small());
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
                        .add_enabled(e.enabled, Button::new(i18n::extensions::options()).small())
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
            .add(Button::new(format!("{} {}", icon::TRASH, i18n::extensions::remove())).small())
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
