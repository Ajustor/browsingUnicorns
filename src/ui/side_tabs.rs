//! The tabs in a column on the left (the other choice than the strip at
//! the top, [`super::tab_strip`]): pinned tabs as icons at the top, then
//! the tabs one per row, a group's under its header. Its right edge sets
//! its width; folded, it shows the icons only.

use egui::{Color32, RichText, ScrollArea, Sense};
use egui_phosphor::regular as icon;

use domain::{GroupId, Session, Tab, TabGroup, TabPosition};

use super::command::Command;
use super::favicons::{Favicons, Sound};
use super::panel_kit::icon_button;
use super::tab_common::{self, Axis};
use super::theme::{self, Chrome};

/// The column's panel (egui keeps its width under it).
const PANEL_ID: &str = "side tabs";
/// The folded column's width (icons only).
const FOLDED_WIDTH: f32 = 52.0;
/// A tab's row.
const ROW_HEIGHT: f32 = 32.0;
/// A pinned tab's icon, in its grid.
const PINNED_SIZE: f32 = 36.0;
/// A group's tabs, right of its colour.
const GROUP_INDENT: f32 = 10.0;

/// What the column shows, borrowed from the app for a frame.
pub struct SideTabs<'a> {
    pub session: &'a Session,
    pub favicons: &'a Favicons,
    pub chrome: &'a Chrome,
    pub private: bool,
    /// Width when unfolded (points), and whether it is folded.
    pub width: u32,
    pub folded: bool,
    /// Tabs not listed: extensions' windows, shown floating.
    pub hidden: &'a [domain::TabId],
}

/// Shows the column; returns its area (the tab search opens beside it).
pub fn show(
    ctx: &egui::Context,
    tabs: SideTabs<'_>,
    naming: &mut Option<GroupId>,
    commands: &mut Vec<Command>,
) -> egui::Rect {
    let (min, _, max) = domain::SIDE_TABS_WIDTH;
    let panel = egui::SidePanel::left(PANEL_ID)
        .frame(
            egui::Frame::NONE
                .fill(tabs.chrome.gradient[0])
                .inner_margin(egui::Margin::symmetric(6, 6)),
        )
        .show_separator_line(false);
    let panel = if tabs.folded {
        panel.resizable(false).exact_width(FOLDED_WIDTH)
    } else {
        panel
            .resizable(true)
            .default_width(tabs.width as f32)
            .width_range(min as f32..=max as f32)
    };
    let response = panel.show(ctx, |ui| {
        // As wide as the panel: egui keeps the drawn width as the panel's,
        // narrower content would shrink it (and the width saved) each run.
        ui.set_min_width(ui.available_width());
        ui.style_mut().spacing.item_spacing = egui::vec2(4.0, 2.0);
        header(ui, &tabs, commands);
        ui.add_space(6.0);
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| list(ui, &tabs, naming, commands));
    });
    let rect = response.response.rect;
    // The edge dragged and let go: its width is remembered (only then: the
    // first frames lay the panel out before the window has its size).
    let resize = ctx.read_response(egui::Id::new(PANEL_ID).with("__resize"));
    if !tabs.folded && resize.is_some_and(|r| r.drag_stopped()) {
        let width = rect.width().round() as u32;
        if width != tabs.width {
            commands.push(Command::SetSideTabsWidth(width.clamp(min, max)));
        }
    }
    rect
}

