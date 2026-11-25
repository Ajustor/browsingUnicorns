//! Side panel with the history, the bookmarks and the browser's own views
//! (sync, extensions, profiles, settings, what's new, about). A side panel
//! and not a popup: popups would be drawn under the page's native view.

use application::Target;
use domain::{Bookmarks, BrowsingHistory, Timestamp, WebAddress};
use egui::{Button, Margin, RichText, ScrollArea, SidePanel, TextEdit};
use egui_phosphor::regular as icon;

use updater::{UpdateState, Updater};

use super::command::Command;
use super::extensions::{ExtensionsPanel, ExtensionsView};
use super::panel_kit::{
    self, ACCENT, badge, card, card_frame, empty_state, icon_button, icon_tabs, list_heading,
    list_row, primary_button, search_field, site_badge,
};
use super::profiles::{ProfilesPanel, ProfilesRunner};
use super::settings_panel::{SettingsPanel, SettingsView};
use super::sync_panel::{SyncPanel, SyncView};
use super::whats_new::WhatsNewPanel;

const MAX_ROWS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryView {
    History,
    Bookmarks,
    Sync,
    Extensions,
    Profiles,
    Settings,
    WhatsNew,
    About,
}

impl LibraryView {
    /// In the order of the bar.
    const ALL: [LibraryView; 8] = [
        LibraryView::History,
        LibraryView::Bookmarks,
        LibraryView::Sync,
        LibraryView::Extensions,
        LibraryView::Profiles,
        LibraryView::Settings,
        LibraryView::WhatsNew,
        LibraryView::About,
    ];

    fn glyph(self) -> &'static str {
        match self {
            LibraryView::History => icon::CLOCK_COUNTER_CLOCKWISE,
            LibraryView::Bookmarks => icon::STAR,
            LibraryView::Sync => icon::ARROWS_CLOCKWISE,
            LibraryView::Extensions => icon::PUZZLE_PIECE,
            LibraryView::Profiles => icon::USER_CIRCLE,
            LibraryView::Settings => icon::GEAR_SIX,
            LibraryView::WhatsNew => icon::SPARKLE,
            LibraryView::About => icon::INFO,
        }
    }

    fn name(self) -> &'static str {
        match self {
            LibraryView::History => i18n::library::history_pill(),
            LibraryView::Bookmarks => i18n::library::bookmarks_pill(),
            LibraryView::Sync => i18n::library::sync_pill(),
            LibraryView::Extensions => i18n::library::extensions_pill(),
            LibraryView::Profiles => i18n::library::profiles_pill(),
            LibraryView::Settings => i18n::library::settings_pill(),
            LibraryView::WhatsNew => i18n::library::whats_new_pill(),
            LibraryView::About => i18n::library::about_pill(),
        }
    }

    /// Writes to the profile: not in a private window.
    fn needs_profile(self) -> bool {
        matches!(
            self,
            LibraryView::Sync
                | LibraryView::Extensions
                | LibraryView::Profiles
                | LibraryView::Settings
        )
    }
}

/// What the panel shows, borrowed for one frame.
pub struct LibraryData<'a> {
    pub history: &'a BrowsingHistory,
    pub bookmarks: &'a Bookmarks,
    pub now: Timestamp,
    pub updater: &'a Updater,
    pub sync: SyncView<'a>,
    pub extensions: ExtensionsView<'a>,
    pub profiles: &'a ProfilesRunner,
    /// A private window: no sync, extensions or profiles.
    pub private: bool,
    /// Whether it is the system's default browser (`None`: unknown, or not
    /// available here).
    pub default_browser: Option<bool>,
    pub settings: SettingsView<'a>,
    /// This version's notes were not looked at yet.
    pub unseen_notes: bool,
}

#[derive(Default)]
pub struct Library {
    pub open: Option<LibraryView>,
    filter: String,
    sync: SyncPanel,
    extensions: ExtensionsPanel,
    pub profiles: ProfilesPanel,
    pub settings: SettingsPanel,
    whats_new: WhatsNewPanel,
    /// The bookmark being moved to a folder, and the folder typed.
    moving: Option<(WebAddress, String)>,
    /// The browser's icon, for the About view.
    logo: Option<egui::TextureHandle>,
}

