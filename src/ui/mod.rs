//! Presentation layer: the browser chrome drawn with egui. Pages themselves
//! are native child views laid over the central area.

mod app;
mod blocker;
mod bookmarks_bar;
mod command;
mod default_browser;
mod downloads;
mod extension_bar;
mod extensions;
mod favicons;
mod find_bar;
mod language;
mod library;
mod omnibox;
mod page_holes;
mod panel_kit;
mod passwords;
mod permissions;
mod profiles;
mod settings_panel;
mod shield;
mod shortcuts;
mod side_tabs;
mod site_info;
mod spinner;
mod split_view;
mod sync;
mod sync_panel;
mod system_font;
mod tab_common;
mod tab_search;
mod tab_strip;
pub mod theme;
mod toolbar;
mod updates;
mod user_scripts;
mod whats_new;
mod window_frame;

pub use blocker::BlockerRunner;
pub use default_browser::DefaultBrowser;
pub use extension_bar::ExtensionBar;
pub use extensions::ExtensionsRunner;
pub use language::{apply_language, device_language};
pub use profiles::ProfilesRunner;
pub use sync::SyncRunner;
pub use updates::Updates;
pub use user_scripts::UserScriptsRunner;

pub use app::{AddOns, BrowserApp, ExitSlot};

pub const APP_NAME: &str = "Browsing Unicorns";

/// [`infrastructure::builtin_extensions`], looked up once (an environment
/// variable read: not each frame).
fn builtin_extensions() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(infrastructure::builtin_extensions)
}

/// The system's downloads folder, looked up once (on Linux, a read of
/// `user-dirs.dirs`: not each frame).
fn downloads_dir() -> Option<&'static std::path::Path> {
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(infrastructure::downloads_dir).as_deref()
}
