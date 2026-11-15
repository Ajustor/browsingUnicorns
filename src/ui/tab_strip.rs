use egui::{Button, ScrollArea, Sense, Spinner};
use egui_phosphor::regular as icon;

use domain::Session;

use super::command::Command;

const TITLE_CHARS: usize = 24;

pub fn show(ui: &mut egui::Ui, session: &Session, commands: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        ScrollArea::horizontal()
            .auto_shrink([false, true])
            .max_width(ui.available_width() - 32.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for tab in session.tabs() {
                        let active = session.active_id() == Some(tab.id());
                        ui.group(|ui| {
                            if tab.is_loading() {
                                ui.add(Spinner::new().size(12.0));
                            }
                            let title = truncate(tab.display_title());
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

fn truncate(title: &str) -> String {
    if title.chars().count() <= TITLE_CHARS {
        return title.to_owned();
    }
    let mut short: String = title.chars().take(TITLE_CHARS - 1).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_titles_are_cut() {
        assert_eq!(truncate("short"), "short");
        let long = "a".repeat(40);
        assert_eq!(truncate(&long).chars().count(), TITLE_CHARS);
        assert!(truncate(&long).ends_with('…'));
    }
}