impl Library {
    pub fn toggle(&mut self, view: LibraryView) {
        self.open = if self.open == Some(view) {
            None
        } else {
            Some(view)
        };
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        data: LibraryData<'_>,
        commands: &mut Vec<Command>,
    ) {
        let Some(view) = self.open else {
            return;
        };
        let LibraryData {
            history,
            bookmarks,
            now,
            updater,
            sync,
            extensions,
            profiles,
            private,
            default_browser,
            settings,
            unseen_notes,
        } = data;
        SidePanel::right("library")
            .resizable(true)
            .default_width(340.0)
            .min_width(260.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style()).inner_margin(Margin::symmetric(12, 10)),
            )
            .show(ctx, |ui| {
                head(ui, view, unseen_notes, commands);
                if private && view.needs_profile() {
                    empty_state(
                        ui,
                        icon::MASK_HAPPY,
                        i18n::errors::infrastructure::not_in_private_window(),
                    );
                    return;
                }
                match view {
                    LibraryView::History => {
                        self.filter_row(ui, Some(history), commands);
                        scrolled(ui, |ui| self.history(ui, history, now, commands));
                    }
                    LibraryView::Bookmarks => {
                        self.filter_row(ui, None, commands);
                        scrolled(ui, |ui| self.bookmarks(ui, bookmarks, commands));
                    }
                    LibraryView::Sync => scrolled(ui, |ui| self.sync.show(ui, sync, now, commands)),
                    LibraryView::Extensions => {
                        scrolled(ui, |ui| self.extensions.show(ui, extensions, commands));
                    }
                    LibraryView::Profiles => {
                        scrolled(ui, |ui| self.profiles.show(ui, profiles, commands));
                    }
                    LibraryView::Settings => {
                        scrolled(ui, |ui| self.settings.show(ui, settings, commands));
                    }
                    LibraryView::WhatsNew => {
                        let current = updater.current_version();
                        scrolled(ui, |ui| self.whats_new.show(ui, current));
                    }
                    LibraryView::About => {
                        let logo = self.logo(ctx).id();
                        scrolled(ui, |ui| about(ui, logo, updater, default_browser, commands));
                    }
                }
            });
    }

    /// The browser's icon, loaded once.
    fn logo(&mut self, ctx: &egui::Context) -> &egui::TextureHandle {
        self.logo.get_or_insert_with(|| {
            let image = eframe::icon_data::from_png_bytes(include_bytes!("../../assets/icon.png"))
                .map(|icon| {
                    egui::ColorImage::from_rgba_unmultiplied(
                        [icon.width as usize, icon.height as usize],
                        &icon.rgba,
                    )
                })
                .unwrap_or_default();
            ctx.load_texture("browser logo", image, egui::TextureOptions::LINEAR)
        })
    }

    /// The field over the history and the bookmarks; the history's has
    /// the button that clears it.
    fn filter_row(
        &mut self,
        ui: &mut egui::Ui,
        history: Option<&BrowsingHistory>,
        commands: &mut Vec<Command>,
    ) {
        ui.horizontal(|ui| {
            let clear = if history.is_some() { 34.0 } else { 0.0 };
            ui.allocate_ui(egui::vec2(ui.available_width() - clear, 0.0), |ui| {
                search_field(ui, &mut self.filter, i18n::library::filter_hint());
            });
            if let Some(history) = history
                && ui
                    .add_enabled_ui(!history.is_empty(), |ui| {
                        icon_button(ui, icon::TRASH, i18n::history::clear_history())
                    })
                    .inner
                    .clicked()
            {
                commands.push(Command::ClearHistory);
            }
        });
        ui.add_space(6.0);
    }

    fn history(
        &self,
        ui: &mut egui::Ui,
        history: &BrowsingHistory,
        now: Timestamp,
        commands: &mut Vec<Command>,
    ) {
        let visits = history.search(&self.filter, MAX_ROWS);
        if visits.is_empty() {
            empty_state(ui, icon::CLOCK_COUNTER_CLOCKWISE, i18n::history::no_pages());
            return;
        }
        let mut part = None;
        for visit in visits {
            let age = now.saturating_sub(visit.at);
            let this = age_part(age);
            if part != Some(this) {
                list_heading(ui, age_part_name(this));
                part = Some(this);
            }
            let title = if visit.title.is_empty() {
                visit.address.host().unwrap_or(visit.address.as_str())
            } else {
                &visit.title
            };
            page_row(ui, title, &visit.address, Some(ago_secs(age)), commands);
        }
    }

    /// Opens the bookmarks of `folder`.
    pub fn show_folder(&mut self, folder: String) {
        self.open = Some(LibraryView::Bookmarks);
        self.filter = folder;
    }

    fn bookmarks(&mut self, ui: &mut egui::Ui, bookmarks: &Bookmarks, commands: &mut Vec<Command>) {
        let filter = self.filter.to_lowercase();
        let matches = |b: &&domain::Bookmark| {
            filter.is_empty()
                || format!("{} {} {}", b.title, b.address, b.folder)
                    .to_lowercase()
                    .contains(&filter)
        };
        let folders = bookmarks.folders();
        let mut any = false;
        let moving = &mut self.moving;
        // Those out of any folder first, then each folder's.
        let mut rows = |ui: &mut egui::Ui, folder: &str| {
            for b in bookmarks
                .items()
                .iter()
                .filter(|b| b.folder == folder)
                .filter(matches)
            {
                any = true;
                bookmark_row(ui, b, &folders, moving, commands);
            }
        };
        rows(ui, "");
        for folder in &folders {
            let count = bookmarks
                .items()
                .iter()
                .filter(|b| &b.folder == folder)
                .filter(matches)
                .count();
            if count == 0 {
                continue;
            }
            let id = ui.make_persistent_id(("bookmark folder", folder));
            egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                id,
                !filter.is_empty(),
            )
            .show_header(ui, |ui| {
                ui.label(RichText::new(icon::FOLDER).color(panel_kit::accent_text(ui)));
                ui.label(RichText::new(folder).strong());
                panel_kit::chip(ui, &count.to_string(), panel_kit::accent_text(ui));
            })
            .body(|ui| rows(ui, folder));
        }
        if !any {
            empty_state(ui, icon::STAR, &i18n::bookmarks::no_bookmarks(icon::STAR));
        }
    }
}

