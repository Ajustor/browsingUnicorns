use egui::{Button, Color32, RichText, ScrollArea, Sense, Spinner};
use egui_phosphor::regular as icon;

use domain::{GroupId, Session, Tab, TabGroup};

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
/// moves the window (double click: maximize), and it ends with the tab
/// search and the window's buttons (but on macOS, where the system draws
/// them on its left).
///
/// Pinned tabs come first, small; a group's tabs follow its label, a line
/// of its colour under them. `naming` is a group whose name is being typed
/// in its label (a new one).
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    session: &Session,
    favicons: &super::favicons::Favicons,
    chrome: &super::theme::Chrome,
    private: bool,
    naming: &mut Option<GroupId>,
    shrink: bool,
    commands: &mut Vec<Command>,
) {
    title_bar(ui);
    ui.spacing_mut().item_spacing.x = SPACING;
    ui.horizontal(|ui| {
        if cfg!(target_os = "macos") {
            ui.add_space(MACOS_BUTTONS);
        }
        if private {
            private_pill(ui, chrome);
        }
        let controls = if cfg!(target_os = "macos") {
            SEARCH_BUTTON
        } else {
            WINDOW_BUTTONS + SEARCH_BUTTON
        };
        // As wide as the tabs: the + follows the last one (room kept for it
        // when they overflow). No scroll bar, which got in the way of the
        // tabs and of the window's dragging: the wheel scrolls the tabs, and
        // the active one is scrolled into view.
        ui.style_mut().always_scroll_the_only_direction = true;
        let room = ui.available_width() - 32.0 - controls;
        // Shrinking, the tabs share what the pinned ones and the labels
        // leave (they still scroll once at their smallest).
        let width = shrink.then(|| {
            shrunk_width(
                room,
                fixed_room(ui, session, *naming),
                visible_tabs(session),
            )
        });
        let scroll = ScrollArea::horizontal()
            .auto_shrink([true, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_width(room)
            .drag_to_scroll(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let mut rects = Vec::with_capacity(session.len());
                    let mut labels = Vec::new();
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
                    let mut label_of = None;
                    for (index, tab) in session.tabs().iter().enumerate() {
                        let active = session.active_id() == Some(tab.id());
                        let group = tab.group().and_then(|g| session.group(g));
                        if group.map(TabGroup::id) != label_of {
                            label_of = group.map(TabGroup::id);
                            if let Some(group) = group {
                                let rect = group_label(ui, session, group, naming, commands);
                                labels.push((group.id(), rect));
                            }
                        }
                        // A collapsed group's tabs take no room (but the
                        // active one): dropped there, a tab goes after them.
                        if group.is_some_and(|g| g.collapsed) && !active {
                            let x = ui.cursor().left();
                            rects.push(egui::Rect::from_x_y_ranges(x..=x, ui.min_rect().y_range()));
                            continue;
                        }
                        let icon = favicons.get(tab.id());
                        let response = if tab.is_pinned() {
                            pinned_button(ui, tab, icon, active, chrome, session, commands)
                        } else {
                            tab_button(
                                ui, tab, icon, favicons, active, chrome, width, session, commands,
                            )
                        };
                        if let Some(group) = group {
                            let r = response.rect;
                            ui.painter().hline(
                                r.x_range().shrink(4.0),
                                r.bottom() - 1.5,
                                egui::Stroke::new(3.0_f32, super::theme::group_color(group.color)),
                            );
                        }
                        if active && newly_active {
                            response.scroll_to_me(None);
                        }
                        if response.dragged() || response.drag_stopped() {
                            dragged = Some((index, tab, response.drag_stopped()));
                        }
                        rects.push(response.rect);
                    }
                    if let Some((from, tab, dropped)) = dragged {
                        // Pinned tabs stay among themselves, the others
                        // after them.
                        let pinned = session.pinned_count();
                        let zone = if from < pinned {
                            0..=pinned
                        } else {
                            pinned..=rects.len()
                        };
                        drag_tab(ui, &rects, &labels, from, tab, dropped, zone, commands);
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
        .on_hover_text(i18n::tabs::new_tab());
        if plus.clicked() {
            commands.push(Command::NewTab);
        }
        if plus.secondary_clicked() {
            commands.push(Command::ReopenClosedTab);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !cfg!(target_os = "macos") {
                window_buttons(ui);
            }
            ui.add_space(6.0);
            let search = super::theme::hover_fill(ui, |ui| {
                ui.add(
                    Button::new(
                        RichText::new(icon::MAGNIFYING_GLASS)
                            .color(Color32::WHITE)
                            .size(15.0),
                    )
                    .frame(false),
                )
            })
            .on_hover_text(i18n::tabs::search_tabs());
            if search.clicked() {
                commands.push(Command::ToggleTabSearch);
            }
        });
    });

    #[allow(clippy::too_many_arguments)]
    fn tab_button(
        ui: &mut egui::Ui,
        tab: &Tab,
        icon: Option<&egui::TextureHandle>,
        favicons: &super::favicons::Favicons,
        active: bool,
        chrome: &super::theme::Chrome,
        width: Option<f32>,
        session: &Session,
        commands: &mut Vec<Command>,
    ) -> egui::Response {
        let text = tab_text(active);
        // Narrow: its icon alone (and its close button when active).
        let compact = width.is_some_and(|w| w < COMPACT_BELOW);
        // The whole tab is clickable, and dragged to move it; sensed before
        // its content, so the title and the close button inside it get
        // their own clicks.
        let response = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .id_salt(tab.id())
                    .sense(Sense::click_and_drag()),
                |ui| {
                    let left: i8 = if compact { 10 } else { 12 };
                    tab_frame(active, chrome, left, 8).show(ui, |ui| {
                        match width {
                            Some(w) => ui.set_width(w - f32::from(left) - 8.0),
                            None => {
                                ui.set_min_width(110.0);
                                ui.set_max_width(190.0);
                            }
                        }
                        ui.horizontal(|ui| {
                            if tab.is_loading() {
                                ui.add(Spinner::new().size(12.0).color(text));
                            } else if let Some(icon) = icon {
                                ui.add(
                                    egui::Image::new(icon)
                                        .fit_to_exact_size(egui::vec2(14.0, 14.0)),
                                );
                            } else if compact {
                                ui.add(
                                    egui::Label::new(RichText::new(icon::GLOBE).color(text))
                                        .selectable(false),
                                );
                            }
                            if compact {
                                if active && width.is_some_and(|w| w >= 56.0) {
                                    close_button(ui, tab, text, commands);
                                }
                                return;
                            }
                            if let Some(sound) = favicons.sound(tab.id()) {
                                let (glyph, tip) = match sound {
                                    super::favicons::Sound::Playing => {
                                        (icon::SPEAKER_HIGH, i18n::tabs::mute_tab())
                                    }
                                    super::favicons::Sound::Muted => {
                                        (icon::SPEAKER_SLASH, i18n::tabs::unmute_tab())
                                    }
                                };
                                if super::theme::hover_fill(ui, |ui| {
                                    ui.add(
                                        Button::new(RichText::new(glyph).color(text).size(13.0))
                                            .frame(false),
                                    )
                                })
                                .on_hover_text(tip)
                                .clicked()
                                {
                                    commands.push(Command::SetMuted(
                                        tab.id(),
                                        sound == super::favicons::Sound::Playing,
                                    ));
                                }
                            }
                            let title = truncate(tab.display_title(), TITLE_CHARS);
                            // Cut to the tab, room left for the cross.
                            let room = ui.available_width() - 22.0;
                            let label = ui
                                .scope(|ui| {
                                    ui.set_max_width(room.max(0.0));
                                    ui.add(
                                        egui::Label::new(RichText::new(title).color(text))
                                            .selectable(false)
                                            .truncate()
                                            .sense(Sense::click()),
                                    )
                                })
                                .inner
                                .on_hover_text(tab.display_title());
                            if label.clicked() {
                                commands.push(Command::Activate(tab.id()));
                            }
                            if label.middle_clicked() {
                                commands.push(Command::CloseTab(tab.id()));
                            }
                            label.context_menu(|ui| tab_menu(ui, tab, session, commands));
                            close_button(ui, tab, text, commands);
                        });
                    })
                },
            )
            .response;
        let response = if compact {
            response.on_hover_text(tab.display_title())
        } else {
            response
        };
        tab_response(ui, &response, tab, active, session, commands);
        response
    }

    /// The cross, at the right end of the tab.
    fn close_button(ui: &mut egui::Ui, tab: &Tab, text: Color32, commands: &mut Vec<Command>) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
            .on_hover_text(i18n::tabs::close_tab());
            if close.clicked() {
                commands.push(Command::CloseTab(tab.id()));
            }
        });
    }

    /// A pinned tab: its icon alone, no close button.
    fn pinned_button(
        ui: &mut egui::Ui,
        tab: &Tab,
        icon: Option<&egui::TextureHandle>,
        active: bool,
        chrome: &super::theme::Chrome,
        session: &Session,
        commands: &mut Vec<Command>,
    ) -> egui::Response {
        let text = tab_text(active);
        let response = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .id_salt(tab.id())
                    .sense(Sense::click_and_drag()),
                |ui| {
                    tab_frame(active, chrome, 11, 11).show(ui, |ui| {
                        ui.set_width(16.0);
                        ui.vertical_centered(|ui| {
                            if tab.is_loading() {
                                ui.add(Spinner::new().size(14.0).color(text));
                            } else if let Some(icon) = icon {
                                ui.add(
                                    egui::Image::new(icon)
                                        .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                                );
                            } else {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(icon::GLOBE).color(text).size(15.0),
                                    )
                                    .selectable(false),
                                );
                            }
                        });
                    })
                },
            )
            .response
            .on_hover_text(tab.display_title());
        tab_response(ui, &response, tab, active, session, commands);
        response
    }

    /// What a whole tab does: hover, click, middle click, menu.
    fn tab_response(
        ui: &egui::Ui,
        response: &egui::Response,
        tab: &Tab,
        active: bool,
        session: &Session,
        commands: &mut Vec<Command>,
    ) {
        // Over the title or the close button too, which sit on top of it.
        if !active && response.contains_pointer() {
            ui.painter()
                .rect_filled(response.rect, TAB_CORNERS, Color32::from_white_alpha(28));
        }
        if response.clicked() {
            commands.push(Command::Activate(tab.id()));
        }
        if response.middle_clicked() {
            commands.push(Command::CloseTab(tab.id()));
        }
        response.context_menu(|ui| tab_menu(ui, tab, session, commands));
    }
}

