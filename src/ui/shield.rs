//! The ad blocker's shield: a bar under the toolbar (the page's native view
//! would hide anything drawn over it) with what was blocked on the page and
//! the switches.

use std::time::SystemTime;

use domain::WebAddress;
use egui::{RichText, TopBottomPanel};
use egui_phosphor::regular as icon;

use super::command::Command;

/// The site whose shield is shown.
pub struct Shield {
    pub address: WebAddress,
}

/// What the bar shows.
pub struct ShieldView {
    /// The ad blocker is on.
    pub enabled: bool,
    /// It blocks on this site.
    pub on_site: bool,
    /// What it stopped on the page; `None` where the web view doesn't tell
    /// (WebKit).
    pub blocked: Option<u32>,
    /// When the lists were downloaded; `None` before they first arrive.
    pub updated: Option<SystemTime>,
}

impl Shield {
    /// Shows the bar; false once closed.
    pub fn show(
        &self,
        ctx: &egui::Context,
        view: &ShieldView,
        commands: &mut Vec<Command>,
    ) -> bool {
        let mut open = true;
        let host = self
            .address
            .host()
            .unwrap_or(self.address.as_str())
            .to_owned();
        TopBottomPanel::top("shield").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let glyph = if view.enabled && view.on_site {
                    icon::SHIELD_CHECK
                } else {
                    icon::SHIELD_SLASH
                };
                ui.label(RichText::new(format!("{glyph} {host}")).strong());
                ui.label(status(view));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(icon::X)
                        .on_hover_text(i18n::common::close())
                        .clicked()
                    {
                        open = false;
                    }
                });
            });
            ui.horizontal(|ui| {
                let mut on_site = view.on_site;
                let site = ui.add_enabled(
                    view.enabled,
                    egui::Checkbox::new(&mut on_site, i18n::shield::block_on_this_site()),
                );
                if site.changed() {
                    commands.push(Command::SetBlockerForSite(host.clone(), on_site));
                }
                let mut enabled = view.enabled;
                if ui
                    .checkbox(&mut enabled, i18n::shield::ad_blocker())
                    .changed()
                {
                    commands.push(Command::SetBlocker(enabled));
                }
            });
            ui.label(RichText::new(lists_line(view.updated, SystemTime::now())).weak());
        });
        open
    }
}

/// The bar's first line.
fn status(view: &ShieldView) -> String {
    if !view.enabled {
        return i18n::shield::blocker_off().into();
    }
    if !view.on_site {
        return i18n::shield::not_blocking_here().into();
    }
    match view.blocked {
        Some(n) => i18n::shield::items_blocked(n),
        None => i18n::shield::active().into(),
    }
}

/// Which lists, and how fresh they are.
fn lists_line(updated: Option<SystemTime>, now: SystemTime) -> String {
    match updated {
        None => i18n::shield::lists_not_downloaded().into(),
        Some(at) => i18n::shield::lists_updated(now.duration_since(at).map_or(0, |d| d.as_secs())),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn the_status_says_what_happens() {
        let mut view = ShieldView {
            enabled: true,
            on_site: true,
            blocked: Some(3),
            updated: None,
        };
        assert_eq!(status(&view), "3 items blocked on this page.");
        view.blocked = None;
        assert_eq!(status(&view), "Active: blocking ads and trackers.");
        view.on_site = false;
        assert_eq!(status(&view), "Not blocking on this site.");
        view.enabled = false;
        assert_eq!(status(&view), "The ad blocker is off.");
    }

    #[test]
    fn the_lists_line_tells_their_age() {
        let now = SystemTime::now();
        assert!(lists_line(None, now).ends_with("not downloaded yet"));
        assert!(lists_line(Some(now), now).ends_with("updated just now"));
        let earlier = now - Duration::from_secs(3 * 3_600);
        assert!(lists_line(Some(earlier), now).ends_with("updated 3 h ago"));
    }

    #[test]
    fn the_bar_speaks_french() {
        let view = ShieldView {
            enabled: true,
            on_site: true,
            blocked: Some(1),
            updated: None,
        };
        let (status, lists) = i18n::with_lang(i18n::Lang::Fr, || {
            (
                status(&view),
                lists_line(Some(SystemTime::now()), SystemTime::now()),
            )
        });
        assert_eq!(status, "1 élément bloqué sur cette page.");
        assert!(lists.ends_with("mises à jour à l'instant"), "{lists}");
    }
}
