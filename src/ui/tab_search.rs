//! The search of the open tabs (Ctrl+Shift+A, or the magnifier of the tab
//! strip): a field over the page, the tabs matching what is typed under it,
//! the way an editor's "go to file" works.

use egui::{Color32, Key, Modifiers, RichText, Sense};
use egui_phosphor::regular as icon;

use domain::{Session, TabMatch, search_tabs};

use super::command::Command;
use super::theme::{CHROME_TEXT, CHROME_TEXT_WEAK, CYAN, Chrome};

const WIDTH: f32 = 560.0;
/// Rows shown before the list scrolls.
const ROWS: f32 = 10.0;
const ROW_HEIGHT: f32 = 44.0;

#[derive(Default)]
pub struct TabSearch {
    open: bool,
    query: String,
    selected: usize,
    /// The field takes the keyboard next frame.
    focus: bool,
    /// The selection moved by the keys: scrolled into view.
    scroll: bool,
}

impl TabSearch {
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            *self = Self {
                open: true,
                focus: true,
                ..Self::default()
            };
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Under the tab strip (`top`), centred. A tab picked (click, Enter)
    /// is shown; Escape, or a click elsewhere, closes it.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        top: f32,
        session: &Session,
        favicons: &super::favicons::Favicons,
        chrome: &Chrome,
        commands: &mut Vec<Command>,
    ) {
        if !self.open {
            return;
        }
        let found = search_tabs(session.tabs(), &self.query);
        // Taken before the field, which would use them.
        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.close();
            return;
        }
        if !found.is_empty() {
            let last = found.len() - 1;
            self.selected = self.selected.min(last);
            if up {
                self.selected = if self.selected == 0 {
                    last
                } else {
                    self.selected - 1
                };
                self.scroll = true;
            }
            if down {
                self.selected = if self.selected == last {
                    0
                } else {
                    self.selected + 1
                };
                self.scroll = true;
            }
            if enter {
                self.pick(found[self.selected].id, commands);
                return;
            }
        }

        let screen = ctx.screen_rect();
        let width = WIDTH.min(screen.width() - 32.0);
        let at = egui::pos2(screen.center().x - width / 2.0, top);
        let mut picked = None;
        let mut closed = None;
        let area = egui::Area::new(egui::Id::new("tab search"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ctx, |ui| {
                super::theme::chrome_style(ui, chrome);
                egui::Frame::NONE
                    .fill(chrome.night)
                    .stroke(egui::Stroke::new(1.0_f32, super::theme::OMNIBOX_STROKE))
                    .corner_radius(egui::CornerRadius::same(10))
                    .shadow(ui.visuals().popup_shadow)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        let field = self.field(ui);
                        ui.add_space(6.0);
                        if found.is_empty() {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(i18n::tabs::no_tab_found()).color(CHROME_TEXT_WEAK),
                            );
                            ui.add_space(4.0);
                        } else {
                            egui::ScrollArea::vertical()
                                .max_height(ROWS * ROW_HEIGHT)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for (i, m) in found.iter().enumerate() {
                                        let row = self.row(ui, i, m, session, favicons, chrome);
                                        if row.clicked() {
                                            picked = Some(m.id);
                                        }
                                        if row.middle_clicked() {
                                            closed = Some(m.id);
                                        }
                                    }
                                });
                        }
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(i18n::tabs::search_tabs_keys(&format!(
                                "{}{}",
                                icon::ARROW_UP,
                                icon::ARROW_DOWN
                            )))
                            .small()
                            .color(CHROME_TEXT_WEAK),
                        );
                        field
                    })
                    .inner
            });
        self.scroll = false;
        if let Some(id) = picked {
            self.pick(id, commands);
            return;
        }
        if let Some(id) = closed {
            commands.push(Command::CloseTab(id));
            self.focus = true;
        }
        let inside = area.response.contains_pointer();
        if area.inner.lost_focus() && !inside {
            // A click elsewhere: in the chrome or the page.
            self.close();
        } else if inside && ctx.input(|i| i.pointer.any_pressed()) {
            self.focus = true;
        }
    }

    fn pick(&mut self, id: domain::TabId, commands: &mut Vec<Command>) {
        commands.push(Command::Activate(id));
        self.close();
    }

    fn field(&mut self, ui: &mut egui::Ui) -> egui::Response {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(
                RichText::new(icon::MAGNIFYING_GLASS)
                    .size(16.0)
                    .color(CHROME_TEXT_WEAK),
            );
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text(i18n::tabs::search_tabs_hint())
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 6))
                    .font(egui::FontId::proportional(15.0)),
            );
            if std::mem::take(&mut self.focus) {
                field.request_focus();
            }
            if field.changed() {
                self.selected = 0;
            }
            field
        })
        .inner
    }

    /// A tab found: its icon, its title and address with the letters typed
    /// in cyan, its group or pin.
    fn row(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        found: &TabMatch,
        session: &Session,
        favicons: &super::favicons::Favicons,
        chrome: &Chrome,
    ) -> egui::Response {
        let Some(tab) = session.tab(found.id) else {
            return ui.label("");
        };
        let background = ui.painter().add(egui::Shape::Noop);
        let width = ui.available_width();
        let row = ui
            .scope_builder(egui::UiBuilder::new().sense(Sense::click()), |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.set_width(width - 16.0);
                        ui.horizontal(|ui| {
                            match favicons.get(tab.id()) {
                                Some(texture) => {
                                    ui.add(
                                        egui::Image::new(texture)
                                            .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                                    );
                                }
                                None => {
                                    ui.label(RichText::new(icon::GLOBE).color(CHROME_TEXT_WEAK));
                                }
                            }
                            ui.add_space(4.0);
                            let text_width = ui.available_width() - 24.0;
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 1.0;
                                ui.add(
                                    egui::Label::new(highlighted(
                                        tab.display_title(),
                                        &found.title_hits,
                                        CHROME_TEXT,
                                        14.0,
                                        text_width,
                                    ))
                                    .selectable(false),
                                );
                                // Its group's name first, in its colour.
                                let group = tab.group().and_then(|g| session.group(g)).map(|g| {
                                    let name = if g.name.is_empty() {
                                        icon::SQUARES_FOUR.to_owned()
                                    } else {
                                        g.name.clone()
                                    };
                                    (format!("{name}  "), super::theme::group_color(g.color))
                                });
                                let mut address = egui::text::LayoutJob::default();
                                if let Some((name, color)) = group {
                                    address.append(
                                        &name,
                                        0.0,
                                        egui::TextFormat::simple(
                                            egui::FontId::proportional(12.0),
                                            color,
                                        ),
                                    );
                                }
                                add_highlighted(
                                    &mut address,
                                    &found.address,
                                    &found.address_hits,
                                    CHROME_TEXT_WEAK,
                                    12.0,
                                );
                                address.wrap = one_line(text_width);
                                ui.add(egui::Label::new(address).selectable(false));
                            });
                            if tab.is_pinned() {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(icon::PUSH_PIN).color(CHROME_TEXT_WEAK),
                                        );
                                    },
                                );
                            }
                        });
                    });
            })
            .response;
        // Under the pointer it is selected, when the pointer moves (not when
        // the list scrolls under it).
        if row.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
            self.selected = index;
        }
        let selected = self.selected == index;
        if selected && self.scroll {
            row.scroll_to_me(None);
        }
        if selected || row.hovered() {
            let fill = if selected {
                chrome.hover
            } else {
                Color32::from_white_alpha(12)
            };
            ui.painter().set(
                background,
                egui::epaint::RectShape::filled(row.rect, egui::CornerRadius::same(6), fill),
            );
        }
        if row.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if session.active_id() == Some(tab.id()) {
            let r = row.rect;
            ui.painter().vline(
                r.left() + 1.0,
                r.y_range().shrink(8.0),
                egui::Stroke::new(2.0_f32, CYAN),
            );
        }
        row
    }
}

