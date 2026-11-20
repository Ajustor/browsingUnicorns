//! Side panel with the history and the bookmarks. A side panel and not a
//! popup: popups would be drawn under the page's native view.

use application::Target;
use domain::{Bookmarks, BrowsingHistory, Timestamp, WebAddress};
use egui::{Button, RichText, ScrollArea, SidePanel, TextEdit};
use egui_phosphor::regular as icon;

use updater::{UpdateState, Updater};

use super::command::Command;
use super::extensions::{ExtensionsPanel, ExtensionsView};
use super::profiles::{ProfilesPanel, ProfilesRunner};
use super::settings_panel::{SettingsPanel, SettingsView};
use super::sync_panel::{SyncPanel, SyncView};

const MAX_ROWS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryView {
    History,
    Bookmarks,
    Sync,
    Extensions,
    Profiles,
    Settings,
    About,
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
}

#[derive(Default)]
pub struct Library {
    pub open: Option<LibraryView>,
    filter: String,
    sync: SyncPanel,
    extensions: ExtensionsPanel,
    pub profiles: ProfilesPanel,
    pub settings: SettingsPanel,
    /// The bookmark being moved to a folder, and the folder typed.
    moving: Option<(WebAddress, String)>,
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
        } = data;
        SidePanel::right("library")
            .resizable(true)
            .default_width(320.0)
            .min_width(220.0)
            .show(ctx, |ui| {
                // The views wrap in the width left by the close button: a row
                // wider than the panel would make it grow.
                ui.horizontal_top(|ui| {
                    let close_width = ui.spacing().interact_size.y + ui.spacing().item_spacing.x;
                    let tabs =
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width() - close_width, 0.0),
                        tabs,
                        |ui| {
                            ui.set_min_width(ui.available_width());
                            for (v, label) in [
                                (
                                    LibraryView::History,
                                    format!(
                                        "{} {}",
                                        icon::CLOCK_COUNTER_CLOCKWISE,
                                        i18n::library::history_pill()
                                    ),
                                ),
                                (
                                    LibraryView::Bookmarks,
                                    format!("{} {}", icon::STAR, i18n::library::bookmarks_pill()),
                                ),
                                (
                                    LibraryView::Sync,
                                    format!(
                                        "{} {}",
                                        icon::ARROWS_CLOCKWISE,
                                        i18n::library::sync_pill()
                                    ),
                                ),
                                (
                                    LibraryView::Extensions,
                                    format!(
                                        "{} {}",
                                        icon::PUZZLE_PIECE,
                                        i18n::library::extensions_pill()
                                    ),
                                ),
                                (
                                    LibraryView::Profiles,
                                    format!(
                                        "{} {}",
                                        icon::USER_CIRCLE,
                                        i18n::library::profiles_pill()
                                    ),
                                ),
                                (
                                    LibraryView::Settings,
                                    format!(
                                        "{} {}",
                                        icon::GEAR_SIX,
                                        i18n::library::settings_pill()
                                    ),
                                ),
                                (
                                    LibraryView::About,
                                    format!("{} {}", icon::INFO, i18n::library::about_pill()),
                                ),
                            ] {
                                if super::theme::pill(ui, view == v, label).clicked() && view != v {
                                    commands.push(Command::ToggleLibrary(v));
                                }
                            }
                        },
                    );
                    if super::theme::hover_fill(ui, |ui| ui.add(Button::new(icon::X).frame(false)))
                        .on_hover_text(i18n::common::close())
                        .clicked()
                    {
                        commands.push(Command::ToggleLibrary(view));
                    }
                });
                let not_private = matches!(
                    view,
                    LibraryView::Sync
                        | LibraryView::Extensions
                        | LibraryView::Profiles
                        | LibraryView::Settings
                );
                if private && not_private {
                    ui.separator();
                    ui.weak(i18n::errors::infrastructure::not_in_private_window());
                    return;
                }
                match view {
                    LibraryView::About => {
                        ui.separator();
                        about(ui, updater, default_browser, commands);
                        return;
                    }
                    LibraryView::Sync => {
                        ui.separator();
                        self.sync.show(ui, sync, now, commands);
                        return;
                    }
                    LibraryView::Extensions => {
                        ui.separator();
                        self.extensions.show(ui, extensions, commands);
                        return;
                    }
                    LibraryView::Profiles => {
                        ui.separator();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            self.profiles.show(ui, profiles, commands);
                        });
                        return;
                    }
                    LibraryView::Settings => {
                        ui.separator();
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            self.settings.show(ui, settings, commands);
                        });
                        return;
                    }
                    _ => {}
                }
                ui.add(
                    TextEdit::singleline(&mut self.filter)
                        .hint_text(i18n::library::filter_hint())
                        .desired_width(f32::INFINITY),
                );
                ui.separator();
                match view {
                    LibraryView::History => self.history(ui, history, now, commands),
                    LibraryView::Bookmarks => self.bookmarks(ui, bookmarks, commands),
                    LibraryView::About
                    | LibraryView::Sync
                    | LibraryView::Extensions
                    | LibraryView::Profiles
                    | LibraryView::Settings => {}
                }
            });
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
            ui.weak(i18n::history::no_pages());
        }
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .max_height(ui.available_height() - 36.0)
            .show(ui, |ui| {
                for visit in visits {
                    let title = if visit.title.is_empty() {
                        visit.address.host().unwrap_or(visit.address.as_str())
                    } else {
                        &visit.title
                    };
                    entry(
                        ui,
                        title,
                        &visit.address,
                        Some(ago(now, visit.at)),
                        commands,
                    );
                }
            });
        ui.separator();
        if ui
            .add_enabled(
                !history.is_empty(),
                Button::new(format!(
                    "{} {}",
                    icon::TRASH,
                    i18n::history::clear_history()
                )),
            )
            .clicked()
        {
            commands.push(Command::ClearHistory);
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
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
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
                    egui::CollapsingHeader::new(format!("{} {folder} ({count})", icon::FOLDER))
                        .id_salt(folder)
                        .default_open(!filter.is_empty())
                        .show(ui, |ui| rows(ui, folder));
                }
            });
        if !any {
            ui.weak(i18n::bookmarks::no_bookmarks(icon::STAR));
        }
    }
}

