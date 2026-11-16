//! Infrastructure layer: implementations of the application ports.
//!
//! The `desktop` feature (default) adds the system web view and the HTTP
//! sync transport; the Android app builds without it.

pub mod blocker;
#[cfg(feature = "desktop")]
pub mod certificates;
mod clock;
mod dto;
pub mod extension_fetch;
pub mod extension_hub;
pub mod extension_server;
pub mod extensions;
mod files;
mod json_store;
pub mod native_messaging;
#[cfg(feature = "desktop")]
mod page_injection;
pub mod page_scripts;
#[cfg(feature = "desktop")]
mod parent_window;
pub mod profiles;
pub mod script_storage;
mod settings_store;
pub mod sync;
pub mod user_scripts;
#[cfg(all(any(target_os = "macos", target_os = "linux"), feature = "desktop"))]
mod webkit_rules;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_blocking;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_extensions;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_keys;
#[cfg(feature = "desktop")]
mod wry_engine;

use std::path::{Path, PathBuf};

use application::ports::Profile;

pub use clock::SystemClock;
pub use json_store::JsonRepository;
#[cfg(feature = "desktop")]
pub use parent_window::ParentWindow;
pub use settings_store::TomlSettingsRepository;
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

/// `<data dir>/browsing-unicorns/blocker`: the ad blocker's lists, shared
/// by the profiles.
pub fn blocker_dir() -> Option<PathBuf> {
    profile_dir().map(|d| d.join("blocker"))
}

/// The user's profile: JSON files in `dir`, settings in `settings`.
pub fn json_profile(dir: &Path, settings: TomlSettingsRepository) -> Profile {
    Profile {
        settings: Box::new(settings),
        history: Box::new(JsonRepository::new(dir.join("history.json"))),
        bookmarks: Box::new(JsonRepository::new(dir.join("bookmarks.json"))),
        session: Box::new(JsonRepository::new(dir.join("session.json"))),
        clock: Box::new(SystemClock),
    }
}
