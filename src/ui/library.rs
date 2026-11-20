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
                                    format!("{} History", icon::CLOCK_COUNTER_CLOCKWISE),
                                ),
                                (LibraryView::Bookmarks, format!("{} Bookmarks", icon::STAR)),
                                (
                                    LibraryView::Sync,
                                    format!("{} Sync", icon::ARROWS_CLOCKWISE),
                                ),
                                (
                                    LibraryView::Extensions,
                                    format!("{} Extensions", icon::PUZZLE_PIECE),
                                ),
                                (
                                    LibraryView::Profiles,
                                    format!("{} Profiles", icon::USER_CIRCLE),
                                ),
                                (
                                    LibraryView::Settings,
                                    format!(
                                        "{} {}",
                                        icon::GEAR_SIX,
                                        i18n::library::settings_pill()
                                    ),
                                ),
                                (LibraryView::About, format!("{} About", icon::INFO)),
                            ] {
                                if super::theme::pill(ui, view == v, label).clicked() && view != v {
                                    commands.push(Command::ToggleLibrary(v));
                                }
                            }
                        },
                    );
                    if super::theme::hover_fill(ui, |ui| ui.add(Button::new(icon::X).frame(false)))
                        .on_hover_text("Close")
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
                    ui.weak(infrastructure::NOT_IN_PRIVATE);
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
                        .hint_text("Filter")
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
            ui.weak("No pages.");
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
                Button::new(format!("{} Clear history", icon::TRASH)),
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
            ui.weak(format!(
                "No bookmarks. {} in the address bar or Ctrl+D saves the page; \
                 Settings imports other browsers'.",
                icon::STAR
            ));
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
            .on_hover_text("Remove bookmark")
            .clicked()
        {
            commands.push(Command::RemoveBookmark(b.address.clone()));
        }
        if super::theme::hover_fill(ui, |ui| {
            ui.add(Button::new(icon::FOLDER_SIMPLE).frame(false).small())
        })
        .on_hover_text("Move to a folder")
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
            if super::theme::pill(ui, b.folder.is_empty(), "No folder").clicked() {
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
                    .hint_text("New folder (Work/Projects)")
                    .desired_width(ui.available_width() - 40.0),
            );
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("OK").clicked() || enter) && !typed.trim().is_empty() {
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
    ui.label(format!("Version {}", updater.current_version()));
    ui.add_space(8.0);
    let status = match &updater.state {
        UpdateState::Idle => String::new(),
        UpdateState::Checking => "Checking for updates…".into(),
        UpdateState::UpToDate => "Up to date.".into(),
        UpdateState::Available(i) => format!("Version {} is available.", i.version),
        UpdateState::Downloading(i) => format!("Downloading version {}…", i.version),
        UpdateState::Ready(i) => format!("Version {} is ready: restart to use it.", i.version),
        UpdateState::Failed(e) => format!("Update check failed: {e}"),
    };
    if !status.is_empty() {
        ui.label(status);
    }
    if ui
        .add_enabled(
            !updater.is_busy(),
            Button::new(format!("{} Check for updates", icon::ARROW_CLOCKWISE)),
        )
        .clicked()
    {
        commands.push(Command::CheckForUpdates);
    }
    ui.add_space(12.0);
    match default_browser {
        Some(true) => {
            ui.label(format!(
                "{} browsingUnicorns is your default browser.",
                icon::CHECK_CIRCLE
            ));
        }
        Some(false) => {
            ui.label("browsingUnicorns is not your default browser.");
            if ui
                .button(format!("{} Make it the default browser", icon::BROWSER))
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

/// "5 min", "3 h", "2 d".
fn ago(now: Timestamp, at: Timestamp) -> String {
    ago_secs(now.saturating_sub(at))
}

/// "now", "5 min", "3 h", "2 d" for an age in seconds.
pub fn ago_secs(secs: u64) -> String {
    match secs {
        0..60 => "now".into(),
        60..3_600 => format!("{} min", secs / 60),
        3_600..86_400 => format!("{} h", secs / 3_600),
        _ => format!("{} d", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_are_short() {
        assert_eq!(ago(100, 90), "now");
        assert_eq!(ago(10_000, 10_000 - 300), "5 min");
        assert_eq!(ago(100_000, 100_000 - 7_200), "2 h");
        assert_eq!(ago(1_000_000, 1_000_000 - 3 * 86_400), "3 d");
        assert_eq!(ago(0, 50), "now");
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
