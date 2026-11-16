use egui::{Button, Color32, RichText, ScrollArea, Sense, Spinner};
use egui_phosphor::regular as icon;

use domain::Session;

use super::command::Command;

const TITLE_CHARS: usize = 24;

pub fn show(ui: &mut egui::Ui, session: &Session, commands: &mut Vec<Command>) {
    use super::theme;

    ui.spacing_mut().item_spacing.x = 3.0;
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
                        tab_button(ui, tab, active, commands);
                    }
                });
            });
        let plus = ui
            .add(
                Button::new(RichText::new(icon::PLUS).color(Color32::WHITE).size(16.0))
                    .frame(false),
            )
            .on_hover_text("New tab (Ctrl+T)");
        hover_glow(ui, &plus);
        if plus.clicked() {
            commands.push(Command::NewTab);
        }
    });

    fn tab_button(ui: &mut egui::Ui, tab: &domain::Tab, active: bool, commands: &mut Vec<Command>) {
        let (fill, text) = if active {
            (theme::NIGHT, Color32::WHITE)
        } else {
            (Color32::TRANSPARENT, theme::TAB_TEXT)
        };
        let frame = egui::Frame::NONE
            .fill(fill)
            .corner_radius(egui::CornerRadius {
                nw: 10,
                ne: 10,
                sw: 0,
                se: 0,
            })
            .inner_margin(egui::Margin {
                left: 12,
                right: 8,
                top: 7,
                bottom: 7,
            });
        // The whole tab is clickable; sensed before its content, so the
        // title and the close button inside it get their own clicks.
        let response = ui
            .scope_builder(egui::UiBuilder::new().sense(Sense::click()), |ui| {
                frame.show(ui, |ui| {
                    ui.set_min_width(110.0);
                    ui.set_max_width(190.0);
                    ui.horizontal(|ui| {
                        if tab.is_loading() {
                            ui.add(Spinner::new().size(12.0).color(text));
                        }
                        let title = truncate(tab.display_title(), TITLE_CHARS);
                        let label = ui
                            .add(
                                egui::Label::new(RichText::new(title).color(text))
                                    .selectable(false)
                                    .sense(Sense::click()),
                            )
                            .on_hover_text(tab.display_title());
                        if label.clicked() {
                            commands.push(Command::Activate(tab.id()));
                        }
                        if label.middle_clicked() {
                            commands.push(Command::CloseTab(tab.id()));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let close = ui
                                .add(
                                    Button::new(
                                        RichText::new(icon::X)
                                            .color(text.gamma_multiply(0.6))
                                            .small(),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text("Close tab (Ctrl+W)");
                            hover_glow(ui, &close);
                            if close.clicked() {
                                commands.push(Command::CloseTab(tab.id()));
                            }
                        });
                    });
                })
            })
            .response;
        if !active && response.hovered() {
            ui.painter().rect_filled(
                response.rect,
                egui::CornerRadius {
                    nw: 10,
                    ne: 10,
                    sw: 0,
                    se: 0,
                },
                Color32::from_white_alpha(28),
            );
        }
        if response.clicked() {
            commands.push(Command::Activate(tab.id()));
        }
        if response.middle_clicked() {
            commands.push(Command::CloseTab(tab.id()));
        }
    }
}

/// A light disc under a hovered button: egui's own hover color (night)
/// hardly shows on the gradient.
fn hover_glow(ui: &egui::Ui, button: &egui::Response) {
    if button.hovered() {
        let r = button.rect;
        ui.painter().circle_filled(
            r.center(),
            r.width().max(r.height()) * 0.55,
            Color32::from_white_alpha(46),
        );
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
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
