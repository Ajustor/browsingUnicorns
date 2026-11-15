use domain::TabId;

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
}
