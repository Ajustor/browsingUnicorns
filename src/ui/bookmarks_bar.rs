use std::hash::{Hash, Hasher};

use application::Target;
use domain::{Bookmarks, WebAddress};
use egui::{Button, RichText, ScrollArea};

use super::command::Command;

const LABEL_CHARS: usize = 20;

/// What the bar shows, worked out again only when the bookmarks change.
#[derive(Default)]
pub struct BookmarksBar {
    /// The [`fingerprint`] of the bookmarks it was worked out from.
    of: Option<u64>,
    /// The bookmarks out of any folder: their label and address.
    pages: Vec<(String, WebAddress)>,
    /// The top folders, sorted, each once whatever its case: its name and
    /// label.
    folders: Vec<(String, String)>,
}

/// Tells bookmarks apart without copying them: changes when one is added,
/// removed, renamed or moved.
pub fn fingerprint(bookmarks: &Bookmarks) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for b in bookmarks.items() {
        (b.address.as_str(), &b.title, &b.folder).hash(&mut hasher);
    }
    hasher.finish()
}

/// The top folders (before the first `/`), sorted without case, each once:
/// "Foo" and "foo" are the same button.
fn top_folders(bookmarks: &Bookmarks) -> Vec<&str> {
    let mut tops: Vec<&str> = bookmarks
        .items()
        .iter()
        .filter_map(|b| b.folder.split('/').next().filter(|f| !f.is_empty()))
        .collect();
    tops.sort_by_cached_key(|f| f.to_lowercase());
    tops.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
    tops
}

impl BookmarksBar {
    fn refresh(&mut self, bookmarks: &Bookmarks) {
        let of = fingerprint(bookmarks);
        if self.of == Some(of) {
            return;
        }
        self.of = Some(of);
        self.pages = bookmarks
            .items()
            .iter()
            .filter(|b| b.folder.is_empty())
            .map(|b| {
                (
                    super::tab_common::truncate(b.label(), LABEL_CHARS),
                    b.address.clone(),
                )
            })
            .collect();
        self.folders = top_folders(bookmarks)
            .into_iter()
            .map(|f| {
                let label = super::tab_common::truncate(f, LABEL_CHARS);
                (
                    f.to_owned(),
                    format!("{} {label}", egui_phosphor::regular::FOLDER),
                )
            })
            .collect();
    }

    /// One button per bookmark under the toolbar. Click opens it here,
    /// middle click in a new tab.
    pub fn show(&mut self, ui: &mut egui::Ui, bookmarks: &Bookmarks, commands: &mut Vec<Command>) {
        self.refresh(bookmarks);
        ScrollArea::horizontal()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (label, address) in &self.pages {
                        let response = super::theme::hover_fill(ui, |ui| {
                            ui.add(
                                Button::new(
                                    RichText::new(label.as_str())
                                        .color(super::theme::CHROME_TEXT_WEAK),
                                )
                                .frame(false)
                                .small(),
                            )
                        })
                        .on_hover_text(address.as_str());
                        if response.clicked() {
                            commands.push(Command::Open(address.clone(), Target::ActiveTab));
                        } else if response.middle_clicked() {
                            commands.push(Command::Open(address.clone(), Target::NewTab));
                        }
                    }
                    // A folder opens in the side panel (a menu would be drawn
                    // under the page's native view).
                    for (folder, label) in &self.folders {
                        if super::theme::hover_fill(ui, |ui| {
                            ui.add(
                                Button::new(
                                    RichText::new(label.as_str())
                                        .color(super::theme::CHROME_TEXT_WEAK),
                                )
                                .frame(false)
                                .small(),
                            )
                        })
                        .on_hover_text(i18n::bookmarks::show_folder_tip())
                        .clicked()
                        {
                            commands.push(Command::ShowBookmarkFolder(folder.clone()));
                        }
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bookmarks(folders: &[&str]) -> Bookmarks {
        let mut b = Bookmarks::default();
        for (i, folder) in folders.iter().enumerate() {
            let address = WebAddress::parse(&format!("https://site{i}.example/")).unwrap();
            b.add_in(address, "page", folder, 0);
        }
        b
    }

    #[test]
    fn a_folder_shows_once_whatever_its_case() {
        let b = bookmarks(&["Foo", "bar/deep", "foo", "Foo/sub", "Bar"]);
        assert_eq!(top_folders(&b), ["bar", "Foo"]);
    }

    #[test]
    fn the_fingerprint_follows_a_move() {
        let mut b = bookmarks(&["Foo"]);
        let before = fingerprint(&b);
        assert_eq!(fingerprint(&b), before);
        let address = b.items()[0].address.clone();
        b.move_to(&address, "Bar", 1);
        assert_ne!(fingerprint(&b), before);
    }
}
