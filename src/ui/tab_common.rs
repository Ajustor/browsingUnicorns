//! What the tab strip at the top and the column on the left share: a
//! tab's colours and menus, a group's menu and name field, dragging tabs,
//! the window's own buttons.

use egui::{Color32, RichText, Sense};
use egui_phosphor::regular as icon;

use domain::{GroupId, Session, Tab, TabGroup, TabPosition};

use super::command::Command;

/// How far under the tab strip a tab must be dropped to get its own window
/// (outside the window works too, but a maximized one has no outside).
const DETACH_BELOW: f32 = 90.0;

/// Room left of the tabs for macOS's window buttons, drawn over the strip.
pub(super) const MACOS_BUTTONS: f32 = 72.0;

/// Whether `tab` shows beside the active tab, in its split view.
pub(super) fn is_split_partner(session: &Session, tab: &Tab) -> bool {
    session.active_id().and_then(|a| session.split_partner(a)) == Some(tab.id())
}

/// A tab of the active split view: a cyan line along its bottom.
pub(super) fn split_mark(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter().hline(
        rect.x_range().shrink(8.0),
        rect.bottom() - 1.5,
        egui::Stroke::new(2.0_f32, super::theme::CYAN),
    );
}

/// A tab's text: [`tab_text`], fainter for a tab asleep.
pub(super) fn tab_ink(tab: &Tab, active: bool) -> Color32 {
    let text = tab_text(active);
    if tab.is_discarded() && !active {
        text.gamma_multiply(0.55)
    } else {
        text
    }
}

fn tab_text(active: bool) -> Color32 {
    if active {
        Color32::WHITE
    } else {
        super::theme::TAB_TEXT
    }
}

/// A tab's menu (right click): pin, groups, where the tabs are, close.
pub(super) fn tab_menu(
    ui: &mut egui::Ui,
    tab: &Tab,
    session: &Session,
    position: TabPosition,
    commands: &mut Vec<Command>,
) {
    let item = |ui: &mut egui::Ui,
                glyph: &str,
                text: &str,
                command: Command,
                commands: &mut Vec<Command>| {
        if ui.button(format!("{glyph}  {text}")).clicked() {
            commands.push(command);
            ui.close_menu();
        }
    };
    if tab.is_pinned() {
        item(
            ui,
            icon::PUSH_PIN_SLASH,
            i18n::tabs::unpin_tab(),
            Command::SetPinned(tab.id(), false),
            commands,
        );
    } else {
        item(
            ui,
            icon::PUSH_PIN,
            i18n::tabs::pin_tab(),
            Command::SetPinned(tab.id(), true),
            commands,
        );
    }
    ui.separator();
    item(
        ui,
        icon::PLUS_SQUARE,
        i18n::tabs::add_to_new_group(),
        Command::NewGroup(tab.id()),
        commands,
    );
    let others: Vec<&TabGroup> = session
        .groups()
        .iter()
        .filter(|g| Some(g.id()) != tab.group())
        .collect();
    if !others.is_empty() {
        ui.menu_button(
            format!("{}  {}", icon::SQUARES_FOUR, i18n::tabs::add_to_group()),
            |ui| {
                for group in others {
                    // A dot of its colour, then its name.
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        icon::CIRCLE,
                        0.0,
                        egui::TextFormat::simple(
                            egui::FontId::proportional(13.0),
                            super::theme::group_color(group.color),
                        ),
                    );
                    job.append(
                        &format!("  {}", group_title(session, group)),
                        0.0,
                        egui::TextFormat::simple(
                            egui::FontId::proportional(14.0),
                            ui.visuals().text_color(),
                        ),
                    );
                    if ui.button(job).clicked() {
                        commands.push(Command::AddToGroup(tab.id(), group.id()));
                        ui.close_menu();
                    }
                }
            },
        );
    }
    if tab.group().is_some() {
        item(
            ui,
            icon::SQUARE_SPLIT_HORIZONTAL,
            i18n::tabs::remove_from_group(),
            Command::RemoveFromGroup(tab.id()),
            commands,
        );
    }
    ui.separator();
    // Split view: end it, or start one.
    let active = session.active_id();
    if session.split_of(tab.id()).is_some() {
        item(
            ui,
            icon::RECTANGLE,
            i18n::tabs::unsplit(),
            Command::Unsplit(tab.id()),
            commands,
        );
    } else if active == Some(tab.id()) {
        item(
            ui,
            icon::SQUARE_SPLIT_HORIZONTAL,
            i18n::tabs::split_with_new_tab(),
            Command::SplitWithNewTab,
            commands,
        );
    } else if let Some(active) = active
        && session.split_of(active).is_none()
    {
        item(
            ui,
            icon::SQUARE_SPLIT_HORIZONTAL,
            i18n::tabs::split_with_active(),
            Command::SplitWithActive(tab.id()),
            commands,
        );
    }
    ui.separator();
    match position {
        TabPosition::Top => item(
            ui,
            icon::SIDEBAR_SIMPLE,
            i18n::tabs::tabs_on_left(),
            Command::SetTabPosition(TabPosition::Left),
            commands,
        ),
        TabPosition::Left => item(
            ui,
            icon::TABS,
            i18n::tabs::tabs_on_top(),
            Command::SetTabPosition(TabPosition::Top),
            commands,
        ),
    }
    ui.separator();
    item(
        ui,
        icon::X,
        i18n::tabs::close(),
        Command::CloseTab(tab.id()),
        commands,
    );
}

