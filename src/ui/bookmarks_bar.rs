use application::Target;
use domain::Bookmarks;
use egui::{Button, RichText, ScrollArea};

use super::command::Command;

const LABEL_CHARS: usize = 20;

/// One button per bookmark under the toolbar. Click opens it here, middle
/// click in a new tab.
pub fn show(ui: &mut egui::Ui, bookmarks: &Bookmarks, commands: &mut Vec<Command>) {
    ScrollArea::horizontal()
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for b in bookmarks.items() {
                    let label = super::tab_strip::truncate(b.label(), LABEL_CHARS);
                    let response = ui
                        .add(
                            Button::new(RichText::new(label).color(super::theme::CHROME_TEXT_WEAK))
                                .frame(false)
                                .small(),
                        )
                        .on_hover_text(b.address.as_str());
                    if response.clicked() {
                        commands.push(Command::Open(b.address.clone(), Target::ActiveTab));
                    } else if response.middle_clicked() {
                        commands.push(Command::Open(b.address.clone(), Target::NewTab));
                    }
                }
            });
        });
}
