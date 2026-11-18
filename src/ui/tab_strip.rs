use egui::{Button, Color32, RichText, ScrollArea, Sense, Spinner};
use egui_phosphor::regular as icon;

use domain::Session;

use super::command::Command;

const TITLE_CHARS: usize = 24;

/// How far under the tab strip a tab must be dropped to get its own window
/// (outside the window works too, but a maximized one has no outside).
const DETACH_BELOW: f32 = 90.0;

/// Room left of the tabs for macOS's window buttons, drawn over the strip.
const MACOS_BUTTONS: f32 = 72.0;

/// Width of the window's buttons (Windows, Linux).
const WINDOW_BUTTONS: f32 = 3.0 * 46.0;

/// Empty space above the tabs: room to grab the window by.
pub const TOP_MARGIN: i8 = 12;

/// The strip's whole area, up to the window's top edge (its panel has
/// margins of 8 on the sides).
pub fn area(ui: &egui::Ui) -> egui::Rect {
    let inner = ui.max_rect();
    egui::Rect::from_min_max(
        egui::pos2(inner.left() - 8.0, 0.0),
        egui::pos2(inner.right() + 8.0, inner.bottom() + 6.0),
    )
}

/// The tab strip, which is also the window's title bar: its empty space
/// moves the window (double click: maximize), and it ends with the window's
/// buttons (but on macOS, where the system draws them on its left).
pub fn show(
    ui: &mut egui::Ui,
    session: &Session,
    favicons: &super::favicons::Favicons,
    chrome: &super::theme::Chrome,
    private: bool,
    commands: &mut Vec<Command>,
) {
    title_bar(ui);
    ui.spacing_mut().item_spacing.x = 3.0;
    ui.horizontal(|ui| {
        if cfg!(target_os = "macos") {
            ui.add_space(MACOS_BUTTONS);
        }
        if private {
            private_pill(ui, chrome);
        }
        let controls = if cfg!(target_os = "macos") {
            0.0
        } else {
            WINDOW_BUTTONS
        };
        // As wide as the tabs: the + follows the last one (room kept for it
        // when they overflow). No scroll bar, which got in the way of the
        // tabs and of the window's dragging: the wheel scrolls the tabs, and
        // the active one is scrolled into view.
        ui.style_mut().always_scroll_the_only_direction = true;
        let scroll = ScrollArea::horizontal()
            .auto_shrink([true, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_width(ui.available_width() - 32.0 - controls)
            .drag_to_scroll(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut rects = Vec::with_capacity(session.len());
                    let mut dragged = None;
                    // A newly active tab (a new one, Ctrl+Tab...) is scrolled
                    // into view, once: the user may scroll away from it.
                    let shown = ui.id().with("active tab");
                    let newly_active = ui.data_mut(|d| {
                        let changed =
                            d.get_temp::<Option<domain::TabId>>(shown) != Some(session.active_id());
                        d.insert_temp(shown, session.active_id());
                        changed
                    });
                    for (index, tab) in session.tabs().iter().enumerate() {
                        let active = session.active_id() == Some(tab.id());
                        let icon = favicons.get(tab.id());
                        let response = tab_button(ui, tab, icon, active, chrome, commands);
                        if active && newly_active {
                            response.scroll_to_me(None);
                        }
                        if response.dragged() || response.drag_stopped() {
                            dragged = Some((index, tab.id(), response.drag_stopped()));
                        }
                        rects.push(response.rect);
                    }
                    if let Some((from, id, dropped)) = dragged {
                        drag_tab(ui, &rects, from, id, dropped, commands);
                    }
                })
                .response
                .rect
            });
        overflow_hints(ui, &scroll);
        let plus = super::theme::hover_fill(ui, |ui| {
            ui.add(
                Button::new(RichText::new(icon::PLUS).color(Color32::WHITE).size(16.0))
                    .frame(false),
            )
        })
        .on_hover_text(
            "New tab (Ctrl+T)
Right-click: reopen the last tab closed (Ctrl+Shift+T)",
        );
        if plus.clicked() {
            commands.push(Command::NewTab);
        }
        if plus.secondary_clicked() {
            commands.push(Command::ReopenClosedTab);
        }
        if !cfg!(target_os = "macos") {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                window_buttons(ui);
            });
        }
    });

    fn tab_button(
        ui: &mut egui::Ui,
        tab: &domain::Tab,
        icon: Option<&egui::TextureHandle>,
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
                            } else if let Some(icon) = icon {
                                ui.add(
                                    egui::Image::new(icon)
                                        .fit_to_exact_size(egui::vec2(14.0, 14.0)),
                                );
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
                                    let close = super::theme::hover_fill(ui, |ui| {
                                        ui.add(
                                            Button::new(
                                                RichText::new(icon::X)
                                                    .color(text.gamma_multiply(0.6))
                                                    .small(),
                                            )
                                            .frame(false),
                                        )
                                    })
                                    .on_hover_text("Close tab (Ctrl+W)");
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
        // Over the title or the close button too, which sit on top of it.
        if !active && response.contains_pointer() {
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

/// Tabs hidden on a side (the strip has no scroll bar): a shadow there, and
/// a caret pointing to them.
fn overflow_hints(ui: &egui::Ui, scroll: &egui::scroll_area::ScrollAreaOutput<egui::Rect>) {
    const SHADE: f32 = 44.0;
    // As tall as the tabs (the scroll area's own rect is shorter).
    let rect = egui::Rect::from_x_y_ranges(scroll.inner_rect.x_range(), scroll.inner.y_range());
    let offset = scroll.state.offset.x;
    let hidden_right = scroll.content_size.x - rect.width() - offset;
    let painter = ui.painter_at(rect);
    for (hidden, left) in [(offset > 1.0, true), (hidden_right > 1.0, false)] {
        if !hidden {
            continue;
        }
        let (edge, inner) = if left {
            (rect.left(), rect.left() + SHADE)
        } else {
            (rect.right(), rect.right() - SHADE)
        };
        let dark = Color32::from_black_alpha(200);
        let mut mesh = egui::Mesh::default();
        for (x, color) in [(edge, dark), (inner, Color32::TRANSPARENT)] {
            mesh.colored_vertex(egui::pos2(x, rect.top()), color);
            mesh.colored_vertex(egui::pos2(x, rect.bottom()), color);
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(2, 1, 3);
        painter.add(mesh);
        let (glyph, anchor, x) = if left {
            (icon::CARET_LEFT, egui::Align2::LEFT_CENTER, edge + 2.0)
        } else {
            (icon::CARET_RIGHT, egui::Align2::RIGHT_CENTER, edge - 2.0)
        };
        painter.text(
            egui::pos2(x, rect.center().y),
            anchor,
            glyph,
            egui::FontId::proportional(14.0),
            Color32::WHITE,
        );
    }
}

/// The strip's empty space, sensed under the tabs and buttons: dragged, it
/// moves the window; double-clicked, it maximizes or restores it.
fn title_bar(ui: &egui::Ui) {
    let rect = area(ui);
    let bar = ui.interact(rect, ui.id().with("title bar"), Sense::click_and_drag());
    if bar.double_clicked() {
        let maximized = ui.ctx().input(|i| i.viewport().maximized == Some(true));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if bar.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// Close, maximize or restore, minimize (right to left), as tall as the
/// strip.
fn window_buttons(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized == Some(true));
    ui.spacing_mut().item_spacing.x = 0.0;
    let height = ui.available_height().max(30.0);
    let mut button = |glyph: &str, tip: &str, danger: bool| {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(46.0, height), Sense::click());
        // Up to the window's top edge.
        let rect = egui::Rect::from_min_max(egui::pos2(rect.left(), 0.0), rect.max);
        if response.hovered() {
            let fill = if danger {
                Color32::from_rgb(0xc4, 0x2b, 0x1c)
            } else {
                Color32::from_white_alpha(36)
            };
            ui.painter().rect_filled(rect, 0.0, fill);
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            glyph,
            egui::FontId::proportional(15.0),
            Color32::WHITE,
        );
        response.on_hover_text(tip).clicked()
    };
    if button(icon::X, "Close", true) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let (glyph, tip) = if maximized {
        (icon::COPY_SIMPLE, "Restore")
    } else {
        (icon::SQUARE, "Maximize")
    };
    if button(glyph, tip, false) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if button(icon::MINUS, "Minimize", false) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

/// A tab being dragged: a bar where it would land; dropped, it moves there.
/// Dropped out of the window, or well under the strip, it gets its own.
fn drag_tab(
    ui: &egui::Ui,
    rects: &[egui::Rect],
    from: usize,
    id: domain::TabId,
    dropped: bool,
    commands: &mut Vec<Command>,
) {
    let ctx = ui.ctx();
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    if detaches(ctx.screen_rect(), rects[from], pointer) {
        if rects.len() < 2 {
            return;
        }
        if dropped {
            commands.push(Command::DetachTab(id, new_window_at(ctx, pointer)));
        } else {
            ctx.set_cursor_icon(egui::CursorIcon::Move);
        }
        return;
    }
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

/// Whether a tab whose place in the strip is `tab`, dropped at `pointer`,
/// leaves the window `window`.
fn detaches(window: egui::Rect, tab: egui::Rect, pointer: egui::Pos2) -> bool {
    !window.contains(pointer) || pointer.y > tab.bottom() + DETACH_BELOW
}

/// The new window of a dragged tab, as big as this one, its tab strip
/// under the pointer: x, y, width, height on the screen, in points.
fn new_window_at(ctx: &egui::Context, pointer: egui::Pos2) -> [f32; 4] {
    let window = ctx
        .input(|i| i.viewport().inner_rect)
        .unwrap_or(ctx.screen_rect());
    let at = window.min + pointer.to_vec2() - egui::vec2(90.0, 18.0);
    [at.x, at.y, window.width(), window.height()]
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
    fn a_tab_dropped_out_of_the_window_or_far_under_the_strip_detaches() {
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let tab = egui::Rect::from_min_max(egui::pos2(10.0, 6.0), egui::pos2(150.0, 36.0));
        assert!(!detaches(window, tab, egui::pos2(300.0, 20.0)));
        assert!(!detaches(window, tab, egui::pos2(300.0, 100.0)));
        assert!(detaches(window, tab, egui::pos2(300.0, 200.0)));
        assert!(detaches(window, tab, egui::pos2(900.0, 20.0)));
        assert!(detaches(window, tab, egui::pos2(300.0, -5.0)));
    }

    #[test]
    fn long_titles_are_cut() {
        assert_eq!(truncate("short", TITLE_CHARS), "short");
        let long = "a".repeat(40);
        assert_eq!(truncate(&long, TITLE_CHARS).chars().count(), TITLE_CHARS);
        assert!(truncate(&long, TITLE_CHARS).ends_with('…'));
    }
}