/// A group's name, else what stands for it in the menus.
pub(super) fn group_title(session: &Session, group: &TabGroup) -> String {
    if group.name.is_empty() {
        i18n::tabs::unnamed_group(session.group_tabs(group.id()).len())
    } else {
        group.name.clone()
    }
}

/// A group's menu: its name, its colour, ungroup, close.
pub(super) fn group_menu(ui: &mut egui::Ui, group: &TabGroup, commands: &mut Vec<Command>) {
    let mut name = group.name.clone();
    let field = ui.add(
        egui::TextEdit::singleline(&mut name)
            .desired_width(180.0)
            .hint_text(i18n::tabs::group_name()),
    );
    if field.changed() {
        commands.push(Command::RenameGroup(group.id(), name));
    }
    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        ui.close_menu();
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for color in domain::GroupColor::ALL {
            let (rect, swatch) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), Sense::click());
            let fill = super::theme::group_color(color);
            ui.painter().circle_filled(rect.center(), 8.0, fill);
            if color == group.color {
                ui.painter().circle_stroke(
                    rect.center(),
                    9.5,
                    egui::Stroke::new(2.0_f32, ui.visuals().strong_text_color()),
                );
            }
            if swatch.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if swatch.clicked() {
                commands.push(Command::SetGroupColor(group.id(), color));
            }
        }
    });
    ui.separator();
    if ui
        .button(format!(
            "{}  {}",
            icon::SQUARE_SPLIT_HORIZONTAL,
            i18n::tabs::ungroup()
        ))
        .clicked()
    {
        commands.push(Command::Ungroup(group.id()));
        ui.close_menu();
    }
    if ui
        .button(format!("{}  {}", icon::X, i18n::tabs::close_group()))
        .clicked()
    {
        commands.push(Command::CloseGroup(group.id()));
        ui.close_menu();
    }
}

