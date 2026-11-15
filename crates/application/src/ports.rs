//! Ports: what the use cases need from the outside world.

use std::fmt;

use domain::{Settings, TabId, WebAddress};

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

pub trait SettingsRepository {
    /// The saved settings, or the defaults when none were saved yet.
    fn load(&self) -> Result<Settings, RepositoryError>;
    fn save(&self, settings: &Settings) -> Result<(), RepositoryError>;
}
