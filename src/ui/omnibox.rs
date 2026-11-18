use application::Target;
use domain::Suggestion;
use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Id, Key, Modifiers, TextEdit, TextFormat, TopBottomPanel};
use egui_phosphor::regular as icon;

use super::command::Command;
use super::theme::{CHROME_TEXT, CHROME_TEXT_WEAK, Chrome};

/// How many suggestions the list shows at most.
pub const SUGGESTIONS: usize = 8;

/// The address bar. Shows the active tab's address unless the user is
/// typing; then past searches and pages of the history are proposed under it.
#[derive(Default)]
pub struct Omnibox {
    text: String,
    focus_requested: bool,
    editing: bool,
    /// What the user typed (the text may show a suggestion picked with the
    /// arrows instead).
    typed: String,
    /// What [`Self::suggestions`] were found for.
    query: String,
    suggestions: Vec<Suggestion>,
    selected: Option<usize>,
    /// The pointer is on the list: it stays while a click on it ends.
    list_hovered: bool,
}

impl Omnibox {
    const ID: &'static str = "omnibox";

    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, current: Option<&str>, commands: &mut Vec<Command>) {
        let id = Id::new(Self::ID);
        self.editing = ui.memory(|m| m.has_focus(id));
        if !self.editing {
            self.text.clear();
            self.text.push_str(current.unwrap_or_default());
            if !self.list_hovered {
                self.close_list();
            }
        }
        if self.editing && self.list_shown() {
            self.follow_arrows(ui, id);
        }

        // A pill, for this widget only: it is the toolbar's last one.
        ui.style_mut().visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(16);
        ui.style_mut().visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(16);
        ui.style_mut().visuals.widgets.active.corner_radius = egui::CornerRadius::same(16);
        let response = ui.add(
            TextEdit::singleline(&mut self.text)
                .id(id)
                .hint_text("Search or enter an address")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(12, 6))
                .frame(true),
        );
        if response.changed() {
            self.typed.clone_from(&self.text);
            self.selected = None;
        }

        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            match self.selected.and_then(|i| self.suggestions.get(i)) {
                Some(s) => commands.push(Command::Open(s.address().clone(), Target::ActiveTab)),
                None => commands.push(Command::Submit(self.text.clone())),
            }
            self.close_list();
        }
        if std::mem::take(&mut self.focus_requested) {
            response.request_focus();
            select_all(ui.ctx(), id, &self.text);
        } else if response.gained_focus() {
            // A click or Tab into the bar selects the address, ready to be
            // typed over; a second click places the cursor.
            select_all(ui.ctx(), id, &self.text);
        }
    }

    /// What was typed when the suggestions don't match it yet.
    pub fn stale_query(&self) -> Option<&str> {
        (self.editing && self.typed != self.query).then_some(self.typed.as_str())
    }

    pub fn set_suggestions(&mut self, query: String, suggestions: Vec<Suggestion>) {
        self.query = query;
        self.suggestions = suggestions;
        self.selected = None;
    }

    /// The suggestions, in a band under the toolbar (the page is a native
    /// view: nothing can be drawn over it).
    pub fn show_suggestions(
        &mut self,
        ctx: &egui::Context,
        chrome: &Chrome,
        commands: &mut Vec<Command>,
    ) {
        if !self.list_shown() {
            self.list_hovered = false;
            return;
        }
        let mut picked = None;
        let mut hovered = false;
        TopBottomPanel::top("suggestions")
            .frame(
                super::theme::chrome_frame(chrome).inner_margin(egui::Margin {
                    left: 10,
                    right: 10,
                    top: 2,
                    bottom: 6,
                }),
            )
            .show_separator_line(false)
            .show(ctx, |ui| {
                super::theme::chrome_style(ui, chrome);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                    for (i, s) in self.suggestions.iter().enumerate() {
                        let row = ui.add(egui::SelectableLabel::new(
                            self.selected == Some(i),
                            row_text(s),
                        ));
                        if row.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if row.clicked() {
                            picked = Some(s.address().clone());
                        }
                    }
                });
                hovered = ui.ui_contains_pointer();
            });
        self.list_hovered = hovered;
        if let Some(address) = picked {
            commands.push(Command::Open(address, Target::ActiveTab));
            self.close_list();
        }
    }

    fn list_shown(&self) -> bool {
        (self.editing || self.list_hovered)
            && !self.typed.trim().is_empty()
            && !self.suggestions.is_empty()
    }

    fn close_list(&mut self) {
        self.typed.clear();
        self.query.clear();
        self.suggestions.clear();
        self.selected = None;
        self.list_hovered = false;
    }

    /// Up and down go through the suggestions, the bar showing the one
    /// picked; past the first one, back to what was typed.
    fn follow_arrows(&mut self, ui: &egui::Ui, id: Id) {
        let (down, up) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
            )
        });
        if !down && !up {
            return;
        }
        let last = self.suggestions.len() - 1;
        self.selected = match (self.selected, down) {
            (None, true) => Some(0),
            (None, false) => Some(last),
            (Some(i), true) if i < last => Some(i + 1),
            (Some(i), false) if i > 0 => Some(i - 1),
            _ => None,
        };
        self.text = match self.selected.map(|i| &self.suggestions[i]) {
            Some(Suggestion::Search { terms, .. }) => terms.clone(),
            Some(Suggestion::Page { address, .. }) => address.to_string(),
            None => self.typed.clone(),
        };
        let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let end = CCursor::new(self.text.chars().count());
        state.cursor.set_char_range(Some(CCursorRange::one(end)));
        state.store(ui.ctx(), id);
    }
}