/// New tab, the tab search, fold or unfold; the private pill above.
fn header(ui: &mut egui::Ui, tabs: &SideTabs<'_>, commands: &mut Vec<Command>) {
    if tabs.private && !tabs.folded {
        ui.horizontal(|ui| tab_common::private_pill(ui, tabs.chrome));
        ui.add_space(4.0);
    }
    let fold = |ui: &mut egui::Ui, commands: &mut Vec<Command>| {
        let (glyph, tip) = if tabs.folded {
            (icon::CARET_DOUBLE_RIGHT, i18n::tabs::expand_column())
        } else {
            (icon::CARET_DOUBLE_LEFT, i18n::tabs::collapse_column())
        };
        if glyph_button(ui, glyph, tip).clicked() {
            commands.push(Command::SetSideTabsCollapsed(!tabs.folded));
        }
    };
    let new_tab = |ui: &mut egui::Ui, commands: &mut Vec<Command>| {
        let plus = glyph_button(ui, icon::PLUS, i18n::tabs::new_tab());
        if plus.clicked() {
            commands.push(Command::NewTab);
        }
        if plus.secondary_clicked() {
            commands.push(Command::ReopenClosedTab);
        }
    };
    let search = |ui: &mut egui::Ui, commands: &mut Vec<Command>| {
        if glyph_button(ui, icon::MAGNIFYING_GLASS, i18n::tabs::search_tabs()).clicked() {
            commands.push(Command::ToggleTabSearch);
        }
    };
    if tabs.folded {
        ui.vertical_centered(|ui| {
            fold(ui, commands);
            new_tab(ui, commands);
            search(ui, commands);
        });
    } else {
        ui.horizontal(|ui| {
            new_tab(ui, commands);
            search(ui, commands);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                fold(ui, commands);
            });
        });
    }
}

fn glyph_button(ui: &mut egui::Ui, glyph: &str, tip: &str) -> egui::Response {
    icon_button(
        ui,
        RichText::new(glyph).color(Color32::WHITE).size(16.0),
        tip,
    )
}

/// The pinned tabs' grid, then the others, their groups' headers among
/// them. Dragging a tab moves it (pinned among pinned).
fn list(
    ui: &mut egui::Ui,
    tabs: &SideTabs<'_>,
    naming: &mut Option<GroupId>,
    commands: &mut Vec<Command>,
) {
    let session = tabs.session;
    let pinned = session.pinned_count();
    let mut rects = Vec::with_capacity(session.len());
    let mut headers = Vec::new();
    let mut dragged = None;
    let newly_active = tab_common::newly_active(ui, session);
    let mut track = |index: usize, tab: &Tab, response: &egui::Response, rects: &mut Vec<_>| {
        if session.active_id() == Some(tab.id()) && newly_active {
            response.scroll_to_me(None);
        }
        if response.dragged() || response.drag_stopped() {
            dragged = Some((index, tab.id(), response.drag_stopped()));
        }
        rects.push(response.rect);
    };

    // Pinned: rows of icons (one per row, folded).
    if pinned > 0 {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (index, tab) in session.tabs()[..pinned].iter().enumerate() {
                let response = pinned_tab(ui, tabs, tab, commands);
                track(index, tab, &response, &mut rects);
            }
        });
        ui.add_space(4.0);
        let r = ui.available_rect_before_wrap();
        ui.painter().hline(
            r.x_range().shrink(4.0),
            r.top(),
            egui::Stroke::new(theme::HAIRLINE, Color32::from_white_alpha(40)),
        );
        ui.add_space(6.0);
    }

    let mut header_of = None;
    for (index, tab) in session.tabs().iter().enumerate().skip(pinned) {
        let active = session.active_id() == Some(tab.id());
        let group = tab.group().and_then(|g| session.group(g));
        if group.map(TabGroup::id) != header_of {
            header_of = group.map(TabGroup::id);
            if let Some(group) = group {
                let rect = group_header(ui, tabs, group, naming, commands);
                headers.push((group.id(), rect));
            }
        }
        // A folded group's tabs take no room (but the active one), nor
        // extensions' windows.
        if (group.is_some_and(|g| g.collapsed) && !active) || tabs.hidden.contains(&tab.id()) {
            let y = ui.cursor().top();
            rects.push(egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), y..=y));
            continue;
        }
        let response = tab_row(ui, tabs, tab, group, commands);
        track(index, tab, &response, &mut rects);
    }

    if let Some((from, id, dropped)) = dragged
        && let Some(tab) = session.tabs().iter().find(|t| t.id() == id)
    {
        let (zone, axis) = if from < pinned {
            (0..=pinned, Axis::Grid)
        } else {
            (pinned..=rects.len(), Axis::Column)
        };
        tab_common::drag_tab(
            ui, &rects, &headers, from, tab, dropped, zone, axis, commands,
        );
    }
}

