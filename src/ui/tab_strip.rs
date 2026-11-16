use egui::{Button, Color32, RichText, ScrollArea, Sense, Spinner};
use egui_phosphor::regular as icon;

use domain::Session;

use super::command::Command;

const TITLE_CHARS: usize = 24;

pub fn show(
    ui: &mut egui::Ui,
    session: &Session,
    chrome: &super::theme::Chrome,
    private: bool,
    commands: &mut Vec<Command>,
) {
    ui.spacing_mut().item_spacing.x = 3.0;
    ui.horizontal(|ui| {
        if private {
            private_pill(ui, chrome);
        }
        // As wide as the tabs: the + follows the last one (room kept for it
        // when they overflow).
        ScrollArea::horizontal()
            .auto_shrink([true, true])
            .max_width(ui.available_width() - 32.0)
            .drag_to_scroll(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut rects = Vec::with_capacity(session.len());
                    let mut dragged = None;
                    for (index, tab) in session.tabs().iter().enumerate() {
                        let active = session.active_id() == Some(tab.id());
                        let response = tab_button(ui, tab, active, chrome, commands);
                        if response.dragged() || response.drag_stopped() {
                            dragged = Some((index, tab.id(), response.drag_stopped()));
                        }
                        rects.push(response.rect);
                    }
                    if let Some((from, id, dropped)) = dragged {
                        drag_tab(ui, &rects, from, id, dropped, commands);
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

    fn tab_button(
        ui: &mut egui::Ui,
        tab: &domain::Tab,
        active: bool,
        chrome: &super::theme::Chrome,
        commands: &mut Vec<Command>,
    ) -> egui::Response {
        let (fill, text) = if active {
            (chrome.night, Color32::WHITE)
        } else {
            (Color32::TRANSPARENT, super::theme::TAB_TEXT)
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
        // The whole tab is clickable, and dragged to move it; sensed before
        // its content, so the title and the close button inside it get
        // their own clicks.
        let response = ui
            .scope_builder(
                egui::UiBuilder::new().sense(Sense::click_and_drag()),
                |ui| {
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
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
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
                                },
                            );
                        });
                    })
                },
            )
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
        response
    }
}

/// A tab being dragged: a bar where it would land; dropped, it moves there.
fn drag_tab(
    ui: &egui::Ui,
    rects: &[egui::Rect],
    from: usize,
    id: domain::TabId,
    dropped: bool,
    commands: &mut Vec<Command>,
) {
    let Some(pointer) = ui.ctx().pointer_latest_pos() else {
        return;
    };
    let (slot, to) = drop_slot(rects, from, pointer.x);
    if to == from {
        return;
    }
    if dropped {
        commands.push(Command::MoveTab(id, to));
        return;
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let x = match rects.get(slot) {
        Some(r) => r.left() - 1.5,
        None => rects[rects.len() - 1].right() + 1.5,
    };
    let r = rects[from];
    ui.painter().vline(
        x,
        r.y_range(),
        egui::Stroke::new(3.0_f32, super::theme::CYAN),
    );
}

/// Where a tab dragged from index `from` to `x` goes: the gap it is over
/// (before tab `slot`), and its index once moved there.
fn drop_slot(rects: &[egui::Rect], from: usize, x: f32) -> (usize, usize) {
    let slot = rects.iter().filter(|r| r.center().x < x).count();
    let to = if slot > from { slot - 1 } else { slot };
    (slot, to)
}

/// "Private", with the mask, before the tabs of a private window.
fn private_pill(ui: &mut egui::Ui, chrome: &super::theme::Chrome) {
    egui::Frame::NONE
        .fill(chrome.night)
        .corner_radius(egui::CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("{} Private", icon::MASK_HAPPY))
                        .color(super::theme::CHROME_TEXT),
                )
                .selectable(false),
            )
            .on_hover_text("Private window: no history, cookies deleted at closing");
        });
    ui.add_space(4.0);
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
    fn a_dragged_tab_lands_in_the_gap_under_the_pointer() {
        let rects: Vec<egui::Rect> = (0..3)
            .map(|i| {
                let left = i as f32 * 100.0;
                egui::Rect::from_min_max(egui::pos2(left, 0.0), egui::pos2(left + 100.0, 30.0))
            })
            .collect();
        // Tab 0 over the second half of tab 2: after it.
        assert_eq!(drop_slot(&rects, 0, 280.0), (3, 2));
        // Tab 2 over the first half of tab 0: before it.
        assert_eq!(drop_slot(&rects, 2, 20.0), (0, 0));
        // Over itself: it stays.
        assert_eq!(drop_slot(&rects, 1, 120.0).1, 1);
        assert_eq!(drop_slot(&rects, 1, 180.0).1, 1);
    }

    #[test]
    fn long_titles_are_cut() {
        assert_eq!(truncate("short", TITLE_CHARS), "short");
        let long = "a".repeat(40);
        assert_eq!(truncate(&long, TITLE_CHARS).chars().count(), TITLE_CHARS);
        assert!(truncate(&long, TITLE_CHARS).ends_with('…'));
    }
}
