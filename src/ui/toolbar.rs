use egui::{Button, RichText};
use egui_phosphor::regular as icon;

use domain::{Profile, Tab};

use super::command::Command;
use super::extension_bar::{self, Action};
use super::library::LibraryView;
use super::omnibox::Omnibox;
use super::profiles::avatar;

pub fn show(
    ui: &mut egui::Ui,
    active: Option<&Tab>,
    bookmarked: bool,
    omnibox: &mut Omnibox,
    actions: &[Action],
    profile: &Profile,
    commands: &mut Vec<Command>,
) {
    let history = active.map(Tab::history);
    let can_back = history.is_some_and(|h| h.can_go_back());
    let can_forward = history.is_some_and(|h| h.can_go_forward());
    let address = active.and_then(Tab::address);

    ui.horizontal(|ui| {
        let nav = [
            (icon::ARROW_LEFT, "Back (Alt+←)", can_back, Command::Back),
            (
                icon::ARROW_RIGHT,
                "Forward (Alt+→)",
                can_forward,
                Command::Forward,
            ),
            (
                icon::ARROW_CLOCKWISE,
                "Reload (F5)",
                active.is_some(),
                Command::Reload,
            ),
            (icon::HOUSE, "Home (Alt+Home)", true, Command::Home),
        ];
        for (glyph, tip, enabled, command) in nav {
            if button(ui, glyph, tip, enabled) {
                commands.push(command);
            }
        }

        ui.label(match address {
            Some(a) if a.is_secure() => icon::LOCK_SIMPLE,
            Some(_) => icon::GLOBE_SIMPLE,
            None => icon::MAGNIFYING_GLASS,
        });

        // Right to left so the address bar takes the room left in between.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if avatar(ui, profile, 20.0)
                .on_hover_text(format!("Profile: {} (profiles)", profile.name))
                .clicked()
            {
                commands.push(Command::ToggleLibrary(LibraryView::Profiles));
            }
            if button(ui, icon::BOOKS, "History and bookmarks (Ctrl+H)", true) {
                commands.push(Command::ToggleLibrary(LibraryView::History));
            }
            if button(ui, icon::CODE, "Developer tools (F12)", active.is_some()) {
                commands.push(Command::OpenDevTools);
            }
            extension_bar::show(ui, actions, commands);
            let (star, tip) = if bookmarked {
                (
                    RichText::new(icon::STAR)
                        .strong()
                        .color(ui.visuals().warn_fg_color),
                    "Remove bookmark (Ctrl+D)",
                )
            } else {
                (RichText::new(icon::STAR), "Bookmark this page (Ctrl+D)")
            };
            if button(ui, star, tip, address.is_some()) {
                commands.push(Command::ToggleBookmark);
            }
            omnibox.show(ui, address.map(|a| a.as_str()), commands);
        });
    });
}

fn button(ui: &mut egui::Ui, glyph: impl Into<egui::WidgetText>, tip: &str, enabled: bool) -> bool {
    ui.add_enabled(enabled, Button::new(glyph).frame(false))
        .on_hover_text(tip)
        .clicked()
}