/// A pinned tab: its icon in a square.
fn pinned_tab(
    ui: &mut egui::Ui,
    tabs: &SideTabs<'_>,
    tab: &Tab,
    commands: &mut Vec<Command>,
) -> egui::Response {
    let active = tabs.session.active_id() == Some(tab.id());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(PINNED_SIZE, PINNED_SIZE),
        Sense::click_and_drag(),
    );
    paint_background(ui, rect, active, &response, tabs.chrome);
    tab_icon(ui, rect.center(), tab, tabs.favicons, active, 16.0);
    let response = response.on_hover_text(tab.display_title());
    tab_actions(&response, tab, tabs.session, commands);
    response
}

/// A tab's row: icon, title, sound, close. Folded: the icon alone.
fn tab_row(
    ui: &mut egui::Ui,
    tabs: &SideTabs<'_>,
    tab: &Tab,
    group: Option<&TabGroup>,
    commands: &mut Vec<Command>,
) -> egui::Response {
    let active = tabs.session.active_id() == Some(tab.id());
    let text = tab_common::tab_ink(tab, active);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    paint_background(ui, rect, active, &response, tabs.chrome);
    // Shown beside the active tab (a split view).
    if tab_common::is_split_partner(tabs.session, tab) {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(8),
            Color32::from_white_alpha(36),
        );
    }
    if tabs.session.split_of(tab.id()).is_some()
        && (active || tab_common::is_split_partner(tabs.session, tab))
    {
        tab_common::split_mark(ui, rect);
    }
    if let Some(group) = group {
        // Its group's colour, down its left side.
        let bar = egui::Rect::from_min_size(rect.min + egui::vec2(1.0, 6.0), egui::vec2(3.0, 20.0));
        ui.painter()
            .rect_filled(bar, 2.0, theme::group_color(group.color));
    }
    if tabs.folded {
        tab_icon(ui, rect.center(), tab, tabs.favicons, active, 16.0);
        let response = response.on_hover_text(tab.display_title());
        tab_actions(&response, tab, tabs.session, commands);
        return response;
    }
    let indent = if group.is_some() { GROUP_INDENT } else { 0.0 };
    let icon_at = egui::pos2(rect.left() + indent + 16.0, rect.center().y);
    tab_icon(ui, icon_at, tab, tabs.favicons, active, 14.0);
    // Right to left: the cross (hovered or active), the speaker, then the
    // title in what is left.
    let mut right = rect.right() - 4.0;
    let show_close = active || response.contains_pointer();
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(4.0, 4.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if show_close {
        let close = icon_button(
            &mut inner,
            RichText::new(icon::X)
                .color(text.gamma_multiply(0.7))
                .small(),
            i18n::tabs::close_tab(),
        );
        if close.clicked() {
            commands.push(Command::CloseTab(tab.id()));
        }
        right = close.rect.left();
    }
    if let Some(sound) = tabs.favicons.sound(tab.id()) {
        let (glyph, tip) = match sound {
            Sound::Playing => (icon::SPEAKER_HIGH, i18n::tabs::mute_tab()),
            Sound::Muted => (icon::SPEAKER_SLASH, i18n::tabs::unmute_tab()),
        };
        let speaker = icon_button(&mut inner, RichText::new(glyph).color(text).size(13.0), tip);
        if speaker.clicked() {
            commands.push(Command::SetMuted(tab.id(), sound == Sound::Playing));
        }
        right = speaker.rect.left();
    }
    let left = icon_at.x + 14.0;
    let title_rect = egui::Rect::from_x_y_ranges(left..=(right - 4.0).max(left), rect.y_range());
    let mut title = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(title_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    // Cut to the row; a cut title shows whole on hover (egui's own).
    title.add(
        egui::Label::new(RichText::new(tab.display_title()).color(text))
            .selectable(false)
            .truncate()
            .sense(Sense::hover()),
    );
    tab_actions(&response, tab, tabs.session, commands);
    response
}

/// The active tab is the page's colour; a hovered one lighter.
fn paint_background(
    ui: &egui::Ui,
    rect: egui::Rect,
    active: bool,
    response: &egui::Response,
    chrome: &Chrome,
) {
    let corners = egui::CornerRadius::same(8);
    if active {
        ui.painter().rect_filled(rect, corners, chrome.night);
    } else if response.contains_pointer() {
        ui.painter()
            .rect_filled(rect, corners, Color32::from_white_alpha(28));
    }
}

/// The tab's icon (a spinner while it loads), centred on `at`.
fn tab_icon(
    ui: &mut egui::Ui,
    at: egui::Pos2,
    tab: &Tab,
    favicons: &Favicons,
    active: bool,
    size: f32,
) {
    let rect = egui::Rect::from_center_size(at, egui::vec2(size, size));
    let text = tab_common::tab_ink(tab, active);
    if tab.is_loading() {
        ui.put(
            rect,
            super::spinner::Spinner::new().size(size - 2.0).color(text),
        );
    } else if let Some(icon) = favicons.get(tab.id()) {
        egui::Image::new(icon)
            .fit_to_exact_size(rect.size())
            .paint_at(ui, rect);
    } else {
        ui.painter().text(
            at,
            egui::Align2::CENTER_CENTER,
            icon::GLOBE,
            egui::FontId::proportional(size),
            text,
        );
    }
}

/// What a whole tab does: click, middle click, menu.
fn tab_actions(
    response: &egui::Response,
    tab: &Tab,
    session: &Session,
    commands: &mut Vec<Command>,
) {
    if response.clicked() {
        commands.push(Command::Activate(tab.id()));
    }
    if response.middle_clicked() {
        commands.push(Command::CloseTab(tab.id()));
    }
    response.context_menu(|ui| {
        tab_common::tab_menu(ui, tab, session, TabPosition::Left, commands);
    });
}

/// A group's header: its colour, its name and a caret; clicked, the group
/// folds or unfolds; right-clicked, its menu. While `naming` it, its name is
/// typed there. Returns where it is: a tab dropped there joins the group.
fn group_header(
    ui: &mut egui::Ui,
    tabs: &SideTabs<'_>,
    group: &TabGroup,
    naming: &mut Option<GroupId>,
    commands: &mut Vec<Command>,
) -> egui::Rect {
    let color = theme::group_color(group.color);
    ui.add_space(4.0);
    if *naming == Some(group.id()) && !tabs.folded {
        return tab_common::name_field(ui, group, color, 3, None, naming, commands);
    }
    let height = if tabs.folded { 10.0 } else { 26.0 };
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
    let painter = ui.painter();
    if tabs.folded {
        // A bar of its colour.
        painter.rect_filled(rect.shrink2(egui::vec2(6.0, 2.0)), 3.0, color);
    } else {
        if response.hovered() {
            painter.rect_filled(rect, 6.0, Color32::from_white_alpha(20));
        }
        let caret = if group.collapsed {
            icon::CARET_RIGHT
        } else {
            icon::CARET_DOWN
        };
        painter.text(
            egui::pos2(rect.left() + 10.0, rect.center().y),
            egui::Align2::CENTER_CENTER,
            caret,
            egui::FontId::proportional(13.0),
            color,
        );
        painter.circle_filled(egui::pos2(rect.left() + 26.0, rect.center().y), 5.0, color);
        let mut name = tab_common::group_title(tabs.session, group);
        if group.collapsed {
            name = format!("{name} · {}", tabs.session.group_tabs(group.id()).len());
        }
        let galley = painter.layout(
            name,
            egui::FontId::proportional(13.0),
            theme::CHROME_TEXT,
            (rect.width() - 44.0).max(0.0),
        );
        painter.galley(
            egui::pos2(rect.left() + 38.0, rect.center().y - galley.size().y / 2.0),
            galley,
            theme::CHROME_TEXT,
        );
    }
    // A tab dragged over it: dropped, it joins the group.
    if ui.ctx().dragged_id().is_some() && response.contains_pointer() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            6.0,
            egui::Stroke::new(2.0_f32, theme::CYAN),
            egui::StrokeKind::Outside,
        );
    }
    let response = response.on_hover_text(i18n::tabs::group_tip());
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        commands.push(Command::SetGroupCollapsed(group.id(), !group.collapsed));
    }
    response.context_menu(|ui| tab_common::group_menu(ui, group, commands));
    rect
}
