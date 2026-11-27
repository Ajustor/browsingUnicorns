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
                for b in bookmarks.items().iter().filter(|b| b.folder.is_empty()) {
                    let label = super::tab_common::truncate(b.label(), LABEL_CHARS);
                    let response = super::theme::hover_fill(ui, |ui| {
                        ui.add(
                            Button::new(RichText::new(label).color(super::theme::CHROME_TEXT_WEAK))
                                .frame(false)
                                .small(),
                        )
                    })
                    .on_hover_text(b.address.as_str());
                    if response.clicked() {
                        commands.push(Command::Open(b.address.clone(), Target::ActiveTab));
                    } else if response.middle_clicked() {
                        commands.push(Command::Open(b.address.clone(), Target::NewTab));
                    }
                }
                // A folder opens in the side panel (a menu would be drawn
                // under the page's native view).
                let mut tops: Vec<&str> = bookmarks
                    .items()
                    .iter()
                    .filter_map(|b| b.folder.split('/').next().filter(|f| !f.is_empty()))
                    .collect();
                tops.sort_by_key(|f| f.to_lowercase());
                tops.dedup();
                for folder in tops {
                    let label = super::tab_common::truncate(folder, LABEL_CHARS);
                    if super::theme::hover_fill(ui, |ui| {
                        ui.add(
                            Button::new(
                                RichText::new(format!(
                                    "{} {label}",
                                    egui_phosphor::regular::FOLDER
                                ))
                                .color(super::theme::CHROME_TEXT_WEAK),
                            )
                            .frame(false)
                            .small(),
                        )
                    })
                    .on_hover_text(i18n::bookmarks::show_folder_tip())
                    .clicked()
                    {
                        commands.push(Command::ShowBookmarkFolder(folder.to_owned()));
                    }
                }
            });
        });
}
