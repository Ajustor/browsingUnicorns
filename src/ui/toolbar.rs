use egui::Button;
use egui_phosphor::regular as icon;

use domain::Tab;

use super::command::Command;
use super::omnibox::Omnibox;

pub fn show(
    ui: &mut egui::Ui,
    active: Option<&Tab>,
    omnibox: &mut Omnibox,
    commands: &mut Vec<Command>,
) {
    let history = active.map(Tab::history);
    let can_back = history.is_some_and(|h| h.can_go_back());
    let can_forward = history.is_some_and(|h| h.can_go_forward());

    ui.horizontal(|ui| {
        let mut button =
            |ui: &mut egui::Ui, glyph: &str, tip: &str, enabled: bool, cmd: Command| {
                if ui
                    .add_enabled(enabled, Button::new(glyph).frame(false))
                    .on_hover_text(tip)
                    .clicked()
                {
                    commands.push(cmd);
                }
            };
        button(
            ui,
            icon::ARROW_LEFT,
            "Back (Alt+←)",
            can_back,
            Command::Back,
        );
        button(
            ui,
            icon::ARROW_RIGHT,
            "Forward (Alt+→)",
            can_forward,
            Command::Forward,
        );
        button(
            ui,
            icon::ARROW_CLOCKWISE,
            "Reload (F5)",
            active.is_some(),
            Command::Reload,
        );
        button(ui, icon::HOUSE, "Home (Alt+Home)", true, Command::Home);

        let lock = match active.and_then(Tab::address) {
            Some(a) if a.is_secure() => icon::LOCK_SIMPLE,
            Some(_) => icon::GLOBE_SIMPLE,
            None => icon::MAGNIFYING_GLASS,
        };
        ui.label(lock);

        let current = active.and_then(Tab::address).map(|a| a.as_str());
        omnibox.show(ui, current, commands);
    });
}