const TAB_CORNERS: egui::CornerRadius = egui::CornerRadius {
    nw: 10,
    ne: 10,
    sw: 0,
    se: 0,
};

/// Room for the tab search's button, right of the tabs.
const SEARCH_BUTTON: f32 = 34.0;

/// Space between the strip's items.
const SPACING: f32 = 3.0;
/// A pinned tab's width.
const PINNED_WIDTH: f32 = 38.0;
/// A tab's width, at most (as when the tabs scroll) and at least (shrunk).
const TAB_MAX: f32 = 210.0;
const TAB_MIN: f32 = 40.0;
/// Narrower, a shrunk tab shows its icon alone.
const COMPACT_BELOW: f32 = 90.0;

/// Each tab's width when `tabs` of them share `room`, less `fixed` (pinned
/// tabs, labels), with no more than [`TAB_MAX`] nor less than [`TAB_MIN`].
fn shrunk_width(room: f32, fixed: f32, tabs: usize) -> f32 {
    if tabs == 0 {
        return TAB_MAX;
    }
    ((room - fixed) / tabs as f32 - SPACING).clamp(TAB_MIN, TAB_MAX)
}

/// The tabs shown at full size: not pinned, nor hidden in a collapsed group.
fn visible_tabs(session: &Session) -> usize {
    session
        .tabs()
        .iter()
        .filter(|t| {
            !t.is_pinned() && (session.active_id() == Some(t.id()) || !session.is_hidden(t.id()))
        })
        .count()
}