/// `rect`, sensed under what is drawn over it: dragged, it moves the
/// window; double-clicked, it maximizes or restores it.
pub(super) fn drag_window_by(ui: &egui::Ui, rect: egui::Rect) {
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
pub(super) fn window_buttons(ui: &mut egui::Ui) {
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
    if button(icon::X, i18n::common::close(), true) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let (glyph, tip) = if maximized {
        (icon::COPY_SIMPLE, i18n::window::restore())
    } else {
        (icon::SQUARE, i18n::window::maximize())
    };
    if button(glyph, tip, false) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if button(icon::MINUS, i18n::window::minimize(), false) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

/// How dragged tabs are laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Axis {
    /// The strip at the top: left to right.
    Row,
    /// The column on the left: top to bottom.
    Column,
    /// The column's pinned tabs: rows of icons.
    Grid,
}

impl Axis {
    /// Whether `r` comes before `pointer`.
    fn before(self, r: &egui::Rect, pointer: egui::Pos2) -> bool {
        match self {
            Self::Row => r.center().x < pointer.x,
            Self::Column => r.center().y < pointer.y,
            Self::Grid => {
                r.bottom() <= pointer.y || (r.top() <= pointer.y && r.center().x < pointer.x)
            }
        }
    }
}

/// A tab being dragged: a bar where it would land; dropped, it moves there.
/// Dropped out of the window, or well away from the tabs, it gets its own.
#[allow(clippy::too_many_arguments)]
pub(super) fn drag_tab(
    ui: &egui::Ui,
    rects: &[egui::Rect],
    labels: &[(GroupId, egui::Rect)],
    from: usize,
    tab: &Tab,
    dropped: bool,
    zone: std::ops::RangeInclusive<usize>,
    axis: Axis,
    commands: &mut Vec<Command>,
) {
    let ctx = ui.ctx();
    let id = tab.id();
    let Some(pointer) = ctx.pointer_latest_pos() else {
        return;
    };
    // Over another group's label: into that group (its label shows it).
    if let Some(&(group, _)) = labels
        .iter()
        .find(|(g, r)| Some(*g) != tab.group() && r.expand(4.0).contains(pointer))
    {
        if dropped {
            commands.push(Command::AddToGroup(id, group));
        } else {
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        return;
    }
    if detaches(ctx.screen_rect(), rects[from], pointer, axis) {
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
    let (slot, to) = drop_slot(rects, from, |r| axis.before(r, pointer), zone.clone());
    if to == from {
        return;
    }
    if dropped {
        commands.push(Command::MoveTab(id, to));
        return;
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let stroke = egui::Stroke::new(3.0_f32, super::theme::CYAN);
    // The bar goes before the tab of the slot, else after the zone's last.
    let last = rects[(*zone.end()).clamp(1, rects.len()) - 1];
    let next = rects.get(slot).filter(|_| slot < *zone.end());
    if axis == Axis::Column {
        let y = next.map_or(last.bottom() + 1.5, |r| r.top() - 1.5);
        ui.painter().hline(rects[from].x_range(), y, stroke);
    } else {
        let (x, y) = next.map_or((last.right() + 1.5, last.y_range()), |r| {
            (r.left() - 1.5, r.y_range())
        });
        ui.painter().vline(x, y, stroke);
    }
}

/// Whether a tab whose place is `tab`, dropped at `pointer`, leaves the
/// window `window`: out of it, or far from the tabs (under the strip,
/// right of the column).
fn detaches(window: egui::Rect, tab: egui::Rect, pointer: egui::Pos2, axis: Axis) -> bool {
    !window.contains(pointer)
        || match axis {
            Axis::Row => pointer.y > tab.bottom() + DETACH_BELOW,
            Axis::Column | Axis::Grid => pointer.x > tab.right() + DETACH_BELOW,
        }
}

/// The new window of a dragged tab, as big as this one, its tab strip
/// under the pointer: x, y, width, height on the screen, in points.
pub(super) fn new_window_at(ctx: &egui::Context, pointer: egui::Pos2) -> [f32; 4] {
    let window = ctx
        .input(|i| i.viewport().inner_rect)
        .unwrap_or(ctx.screen_rect());
    let at = window.min + pointer.to_vec2() - egui::vec2(90.0, 18.0);
    [at.x, at.y, window.width(), window.height()]
}

/// Where a tab dragged from index `from` goes: the gap it is over (after
/// the tabs `before` the pointer, kept in `zone`), and its index once moved
/// there.
fn drop_slot(
    rects: &[egui::Rect],
    from: usize,
    before: impl Fn(&egui::Rect) -> bool,
    zone: std::ops::RangeInclusive<usize>,
) -> (usize, usize) {
    let slot = rects
        .iter()
        .filter(|r| before(r))
        .count()
        .clamp(*zone.start(), *zone.end());
    let to = if slot > from { slot - 1 } else { slot };
    (slot, to)
}

/// "Private", with the mask, before the tabs of a private window.
pub(super) fn private_pill(ui: &mut egui::Ui, chrome: &super::theme::Chrome) {
    egui::Frame::NONE
        .fill(chrome.night)
        .corner_radius(egui::CornerRadius::same(255))
        .inner_margin(egui::Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "{} {}",
                        icon::MASK_HAPPY,
                        i18n::tabs::private_pill()
                    ))
                    .color(super::theme::CHROME_TEXT),
                )
                .selectable(false),
            )
            .on_hover_text(i18n::tabs::private_window());
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

/// Whether the active tab changed since the last frame (it is then scrolled
/// into view, once: the user may scroll away from it).
pub(super) fn newly_active(ui: &egui::Ui, session: &Session) -> bool {
    let shown = ui.id().with("active tab");
    ui.data_mut(|d| {
        let changed = d.get_temp::<Option<domain::TabId>>(shown) != Some(session.active_id());
        d.insert_temp(shown, session.active_id());
        changed
    })
}

/// A new group's name, typed in a pill of its `color`, `margin_y` above and
/// under it, `width` wide (else as wide as there is room). Returns where it
/// is: a tab dropped there joins the group.
pub(super) fn name_field(
    ui: &mut egui::Ui,
    group: &TabGroup,
    color: Color32,
    margin_y: i8,
    width: Option<f32>,
    naming: &mut Option<GroupId>,
    commands: &mut Vec<Command>,
) -> egui::Rect {
    let buffer = ui.id().with(("naming", group.id()));
    let started = ui.data(|d| d.get_temp::<String>(buffer));
    let mut name = started.clone().unwrap_or_else(|| group.name.clone());
    let named = egui::Frame::NONE
        .fill(color)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(6, margin_y))
        .show(ui, |ui| {
            let ink = super::theme::NIGHT_DEEP;
            let style = ui.visuals_mut();
            style.override_text_color = Some(ink);
            style.extreme_bg_color = Color32::TRANSPARENT;
            style.widgets.inactive.bg_stroke = egui::Stroke::NONE;
            style.widgets.hovered.bg_stroke = egui::Stroke::NONE;
            style.widgets.active.bg_stroke = egui::Stroke::NONE;
            style.selection.stroke = egui::Stroke::NONE;
            let width = width.unwrap_or_else(|| ui.available_width());
            ui.add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(width)
                    .frame(false)
                    .hint_text(
                        RichText::new(i18n::tabs::group_name()).color(ink.gamma_multiply(0.6)),
                    ),
            )
        });
    let field = named.inner;
    if started.is_none() {
        field.request_focus();
    }
    if field.lost_focus() {
        commands.push(Command::RenameGroup(group.id(), name));
        ui.data_mut(|d| d.remove::<String>(buffer));
        *naming = None;
    } else {
        ui.data_mut(|d| d.insert_temp(buffer, name));
    }
    named.response.rect
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
        let at = |x: f32| move |r: &egui::Rect| Axis::Row.before(r, egui::pos2(x, 15.0));
        let all = 0..=3;
        // Tab 0 over the second half of tab 2: after it.
        assert_eq!(drop_slot(&rects, 0, at(280.0), all.clone()), (3, 2));
        // Tab 2 over the first half of tab 0: before it.
        assert_eq!(drop_slot(&rects, 2, at(20.0), all.clone()), (0, 0));
        // Over itself: it stays.
        assert_eq!(drop_slot(&rects, 1, at(120.0), all.clone()).1, 1);
        assert_eq!(drop_slot(&rects, 1, at(180.0), all).1, 1);
        // Tab 0 pinned, the others not: they stay apart.
        assert_eq!(drop_slot(&rects, 0, at(280.0), 0..=1), (1, 0));
        assert_eq!(drop_slot(&rects, 2, at(20.0), 1..=3), (1, 1));
    }

    #[test]
    fn in_the_column_a_dragged_tab_lands_by_height() {
        let rects: Vec<egui::Rect> = (0..3)
            .map(|i| {
                let top = i as f32 * 30.0;
                egui::Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(200.0, top + 30.0))
            })
            .collect();
        // Whatever the x: the column's rows are full width.
        let at = |y: f32| move |r: &egui::Rect| Axis::Column.before(r, egui::pos2(190.0, y));
        assert_eq!(drop_slot(&rects, 0, at(80.0), 0..=3), (3, 2));
        assert_eq!(drop_slot(&rects, 2, at(5.0), 0..=3), (0, 0));
        assert_eq!(drop_slot(&rects, 1, at(40.0), 0..=3).1, 1);
    }

    #[test]
    fn pinned_icons_in_rows_are_read_left_to_right_then_down() {
        // Two rows of two icons.
        let icon = |col: f32, row: f32| {
            egui::Rect::from_min_size(egui::pos2(col * 40.0, row * 40.0), egui::vec2(36.0, 36.0))
        };
        let rects = [
            icon(0.0, 0.0),
            icon(1.0, 0.0),
            icon(0.0, 1.0),
            icon(1.0, 1.0),
        ];
        let at = |x: f32, y: f32| move |r: &egui::Rect| Axis::Grid.before(r, egui::pos2(x, y));
        // Second row, right of its first icon: before the last one.
        assert_eq!(drop_slot(&rects, 0, at(30.0, 50.0), 0..=4), (3, 2));
        // First row, left: first.
        assert_eq!(drop_slot(&rects, 3, at(5.0, 10.0), 0..=4), (0, 0));
    }

    #[test]
    fn a_tab_dropped_out_of_the_window_or_far_under_the_strip_detaches() {
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let tab = egui::Rect::from_min_max(egui::pos2(10.0, 6.0), egui::pos2(150.0, 36.0));
        let row = |p| detaches(window, tab, p, Axis::Row);
        assert!(!row(egui::pos2(300.0, 20.0)));
        assert!(!row(egui::pos2(300.0, 100.0)));
        assert!(row(egui::pos2(300.0, 200.0)));
        assert!(row(egui::pos2(900.0, 20.0)));
        assert!(row(egui::pos2(300.0, -5.0)));
        // In the column: far to its right.
        let column = |p| detaches(window, tab, p, Axis::Column);
        assert!(!column(egui::pos2(100.0, 500.0)));
        assert!(!column(egui::pos2(200.0, 20.0)));
        assert!(column(egui::pos2(300.0, 20.0)));
    }
}
