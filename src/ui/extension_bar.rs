//! The extensions' buttons in the toolbar, next to the bookmark star, and
//! their popups: a native view laid over the page, under the button.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use domain::{ExtensionId, Extensions, TabId};
use egui::{Button, Rect, TextureHandle, TextureOptions, Vec2};
use infrastructure::extensions::{extension_dir, manifest};

use super::command::Command;

/// A button of the toolbar.
pub struct Action {
    pub id: ExtensionId,
    pub name: String,
    pub icon: Option<TextureHandle>,
    pub open: bool,
}

/// The popup shown, under the button it belongs to.
pub struct OpenPopup {
    pub id: ExtensionId,
    pub url: String,
    /// The button, in points.
    pub anchor: Rect,
    /// The tab it was opened over: it closes when another one shows.
    pub tab: Option<TabId>,
}

pub struct ExtensionBar {
    dir: PathBuf,
    /// Decoded icons, by extension, with the version they are of.
    icons: HashMap<ExtensionId, (String, Option<TextureHandle>)>,
    pub popup: Option<OpenPopup>,
    /// The popup that just closed on its own (a click elsewhere): the click
    /// on its button that caused it must not open it again.
    closed: Option<(ExtensionId, Instant)>,
}

/// Size of a popup before its content is measured, in points.
pub const DEFAULT_POPUP: Vec2 = Vec2::new(360.0, 480.0);

impl ExtensionBar {
    pub fn new(extensions_dir: PathBuf) -> Self {
        Self {
            dir: extensions_dir,
            icons: HashMap::new(),
            popup: None,
            closed: None,
        }
    }

    fn icon(
        &mut self,
        ctx: &egui::Context,
        id: &ExtensionId,
        version: &str,
    ) -> Option<TextureHandle> {
        // Looked up without copying the key: called each frame.
        if let Some((known, icon)) = self.icons.get(id)
            && known == version
        {
            return icon.clone();
        }
        let icon = (|| {
            let dir = extension_dir(&self.dir, id);
            let path = manifest::action_icon(&dir)?;
            let bytes = std::fs::read(dir.join(path.trim_start_matches('/'))).ok()?;
            let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .ok()?
                .to_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            let pixels = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
            Some(ctx.load_texture(format!("extension-{id}"), pixels, TextureOptions::LINEAR))
        })();
        self.icons
            .insert(id.clone(), (version.to_owned(), icon.clone()));
        icon
    }

    /// The toolbar's buttons: enabled extensions with a popup.
    pub fn actions(&mut self, ctx: &egui::Context, extensions: &Extensions) -> Vec<Action> {
        extensions
            .items()
            .iter()
            .filter(|e| e.enabled && e.popup.is_some())
            .map(|e| Action {
                icon: self.icon(ctx, &e.id, &e.version),
                open: self.popup.as_ref().is_some_and(|p| p.id == e.id),
                id: e.id.clone(),
                name: e.name.clone(),
            })
            .collect()
    }

    /// Opens `id`'s popup at `url` under `anchor`, or closes it if it shows.
    pub fn toggle(&mut self, id: ExtensionId, url: String, anchor: Rect, tab: Option<TabId>) {
        if self.popup.as_ref().is_some_and(|p| p.id == id) {
            self.popup = None;
            return;
        }
        if self
            .closed
            .as_ref()
            .is_some_and(|(closed, at)| *closed == id && at.elapsed() < Duration::from_millis(500))
        {
            return;
        }
        self.popup = Some(OpenPopup {
            id,
            url,
            anchor,
            tab,
        });
    }

    /// The popup closed on its own (lost the focus).
    pub fn closed_by_page(&mut self) {
        if let Some(p) = self.popup.take() {
            self.closed = Some((p.id, Instant::now()));
        }
    }
}

/// The buttons, laid right to left (the toolbar's right side).
pub fn show(ui: &mut egui::Ui, actions: &[Action], commands: &mut Vec<Command>) {
    for a in actions.iter().rev() {
        let size = Vec2::splat(18.0);
        let button = match &a.icon {
            Some(texture) => Button::image(egui::Image::new((texture.id(), size))),
            None => Button::new(a.name.chars().next().unwrap_or('?').to_string()),
        }
        .frame(a.open)
        .selected(a.open);
        let response = super::theme::hover_fill(ui, |ui| ui.add(button)).on_hover_text(&a.name);
        if response.clicked() {
            commands.push(Command::ToggleExtensionPopup {
                id: a.id.clone(),
                anchor: response.rect,
            });
        }
    }
}
