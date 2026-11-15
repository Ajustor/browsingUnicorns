//! Infrastructure layer: implementations of the application ports.
//!
//! The `desktop` feature (default) adds the system web view and the HTTP
//! sync transport; the Android app builds without it.

mod clock;
mod dto;
#[cfg(feature = "desktop")]
pub mod extensions;
mod files;
mod json_store;
#[cfg(feature = "desktop")]
mod parent_window;
mod settings_store;
pub mod sync;
#[cfg(all(windows, feature = "desktop"))]
mod webview2_extensions;
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
pub use wry_engine::WryEngine;

/// `<data dir>/browsing-unicorns`, where history, bookmarks and the session
/// are kept. `BU_PROFILE_DIR` replaces it (tests, a second profile).
pub fn profile_dir() -> Option<PathBuf> {
    match std::env::var_os("BU_PROFILE_DIR") {
        Some(dir) if !dir.is_empty() => Some(dir.into()),
        _ => dirs::data_dir().map(|d| d.join("browsing-unicorns")),
    }
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
