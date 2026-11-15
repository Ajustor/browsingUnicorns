//! Domain layer: the browser's business rules.
//!
//! Pure Rust, no I/O, no UI, no web engine. Every other layer depends on this
//! one; this one depends on nothing of ours.

mod address;
mod bookmarks;
mod history;
mod omnibox;
mod search;
mod session;
mod settings;
mod sync;
mod tab;
mod visits;

pub use address::{AddressError, WebAddress};
pub use bookmarks::{Bookmark, Bookmarks, Removal};
pub use history::History;
pub use omnibox::resolve_omnibox;
pub use search::SearchEngine;
pub use session::{Session, SessionSnapshot};
pub use settings::Settings;
pub use sync::{DeviceId, DeviceRecord, RemoteTab};
pub use tab::{LoadState, Tab, TabId};
pub use visits::{BrowsingHistory, Timestamp, Visit};
