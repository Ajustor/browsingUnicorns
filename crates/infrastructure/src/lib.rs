//! Infrastructure layer: implementations of the application ports.
//!
//! The `desktop` feature (default) adds the system web view and the HTTP
//! sync transport; the Android app builds without it.

pub mod blocker;
pub mod bookmark_import;
#[cfg(feature = "desktop")]
pub mod certificates;
mod clock;
#[cfg(feature = "desktop")]
pub mod default_browser;
mod dto;
pub mod extension_fetch;
pub mod extension_hub;
pub mod extension_server;
pub mod extensions;
mod files;
mod json_store;
mod memory_store;
pub mod native_messaging;
#[cfg(feature = "desktop")]
mod page_injection;
pub mod page_scripts;
#[cfg(feature = "desktop")]
mod parent_window;
#[cfg(feature = "desktop")]
mod private_data;
pub mod profiles;
pub mod script_storage;
mod settings_store;
mod shared_store;
pub mod site_scripts;
pub mod sync;
pub mod user_scripts;
#[cfg(all(any(target_os = "macos", target_os = "linux"), feature = "desktop"))]
mod webkit_rules;
#[cfg(all(any(target_os = "macos", target_os = "linux"), feature = "desktop"))]
mod webkit_scriptlets;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_address;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_blocking;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_downloads;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_extensions;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_frames;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_keys;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_region;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_scriptlets;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_settings;
#[cfg(feature = "desktop")]
mod wry_engine;

use std::path::{Path, PathBuf};

use application::ports::Profile;
use domain::{BrowsingHistory, SessionSnapshot, Settings};

pub use clock::SystemClock;
pub use json_store::JsonRepository;
pub use memory_store::{MemoryRepository, NOT_IN_PRIVATE, NoPackages, ReadOnlyRepository};
#[cfg(feature = "desktop")]
pub use parent_window::ParentWindow;
#[cfg(feature = "desktop")]
pub use private_data::{PrivateDataDir, clean_leftovers};
pub use settings_store::TomlSettingsRepository;
pub use shared_store::MergingRepository;
#[cfg(feature = "desktop")]
pub use wry_engine::{EngineFiles, KeyPress, WryEngine, builtin_extensions};

/// `<data dir>/browsing-unicorns`, where history, bookmarks and the session
/// of the default profile are kept, the other profiles under it.
/// `BU_PROFILE_DIR` replaces it (tests).
pub fn profile_dir() -> Option<PathBuf> {
    match std::env::var_os("BU_PROFILE_DIR") {
        Some(dir) if !dir.is_empty() => Some(dir.into()),
        _ => dirs::data_dir().map(|d| d.join("browsing-unicorns")),
    }
}

/// The system's Downloads folder.
pub fn downloads_dir() -> Option<PathBuf> {
    dirs::download_dir()
}

/// `<data dir>/browsing-unicorns/blocker`: the ad blocker's lists, shared
/// by the profiles.
pub fn blocker_dir() -> Option<PathBuf> {
    profile_dir().map(|d| d.join("blocker"))
}

/// The user's profile: JSON files in `dir`, settings in `settings`.
/// History and bookmarks are merged with the file when saved: other windows
/// of the profile write them too.
pub fn json_profile(dir: &Path, settings: TomlSettingsRepository) -> Profile {
    Profile {
        settings: Box::new(settings),
        history: Box::new(MergingRepository::new(JsonRepository::new(
            dir.join("history.json"),
        ))),
        bookmarks: Box::new(MergingRepository::new(JsonRepository::new(
            dir.join("bookmarks.json"),
        ))),
        session: Box::new(JsonRepository::new(dir.join("session.json"))),
        clock: Box::new(SystemClock),
    }
}

/// [`json_profile`] for a window of a profile already open in another one
/// (a tab moved to its own window): its tabs are not saved, the profile's
/// window keeps the session.
pub fn tab_window_profile(dir: &Path, settings: TomlSettingsRepository) -> Profile {
    Profile {
        session: Box::new(MemoryRepository::<SessionSnapshot>::default()),
        ..json_profile(dir, settings)
    }
}

/// A private window's view of the profile in `dir`: history and session in
/// memory (empty), the bookmarks read only, `settings` (the profile's)
/// changed in memory only. Nothing is ever written to `dir`.
pub fn private_profile(dir: &Path, settings: Settings) -> Profile {
    Profile {
        settings: Box::new(MemoryRepository::new(settings)),
        history: Box::new(MemoryRepository::<BrowsingHistory>::default()),
        bookmarks: Box::new(ReadOnlyRepository::new(JsonRepository::new(
            dir.join("bookmarks.json"),
        ))),
        session: Box::new(MemoryRepository::<SessionSnapshot>::default()),
        clock: Box::new(SystemClock),
    }
}

#[cfg(test)]
mod tests {
    use domain::{Bookmarks, WebAddress};

    use super::*;

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    #[test]
    fn a_private_profile_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let settings_file = TomlSettingsRepository::new(dir.path().join("settings.toml"));
        let normal = json_profile(dir.path(), settings_file);
        let mut history = BrowsingHistory::default();
        history.record(addr("https://a.com/"), 1);
        normal.history.save(&history).unwrap();
        let mut session = SessionSnapshot::default();
        session.tabs.push(addr("https://a.com/"));
        normal.session.save(&session).unwrap();
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(addr("https://a.com/"), "A", 1);
        normal.bookmarks.save(&bookmarks).unwrap();
        normal.settings.save(&Settings::default()).unwrap();
        let names = [
            "history.json",
            "session.json",
            "bookmarks.json",
            "settings.toml",
        ];
        let before: Vec<Vec<u8>> = names
            .iter()
            .map(|n| std::fs::read(dir.path().join(n)).unwrap())
            .collect();

        let private = private_profile(dir.path(), normal.settings.load().unwrap());
        assert!(private.history.load().unwrap().is_empty());
        assert!(private.session.load().unwrap().is_empty());
        let mut more = BrowsingHistory::default();
        more.record(addr("https://secret.com/"), 2);
        private.history.save(&more).unwrap();
        assert_eq!(private.history.load().unwrap(), more);
        let mut tabs = SessionSnapshot::default();
        tabs.tabs.push(addr("https://secret.com/"));
        private.session.save(&tabs).unwrap();
        assert_eq!(private.bookmarks.load().unwrap(), bookmarks);
        let mut added = bookmarks.clone();
        added.add(addr("https://secret.com/"), "S", 2);
        assert_eq!(
            private.bookmarks.save(&added).unwrap_err().0,
            NOT_IN_PRIVATE
        );
        let changed = Settings {
            blocker: false,
            ..Settings::default()
        };
        private.settings.save(&changed).unwrap();
        assert_eq!(private.settings.load().unwrap(), changed);

        let after: Vec<Vec<u8>> = names
            .iter()
            .map(|n| std::fs::read(dir.path().join(n)).unwrap())
            .collect();
        assert_eq!(before, after);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), names.len());
    }
}
