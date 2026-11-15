//! Domain layer: the browser's business rules.
//!
//! Pure Rust, no I/O, no UI, no web engine. Every other layer depends on this
//! one; this one depends on nothing of ours.

mod address;
mod history;
mod omnibox;
mod search;
mod session;
mod settings;
mod tab;

pub use address::{AddressError, WebAddress};
pub use history::History;
pub use omnibox::resolve_omnibox;
pub use search::SearchEngine;
pub use session::Session;
pub use settings::Settings;
pub use tab::{LoadState, Tab, TabId};
