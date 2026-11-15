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

/// What the input names: a `.crx` file, or a Web Store link or id.
fn source_of(input: &str) -> Option<ExtensionSource> {
    let input = input.trim().trim_matches('"');
    if input.to_ascii_lowercase().ends_with(".crx") {
        return Some(ExtensionSource::File(input.into()));
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
            self.status = Some(Err(
                "Paste a Chrome Web Store link, an extension id or the path of a .crx file.".into(),
            ));
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
                let name = format!("{} {} installed.", extension.name, extension.version);
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
        ui.heading("Extensions");
        match view.support {
            ExtensionSupport::None => {
                ui.label(
                    "Extensions need the Windows web view (WebView2): the web view \
                     of this system cannot run them.",
                );
                return;
            }
            ExtensionSupport::Partial => {
                ui.weak(
                    "On this system the browser runs the extensions' blocking rules \
                     and page scripts; their background pages and popups do not run.",
                );
            }
            ExtensionSupport::Full => {}
        }
        let runner = view.runner;
        ui.label("Chrome Web Store link, extension id or .crx file:");
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
                    Button::new(format!("{} Install", icon::DOWNLOAD_SIMPLE)),
                )
                .clicked()
                || (submitted && can)
            {
                commands.push(Command::InstallExtension(std::mem::take(&mut self.input)));
            }
            if ui
                .add(Button::new(format!("{} Web Store", icon::STOREFRONT)).frame(false))
                .on_hover_text("Find extensions in the Chrome Web Store")
                .clicked()
                && let Ok(store) = domain::WebAddress::parse("https://chromewebstore.google.com/")
            {
                commands.push(Command::Open(store, Target::NewTab));
            }
        });
        if let Some(what) = &runner.installing {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.add(egui::Label::new(format!("Installing {what}…")).truncate());
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
            ui.weak("No extensions.");
            return;
        }
        for e in extensions.items() {
            extension_row(ui, e, view.origins, commands);
            ui.add_space(6.0);
        }
        ui.weak(if view.origins.is_some() {
            "Pages already open see a change after a reload. Extensions with a \
             popup have a button next to the bookmark star; Options opens their \
             settings in a tab."
        } else {
            "Pages already open see a change after a reload. Toolbar buttons do \
             not exist here: Popup and Options open the extension's pages in a tab."
        });
    }
}

fn extension_row(
    ui: &mut egui::Ui,
    e: &Extension,
    origins: Option<infrastructure::extension_server::Origins>,
    commands: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        let mut enabled = e.enabled;
        if ui
            .checkbox(&mut enabled, "")
            .on_hover_text(if e.enabled { "Disable" } else { "Enable" })
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
                    let response = ui.add_enabled(e.enabled, Button::new("Popup").small());
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
                        .add_enabled(e.enabled, Button::new("Options").small())
                        .clicked()
                {
                    commands.push(Command::Open(page, Target::NewTab));
                }
            }
            // WebView2's own: its pages in a tab.
            None => {
                for (label, page) in [("Popup", e.popup_page()), ("Options", e.options_page())] {
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
            .add(Button::new(format!("{} Remove", icon::TRASH)).small())
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
    }
}
