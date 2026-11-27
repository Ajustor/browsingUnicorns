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

/// Stores of the profile, as a set of bits: those with changes to save.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Stores(u8);

impl Stores {
    const NONE: Self = Self(0);
    const HISTORY: Self = Self(1);
    const BOOKMARKS: Self = Self(1 << 1);
    const SETTINGS: Self = Self(1 << 2);
    const CREDENTIALS: Self = Self(1 << 3);
    const SEEN_COOKIES: Self = Self(1 << 4);
    const ALL: Self = Self(0b1_1111);
    const EACH: [Self; 5] = [
        Self::HISTORY,
        Self::BOOKMARKS,
        Self::SETTINGS,
        Self::CREDENTIALS,
        Self::SEEN_COOKIES,
    ];

    fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Stores {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// Changes worth syncing, counted: those made while a round is under way
/// are still to sync after it, and a round that fails leaves them all.
#[derive(Debug, Default)]
struct SyncChanges {
    made: u64,
    synced: u64,
    /// `made` when the record of the round under way was taken.
    in_round: Option<u64>,
}

impl SyncChanges {
    fn note(&mut self) {
        self.made += 1;
    }

    fn pending(&self) -> bool {
        self.made != self.synced
    }
}

/// The browser window's use cases.
pub struct Browser<E: WebEngine> {
    session: Session,
    settings: Settings,
    engine: E,
    profile: Profile,
    history: BrowsingHistory,
    /// The stores changed since they were last saved (or whose save
    /// failed): saved by [`Self::save_dirty`], at the latest on
    /// [`Self::persist`].
    dirty: Stores,
    bookmarks: Bookmarks,
    saved_session: SessionSnapshot,
    /// The session's revision when `saved_session` was last written (or
    /// found equal to it); `None`: unknown, to compare again.
    saved_revision: Option<u64>,
    remote_devices: Vec<RemoteDevice>,
    /// Bookmarks, settings, sign-ins or history changed since the last
    /// sync round that succeeded.
    unsynced: SyncChanges,
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
            dirty: Stores::NONE,
            bookmarks: Bookmarks::default(),
            saved_session: SessionSnapshot::default(),
            // An empty session: its snapshot is the default one.
            saved_revision: Some(Session::new().revision()),
            remote_devices: Vec::new(),
            unsynced: SyncChanges::default(),
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
    /// left empty (its file set aside, not to be overwritten); the errors
    /// are returned.
    pub fn load_profile(&mut self) -> Vec<BrowserError> {
        let mut errors = Vec::new();
        match self.profile.history.load() {
            Ok(history) => self.history = history,
            Err(e) => {
                errors.push(e.into());
                if let Err(e) = self.profile.history.set_aside() {
                    errors.push(e.into());
                }
            }
        }
        match self.profile.bookmarks.load() {
            Ok(bookmarks) => self.bookmarks = bookmarks,
            Err(e) => {
                errors.push(e.into());
                if let Err(e) = self.profile.bookmarks.set_aside() {
                    errors.push(e.into());
                }
            }
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
    /// none. Returns how many tabs were restored. A session that can't be
    /// read is logged and set aside (not overwritten by the next save).
    pub fn restore_session(&mut self) -> Result<usize, BrowserError> {
        let snapshot = match self.profile.session.load() {
            Ok(snapshot) => snapshot,
            Err(e) => {
                log::warn!("session: {e}");
                if let Err(e) = self.profile.session.set_aside() {
                    log::warn!("session: {e}");
                }
                SessionSnapshot::default()
            }
        };
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
        // Restored as saved, unless the file had something the tabs lost.
        self.saved_revision =
            (self.session.snapshot() == snapshot).then(|| self.session.revision());
        self.saved_session = snapshot;
        Ok(ids.len())
    }

    /// Whether [`Self::persist`] may have something to write. Cheap enough
    /// for every frame: the session is compared by revision, not snapshot.
    pub fn has_unsaved_changes(&self) -> bool {
        self.dirty != Stores::NONE || self.saved_revision != Some(self.session.revision())
    }

    /// Saves what changed since the last call: history, open tabs, and
    /// whatever failed to save before. Each is tried; the first error is
    /// returned, what failed is tried again next time.
    pub fn persist(&mut self) -> Result<(), BrowserError> {
        let stores = self.save_dirty(Stores::ALL);
        self.persist_session()?;
        stores
    }

    fn persist_session(&mut self) -> Result<(), BrowserError> {
        let revision = self.session.revision();
        if self.saved_revision != Some(revision) {
            // A revision may change with no change to save: written only
            // when the snapshot differs.
            let snapshot = self.session.snapshot();
            if snapshot != self.saved_session {
                self.profile.session.save(&snapshot)?;
                self.saved_session = snapshot;
            }
            self.saved_revision = Some(revision);
        }
        Ok(())
    }

    /// Notes that `stores` changed, to save.
    fn mark_dirty(&mut self, stores: Stores) {
        self.dirty = self.dirty | stores;
    }

    /// Saves those of `which` that changed. Each is tried: one failing does
    /// not keep the others from being saved, and stays to save (the next
    /// [`Self::persist`] tries again). Returns the first error, logs the
    /// others.
    fn save_dirty(&mut self, which: Stores) -> Result<(), BrowserError> {
        let mut first = None;
        for store in Stores::EACH {
            if !which.contains(store) || !self.dirty.contains(store) {
                continue;
            }
            let saved = match store {
                Stores::HISTORY => self.profile.history.save(&self.history),
                Stores::BOOKMARKS => self.profile.bookmarks.save(&self.bookmarks),
                Stores::SETTINGS => self.profile.settings.save(&self.settings),
                Stores::CREDENTIALS => self.profile.credentials.save(&self.credentials),
                _ => self.profile.seen_cookies.save(&self.seen_cookies),
            };
            match saved {
                Ok(()) => self.dirty = Stores(self.dirty.0 & !store.0),
                Err(e) if first.is_none() => first = Some(e),
                Err(e) => log::warn!("{e}"),
            }
        }
        first.map_or(Ok(()), |e| Err(e.into()))
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
        self.unsynced.note();
        self.mark_dirty(Stores::SETTINGS);
        self.save_dirty(Stores::SETTINGS)
    }

    /// Replaces this device's own settings (not synced).
    pub fn update_device_settings(
        &mut self,
        device: domain::DeviceSettings,
    ) -> Result<(), BrowserError> {
        if self.settings.device != device {
            self.settings.device = device;
            self.mark_dirty(Stores::SETTINGS);
        }
        self.save_dirty(Stores::SETTINGS)
    }

    /// Loads `address` in the active tab or a new one.
    pub fn load(&mut self, address: WebAddress, target: Target) -> Result<(), BrowserError> {
        match (target, self.session.active_id()) {
            // Asleep or crashed, it has no view: woken on `address`.
            (Target::ActiveTab, Some(id)) => self.load_in_tab(id, &address)?,
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
                        self.dirty = self.dirty | Stores::HISTORY;
                        self.unsynced.note();
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
                        self.dirty = self.dirty | Stores::HISTORY;
                        self.unsynced.note();
                    }
                }
            }
            EngineEvent::NewTabRequested { opener, address } => {
                self.open_link_tab(opener, address)?;
            }
            // Kept for the site's next pages; this device's, not synced.
            EngineEvent::ZoomChanged { host, percent } => {
                if self.settings.device.set_zoom(&host, percent) {
                    self.mark_dirty(Stores::SETTINGS);
                    self.save_dirty(Stores::SETTINGS)?;
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
    fn an_unreadable_session_is_set_aside_before_being_overwritten() {
        let aside = std::sync::Arc::new(std::sync::Mutex::new(0));
        let mut profile = Store::default().profile();
        profile.session = Box::new(CorruptRepository(aside.clone()));
        profile.history = Box::new(CorruptRepository(aside.clone()));
        let mut b = Browser::new(FakeEngine::default(), Settings::default(), profile);
        assert_eq!(b.load_profile().len(), 1);
        assert_eq!(*aside.lock().unwrap(), 1, "history");
        assert_eq!(b.restore_session().unwrap(), 0);
        assert_eq!(*aside.lock().unwrap(), 2, "session");
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

    /// `change` marks `b` dirty, and persisting saves the session as it is
    /// and leaves it clean.
    fn saves(
        b: &mut Browser<FakeEngine>,
        store: &Store,
        what: &str,
        change: impl FnOnce(&mut Browser<FakeEngine>),
    ) {
        assert!(!b.has_unsaved_changes(), "{what}: clean before");
        change(b);
        assert!(b.has_unsaved_changes(), "{what}: marks it dirty");
        b.persist().unwrap();
        assert!(!b.has_unsaved_changes(), "{what}: persist clears it");
        let saved = store.session.lock().unwrap().clone().unwrap_or_default();
        assert_eq!(saved, b.session().snapshot(), "{what}: saved as it is");
    }

    #[test]
    fn every_change_to_the_tabs_is_saved() {
        let store = Store::default();
        let mut b = browser_with(&store);
        b.restore_session().unwrap();
        assert!(!b.has_unsaved_changes(), "restored as saved");
        let t1 = b.session().active_id().unwrap();
        saves(&mut b, &store, "load", |b| finish(b, t1, "https://a.com/"));
        let (mut t2, mut t3) = (t1, t1);
        saves(&mut b, &store, "open", |b| {
            t2 = b.open_tab(Some(addr("https://b.com/"))).unwrap();
            finish(b, t2, "https://b.com/");
        });
        saves(&mut b, &store, "open another", |b| {
            t3 = b.open_tab(Some(addr("https://c.com/"))).unwrap();
            finish(b, t3, "https://c.com/");
        });
        saves(&mut b, &store, "title", |b| {
            b.handle(EngineEvent::TitleChanged {
                tab: t2,
                title: "B".into(),
            })
            .unwrap();
        });
        saves(&mut b, &store, "activate", |b| b.activate(t1));
        saves(&mut b, &store, "cycle", |b| b.cycle_tabs(1));
        saves(&mut b, &store, "move", |b| b.move_tab(t2, 0));
        saves(&mut b, &store, "pin", |b| b.set_pinned(t2, true));
        saves(&mut b, &store, "unpin", |b| b.set_pinned(t2, false));
        let mut group = None;
        saves(&mut b, &store, "new group", |b| group = b.new_group(t1));
        let group = group.unwrap();
        saves(&mut b, &store, "add to group", |b| {
            b.add_to_group(t2, group)
        });
        saves(&mut b, &store, "rename group", |b| {
            b.rename_group(group, "G")
        });
        saves(&mut b, &store, "group colour", |b| {
            let other = domain::GroupColor::next([b.session().group(group).unwrap().color]);
            b.set_group_color(group, other);
        });
        saves(&mut b, &store, "collapse", |b| {
            b.set_group_collapsed(group, true)
        });
        saves(&mut b, &store, "expand", |b| {
            b.set_group_collapsed(group, false)
        });
        saves(&mut b, &store, "out of group", |b| b.remove_from_group(t2));
        saves(&mut b, &store, "ungroup", |b| b.ungroup(group));
        saves(&mut b, &store, "split", |b| {
            b.activate(t1);
            b.split_with_active(t2);
        });
        saves(&mut b, &store, "split ratio", |b| {
            b.set_split_ratio(t1, 300)
        });
        saves(&mut b, &store, "unsplit", |b| b.unsplit(t1));
        let mut link = t1;
        saves(&mut b, &store, "link", |b| {
            b.handle(EngineEvent::NewTabRequested {
                opener: Some(t1),
                address: addr("https://link.com/"),
            })
            .unwrap();
            link = b.session().tabs().iter().map(|t| t.id()).max().unwrap();
            finish(b, link, "https://link.com/");
        });
        saves(&mut b, &store, "extension window", |b| {
            b.keep_out_of_session(link)
        });
        saves(&mut b, &store, "close", |b| b.close_tab(t2));
        saves(&mut b, &store, "reopen", |b| {
            let id = b.reopen_closed_tab().unwrap().unwrap();
            finish(b, id, "https://b.com/");
        });
        saves(&mut b, &store, "close group", |b| {
            let group = b.new_group(t1).unwrap();
            b.close_group(group);
        });
    }

    #[test]
    fn changes_left_out_of_the_session_write_nothing() {
        let store = Store::default();
        let mut b = browser_with(&store);
        let id = b.open_tab_in(Some(addr("https://p.com/")), true).unwrap();
        finish(&mut b, id, "https://p.com/");
        // The tab changed, not the snapshot: checked, nothing written.
        b.persist().unwrap();
        assert!(!b.has_unsaved_changes());
        assert!(store.session.lock().unwrap().is_none());
    }
}