/// The panel's head: the view's name, the close button, and the bar of
/// views.
fn head(ui: &mut egui::Ui, view: LibraryView, unseen_notes: bool, commands: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        badge(ui, view.glyph(), 30.0, ACCENT);
        ui.add_space(2.0);
        ui.label(RichText::new(view.name()).strong().size(18.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_button(ui, icon::X, i18n::common::close()).clicked() {
                commands.push(Command::ToggleLibrary(view));
            }
        });
    });
    ui.add_space(8.0);
    let tabs: Vec<(&str, &str, bool)> = LibraryView::ALL
        .iter()
        .map(|v| {
            (
                v.glyph(),
                v.name(),
                *v == LibraryView::WhatsNew && unseen_notes,
            )
        })
        .collect();
    let chosen = LibraryView::ALL
        .iter()
        .position(|v| *v == view)
        .unwrap_or(0);
    if let Some(i) = icon_tabs(ui, &tabs, chosen) {
        commands.push(Command::ToggleLibrary(LibraryView::ALL[i]));
    }
    ui.add_space(10.0);
}

/// A view in a scroll area, with room on the right for the scroll bar.
fn scrolled(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(Margin {
                    right: 10,
                    ..Default::default()
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    body(ui);
                });
        });
}

/// The parts of the history, by age.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AgePart {
    LastHour,
    LastDay,
    ThisWeek,
    ThisMonth,
    Older,
}

fn age_part(secs: u64) -> AgePart {
    const HOUR: u64 = 3600;
    const DAY: u64 = 24 * HOUR;
    match secs {
        s if s < HOUR => AgePart::LastHour,
        s if s < DAY => AgePart::LastDay,
        s if s < 7 * DAY => AgePart::ThisWeek,
        s if s < 30 * DAY => AgePart::ThisMonth,
        _ => AgePart::Older,
    }
}

fn age_part_name(part: AgePart) -> &'static str {
    match part {
        AgePart::LastHour => i18n::history::last_hour(),
        AgePart::LastDay => i18n::history::last_day(),
        AgePart::ThisWeek => i18n::history::this_week(),
        AgePart::ThisMonth => i18n::history::this_month(),
        AgePart::Older => i18n::history::older(),
    }
}

