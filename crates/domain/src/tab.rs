use std::fmt;

use crate::{History, WebAddress};

/// Identifies a tab for the whole life of the browser (never reused).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(u64);

impl TabId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LoadState {
    #[default]
    Idle,
    Loading,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    id: TabId,
    title: String,
    history: History,
    load_state: LoadState,
    /// Left out of the history, the saved session and sync.
    private: bool,
    /// The tab whose page opened this one (a link opened in a new tab).
    pub(crate) opener: Option<TabId>,
}

impl Tab {
    pub fn new(id: TabId) -> Self {
        Self {
            id,
            title: String::new(),
            history: History::default(),
            load_state: LoadState::Idle,
            private: false,
            opener: None,
        }
    }

    /// A tab whose pages leave nothing behind.
    pub fn new_private(id: TabId) -> Self {
        Self {
            private: true,
            ..Self::new(id)
        }
    }

    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn is_private(&self) -> bool {
        self.private
    }

    /// The tab whose page opened this one, if any.
    pub fn opener(&self) -> Option<TabId> {
        self.opener
    }

    pub fn address(&self) -> Option<&WebAddress> {
        self.history.current()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn load_state(&self) -> LoadState {
        self.load_state
    }

    pub fn is_loading(&self) -> bool {
        self.load_state == LoadState::Loading
    }

    /// The page title, else the host, else a placeholder.
    pub fn display_title(&self) -> &str {
        if !self.title.trim().is_empty() {
            return self.title.trim();
        }
        self.address()
            .and_then(WebAddress::host)
            .unwrap_or_else(|| i18n::errors::domain::untitled_tab())
    }

    /// The title the page gave itself (may be empty).
    pub fn title(&self) -> &str {
        self.title.trim()
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    /// The engine started loading `address` in this tab.
    pub fn load_started(&mut self, address: WebAddress) {
        if self.address() != Some(&address) {
            // The previous page's title no longer applies.
            self.title.clear();
        }
        self.history.observe(address);
        self.load_state = LoadState::Loading;
    }

    /// The engine finished loading `address` (after possible redirects).
    pub fn load_finished(&mut self, address: WebAddress) {
        self.history.observe(address);
        self.load_state = LoadState::Idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    #[test]
    fn display_title_falls_back_to_host_then_placeholder() {
        let mut tab = Tab::new(TabId::new(1));
        assert_eq!(
            i18n::with_lang(i18n::Lang::En, || tab.display_title().to_owned()),
            "New tab"
        );
        assert_eq!(
            i18n::with_lang(i18n::Lang::Fr, || tab.display_title().to_owned()),
            "Nouvel onglet"
        );
        tab.load_started(addr("https://example.com/x"));
        assert_eq!(tab.display_title(), "example.com");
        tab.set_title("  Example  ");
        assert_eq!(tab.display_title(), "Example");
    }

    #[test]
    fn loading_tracks_state_and_history() {
        let mut tab = Tab::new(TabId::new(1));
        tab.load_started(addr("https://a.com"));
        assert!(tab.is_loading());
        tab.load_finished(addr("https://a.com"));
        assert!(!tab.is_loading());
        assert_eq!(tab.history().len(), 1);
    }

    #[test]
    fn a_new_page_clears_the_old_title() {
        let mut tab = Tab::new(TabId::new(1));
        tab.load_started(addr("https://a.com"));
        tab.set_title("A");
        tab.load_started(addr("https://b.com"));
        assert_eq!(tab.display_title(), "b.com");
    }
}
