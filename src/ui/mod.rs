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
mod library;
mod omnibox;
mod page_holes;
mod profiles;
mod settings_panel;
mod shield;
mod shortcuts;
mod site_info;
mod sync;
mod sync_panel;
mod system_font;
mod tab_strip;
pub mod theme;
mod toolbar;
mod updates;
mod user_scripts;
mod window_frame;

pub use blocker::BlockerRunner;
pub use default_browser::DefaultBrowser;
pub use extension_bar::ExtensionBar;
pub use extensions::ExtensionsRunner;
pub use profiles::ProfilesRunner;
pub use sync::SyncRunner;
pub use updates::Updates;
pub use user_scripts::UserScriptsRunner;

pub use app::{AddOns, BrowserApp, ExitSlot};

pub const APP_NAME: &str = "Browsing Unicorns";
