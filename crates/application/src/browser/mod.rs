//! The browser window's use cases, by concern: this module holds the state
//! and its persistence; the submodules add the rest to [`Browser`].

use std::collections::HashMap;

use domain::{
    Bookmarks, BrowsingHistory, Credentials, SeenCookies, Session, SessionSnapshot, Settings,
    Suggestion, TabId, Timestamp, WebAddress, suggest,
};

use crate::BrowserError;
use crate::ports::{EngineEvent, Profile, WebEngine};
use crate::sync::RemoteDevice;

mod credentials;
mod library;
mod navigation;
mod sync;
mod tab_lifecycle;
mod tabs;
#[cfg(test)]
mod test_support;

pub use credentials::{CredentialAnswer, PendingCredential};

/// Where to load a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The active tab (a new one when there is none).
    ActiveTab,
    NewTab,
}

/// The browser window's use cases.
pub struct Browser<E: WebEngine> {
    session: Session,
    settings: Settings,
    engine: E,
    profile: Profile,
    history: BrowsingHistory,
    history_dirty: bool,
    bookmarks: Bookmarks,
    saved_session: SessionSnapshot,
    remote_devices: Vec<RemoteDevice>,
    /// Bookmarks, settings or history changed since the last sync record.
    unsynced: bool,
    /// The tabs closed, last closed last: their address and where they were.
    closed: Vec<(WebAddress, usize)>,
    credentials: Credentials,
    pending_credential: Option<PendingCredential>,
    /// By tab: the origin, user name and time of a user name typed alone.
    typed_usernames: HashMap<TabId, (String, String, Timestamp)>,
    /// When each tab was last seen (shown, or first looked at by
    /// [`Self::discard_idle`]).
    last_seen: HashMap<TabId, Timestamp>,
    /// Tabs whose page crashed: they stay without a page (the presentation
    /// says so) until reloaded or shown again.
    crashed: std::collections::HashSet<TabId>,
    seen_cookies: SeenCookies,
}

impl<E: WebEngine> Browser<E> {
    /// A browser with no tab. Call [`Self::load_profile`] then
    /// [`Self::restore_session`] to start it.
    pub fn new(engine: E, settings: Settings, profile: Profile) -> Self {
        Self {
            session: Session::new(),
            settings,
            engine,
            profile,
            history: BrowsingHistory::default(),
            history_dirty: false,
            bookmarks: Bookmarks::default(),
            saved_session: SessionSnapshot::default(),
            remote_devices: Vec::new(),
            unsynced: false,
            closed: Vec::new(),
            credentials: Credentials::default(),
            pending_credential: None,
            typed_usernames: HashMap::new(),
            last_seen: HashMap::new(),
            crashed: std::collections::HashSet::new(),
            seen_cookies: SeenCookies::default(),
        }
    }

    /// Reads the saved history and bookmarks. Whatever fails to load is
    /// left empty; the errors are returned.
    pub fn load_profile(&mut self) -> Vec<BrowserError> {
        let mut errors = Vec::new();
        match self.profile.history.load() {
            Ok(history) => self.history = history,
            Err(e) => errors.push(e.into()),
        }
        match self.profile.bookmarks.load() {
            Ok(bookmarks) => self.bookmarks = bookmarks,
            Err(e) => errors.push(e.into()),
        }
        match self.profile.credentials.load() {
            Ok(credentials) => self.credentials = credentials,
            Err(e) => errors.push(e.into()),
        }
        match self.profile.seen_cookies.load() {
            Ok(seen) => self.seen_cookies = seen,
            Err(e) => errors.push(e.into()),
        }
        errors
    }

