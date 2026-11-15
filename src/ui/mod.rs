//! Presentation layer: the browser chrome drawn with egui. Pages themselves
//! are native child views laid over the central area.

mod app;
mod bookmarks_bar;
mod command;
mod library;
mod omnibox;
mod shortcuts;
mod tab_strip;
mod toolbar;
mod updates;

pub use updates::Updates;

pub use app::{BrowserApp, ExitSlot};

pub const APP_NAME: &str = "Browsing Unicorns";

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
}