/// `text` on one line cut to `width`, the chars at `hits` in cyan.
fn highlighted(
    text: &str,
    hits: &[usize],
    color: Color32,
    size: f32,
    width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    add_highlighted(&mut job, text, hits, color, size);
    job.wrap = one_line(width);
    job
}

/// One line, cut with an ellipsis.
fn one_line(width: f32) -> egui::text::TextWrapping {
    egui::text::TextWrapping {
        max_width: width.max(10.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    }
}

/// Adds `text` to `job`, the chars at `hits` in cyan.
fn add_highlighted(
    job: &mut egui::text::LayoutJob,
    text: &str,
    hits: &[usize],
    color: Color32,
    size: f32,
) {
    let font = egui::FontId::proportional(size);
    let mut run = String::new();
    let mut run_hit = false;
    for (i, c) in text.chars().enumerate() {
        let hit = hits.binary_search(&i).is_ok();
        if hit != run_hit && !run.is_empty() {
            append(job, &std::mem::take(&mut run), run_hit, color, &font);
        }
        run_hit = hit;
        run.push(c);
    }
    if !run.is_empty() {
        append(job, &run, run_hit, color, &font);
    }
}

fn append(
    job: &mut egui::text::LayoutJob,
    text: &str,
    hit: bool,
    color: Color32,
    font: &egui::FontId,
) {
    let format = egui::TextFormat {
        font_id: font.clone(),
        color: if hit { CYAN } else { color },
        underline: if hit {
            egui::Stroke::new(1.0_f32, CYAN)
        } else {
            egui::Stroke::NONE
        },
        ..Default::default()
    };
    job.append(text, 0.0, format);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_letters_are_highlighted_in_runs() {
        let job = highlighted("Rust doc", &[0, 5, 6], CHROME_TEXT, 14.0, 100.0);
        let runs: Vec<(&str, bool)> = job
            .sections
            .iter()
            .map(|s| (&job.text[s.byte_range.clone()], s.format.color == CYAN))
            .collect();
        assert_eq!(
            runs,
            vec![("R", true), ("ust ", false), ("do", true), ("c", false)]
        );
    }

    #[test]
    fn it_opens_and_closes() {
        let mut search = TabSearch::default();
        search.toggle();
        assert!(search.open && search.focus);
        search.query.push('x');
        search.toggle();
        assert!(!search.open);
        search.toggle();
        assert!(search.query.is_empty(), "a new search");
    }
}
