use egui::{Button, RichText};
use egui_phosphor::regular as icon;

use domain::{Profile, Tab};

use super::command::Command;
use super::extension_bar::{self, Action};
use super::library::LibraryView;
use super::omnibox::Omnibox;
use super::profiles::avatar;

/// The ad blocker's button: whether it blocks on the page, and what it
/// stopped there (`None` where the web view doesn't tell).
pub struct ShieldButton {
    pub on: bool,
    pub blocked: Option<u32>,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    active: Option<&Tab>,
    bookmarked: bool,
    shield: &ShieldButton,
    omnibox: &mut Omnibox,
    actions: &[Action],
    profile: &Profile,
    downloads: Option<(&str, bool)>,
    zoom: Option<u32>,
    commands: &mut Vec<Command>,
) {
    let history = active.map(Tab::history);
    let can_back = history.is_some_and(|h| h.can_go_back());
    let can_forward = history.is_some_and(|h| h.can_go_forward());
    let address = active.and_then(Tab::address);

    ui.horizontal(|ui| {
        let nav = [
            (
                icon::ARROW_LEFT,
                i18n::toolbar::back(),
                can_back,
                Command::Back,
            ),
            (
                icon::ARROW_RIGHT,
                i18n::toolbar::forward(),
                can_forward,
                Command::Forward,
            ),
            (
                icon::ARROW_CLOCKWISE,
                i18n::toolbar::reload(),
                active.is_some(),
                Command::Reload,
            ),
            (icon::HOUSE, i18n::toolbar::home(), true, Command::Home),
        ];
        for (glyph, tip, enabled, command) in nav {
            if button(ui, RichText::new(glyph).size(16.0), tip, enabled) {
                // Shift+click on Reload: without the cache.
                let shift = ui.input(|i| i.modifiers.shift);
                commands.push(match command {
                    Command::Reload if shift => Command::HardReload,
                    other => other,
                });
            }
        }

        match address {
            Some(a) => {
                let (glyph, tip) = if a.is_secure() {
                    (
                        RichText::new(icon::LOCK_SIMPLE).color(super::theme::CYAN),
                        i18n::toolbar::connection_secure(),
                    )
                } else {
                    (
                        RichText::new(icon::GLOBE_SIMPLE),
                        i18n::toolbar::connection_not_secure(),
                    )
                };
                let glyph = glyph.size(16.0);
                if button(ui, glyph, tip, true) {
                    commands.push(Command::ToggleSiteInfo);
                }
                if shield_button(ui, shield) {
                    commands.push(Command::ToggleShield);
                }
            }
            None => {
                ui.label(icon::MAGNIFYING_GLASS);
            }
        }

        // Right to left so the address bar takes the room left in between.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if super::theme::hover_fill(ui, |ui| avatar(ui, profile, 20.0))
                .on_hover_text(i18n::toolbar::profile_of(&profile.name))
                .clicked()
            {
                commands.push(Command::ToggleLibrary(LibraryView::Profiles));
            }
            if button(
                ui,
                RichText::new(icon::MASK_HAPPY).size(16.0),
                i18n::toolbar::new_private_window(),
                true,
            ) {
                commands.push(Command::OpenPrivateWindow);
            }
            if button(
                ui,
                RichText::new(icon::BOOKS).size(16.0),
                i18n::toolbar::history_and_bookmarks(),
                true,
            ) {
                commands.push(Command::ToggleLibrary(LibraryView::History));
            }
            if button(
                ui,
                RichText::new(icon::CODE).size(16.0),
                i18n::toolbar::developer_tools(),
                active.is_some(),
            ) {
                commands.push(Command::OpenDevTools);
            }
            if let Some((tip, running)) = downloads {
                let glyph = RichText::new(icon::DOWNLOAD_SIMPLE).size(16.0);
                let glyph = if running {
                    glyph.color(super::theme::CYAN)
                } else {
                    glyph
                };
                if button(ui, glyph, tip, true) {
                    commands.push(Command::ToggleDownloads);
                }
            }
            extension_bar::show(ui, actions, commands);
            if let Some(percent) = zoom
                && button(
                    ui,
                    RichText::new(i18n::toolbar::zoom_percent(percent)).size(13.0),
                    i18n::toolbar::zoom_of_site(),
                    true,
                )
            {
                commands.push(Command::ResetZoom);
            }
            let (star, tip) = if bookmarked {
                (
                    RichText::new(icon::STAR).strong().color(super::theme::CYAN),
                    i18n::toolbar::remove_bookmark(),
                )
            } else {
                (
                    RichText::new(icon::STAR),
                    i18n::toolbar::bookmark_this_page(),
                )
            };
            if button(ui, star.size(16.0), tip, address.is_some()) {
                commands.push(Command::ToggleBookmark);
            }
            omnibox.show(ui, address.map(|a| a.as_str()), commands);
        });
    });
}

/// The shield, with the count in a badge over its corner.
fn shield_button(ui: &mut egui::Ui, shield: &ShieldButton) -> bool {
    let (glyph, tip) = if shield.on {
        (
            RichText::new(icon::SHIELD_CHECK).color(super::theme::CYAN),
            i18n::shield::on_for_site(),
        )
    } else {
        (
            RichText::new(icon::SHIELD_SLASH),
            i18n::shield::off_for_site(),
        )
    };
    let response =
        super::theme::hover_fill(ui, |ui| ui.add(Button::new(glyph.size(16.0)).frame(false)))
            .on_hover_text(tip);
    if let Some(count) = shield.blocked.filter(|&n| n > 0 && shield.on) {
        let text = if count > 99 {
            "99+".to_owned()
        } else {
            count.to_string()
        };
        let font = egui::FontId::proportional(9.0);
        let galley = ui.painter().layout_no_wrap(text, font, super::theme::NIGHT);
        let size = egui::vec2((galley.size().x + 6.0).max(12.0), 11.0);
        let corner = response.rect.right_top() + egui::vec2(2.0, -2.0);
        let badge = egui::Rect::from_min_size(corner - egui::vec2(size.x, 0.0), size);
        let painter = ui.painter();
        painter.rect_filled(badge, 5.5, super::theme::CYAN);
        painter.galley(
            badge.center() - galley.size() / 2.0,
            galley,
            super::theme::NIGHT,
        );
    }
    response.clicked()
}

fn button(ui: &mut egui::Ui, glyph: impl Into<egui::WidgetText>, tip: &str, enabled: bool) -> bool {
    super::theme::hover_fill(ui, |ui| {
        ui.add_enabled(enabled, Button::new(glyph).frame(false))
    })
    .on_hover_text(tip)
    .clicked()
}