/// The room the pinned tabs and the labels of the groups take.
fn fixed_room(ui: &egui::Ui, session: &Session, naming: Option<GroupId>) -> f32 {
    let pinned = session.pinned_count() as f32 * (PINNED_WIDTH + SPACING);
    let labels: f32 = session
        .groups()
        .iter()
        .map(|g| label_room(ui, session, g, naming == Some(g.id())))
        .sum();
    pinned + labels
}

fn tab_text(active: bool) -> Color32 {
    if active {
        Color32::WHITE
    } else {
        super::theme::TAB_TEXT
    }
}

fn tab_frame(active: bool, chrome: &super::theme::Chrome, left: i8, right: i8) -> egui::Frame {
    let fill = if active {
        chrome.night
    } else {
        Color32::TRANSPARENT
    };
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(TAB_CORNERS)
        .inner_margin(egui::Margin {
            left,
            right,
            top: 7,
            bottom: 7,
        })
}

/// A tab's menu (right click): pin, groups, close.
fn tab_menu(ui: &mut egui::Ui, tab: &Tab, session: &Session, commands: &mut Vec<Command>) {
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
    item(
        ui,
        icon::X,
        i18n::tabs::close(),
        Command::CloseTab(tab.id()),
        commands,
    );
}

/// A group's name, else what stands for it in the menus.
fn group_title(session: &Session, group: &TabGroup) -> String {
    if group.name.is_empty() {
        i18n::tabs::unnamed_group(session.group_tabs(group.id()).len())
    } else {
        group.name.clone()
    }
}

