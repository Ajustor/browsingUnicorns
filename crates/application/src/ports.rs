//! Ports: what the use cases need from the outside world.

use std::fmt;
use std::path::PathBuf;

use domain::{
    Bookmarks, BrowsingHistory, DeviceId, DeviceRecord, Extension, ExtensionId, Extensions,
    SessionSnapshot, Settings, TabId, Timestamp, WebAddress,
};

/// Where pages are drawn, in physical pixels relative to the window's client
/// area.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError(pub String);

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EngineError {}

/// Renders web pages: one view per tab, only one shown at a time.
///
/// What happens inside a view comes back asynchronously as [`EngineEvent`]s,
/// fed to [`crate::Browser::handle`].
pub trait WebEngine {
    /// Creates the view of `tab` and starts loading `address` in it.
    fn open_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError>;
    fn close_view(&mut self, tab: TabId);
    /// Shows the view of `tab` (hides the others); `None` hides them all.
    fn show(&mut self, tab: Option<TabId>);
    fn navigate(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError>;
    fn go_back(&mut self, tab: TabId);
    fn go_forward(&mut self, tab: TabId);
    fn reload(&mut self, tab: TabId);
    fn set_viewport(&mut self, viewport: Viewport);

    /// How this engine runs browser extensions.
    fn extension_support(&self) -> ExtensionSupport {
        ExtensionSupport::None
    }
    /// The extensions to run from now on (installed, enabled or disabled).
    fn set_extensions(&mut self, _extensions: &Extensions) {}
}

/// What an engine does with extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionSupport {
    None,
    /// The web view runs them itself (WebView2).
    Full,
    /// Only their network rules (blocking), applied by the browser.
    NetworkRules,
}

/// Something that happened in a view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    LoadStarted {
        tab: TabId,
        address: WebAddress,
    },
    LoadFinished {
        tab: TabId,
        address: WebAddress,
    },
    TitleChanged {
        tab: TabId,
        title: String,
    },
    /// A page asked for a new window (`target="_blank"`, `window.open`).
    NewTabRequested {
        address: WebAddress,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryError(pub String);

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RepositoryError {}

/// Keeps one value across runs (settings, history, bookmarks, session).
/// `Send`, so that a browser can be shared with another thread (Android).
pub trait Repository<T>: Send {
    /// The saved value, or the default one when nothing was saved yet.
    fn load(&self) -> Result<T, RepositoryError>;
    fn save(&self, value: &T) -> Result<(), RepositoryError>;
}

pub trait Clock: Send {
    fn now(&self) -> Timestamp;
}

/// Where the user's data lives.
pub struct Profile {
    pub settings: Box<dyn Repository<Settings>>,
    pub history: Box<dyn Repository<BrowsingHistory>>,
    pub bookmarks: Box<dyn Repository<Bookmarks>>,
    pub session: Box<dyn Repository<SessionSnapshot>>,
    pub clock: Box<dyn Clock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncError(pub String);

impl fmt::Display for SyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SyncError {}

/// Where the devices of a sync group leave their records for each other.
/// Blocking: call it off the UI thread.
pub trait SyncStore: Send + Sync {
    /// Replaces this device's record.
    fn push(&self, record: &DeviceRecord) -> Result<(), SyncError>;
    /// Every device's record, this one's included.
    fn pull(&self) -> Result<Vec<DeviceRecord>, SyncError>;
    /// Removes this device's record (it leaves the group).
    fn leave(&self, device: &DeviceId) -> Result<(), SyncError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionError(pub String);

impl fmt::Display for ExtensionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ExtensionError {}

/// Where an extension is installed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionSource {
    /// Downloaded from the Chrome Web Store.
    Store(ExtensionId),
    /// A `.crx` file on this computer.
    File(PathBuf),
}

/// The extensions' files: fetches and unpacks packages, removes them.
/// Installing may download: callers run it off the UI thread.
pub trait ExtensionPackages: Send + Sync {
    fn install(&self, source: &ExtensionSource) -> Result<Extension, ExtensionError>;
    fn remove(&self, id: &ExtensionId) -> Result<(), ExtensionError>;
    /// Deletes the files of extensions that are not in `keep` (left over by
    /// a removal that could not finish).
    fn prune(&self, keep: &Extensions);
}
