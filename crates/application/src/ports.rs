//! Ports: what the use cases need from the outside world.

use std::fmt;
use std::path::PathBuf;

use domain::{
    Bookmarks, BrowsingHistory, Credentials, DeviceId, DeviceRecord, Extension, ExtensionId,
    Extensions, ProfileId, SeenCookies, SessionSnapshot, Settings, SiteCookie, TabId, Timestamp,
    UserScript, UserScriptId, UserScripts, WebAddress,
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
    /// [`Self::open_view`] for a private tab: its cookies and site storage
    /// are kept apart from the others'. An error where the engine can't.
    fn open_private_view(&mut self, _tab: TabId, _address: &WebAddress) -> Result<(), EngineError> {
        Err(EngineError(
            i18n::errors::application::private_tabs_unavailable().into(),
        ))
    }
    fn close_view(&mut self, tab: TabId);
    /// Shows the view of `tab` (hides the others); `None` hides them all.
    fn show(&mut self, tab: Option<TabId>);
    fn navigate(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError>;
    fn go_back(&mut self, tab: TabId);
    fn go_forward(&mut self, tab: TabId);
    fn reload(&mut self, tab: TabId);
    /// Reloads `tab` fetching everything again, nothing from the cache.
    fn reload_bypassing_cache(&mut self, tab: TabId) {
        self.reload(tab);
    }
    fn set_viewport(&mut self, viewport: Viewport);
    /// The parts of the shown page to cut out, relative to its viewport's
    /// top-left corner: what the browser draws there (tooltips, menus)
    /// shows through. Empty: the whole page.
    fn set_page_holes(&mut self, _holes: &[Viewport]) {}

    /// How this engine runs browser extensions.
    fn extension_support(&self) -> ExtensionSupport {
        ExtensionSupport::None
    }
    /// The extensions to run from now on (installed, enabled or disabled).
    fn set_extensions(&mut self, _extensions: &Extensions) {}
    /// The user scripts to run in pages from now on.
    fn set_user_scripts(&mut self, _scripts: &UserScripts) {}
    /// Sends a message to the page scripts of `to` (answer or event).
    fn reply_to_page(&mut self, _to: &PageSource, _message: &str) {}
    /// Runs `script` in `tab`'s page (an extension's `executeScript`).
    fn run_in_tab(&mut self, _tab: TabId, _script: &str) {}
    /// Opens the web inspector of `page`.
    fn open_devtools(&mut self, _page: &PageSource) {}
    /// Shows `url` (an extension's popup) in the side panel area, or hides it.
    fn show_panel_page(&mut self, _url: Option<&str>, _area: Viewport) {}
    /// The downloads under way, where the engine tells how far they are
    /// (empty otherwise).
    fn downloads(&mut self) -> Vec<DownloadProgress> {
        Vec::new()
    }
    /// Whether [`Self::cancel_download`] works here.
    fn can_cancel_downloads(&self) -> bool {
        false
    }
    /// Stops downloading `file`.
    fn cancel_download(&mut self, _file: &std::path::Path) {}
    /// Applies this device's preferences: download folder, passwords,
    /// zoom by site.
    fn apply_device_settings(&mut self, _settings: &domain::DeviceSettings) {}
    /// Whether the browser can save the passwords typed in pages (it needs
    /// [`Self::fill_credentials`] and the pages to report their sign-ins).
    fn can_save_passwords(&self) -> bool {
        true
    }
    /// Whether [`Self::clear_site_data`] does something.
    fn can_clear_site_data(&self) -> bool {
        false
    }
    /// Removes what sites left: their cookies and storage, the cache.
    fn clear_site_data(&mut self, _cookies: bool, _cache: bool) {}
    /// Mutes `tab`'s page, or lets it play sound again.
    fn set_muted(&mut self, _tab: TabId, _muted: bool) {}
    /// Fills the sign-in form of `tab`'s page with `username` and
    /// `password`, if the page is still at `origin`.
    fn fill_credentials(&mut self, _tab: TabId, _origin: &str, _username: &str, _password: &str) {}
    /// Every cookie of the (non-private) web views; `None` where the engine
    /// can't tell.
    fn site_cookies(&mut self) -> Option<Vec<SiteCookie>> {
        None
    }
    /// Sets `cookies` in the (non-private) web views.
    fn set_site_cookies(&mut self, _cookies: &[SiteCookie]) {}
}

/// A download under way: how much of `file` arrived, of `total` when known.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadProgress {
    pub file: std::path::PathBuf,
    pub received: u64,
    pub total: Option<u64>,
}

