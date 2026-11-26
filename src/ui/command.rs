use application::Target;
use domain::{ExtensionId, GroupColor, GroupId, TabId, UserScriptId, WebAddress};

use super::library::LibraryView;
use super::profiles::ProfileCommand;

/// What is imported, from where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportFrom {
    /// A browser found here: its bookmarks and history.
    Browser(infrastructure::browser_import::Source),
    /// Bookmarks exported as an HTML file (Safari...).
    Html(std::path::PathBuf),
    /// Passwords exported as a CSV file.
    Passwords(std::path::PathBuf),
}

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
    /// Speaks another language, on this computer.
    SetLanguage(domain::LanguageChoice),
    /// Removes the history, the sites' data or the cache.
    ClearBrowsingData(ClearData),
    /// Resets the protected videos' data (DRM) and restarts the app.
    ResetDrm,
    /// Applies the zoom by site to the open tabs.
    ApplyZoom,
    /// Shows the tabs at the top or in a column on the left (this device).
    SetTabPosition(domain::TabPosition),
    /// The tabs' column's width, in points (this device).
    SetSideTabsWidth(u32),
    /// The tabs' column shows only the icons, or the titles too.
    SetSideTabsCollapsed(bool),
    /// Shows a tab beside the active one (a split view).
    SplitWithActive(TabId),
    /// Opens a new tab beside the active one, in a split view.
    SplitWithNewTab,
    /// Shows the tabs of a split view one at a time again.
    Unsplit(TabId),
    /// The first tab's share of a split view's width, per thousand.
    SetSplitRatio(TabId, u16),
    /// Back to 100% on the active tab's site.
    ResetZoom,
    /// Mutes a tab's page, or lets it play sound again.
    SetMuted(TabId, bool),
    /// Imports another browser's bookmarks.
    Import(ImportFrom),
    /// Moves a bookmark to a folder (empty: out of any).
    MoveBookmark(WebAddress, String),
    /// Shows a folder's bookmarks in the side panel.
    ShowBookmarkFolder(String),
    /// Reopens the last tab closed (Ctrl+Shift+T).
    ReopenClosedTab,
    /// A private window on this profile (its own process).
    OpenPrivateWindow,
    CloseTab(TabId),
    CloseActiveTab,
    Activate(TabId),
    /// Moves a tab to that index of the tab strip.
    MoveTab(TabId, usize),
    /// Pins a tab, or unpins it.
    SetPinned(TabId, bool),
    /// Puts a tab in a new group of its own.
    NewGroup(TabId),
    AddToGroup(TabId, GroupId),
    RemoveFromGroup(TabId),
    RenameGroup(GroupId, String),
    SetGroupColor(GroupId, GroupColor),
    SetGroupCollapsed(GroupId, bool),
    /// Takes every tab out of a group.
    Ungroup(GroupId),
    /// Closes every tab of a group.
    CloseGroup(GroupId),
    /// Shows the search of the open tabs, or hides it.
    ToggleTabSearch,
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
    /// The answer to the bar offering to save a sign-in.
    AnswerCredential(application::CredentialAnswer),
    /// Deletes a saved sign-in: origin, user name (empty with "never").
    RemoveCredential(String, String),
    /// Opens an extension's popup under `anchor` (its button), or
    /// closes it.
    ToggleExtensionPopup {
        id: ExtensionId,
        anchor: egui::Rect,
    },
}
