use egui::{Button, ScrollArea, Sense, Spinner};
use egui_phosphor::regular as icon;

use domain::Session;

use super::command::Command;

const TITLE_CHARS: usize = 24;

pub fn show(ui: &mut egui::Ui, session: &Session, commands: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        // As wide as the tabs: the + follows the last one (room kept for it
        // when they overflow).
        ScrollArea::horizontal()
            .auto_shrink([true, true])
            .max_width(ui.available_width() - 32.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for tab in session.tabs() {
                        let active = session.active_id() == Some(tab.id());
                        ui.group(|ui| {
                            if tab.is_loading() {
                                ui.add(Spinner::new().size(12.0));
                            }
                            let title = truncate(tab.display_title(), TITLE_CHARS);
                            let label = ui
                                .selectable_label(active, title)
                                .interact(Sense::click())
                                .on_hover_text(tab.display_title());
                            if label.clicked() {
                                commands.push(Command::Activate(tab.id()));
                            }
                            if label.middle_clicked()
                                || ui
                                    .add(Button::new(icon::X).frame(false).small())
                                    .on_hover_text("Close tab (Ctrl+W)")
                                    .clicked()
                            {
                                commands.push(Command::CloseTab(tab.id()));
                            }
                        });
                    }
                });
            });
        if ui
            .add(Button::new(icon::PLUS).frame(false))
            .on_hover_text("New tab (Ctrl+T)")
            .clicked()
        {
            commands.push(Command::NewTab);
        }
    });
}

pub fn truncate(title: &str, max: usize) -> String {
    if title.chars().count() <= max {
        return title.to_owned();
    }
    let mut short: String = title.chars().take(max - 1).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_titles_are_cut() {
        assert_eq!(truncate("short", TITLE_CHARS), "short");
        let long = "a".repeat(40);
        assert_eq!(truncate(&long, TITLE_CHARS).chars().count(), TITLE_CHARS);
        assert!(truncate(&long, TITLE_CHARS).ends_with('…'));
    }
}
