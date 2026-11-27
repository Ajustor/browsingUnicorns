//! Find in page (Ctrl/Cmd+F): a bar under the toolbar. The page selects
//! and scrolls to each match (`window.find`, in WebKit and Chromium alike).
//! On Windows, a page with the keyboard keeps WebView2's own find bar.

use egui::{RichText, TopBottomPanel};
use egui_phosphor::regular as icon;

use super::command::Command;

#[derive(Default)]
pub struct FindBar {
    open: bool,
    query: String,
    /// The field takes the keyboard on the next frame (just opened).
    focus: bool,
}

impl FindBar {
    /// Opens the bar, its field ready to type into (the text kept).
    pub fn open(&mut self) {
        self.open = true;
        self.focus = true;
    }

    /// The bar, while open: typing finds from the top, Enter / Shift+Enter
    /// the next / previous match, Escape closes.
    pub fn show(&mut self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        if !self.open {
            return;
        }
        let mut close = false;
        TopBottomPanel::top("find_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::MAGNIFYING_GLASS).size(15.0));
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .desired_width(240.0)
                        .hint_text(i18n::find::hint()),
                );
                if std::mem::take(&mut self.focus) {
                    field.request_focus();
                }
                let find = |query: &str, backwards, restart| Command::FindInPage {
                    query: query.to_owned(),
                    backwards,
                    restart,
                };
                if field.changed() {
                    commands.push(find(&self.query, false, true));
                }
                let (enter, shift, escape) = ui.input(|i| {
                    (
                        i.key_pressed(egui::Key::Enter),
                        i.modifiers.shift,
                        i.key_pressed(egui::Key::Escape),
                    )
                });
                // Enter leaves the field: it takes the keyboard back.
                if field.lost_focus() && enter {
                    commands.push(find(&self.query, shift, false));
                    field.request_focus();
                }
                if (field.has_focus() || field.lost_focus()) && escape {
                    close = true;
                }
                let button = |ui: &mut egui::Ui, glyph: &str, tip: &str| {
                    super::panel_kit::icon_button(ui, RichText::new(glyph).size(15.0), tip)
                        .clicked()
                };
                if button(ui, icon::CARET_UP, i18n::find::previous()) {
                    commands.push(find(&self.query, true, false));
                }
                if button(ui, icon::CARET_DOWN, i18n::find::next()) {
                    commands.push(find(&self.query, false, false));
                }
                if button(ui, icon::X, i18n::common::close()) {
                    close = true;
                }
            });
        });
        if close {
            self.open = false;
            // The match's selection goes with the bar.
            commands.push(Command::FindInPage {
                query: String::new(),
                backwards: false,
                restart: true,
            });
        }
    }
}