/// A bookmark: the page, then the buttons to move it to a folder and to
/// remove it.
fn bookmark_row(
    ui: &mut egui::Ui,
    b: &domain::Bookmark,
    folders: &[String],
    moving: &mut Option<(WebAddress, String)>,
    commands: &mut Vec<Command>,
) {
    let response = list_row(ui, |ui| {
        page_line(ui, b.label(), &b.address, |ui| {
            if icon_button(ui, icon::X, i18n::bookmarks::remove_tip()).clicked() {
                commands.push(Command::RemoveBookmark(b.address.clone()));
            }
            if icon_button(ui, icon::FOLDER_SIMPLE, i18n::bookmarks::move_tip()).clicked() {
                *moving = match moving {
                    Some((a, _)) if *a == b.address => None,
                    _ => Some((b.address.clone(), b.folder.clone())),
                };
            }
        });
    })
    .on_hover_text(b.address.as_str());
    open_on_click(&response, &b.address, commands);
    let Some((address, typed)) = moving.as_mut().filter(|(a, _)| *a == b.address) else {
        return;
    };
    let mut chosen = None;
    card_frame(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            if super::theme::pill(ui, b.folder.is_empty(), i18n::bookmarks::no_folder()).clicked() {
                chosen = Some(String::new());
            }
            for f in folders {
                if super::theme::pill(ui, &b.folder == f, f.as_str()).clicked() {
                    chosen = Some(f.clone());
                }
            }
        });
        ui.horizontal(|ui| {
            let field = ui.add(
                TextEdit::singleline(typed)
                    .hint_text(i18n::bookmarks::new_folder_hint())
                    .margin(Margin::symmetric(8, 6))
                    .desired_width(ui.available_width() - 48.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (primary_button(ui, i18n::common::ok(), !typed.trim().is_empty()).clicked() || enter)
                && !typed.trim().is_empty()
            {
                chosen = Some(typed.clone());
            }
        });
    });
    if let Some(folder) = chosen {
        commands.push(Command::MoveBookmark(address.clone(), folder));
        *moving = None;
    }
}

/// A page of the history: click opens it here, middle click in a new tab.
fn page_row(
    ui: &mut egui::Ui,
    title: &str,
    address: &WebAddress,
    age: Option<String>,
    commands: &mut Vec<Command>,
) {
    let response = list_row(ui, |ui| {
        page_line(ui, title, address, |ui| {
            if let Some(age) = age {
                ui.label(RichText::new(age).size(11.5).weak());
            }
        });
    })
    .on_hover_text(address.as_str());
    open_on_click(&response, address, commands);
}

/// A site's badge, the title and the host under it, then `trailing`
/// laid out from the right. The texts are cut to the width left (content
/// wider than a resizable panel makes it grow at every frame).
fn page_line(
    ui: &mut egui::Ui,
    title: &str,
    address: &WebAddress,
    trailing: impl FnOnce(&mut egui::Ui),
) {
    let host = address.host().unwrap_or(address.as_str());
    ui.horizontal(|ui| {
        site_badge(ui, host, 28.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            trailing(ui);
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(egui::Label::new(RichText::new(title).strong()).truncate());
                ui.add(egui::Label::new(RichText::new(host).size(11.5).weak()).truncate());
            });
        });
    });
}

fn open_on_click(response: &egui::Response, address: &WebAddress, commands: &mut Vec<Command>) {
    if response.clicked() {
        commands.push(Command::Open(address.clone(), Target::ActiveTab));
    } else if response.middle_clicked() {
        commands.push(Command::Open(address.clone(), Target::NewTab));
    }
}