/// An icon, the search or the page title, and the page's address dimmed.
fn row_text(s: &Suggestion) -> LayoutJob {
    let mut job = LayoutJob::default();
    let strong = TextFormat {
        color: CHROME_TEXT,
        ..Default::default()
    };
    let weak = TextFormat {
        color: CHROME_TEXT_WEAK,
        ..Default::default()
    };
    match s {
        Suggestion::Search { terms, .. } => {
            job.append(icon::MAGNIFYING_GLASS, 0.0, weak.clone());
            job.append(terms, 8.0, strong);
            job.append("— search", 10.0, weak);
        }
        Suggestion::Page { title, address } => {
            job.append(icon::CLOCK_COUNTER_CLOCKWISE, 0.0, weak.clone());
            let url = address.as_str();
            if title.is_empty() {
                job.append(url, 8.0, strong);
            } else {
                job.append(&super::tab_strip::truncate(title, 70), 8.0, strong);
                job.append(&super::tab_strip::truncate(url, 80), 10.0, weak);
            }
        }
    }
    job
}

fn select_all(ctx: &egui::Context, id: Id, text: &str) {
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    let end = CCursor::new(text.chars().count());
    state
        .cursor
        .set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
    state.store(ctx, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDRESS: &str = "https://example.com/watch?v=1";

    /// One frame of the bar alone, with `events`.
    fn frame(ctx: &egui::Context, bar: &mut Omnibox, events: Vec<egui::Event>) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 100.0),
            )),
            events,
            // A second apart: two clicks are not a double click.
            time: Some(ctx.input(|i| i.time) + 1.0),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                bar.show(ui, Some(ADDRESS), &mut Vec::new());
            });
        });
    }

    fn selection(ctx: &egui::Context) -> Option<(usize, usize)> {
        let range = TextEdit::load_state(ctx, Id::new(Omnibox::ID))?
            .cursor
            .char_range()?;
        let (a, b) = (range.primary.index, range.secondary.index);
        Some((a.min(b), a.max(b)))
    }

    fn click(ctx: &egui::Context, bar: &mut Omnibox, pos: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        frame(ctx, bar, vec![egui::Event::PointerMoved(pos)]);
        frame(ctx, bar, vec![button(true)]);
        frame(ctx, bar, vec![button(false)]);
        frame(ctx, bar, Vec::new());
    }

    #[test]
    fn a_click_into_the_bar_selects_the_address_then_places_the_cursor() {
        let ctx = egui::Context::default();
        let mut bar = Omnibox::default();
        frame(&ctx, &mut bar, Vec::new());
        let pos = egui::pos2(100.0, 20.0);
        click(&ctx, &mut bar, pos);
        let all = ADDRESS.chars().count();
        assert_eq!(selection(&ctx), Some((0, all)));
        // Already in it: the click places the cursor.
        click(&ctx, &mut bar, pos);
        let (a, b) = selection(&ctx).unwrap();
        assert_eq!(a, b);
        assert!(a > 0 && a < all);
    }
}
