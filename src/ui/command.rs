use application::Target;
use domain::{ExtensionId, TabId, UserScriptId, WebAddress};

use super::library::LibraryView;
use super::profiles::ProfileCommand;

/// What "Clear browsing data" removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClearData {
    pub history: bool,
    pub cookies: bool,
    pub cache: bool,
}

/// What the user asked for, collected from widgets and shortcuts and run
/// once per frame by [`super::BrowserApp`].
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    NewTab,
    /// Saves new settings (the Settings view).
    UpdateSettings(Box<domain::Settings>),
    /// Removes the history, the sites' data or the cache.
    ClearBrowsingData(ClearData),
    /// Applies the zoom by site to the open tabs.
    ApplyZoom,
    /// Back to 100% on the active tab's site.
    ResetZoom,
    /// Mutes a tab's page, or lets it play sound again.
    SetMuted(TabId, bool),
    /// Reopens the last tab closed (Ctrl+Shift+T).
    ReopenClosedTab,
    /// A private window on this profile (its own process).
    OpenPrivateWindow,
    CloseTab(TabId),
    CloseActiveTab,
    Activate(TabId),
    /// Moves a tab to that index of the tab strip.
    MoveTab(TabId, usize),
    /// Moves a tab to a new window: x, y, width, height on the screen.
    DetachTab(TabId, [f32; 4]),
    CycleTabs(isize),
    Back,
    Forward,
    Reload,
    /// Reload without the cache (Ctrl+Shift+R, Ctrl+F5, Shift+click).
    HardReload,
    Home,
    Submit(String),
    FocusOmnibox,
    Open(WebAddress, Target),
    ToggleBookmark,
    RemoveBookmark(WebAddress),
    ClearHistory,
    /// Shows the library on that view, or hides it when it already shows it.
    ToggleLibrary(LibraryView),
    CheckForUpdates,
    InstallUpdate,
    UpdateLater,
    SkipUpdate,
    RestartToUpdate,
    SyncTurnOn,
    SyncJoin(String),
    SyncTurnOff,
    SyncNow,
    /// A Web Store link, an extension id or a `.crx` path.
    InstallExtension(String),
    SetExtensionEnabled(ExtensionId, bool),
    RemoveExtension(ExtensionId),
    /// The address of a `.user.js`.
    InstallUserScript(String),
    SetUserScriptEnabled(UserScriptId, bool),
    RemoveUserScript(UserScriptId),
    Profile(ProfileCommand),
    /// Shows the active site's connection and certificate, or hides them.
    ToggleSiteInfo,
    /// Shows the downloads bar, or hides it.
    ToggleDownloads,
    /// Becoming the system's default browser, or the answer to its bar.
    DefaultBrowser(super::default_browser::DefaultBrowserCommand),
    /// Shows the ad blocker's bar for the active site, or hides it.
    ToggleShield,
    /// Turns the ad blocker on (or off) for a host and its subdomains.
    SetBlockerForSite(String, bool),
    /// Turns the ad blocker on or off.
    SetBlocker(bool),
    /// The web inspector of the active tab.
    OpenDevTools,
    /// The web inspector of an extension's background page.
    InspectExtension(ExtensionId),
    /// Opens an extension's popup under `anchor` (its button), or
    /// closes it.
    ToggleExtensionPopup {
        id: ExtensionId,
        anchor: egui::Rect,
    },
}
