use egui::text::{CCursor, CCursorRange};
use egui::{Id, TextEdit};

use super::command::Command;

/// The address bar. Shows the active tab's address unless the user is typing.
#[derive(Default)]
pub struct Omnibox {
    text: String,
    focus_requested: bool,
}

impl Omnibox {
    const ID: &'static str = "omnibox";

    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    pub fn show(&mut self, ui: &mut egui::Ui, current: Option<&str>, commands: &mut Vec<Command>) {
        let id = Id::new(Self::ID);
        let editing = ui.memory(|m| m.has_focus(id));
        if !editing {
            self.text.clear();
            self.text.push_str(current.unwrap_or_default());
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

        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            commands.push(Command::Submit(self.text.clone()));
        }
        if std::mem::take(&mut self.focus_requested) {
            response.request_focus();
            select_all(ui.ctx(), id, &self.text);
        }
    }
}

fn select_all(ctx: &egui::Context, id: Id, text: &str) {
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    let end = CCursor::new(text.chars().count());
    state
        .cursor
        .set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
    state.store(ctx, id);
}