/// What an engine does with extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionSupport {
    None,
    /// The web view runs them itself (WebView2).
    Full,
    /// What the browser applies itself: their network rules (blocking)
    /// and content scripts; no background pages.
    Partial,
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
    /// A page asked for a new window (`target="_blank"`, `window.open`,
    /// a link opened in a new tab). `opener` is its tab, when known.
    NewTabRequested {
        opener: Option<TabId>,
        address: WebAddress,
    },
    /// A message of the scripts the browser runs in pages (extension
    /// storage, messages...), for the presentation to answer.
    PageMessage {
        from: PageSource,
        body: String,
    },
    /// A page's store button ("Add to Chrome") fetched the package of
    /// extension `id`: its download was stopped, the browser installs it.
    ExtensionOffered {
        id: ExtensionId,
    },
    /// A page started downloading `url`, saved to `file`.
    DownloadStarted {
        url: String,
        file: std::path::PathBuf,
    },
    /// `tab`'s page started or stopped playing sound, or was muted.
    AudioChanged {
        tab: TabId,
        playing: bool,
        muted: bool,
    },
    /// `tab`'s page icon changed: a PNG (empty: none).
    FaviconChanged {
        tab: TabId,
        png: Vec<u8>,
    },
    /// `tab`'s page shows an element in full screen (a video), or not.
    FullScreenChanged {
        tab: TabId,
        on: bool,
    },
    /// The user zoomed `host`'s pages in or out (Ctrl+wheel, Ctrl +/-).
    ZoomChanged {
        host: String,
        percent: u32,
    },
    /// A download ended, well (`ok`) or not (failed, cancelled); `file`
    /// where the engine tells it.
    DownloadFinished {
        url: String,
        file: Option<std::path::PathBuf>,
        ok: bool,
    },
}

/// Where page scripts run: a tab, an extension's background page, the
/// extension popup shown in the side panel.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PageSource {
    Tab(TabId),
    Background(ExtensionId),
    Panel,
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
    /// The saved sign-ins.
    pub credentials: Box<dyn Repository<Credentials>>,
    /// The cookies already shared with or taken from the other devices.
    pub seen_cookies: Box<dyn Repository<SeenCookies>>,
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
    /// An unpacked extension: a folder with its `manifest.json` (as Chrome's
    /// "Load unpacked"). Installed again from there to update it.
    Folder(PathBuf),
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

/// User scripts' files: downloads a `.user.js` (and what it `@require`s),
/// keeps its code, removes it. Installing downloads: off the UI thread.
pub trait UserScriptPackages: Send + Sync {
    fn install(&self, url: &str) -> Result<UserScript, ExtensionError>;
    fn remove(&self, id: &UserScriptId) -> Result<(), ExtensionError>;
    /// Deletes the code of scripts that are not in `keep`.
    fn prune(&self, keep: &UserScripts);
}

/// The profiles' data folders.
pub trait ProfileStore: Send {
    /// Makes the empty folder of a new profile.
    fn create(&self, id: &ProfileId) -> Result<(), RepositoryError>;
    /// Deletes a profile's data.
    fn delete(&self, id: &ProfileId) -> Result<(), RepositoryError>;
    /// Whether a window runs the profile.
    fn is_open(&self, id: &ProfileId) -> bool;
}
