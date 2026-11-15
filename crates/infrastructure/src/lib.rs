//! Infrastructure layer: implementations of the application ports.

mod clock;
mod files;
mod json_store;
mod parent_window;
mod settings_store;
mod wry_engine;

use std::path::PathBuf;

use application::ports::Profile;

pub use clock::SystemClock;
pub use json_store::JsonRepository;
pub use parent_window::ParentWindow;
pub use settings_store::TomlSettingsRepository;
pub use wry_engine::WryEngine;

/// `<data dir>/browsing-unicorns`, where history, bookmarks and the session
/// are kept.
pub fn profile_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("browsing-unicorns"))
}

/// The user's profile stored as JSON files in `dir`.
pub fn json_profile(dir: impl Into<PathBuf>) -> Profile {
    let dir = dir.into();
    Profile {
        history: Box::new(JsonRepository::new(dir.join("history.json"))),
        bookmarks: Box::new(JsonRepository::new(dir.join("bookmarks.json"))),
        session: Box::new(JsonRepository::new(dir.join("session.json"))),
        clock: Box::new(SystemClock),
    }
}