/// The label before a group's tabs, in its colour: clicked, it collapses or
/// expands the group; right-clicked, its menu (name, colour, ungroup,
/// close). While `naming` it, its name is typed in it.
/// Returns where it is: a tab dropped there joins the group.
fn group_label(
    ui: &mut egui::Ui,
    session: &Session,
    group: &TabGroup,
    naming: &mut Option<GroupId>,
    commands: &mut Vec<Command>,
) -> egui::Rect {
    let color = super::theme::group_color(group.color);
    let ink = super::theme::NIGHT_DEEP;
    ui.add_space(4.0);
    if *naming == Some(group.id()) {
        let buffer = ui.id().with(("naming", group.id()));
        let started = ui.data(|d| d.get_temp::<String>(buffer));
        let mut name = started.clone().unwrap_or_else(|| group.name.clone());
        let named = egui::Frame::NONE
            .fill(color)
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                let style = ui.visuals_mut();
                style.override_text_color = Some(ink);
                style.extreme_bg_color = Color32::TRANSPARENT;
                style.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                style.widgets.hovered.bg_stroke = egui::Stroke::NONE;
                style.widgets.active.bg_stroke = egui::Stroke::NONE;
                style.selection.stroke = egui::Stroke::NONE;
                ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .desired_width(NAMING_WIDTH)
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
        ui.add_space(2.0);
        return named.response.rect;
    }
    // A pill as tall as a line of text (a dot without a name), centred on
    // the tabs.
    let galley = label_galley(ui, session, group);
    let size = label_size(galley.as_deref());
    // Its own id, the same while the tabs around it come and go.
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let response = ui.interact(
        rect,
        ui.id().with(("group label", group.id())),
        Sense::click(),
    );
    let corners = if galley.is_some() { 6 } else { 255 };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(corners), color);
    if let Some(galley) = galley {
        let at = rect.center() - galley.size() / 2.0;
        ui.painter().galley(at, galley, ink);
    }
    let response = response.on_hover_text(i18n::tabs::group_tip());
    // A tab dragged over it: dropped, it joins the group.
    if ui.ctx().dragged_id().is_some() && response.contains_pointer() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            egui::CornerRadius::same(corners + 2),
            egui::Stroke::new(2.0_f32, super::theme::CYAN),
            egui::StrokeKind::Outside,
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        commands.push(Command::SetGroupCollapsed(group.id(), !group.collapsed));
    }
    response.context_menu(|ui| group_menu(ui, group, commands));
    ui.add_space(2.0);
    rect
}

/// Width of the field where a new group's name is typed.
const NAMING_WIDTH: f32 = 110.0;

/// The text of a group's label: its name (and how many tabs it hides when
/// collapsed); `None` for an unnamed open group, a dot.
fn label_galley(
    ui: &egui::Ui,
    session: &Session,
    group: &TabGroup,
) -> Option<std::sync::Arc<egui::Galley>> {
    let collapsed_count = group
        .collapsed
        .then(|| session.group_tabs(group.id()).len());
    let text = match (group.name.is_empty(), collapsed_count) {
        (true, None) => return None,
        (true, Some(n)) => format!("+{n}"),
        (false, None) => truncate(&group.name, TITLE_CHARS),
        (false, Some(n)) => format!("{} · {n}", truncate(&group.name, TITLE_CHARS)),
    };
    Some(ui.painter().layout_no_wrap(
        text,
        egui::FontId::proportional(13.0),
        super::theme::NIGHT_DEEP,
    ))
}