/// The browser's icon and version, the updates, the default browser.
fn about(
    ui: &mut egui::Ui,
    logo: egui::TextureId,
    updater: &Updater,
    default_browser: Option<bool>,
    commands: &mut Vec<Command>,
) {
    card_frame(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            ui.add(egui::Image::new((logo, egui::vec2(72.0, 72.0))));
            ui.add_space(4.0);
            ui.label(RichText::new("browsingUnicorns").strong().size(20.0));
            ui.label(RichText::new(i18n::about::tagline()).weak());
            ui.add_space(4.0);
            panel_kit::chip(
                ui,
                &i18n::about::version(&updater.current_version()),
                panel_kit::accent_text(ui),
            );
            ui.add_space(4.0);
            if ui
                .add(Button::new(format!(
                    "{} {}",
                    icon::SPARKLE,
                    i18n::whats_new::see_whats_new()
                )))
                .clicked()
            {
                commands.push(Command::ToggleLibrary(LibraryView::WhatsNew));
            }
            ui.add_space(4.0);
        });
    });

    card(
        ui,
        icon::ARROW_CIRCLE_UP,
        i18n::about::updates(),
        None,
        |ui| {
            let (glyph, status, color) = match &updater.state {
                UpdateState::Idle => (icon::CIRCLE, String::new(), None),
                UpdateState::Checking => (icon::HOURGLASS, i18n::about::checking().into(), None),
                UpdateState::UpToDate => (
                    icon::CHECK_CIRCLE,
                    i18n::about::up_to_date().into(),
                    Some(panel_kit::ok_text(ui)),
                ),
                UpdateState::Available(i) => (
                    icon::GIFT,
                    i18n::about::available(&i.version),
                    Some(panel_kit::cyan_text(ui)),
                ),
                UpdateState::Downloading(i) => (
                    icon::DOWNLOAD_SIMPLE,
                    i18n::about::downloading(&i.version),
                    None,
                ),
                UpdateState::Ready(i) => (
                    icon::ROCKET_LAUNCH,
                    i18n::about::ready(&i.version),
                    Some(panel_kit::cyan_text(ui)),
                ),
                UpdateState::Failed(e) => (
                    icon::WARNING,
                    i18n::about::check_failed(e),
                    Some(ui.visuals().error_fg_color),
                ),
            };
            if !status.is_empty() {
                ui.horizontal(|ui| {
                    if matches!(
                        updater.state,
                        UpdateState::Checking | UpdateState::Downloading(_)
                    ) {
                        ui.spinner();
                    } else {
                        let glyph = RichText::new(glyph).size(16.0);
                        ui.label(match color {
                            Some(c) => glyph.color(c),
                            None => glyph,
                        });
                    }
                    ui.add(egui::Label::new(status).wrap());
                });
            }
            if primary_button(
                ui,
                format!(
                    "{} {}",
                    icon::ARROW_CLOCKWISE,
                    i18n::about::check_for_updates()
                ),
                !updater.is_busy(),
            )
            .clicked()
            {
                commands.push(Command::CheckForUpdates);
            }
            // What the update brings.
            if let UpdateState::Available(i) | UpdateState::Downloading(i) | UpdateState::Ready(i) =
                &updater.state
                && !i.notes.trim().is_empty()
            {
                ui.separator();
                ui.label(RichText::new(i18n::whats_new::in_this_update(&i.version)).strong());
                let lines: Vec<&str> = i.notes.lines().collect();
                super::whats_new::notes(ui, &super::whats_new::sections(&lines));
            }
        },
    );

    if let Some(is_default) = default_browser {
        card(
            ui,
            icon::BROWSER,
            i18n::about::default_browser(),
            None,
            |ui| {
                if is_default {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(icon::CHECK_CIRCLE)
                                .size(16.0)
                                .color(panel_kit::ok_text(ui)),
                        );
                        ui.add(egui::Label::new(i18n::about::is_default()).wrap());
                    });
                } else {
                    ui.add(egui::Label::new(i18n::about::is_not_default()).wrap());
                    if primary_button(
                        ui,
                        format!("{} {}", icon::BROWSER, i18n::about::make_default()),
                        true,
                    )
                    .clicked()
                    {
                        commands.push(Command::DefaultBrowser(
                            super::default_browser::DefaultBrowserCommand::Make,
                        ));
                    }
                }
            },
        );
    }
}

/// "5 min ago", "3 h ago", "2 d ago".
#[cfg(test)]
fn ago(now: Timestamp, at: Timestamp) -> String {
    ago_secs(now.saturating_sub(at))
}

/// "just now", "5 min ago", "3 h ago", "2 d ago" for an age in seconds
/// ("à l’instant", "il y a 5 min"… in French).
pub fn ago_secs(secs: u64) -> String {
    i18n::history::ago(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_are_short() {
        i18n::with_lang(i18n::Lang::En, || {
            assert_eq!(ago(100, 90), "just now");
            assert_eq!(ago(10_000, 10_000 - 300), "5 min ago");
            assert_eq!(ago(100_000, 100_000 - 7_200), "2 h ago");
            assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "3 d ago");
            assert_eq!(ago(0, 50), "just now");
        });
    }

    #[test]
    fn ages_in_french() {
        i18n::with_lang(i18n::Lang::Fr, || {
            assert_eq!(ago(100, 90), "à l’instant");
            assert_eq!(ago(10_000, 10_000 - 300), "il y a 5\u{a0}min");
            assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "il y a 3\u{a0}j");
        });
    }

    #[test]
    fn the_history_is_cut_by_age() {
        assert_eq!(age_part(0), AgePart::LastHour);
        assert_eq!(age_part(3_599), AgePart::LastHour);
        assert_eq!(age_part(3_600), AgePart::LastDay);
        assert_eq!(age_part(2 * 86_400), AgePart::ThisWeek);
        assert_eq!(age_part(10 * 86_400), AgePart::ThisMonth);
        assert_eq!(age_part(90 * 86_400), AgePart::Older);
    }

    #[test]
    fn toggling_the_open_view_closes_it() {
        let mut lib = Library::default();
        lib.toggle(LibraryView::History);
        assert_eq!(lib.open, Some(LibraryView::History));
        lib.toggle(LibraryView::Bookmarks);
        assert_eq!(lib.open, Some(LibraryView::Bookmarks));
        lib.toggle(LibraryView::Bookmarks);
        assert_eq!(lib.open, None);
    }
}
