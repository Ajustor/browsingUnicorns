//! Domain layer: the browser's business rules.
//!
//! Pure Rust, no I/O, no UI, no web engine. Every other layer depends on this
//! one; this one depends on nothing of ours.

mod address;
mod bookmarks;
mod content_rules;
mod extensions;
mod history;
mod omnibox;
mod page_scripts;
mod search;
mod session;
mod settings;
mod sync;
mod tab;
mod visits;

pub use address::{AddressError, WebAddress};
pub use bookmarks::{Bookmark, Bookmarks, Removal};
pub use content_rules::{
    DomainType, NetworkRule, Request, ResourceType, RuleAction, RuleSet, UrlFilter, Verdict,
};
pub use extensions::{Extension, ExtensionId, Extensions};
pub use history::History;
pub use omnibox::resolve_omnibox;
pub use page_scripts::{RunAt, UrlPattern, UserScript, UserScriptId, UserScriptMeta, UserScripts};
pub use search::SearchEngine;
pub use session::{Session, SessionSnapshot};
pub use settings::Settings;
pub use sync::{DeviceId, DeviceRecord, RemoteTab};
pub use tab::{LoadState, Tab, TabId};
pub use visits::{BrowsingHistory, Timestamp, Visit};