fn label_size(galley: Option<&egui::Galley>) -> egui::Vec2 {
    match galley {
        Some(g) => egui::vec2(g.size().x + 16.0, 20.0),
        None => egui::vec2(12.0, 12.0),
    }
}

/// The room a group's label takes in the strip, spaces around included.
fn label_room(ui: &egui::Ui, session: &Session, group: &TabGroup, naming: bool) -> f32 {
    let width = if naming {
        NAMING_WIDTH + 12.0
    } else {
        label_size(label_galley(ui, session, group).as_deref()).x
    };
    width + 4.0 + 2.0 + 2.0 * SPACING
}

/// A group's menu: its name, its colour, ungroup, close.
fn group_menu(ui: &mut egui::Ui, group: &TabGroup, commands: &mut Vec<Command>) {
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

/// A tab being dragged: a bar where it would land; dropped, it moves there.
/// Dropped out of the window, or well under the strip, it gets its own.
#[allow(clippy::too_many_arguments)]
fn drag_tab(
    ui: &egui::Ui,
    rects: &[egui::Rect],
    labels: &[(GroupId, egui::Rect)],
    from: usize,
    tab: &Tab,
    dropped: bool,
    zone: std::ops::RangeInclusive<usize>,
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
    let (slot, to) = drop_slot(rects, from, pointer.x, zone);
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
/// (before tab `slot`, kept in `zone`), and its index once moved there.
fn drop_slot(
    rects: &[egui::Rect],
    from: usize,
    x: f32,
    zone: std::ops::RangeInclusive<usize>,
) -> (usize, usize) {
    let slot = rects
        .iter()
        .filter(|r| r.center().x < x)
        .count()
        .clamp(*zone.start(), *zone.end());
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
        let all = 0..=3;
        // Tab 0 over the second half of tab 2: after it.
        assert_eq!(drop_slot(&rects, 0, 280.0, all.clone()), (3, 2));
        // Tab 2 over the first half of tab 0: before it.
        assert_eq!(drop_slot(&rects, 2, 20.0, all.clone()), (0, 0));
        // Over itself: it stays.
        assert_eq!(drop_slot(&rects, 1, 120.0, all.clone()).1, 1);
        assert_eq!(drop_slot(&rects, 1, 180.0, all).1, 1);
        // Tab 0 pinned, the others not: they stay apart.
        assert_eq!(drop_slot(&rects, 0, 280.0, 0..=1), (1, 0));
        assert_eq!(drop_slot(&rects, 2, 20.0, 1..=3), (1, 1));
    }

    #[test]
    fn shrunk_tabs_share_the_room_between_bounds() {
        // Room to spare: as wide as when they scroll.
        assert_eq!(shrunk_width(1000.0, 0.0, 2), TAB_MAX);
        // 10 tabs in 1000 points, 100 taken by pinned tabs: 87 each.
        assert_eq!(shrunk_width(1000.0, 100.0, 10), 87.0);
        // Never under the minimum: past it, they scroll.
        assert_eq!(shrunk_width(500.0, 0.0, 100), TAB_MIN);
        assert_eq!(shrunk_width(500.0, 0.0, 0), TAB_MAX);
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
    fn a_tab_without_title_nor_address_is_a_new_tab() {
        let tab = domain::Tab::new(domain::TabId::new(1));
        let en = i18n::with_lang(i18n::Lang::En, || tab.display_title().to_owned());
        assert_eq!(en, "New tab");
        let fr = i18n::with_lang(i18n::Lang::Fr, || tab.display_title().to_owned());
        assert_eq!(fr, "Nouvel onglet");
    }

    #[test]
    fn long_titles_are_cut() {
        assert_eq!(truncate("short", TITLE_CHARS), "short");
        let long = "a".repeat(40);
        assert_eq!(truncate(&long, TITLE_CHARS).chars().count(), TITLE_CHARS);
        assert!(truncate(&long, TITLE_CHARS).ends_with('…'));
    }
}
