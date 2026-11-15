use application::Target;
use domain::{TabId, WebAddress};

use super::library::LibraryView;

/// What the user asked for, collected from widgets and shortcuts and run
/// once per frame by [`super::BrowserApp`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    NewTab,
    CloseTab(TabId),
    CloseActiveTab,
    Activate(TabId),
    CycleTabs(isize),
    Back,
    Forward,
    Reload,
    Home,
    Submit(String),
    FocusOmnibox,
    Open(WebAddress, Target),
    ToggleBookmark,
    RemoveBookmark(WebAddress),
    ClearHistory,
    /// Shows the library on that view, or hides it when it already shows it.
    ToggleLibrary(LibraryView),
}