/// A bookmark: remove, move to a folder, and the page itself.
fn bookmark_row(
    ui: &mut egui::Ui,
    b: &domain::Bookmark,
    folders: &[String],
    moving: &mut Option<(WebAddress, String)>,
    commands: &mut Vec<Command>,
) {
    ui.horizontal(|ui| {
        if super::theme::hover_fill(ui, |ui| ui.add(Button::new(icon::X).frame(false).small()))
            .on_hover_text(i18n::bookmarks::remove_tip())
            .clicked()
        {
            commands.push(Command::RemoveBookmark(b.address.clone()));
        }
        if super::theme::hover_fill(ui, |ui| {
            ui.add(Button::new(icon::FOLDER_SIMPLE).frame(false).small())
        })
        .on_hover_text(i18n::bookmarks::move_tip())
        .clicked()
        {
            *moving = match moving {
                Some((a, _)) if *a == b.address => None,
                _ => Some((b.address.clone(), b.folder.clone())),
            };
        }
        ui.vertical(|ui| entry(ui, b.label(), &b.address, None, commands));
    });
    let Some((address, typed)) = moving.as_mut().filter(|(a, _)| *a == b.address) else {
        return;
    };
    let mut chosen = None;
    egui::Frame::group(ui.style()).show(ui, |ui| {
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
                    .desired_width(ui.available_width() - 40.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button(i18n::common::ok()).clicked() || enter) && !typed.trim().is_empty() {
                chosen = Some(typed.clone());
            }
        });
    });
    if let Some(folder) = chosen {
        commands.push(Command::MoveBookmark(address.clone(), folder));
        *moving = None;
    }
}

/// Version and update check.
fn about(
    ui: &mut egui::Ui,
    updater: &Updater,
    default_browser: Option<bool>,
    commands: &mut Vec<Command>,
) {
    ui.heading("browsingUnicorns");
    ui.label(i18n::about::version(&updater.current_version()));
    ui.add_space(8.0);
    let status = match &updater.state {
        UpdateState::Idle => String::new(),
        UpdateState::Checking => i18n::about::checking().into(),
        UpdateState::UpToDate => i18n::about::up_to_date().into(),
        UpdateState::Available(i) => i18n::about::available(&i.version),
        UpdateState::Downloading(i) => i18n::about::downloading(&i.version),
        UpdateState::Ready(i) => i18n::about::ready(&i.version),
        UpdateState::Failed(e) => i18n::about::check_failed(e),
    };
    if !status.is_empty() {
        ui.label(status);
    }
    if ui
        .add_enabled(
            !updater.is_busy(),
            Button::new(format!(
                "{} {}",
                icon::ARROW_CLOCKWISE,
                i18n::about::check_for_updates()
            )),
        )
        .clicked()
    {
        commands.push(Command::CheckForUpdates);
    }
    ui.add_space(12.0);
    match default_browser {
        Some(true) => {
            ui.label(format!(
                "{} {}",
                icon::CHECK_CIRCLE,
                i18n::about::is_default()
            ));
        }
        Some(false) => {
            ui.label(i18n::about::is_not_default());
            if ui
                .button(format!("{} {}", icon::BROWSER, i18n::about::make_default()))
                .clicked()
            {
                commands.push(Command::DefaultBrowser(
                    super::default_browser::DefaultBrowserCommand::Make,
                ));
            }
        }
        None => {}
    }
}

/// One page: title, address and age. Click opens it here, middle click in a
/// new tab.
fn entry(
    ui: &mut egui::Ui,
    title: &str,
    address: &WebAddress,
    age: Option<String>,
    commands: &mut Vec<Command>,
) {
    let response = super::theme::hover_fill(ui, |ui| {
        ui.add(
            Button::new(RichText::new(title).strong())
                .frame(false)
                .truncate(),
        )
    })
    .on_hover_text(address.as_str());
    // One row: the address cut to what the age leaves, the age on the right.
    // Both must fit the panel's width (content wider than a resizable panel
    // makes it grow at every frame), and the row its own height (a layout
    // without a row would take the whole panel).
    ui.horizontal(|ui| {
        let age = age.map(|a| RichText::new(a).small());
        let age_width = age.as_ref().map_or(0.0, |a| {
            egui::WidgetText::from(a.clone())
                .into_galley(
                    ui,
                    Some(egui::TextWrapMode::Extend),
                    f32::INFINITY,
                    egui::TextStyle::Small,
                )
                .size()
                .x
                + ui.spacing().item_spacing.x
        });
        let address_width = (ui.available_width() - age_width).max(0.0);
        ui.scope(|ui| {
            ui.set_max_width(address_width);
            ui.add(egui::Label::new(RichText::new(address.as_str()).small().weak()).truncate());
        });
        if let Some(age) = age {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(age);
            });
        }
    });
    ui.add_space(4.0);
    if response.clicked() {
        commands.push(Command::Open(address.clone(), Target::ActiveTab));
    } else if response.middle_clicked() {
        commands.push(Command::Open(address.clone(), Target::NewTab));
    }
}

/// "5 min ago", "3 h ago", "2 d ago".
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
