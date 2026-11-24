//! Domain layer: the browser's business rules.
//!
//! Pure Rust, no I/O, no UI, no web engine. Every other layer depends on this
//! one; this one depends only on `i18n`, for the messages it makes.

// Its errors' messages (i18n::errors::domain), coming.
use i18n as _;

mod address;
mod bookmarks;
mod content_rules;
mod credentials;
mod extensions;
mod history;
mod omnibox;
mod page_scripts;
mod profiles;
mod search;
mod session;
mod settings;
mod suggestions;
mod sync;
mod tab;
mod visits;

pub use address::{AddressError, WebAddress};
pub use bookmarks::{Bookmark, Bookmarks, Removal};
pub use content_rules::{
    DomainType, NetworkRule, Request, ResourceType, RuleAction, RuleSet, UrlFilter, Verdict,
};
pub use credentials::{
    Credential, CredentialRemoval, Credentials, Offer, SeenCookies, SiteCookie, cookies_to_import,
    cookies_to_share,
};
pub use extensions::{Extension, ExtensionId, Extensions, is_newer_version};
pub use history::History;
pub use omnibox::resolve_omnibox;
pub use page_scripts::{RunAt, UrlPattern, UserScript, UserScriptId, UserScriptMeta, UserScripts};
pub use profiles::{PROFILE_COLORS, Profile, ProfileError, ProfileId, Profiles};
pub use search::SearchEngine;
pub use session::{Session, SessionSnapshot};
pub use settings::{DeviceSettings, LanguageChoice, Settings, allowlisted};
pub use suggestions::{Suggestion, suggest};
pub use sync::{DeviceId, DeviceRecord, RemoteTab};
pub use tab::{LoadState, Tab, TabId};
pub use visits::{BrowsingHistory, Timestamp, Visit};