    /// Reopens the tabs of the last run, or the home page when there were
    /// none. Returns how many tabs were restored.
    pub fn restore_session(&mut self) -> Result<usize, BrowserError> {
        let snapshot = self.profile.session.load().unwrap_or_default();
        let mut ids = Vec::with_capacity(snapshot.tabs.len());
        // Each new tab opens after the last one, asleep: only the tabs shown
        // load their page (a startup with many tabs stays quick and light).
        for (i, address) in snapshot.tabs.iter().enumerate() {
            let id = self.session.open_tab();
            let title = snapshot.titles.get(i).map_or("", String::as_str);
            if let Some(tab) = self.session.tab_mut(id) {
                tab.restore(address.clone(), title);
            }
            ids.push(id);
        }
        self.session.restore_layout(&ids, &snapshot);
        match snapshot.active.and_then(|i| ids.get(i)) {
            Some(&id) => self.activate(id),
            None if ids.is_empty() => {
                self.open_tab(None)?;
            }
            None => {}
        }
        self.saved_session = snapshot;
        Ok(ids.len())
    }

    /// Whether [`Self::persist`] has something to write.
    pub fn has_unsaved_changes(&self) -> bool {
        self.history_dirty || self.session.snapshot() != self.saved_session
    }

    /// Saves what changed since the last call: history and open tabs.
    pub fn persist(&mut self) -> Result<(), BrowserError> {
        if self.history_dirty {
            self.profile.history.save(&self.history)?;
            self.history_dirty = false;
        }
        let snapshot = self.session.snapshot();
        if snapshot != self.saved_session {
            self.profile.session.save(&snapshot)?;
            self.saved_session = snapshot;
        }
        Ok(())
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn now(&self) -> Timestamp {
        self.profile.clock.now()
    }

    pub fn history(&self) -> &BrowsingHistory {
        &self.history
    }

    pub fn bookmarks(&self) -> &Bookmarks {
        &self.bookmarks
    }

    /// Replaces the settings (they win over older ones on other devices).
    pub fn update_settings(&mut self, mut settings: Settings) -> Result<(), BrowserError> {
        settings.updated_at = self.now();
        self.settings = settings;
        self.unsynced = true;
        self.profile.settings.save(&self.settings)?;
        Ok(())
    }

    /// Replaces this device's own settings (not synced).
    pub fn update_device_settings(
        &mut self,
        device: domain::DeviceSettings,
    ) -> Result<(), BrowserError> {
        if self.settings.device != device {
            self.settings.device = device;
            self.profile.settings.save(&self.settings)?;
        }
        Ok(())
    }

    /// Loads `address` in the active tab or a new one.
    pub fn load(&mut self, address: WebAddress, target: Target) -> Result<(), BrowserError> {
        match (target, self.session.active_id()) {
            (Target::ActiveTab, Some(id)) => self.engine.navigate(id, &address)?,
            _ => {
                self.open_tab_in(Some(address), self.active_is_private())?;
            }
        }
        Ok(())
    }

    pub fn engine(&self) -> &E {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut E {
        &mut self.engine
    }

    /// What the address bar proposes for `input`: past searches and pages
    /// of the history.
    pub fn suggest(&self, input: &str, limit: usize) -> Vec<Suggestion> {
        suggest(input, &self.history, &self.settings.search_engine, limit)
    }

    /// Applies what the engine reported. Events about closed tabs are ignored.
    pub fn handle(&mut self, event: EngineEvent) -> Result<(), BrowserError> {
        match event {
            EngineEvent::LoadStarted { tab, address } => {
                if let Some(t) = self.session.tab_mut(tab) {
                    t.load_started(address);
                }
            }
            EngineEvent::LoadFinished { tab, address } => {
                if self.session.tab(tab).is_some_and(|t| !t.is_private()) {
                    self.fill_credentials(tab, &address);
                }
                if let Some(t) = self.session.tab_mut(tab) {
                    t.load_finished(address.clone());
                    let title = t.title().to_owned();
                    if !t.is_private()
                        && self
                            .history
                            .record(address.clone(), self.profile.clock.now())
                    {
                        // The title may have come before the end of the load.
                        self.history.retitle(&address, &title);
                        self.history_dirty = true;
                        self.unsynced = true;
                    }
                }
            }
            EngineEvent::TitleChanged { tab, title } => {
                if let Some(t) = self.session.tab_mut(tab) {
                    t.set_title(title);
                    if !t.is_private()
                        && let Some(address) = t.address()
                        && self.history.retitle(address, t.title())
                    {
                        self.history_dirty = true;
                    }
                }
            }
            EngineEvent::NewTabRequested { opener, address } => {
                self.open_link_tab(opener, address)?;
            }
            // Kept for the site's next pages; this device's, not synced.
            EngineEvent::ZoomChanged { host, percent } => {
                if self.settings.device.set_zoom(&host, percent) {
                    self.profile.settings.save(&self.settings)?;
                }
            }
            // Answered by the presentation (page scripts' bridge).
            EngineEvent::PageMessage { .. } => {}
            // Shown by the presentation.
            EngineEvent::FullScreenChanged { .. }
            | EngineEvent::FaviconChanged { .. }
            | EngineEvent::AudioChanged { .. } => {}
            // Shown by the presentation.
            EngineEvent::DownloadStarted { .. }
            | EngineEvent::DownloadFinished { .. }
            | EngineEvent::ExtensionOffered { .. }
            | EngineEvent::PermissionRequested { .. } => {}
            // The page died: its view goes, the tab waits to be reloaded.
            // The whole engine: every page went with it, the tabs sleep and
            // the one shown starts again.
            EngineEvent::PageCrashed { tab, all } => {
                // From a view already gone (the engine's crash reaches every
                // view): handled with the first.
                if self.session.tab(tab).is_none_or(|t| t.is_discarded()) {
                    return Ok(());
                }
                if all {
                    let ids: Vec<TabId> = self
                        .session
                        .tabs()
                        .iter()
                        .filter(|t| !t.is_discarded())
                        .map(domain::Tab::id)
                        .collect();
                    for id in ids {
                        self.engine.close_view(id);
                        if let Some(t) = self.session.tab_mut(id) {
                            t.discard();
                        }
                    }
                    let active = self.session.active_id();
                    self.show(active);
                } else {
                    self.engine.close_view(tab);
                    if let Some(t) = self.session.tab_mut(tab) {
                        t.discard();
                    }
                    self.crashed.insert(tab);
                }
            }
            // The other pane of the split view: it becomes the active tab.
            EngineEvent::PageFocused { tab } => {
                if let Some(active) = self.session.active_id()
                    && self.session.split_partner(active) == Some(tab)
                {
                    self.activate(tab);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn finished_loads_are_recorded_with_their_title() {
        let store = Store::default();
        let mut b = browser_with(&store);
        let id = b.open_tab(None).unwrap();
        b.handle(EngineEvent::TitleChanged {
            tab: id,
            title: "early".into(),
        })
        .unwrap();
        finish(&mut b, id, "https://a.com/");
        b.handle(EngineEvent::TitleChanged {
            tab: id,
            title: "A".into(),
        })
        .unwrap();
        assert_eq!(b.history().len(), 1);
        assert_eq!(b.history().visits()[0].title, "A");
        assert_eq!(b.history().visits()[0].at, 1_000);

        assert!(
            store.history.lock().unwrap().is_none(),
            "saved only on persist"
        );
        assert!(b.has_unsaved_changes());
        b.persist().unwrap();
        assert!(!b.has_unsaved_changes());
        assert_eq!(store.history.lock().unwrap().as_ref().unwrap().len(), 1);

        b.clear_history().unwrap();
        assert!(store.history.lock().unwrap().as_ref().unwrap().is_empty());
    }

    #[test]
    fn profile_errors_are_reported_and_leave_empty_data() {
        let mut b = Browser::new(
            FakeEngine::default(),
            Settings::default(),
            Profile {
                settings: Box::new(FailingRepository),
                history: Box::new(FailingRepository),
                bookmarks: Box::new(FailingRepository),
                session: Box::new(FailingRepository),
                credentials: Box::new(FailingRepository),
                seen_cookies: Box::new(FailingRepository),
                clock: Box::new(FixedClock),
            },
        );
        assert_eq!(b.load_profile().len(), 4);
        assert!(b.history().is_empty());
        // An unreadable session falls back to the home page.
        assert_eq!(b.restore_session().unwrap(), 0);
        assert_eq!(b.session().len(), 1);
    }

    #[test]
    fn session_is_saved_and_restored_in_order() {
        let store = Store::default();
        let mut b = browser_with(&store);
        b.restore_session().unwrap();
        let first = b.session().active_id().unwrap();
        finish(&mut b, first, "https://a.com/");
        let second = b.open_tab(None).unwrap();
        finish(&mut b, second, "https://b.com/");
        b.activate(first);
        b.persist().unwrap();

        let saved = store.session.lock().unwrap().clone().unwrap();
        assert_eq!(
            saved.tabs,
            vec![addr("https://a.com/"), addr("https://b.com/")]
        );
        assert_eq!(saved.active, Some(0));

        let mut restored = browser_with(&store);
        assert_eq!(restored.restore_session().unwrap(), 2);
        let calls = take_calls(&mut restored);
        // Only the tab shown loads; the other one sleeps until shown.
        assert!(calls.contains(&Call::Open(1, "https://a.com/".into())));
        assert!(!calls.iter().any(|c| matches!(c, Call::Open(2, _))));
        assert_eq!(calls.last(), Some(&Call::Show(Some(1))));
        assert_eq!(restored.session().active_id().map(TabId::get), Some(1));
        let asleep = restored.session().tab(TabId::new(2)).unwrap();
        assert!(asleep.is_discarded());
        assert_eq!(asleep.address(), Some(&addr("https://b.com/")));

        restored.activate(TabId::new(2));
        let calls = take_calls(&mut restored);
        assert_eq!(
            calls,
            vec![Call::Open(2, "https://b.com/".into()), Call::Show(Some(2))]
        );
        assert!(
            !restored
                .session()
                .tab(TabId::new(2))
                .unwrap()
                .is_discarded()
        );
    }

    #[test]
    fn restored_tabs_keep_their_titles() {
        let store = Store::default();
        let mut b = browser_with(&store);
        b.restore_session().unwrap();
        let first = b.session().active_id().unwrap();
        finish(&mut b, first, "https://a.com/");
        let second = b.open_tab(None).unwrap();
        finish(&mut b, second, "https://b.com/");
        b.handle(EngineEvent::TitleChanged {
            tab: second,
            title: "Page B".into(),
        })
        .unwrap();
        b.activate(first);
        b.persist().unwrap();

        let mut restored = browser_with(&store);
        restored.restore_session().unwrap();
        assert_eq!(restored.session().tabs()[1].display_title(), "Page B");
    }

    #[test]
    fn load_targets_the_active_or_a_new_tab() {
        let mut b = browser();
        b.load(addr("https://a.com/"), Target::ActiveTab).unwrap();
        assert_eq!(b.session().len(), 1);
        b.load(addr("https://b.com/"), Target::ActiveTab).unwrap();
        assert_eq!(b.session().len(), 1);
        b.load(addr("https://c.com/"), Target::NewTab).unwrap();
        assert_eq!(b.session().len(), 2);
    }

    #[test]
    fn events_update_the_tab() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        b.handle(EngineEvent::LoadStarted {
            tab: id,
            address: addr("https://a.com/"),
        })
        .unwrap();
        assert!(b.session().active().unwrap().is_loading());
        b.handle(EngineEvent::TitleChanged {
            tab: id,
            title: "A".into(),
        })
        .unwrap();
        assert_eq!(b.session().active().unwrap().display_title(), "A");
        // Events for unknown tabs are ignored.
        b.handle(EngineEvent::TitleChanged {
            tab: TabId::new(42),
            title: "x".into(),
        })
        .unwrap();
    }

    #[test]
    fn suggestions_come_from_the_history() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        finish(&mut b, id, "https://duckduckgo.com/?q=rust+book");
        finish(&mut b, id, "https://doc.rust-lang.org/book/");
        let s = b.suggest("rust", 8);
        assert_eq!(s.len(), 2);
        assert!(matches!(&s[0], Suggestion::Search { terms, .. } if terms == "rust book"));
    }
}
