//! Side panel with the history and the bookmarks. A side panel and not a
//! popup: popups would be drawn under the page's native view.

use application::Target;
use domain::{Bookmarks, BrowsingHistory, Timestamp, WebAddress};
use egui::{Button, RichText, ScrollArea, SidePanel, TextEdit};
use egui_phosphor::regular as icon;

use super::command::Command;

const MAX_ROWS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryView {
    History,
    Bookmarks,
}

#[derive(Default)]
pub struct Library {
    pub open: Option<LibraryView>,
    filter: String,
}

impl Library {
    pub fn toggle(&mut self, view: LibraryView) {
        self.open = if self.open == Some(view) {
            None
        } else {
            Some(view)
        };
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        history: &BrowsingHistory,
        bookmarks: &Bookmarks,
        now: Timestamp,
        commands: &mut Vec<Command>,
    ) {
        let Some(view) = self.open else {
            return;
        };
        SidePanel::right("library")
            .resizable(true)
            .default_width(320.0)
            .min_width(220.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (v, label) in [
                        (
                            LibraryView::History,
                            format!("{} History", icon::CLOCK_COUNTER_CLOCKWISE),
                        ),
                        (LibraryView::Bookmarks, format!("{} Bookmarks", icon::STAR)),
                    ] {
                        if ui.selectable_label(view == v, label).clicked() && view != v {
                            commands.push(Command::ToggleLibrary(v));
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(Button::new(icon::X).frame(false))
                            .on_hover_text("Close")
                            .clicked()
                        {
                            commands.push(Command::ToggleLibrary(view));
                        }
                    });
                });
                ui.add(
                    TextEdit::singleline(&mut self.filter)
                        .hint_text("Filter")
                        .desired_width(f32::INFINITY),
                );
                ui.separator();
                match view {
                    LibraryView::History => self.history(ui, history, now, commands),
                    LibraryView::Bookmarks => self.bookmarks(ui, bookmarks, commands),
                }
            });
    }

    fn history(
        &self,
        ui: &mut egui::Ui,
        history: &BrowsingHistory,
        now: Timestamp,
        commands: &mut Vec<Command>,
    ) {
        let visits = history.search(&self.filter, MAX_ROWS);
        if visits.is_empty() {
            ui.weak("No pages.");
        }
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height(ui.available_height() - 36.0)
            .show(ui, |ui| {
                for visit in visits {
                    let title = if visit.title.is_empty() {
                        visit.address.host().unwrap_or(visit.address.as_str())
                    } else {
                        &visit.title
                    };
                    entry(
                        ui,
                        title,
                        &visit.address,
                        Some(ago(now, visit.at)),
                        commands,
                    );
                }
            });
        ui.separator();
        if ui
            .add_enabled(
                !history.is_empty(),
                Button::new(format!("{} Clear history", icon::TRASH)),
            )
            .clicked()
        {
            commands.push(Command::ClearHistory);
        }
    }

    fn bookmarks(&self, ui: &mut egui::Ui, bookmarks: &Bookmarks, commands: &mut Vec<Command>) {
        let filter = self.filter.to_lowercase();
        let mut any = false;
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for b in bookmarks.items().iter().filter(|b| {
                    filter.is_empty()
                        || format!("{} {}", b.title, b.address)
                            .to_lowercase()
                            .contains(&filter)
                }) {
                    any = true;
                    ui.horizontal(|ui| {
                        if ui
                            .add(Button::new(icon::X).frame(false).small())
                            .on_hover_text("Remove bookmark")
                            .clicked()
                        {
                            commands.push(Command::RemoveBookmark(b.address.clone()));
                        }
                        ui.vertical(|ui| entry(ui, b.label(), &b.address, None, commands));
                    });
                }
            });
        if !any {
            ui.weak(format!(
                "No bookmarks. {} in the address bar or Ctrl+D saves the page.",
                icon::STAR
            ));
        }
    }
}

/// One page: title, address and age. Click opens it here, middle click in a
/// new tab.
fn entry(
    ui: &mut egui::Ui,
    title: &str,
    address: &WebAddress,
    age: Option<String>,
    commands: &mut Vec<Command>,
) {
    let response = ui
        .add(
            Button::new(RichText::new(title).strong())
                .frame(false)
                .truncate(),
        )
        .on_hover_text(address.as_str());
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(address.as_str()).small().weak()).truncate());
        if let Some(age) = age {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.small(age);
            });
        }
    });
    ui.add_space(4.0);
    if response.clicked() {
        commands.push(Command::Open(address.clone(), Target::ActiveTab));
    } else if response.middle_clicked() {
        commands.push(Command::Open(address.clone(), Target::NewTab));
    }
}

/// "5 min", "3 h", "2 d".
fn ago(now: Timestamp, at: Timestamp) -> String {
    let secs = now.saturating_sub(at);
    match secs {
        0..60 => "now".into(),
        60..3_600 => format!("{} min", secs / 60),
        3_600..86_400 => format!("{} h", secs / 3_600),
        _ => format!("{} d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_are_short() {
        assert_eq!(ago(100, 90), "now");
        assert_eq!(ago(10_000, 10_000 - 300), "5 min");
        assert_eq!(ago(100_000, 100_000 - 7_200), "2 h");
        assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "3 d");
        assert_eq!(ago(0, 50), "now");
    }

    #[test]
    fn toggling_the_open_view_closes_it() {
        let mut lib = Library::default();
        lib.toggle(LibraryView::History);
        assert_eq!(lib.open, Some(LibraryView::History));
        lib.toggle(LibraryView::Bookmarks);
        assert_eq!(lib.open, Some(LibraryView::Bookmarks));
        lib.toggle(LibraryView::Bookmarks);
        assert_eq!(lib.open, None);
    }
}
