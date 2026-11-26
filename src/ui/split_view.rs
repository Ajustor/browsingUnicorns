//! The page area of a split view: the two tabs' pages side by side, a bar
//! between them that sets their widths, a line around the active one.

use domain::{Session, TabId};

use super::command::Command;

/// The bar between the two pages (points).
const GAP: f32 = 6.0;
/// The line around the active page, in the room left around each page.
const BORDER: f32 = 2.0;

/// Where `rect` puts the two pages of a split whose first page takes
/// `ratio` per thousand of the width, and the bar between them.
fn layout(rect: egui::Rect, ratio: u16) -> (egui::Rect, egui::Rect, egui::Rect) {
    let room = (rect.width() - GAP).max(0.0);
    let left_width = (room * f32::from(ratio) / 1000.0).round();
    let left = egui::Rect::from_min_size(rect.min, egui::vec2(left_width, rect.height()));
    let bar = egui::Rect::from_min_size(
        egui::pos2(left.right(), rect.top()),
        egui::vec2(GAP, rect.height()),
    );
    let right = egui::Rect::from_min_max(egui::pos2(bar.right(), rect.top()), rect.max);
    (left, bar, right)
}

/// The ratio (per thousand) that puts the bar under `x`, in `rect`.
fn ratio_at(rect: egui::Rect, x: f32) -> u16 {
    let room = (rect.width() - GAP).max(1.0);
    let (min, _, max) = domain::SPLIT_RATIO;
    (((x - GAP / 2.0 - rect.left()) / room * 1000.0).round() as i32)
        .clamp(i32::from(min), i32::from(max)) as u16
}

/// The pages to show in `rect`, each with its area: the active tab's alone,
/// or the two of its split (the bar between them dragged here, the active
/// one outlined).
pub fn panes(
    ui: &egui::Ui,
    rect: egui::Rect,
    session: &Session,
    commands: &mut Vec<Command>,
) -> Vec<(TabId, egui::Rect)> {
    let Some(active) = session.active_id() else {
        return Vec::new();
    };
    let Some(split) = session.split_of(active).copied() else {
        return vec![(active, rect)];
    };
    let (left, bar, right) = layout(rect, split.ratio);
    let response = ui
        .interact(
            bar,
            ui.id().with("split bar"),
            egui::Sense::click_and_drag(),
        )
        .on_hover_text(i18n::tabs::split_divider());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if response.double_clicked() {
        commands.push(Command::SetSplitRatio(active, domain::SPLIT_RATIO.1));
    } else if response.dragged()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let ratio = ratio_at(rect, pointer.x);
        if ratio != split.ratio {
            commands.push(Command::SetSplitRatio(active, ratio));
        }
    }
    let painter = ui.painter();
    let grip = if response.hovered() || response.dragged() {
        super::theme::CYAN
    } else {
        egui::Color32::from_white_alpha(70)
    };
    painter.vline(
        bar.center().x,
        egui::Rangef::new(bar.center().y - 18.0, bar.center().y + 18.0),
        egui::Stroke::new(2.0_f32, grip),
    );
    let focused = if active == split.first { left } else { right };
    painter.rect_stroke(
        focused.shrink(BORDER / 2.0),
        4.0,
        egui::Stroke::new(BORDER, super::theme::CYAN),
        egui::StrokeKind::Middle,
    );
    vec![
        (split.first, left.shrink(BORDER)),
        (split.second, right.shrink(BORDER)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_share_the_width_less_the_bar() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 50.0), egui::vec2(1006.0, 600.0));
        let (left, bar, right) = layout(rect, 500);
        assert_eq!(left.width(), 500.0);
        assert_eq!(bar.width(), GAP);
        assert_eq!(right.width(), 500.0);
        assert_eq!(left.left(), 10.0);
        assert_eq!(right.right(), 1016.0);
        let (left, _, right) = layout(rect, 300);
        assert_eq!((left.width(), right.width()), (300.0, 700.0));
    }

    #[test]
    fn dragging_the_bar_sets_the_ratio_within_bounds() {
        let rect = egui::Rect::from_min_size(egui::pos2(10.0, 50.0), egui::vec2(1006.0, 600.0));
        // The bar's middle under the pointer.
        assert_eq!(ratio_at(rect, 10.0 + 300.0 + GAP / 2.0), 300);
        assert_eq!(ratio_at(rect, 0.0), 100);
        assert_eq!(ratio_at(rect, 2000.0), 900);
    }
}
