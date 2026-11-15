use domain::{
    Bookmarks, BrowsingHistory, Session, SessionSnapshot, Settings, TabId, Timestamp, WebAddress,
    resolve_omnibox,
};

use crate::BrowserError;
use crate::ports::{EngineEvent, Profile, Viewport, WebEngine};

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
        errors
    }

    /// Reopens the tabs of the last run, or the home page when there were
    /// none. Returns how many tabs were restored.
    pub fn restore_session(&mut self) -> Result<usize, BrowserError> {
        let snapshot = self.profile.session.load().unwrap_or_default();
        let mut ids = Vec::with_capacity(snapshot.tabs.len());
        // Each new tab opens after the active one, i.e. the previous one.
        for address in &snapshot.tabs {
            ids.push(self.open_tab(Some(address.clone()))?);
        }
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

    pub fn clear_history(&mut self) -> Result<(), BrowserError> {
        self.history.clear();
        self.profile.history.save(&self.history)?;
        self.history_dirty = false;
        Ok(())
    }

    pub fn is_active_bookmarked(&self) -> bool {
        self.session
            .active()
            .and_then(|t| t.address())
            .is_some_and(|a| self.bookmarks.contains(a))
    }

    /// Bookmarks the active page, or removes its bookmark. Returns whether
    /// it is bookmarked afterwards.
    pub fn toggle_bookmark(&mut self) -> Result<bool, BrowserError> {
        let Some(tab) = self.session.active() else {
            return Ok(false);
        };
        let Some(address) = tab.address().cloned() else {
            return Ok(false);
        };
        let saved = self.bookmarks.toggle(address, tab.title());
        self.profile.bookmarks.save(&self.bookmarks)?;
        Ok(saved)
    }

    pub fn remove_bookmark(&mut self, address: &WebAddress) -> Result<(), BrowserError> {
        if self.bookmarks.remove(address) {
            self.profile.bookmarks.save(&self.bookmarks)?;
        }
        Ok(())
    }

    /// Loads `address` in the active tab or a new one.
    pub fn load(&mut self, address: WebAddress, target: Target) -> Result<(), BrowserError> {
        match (target, self.session.active_id()) {
            (Target::ActiveTab, Some(id)) => self.engine.navigate(id, &address)?,
            _ => {
                self.open_tab(Some(address))?;
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

    /// Opens a tab on `address` (the home page when `None`) and shows it.
    pub fn open_tab(&mut self, address: Option<WebAddress>) -> Result<TabId, BrowserError> {
        let address = address.unwrap_or_else(|| self.settings.home_page.clone());
        let id = self.session.open_tab();
        if let Err(e) = self.engine.open_view(id, &address) {
            self.session.close_tab(id);
            return Err(e.into());
        }
        self.engine.show(Some(id));
        Ok(id)
    }

    pub fn close_tab(&mut self, id: TabId) {
        if self.session.close_tab(id) {
            self.engine.close_view(id);
            self.engine.show(self.session.active_id());
        }
    }

    pub fn close_active_tab(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.close_tab(id);
        }
    }

    pub fn activate(&mut self, id: TabId) {
        if self.session.activate(id) {
            self.engine.show(Some(id));
        }
    }

    /// Activates the tab `offset` places away from the active one.
    pub fn cycle_tabs(&mut self, offset: isize) {
        if let Some(id) = self.session.neighbour(offset) {
            self.activate(id);
        }
    }

    /// Loads what was typed in the address bar in the active tab (in a new
    /// one when there is none). Returns `false` when there was nothing to load.
    pub fn submit_omnibox(&mut self, input: &str) -> Result<bool, BrowserError> {
        self.submit(input, Target::ActiveTab)
    }

    /// Loads typed input (address, host name or search) in `target`.
    /// Returns `false` when there was nothing to load.
    pub fn submit(&mut self, input: &str, target: Target) -> Result<bool, BrowserError> {
        let Some(address) = resolve_omnibox(input, &self.settings.search_engine) else {
            return Ok(false);
        };
        self.load(address, target)?;
        Ok(true)
    }

    pub fn go_home(&mut self) -> Result<(), BrowserError> {
        self.load(self.settings.home_page.clone(), Target::ActiveTab)
    }

    pub fn go_back(&mut self) {
        if let Some(tab) = self.session.active()
            && tab.history().can_go_back()
        {
            self.engine.go_back(tab.id());
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(tab) = self.session.active()
            && tab.history().can_go_forward()
        {
            self.engine.go_forward(tab.id());
        }
    }

    pub fn reload(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.engine.reload(id);
        }
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.engine.set_viewport(viewport);
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
                if let Some(t) = self.session.tab_mut(tab) {
                    t.load_finished(address.clone());
                    let title = t.title().to_owned();
                    if self
                        .history
                        .record(address.clone(), self.profile.clock.now())
                    {
                        // The title may have come before the end of the load.
                        self.history.retitle(&address, &title);
                        self.history_dirty = true;
                    }
                }
            }
            EngineEvent::TitleChanged { tab, title } => {
                if let Some(t) = self.session.tab_mut(tab) {
                    t.set_title(title);
                    if let Some(address) = t.address()
                        && self.history.retitle(address, t.title())
                    {
                        self.history_dirty = true;
                    }
                }
            }
            EngineEvent::NewTabRequested { address } => {
                self.open_tab(Some(address))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use crate::ports::{Clock, EngineError, Repository, RepositoryError};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Open(u64, String),
        Close(u64),
        Show(Option<u64>),
        Navigate(u64, String),
        Back(u64),
        Forward(u64),
        Reload(u64),
        Viewport(Viewport),
    }

    #[derive(Default)]
    struct FakeEngine {
        calls: Vec<Call>,
        fail_open: bool,
    }

    impl WebEngine for FakeEngine {
        fn open_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
            if self.fail_open {
                return Err(EngineError("boom".into()));
            }
            self.calls.push(Call::Open(tab.get(), address.to_string()));
            Ok(())
        }
        fn close_view(&mut self, tab: TabId) {
            self.calls.push(Call::Close(tab.get()));
        }
        fn show(&mut self, tab: Option<TabId>) {
            self.calls.push(Call::Show(tab.map(TabId::get)));
        }
        fn navigate(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
            self.calls
                .push(Call::Navigate(tab.get(), address.to_string()));
            Ok(())
        }
        fn go_back(&mut self, tab: TabId) {
            self.calls.push(Call::Back(tab.get()));
        }
        fn go_forward(&mut self, tab: TabId) {
            self.calls.push(Call::Forward(tab.get()));
        }
        fn reload(&mut self, tab: TabId) {
            self.calls.push(Call::Reload(tab.get()));
        }
        fn set_viewport(&mut self, viewport: Viewport) {
            self.calls.push(Call::Viewport(viewport));
        }
    }

    /// A repository whose content the test can read and preset.
    struct MemoryRepository<T>(Rc<RefCell<Option<T>>>);

    impl<T: Clone + Default> Repository<T> for MemoryRepository<T> {
        fn load(&self) -> Result<T, RepositoryError> {
            Ok(self.0.borrow().clone().unwrap_or_default())
        }
        fn save(&self, value: &T) -> Result<(), RepositoryError> {
            *self.0.borrow_mut() = Some(value.clone());
            Ok(())
        }
    }

    struct FailingRepository;

    impl<T> Repository<T> for FailingRepository {
        fn load(&self) -> Result<T, RepositoryError> {
            Err(RepositoryError("disk".into()))
        }
        fn save(&self, _: &T) -> Result<(), RepositoryError> {
            Err(RepositoryError("disk".into()))
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> domain::Timestamp {
            1_000
        }
    }

    type Cell<T> = Rc<RefCell<Option<T>>>;

    #[derive(Default)]
    struct Store {
        history: Cell<BrowsingHistory>,
        bookmarks: Cell<Bookmarks>,
        session: Cell<SessionSnapshot>,
    }

    impl Store {
        fn profile(&self) -> Profile {
            Profile {
                history: Box::new(MemoryRepository(self.history.clone())),
                bookmarks: Box::new(MemoryRepository(self.bookmarks.clone())),
                session: Box::new(MemoryRepository(self.session.clone())),
                clock: Box::new(FixedClock),
            }
        }
    }

    fn browser_with(store: &Store) -> Browser<FakeEngine> {
        Browser::new(FakeEngine::default(), Settings::default(), store.profile())
    }

    fn browser() -> Browser<FakeEngine> {
        browser_with(&Store::default())
    }

    fn finish(b: &mut Browser<FakeEngine>, tab: TabId, url: &str) {
        b.handle(EngineEvent::LoadFinished {
            tab,
            address: addr(url),
        })
        .unwrap();
    }

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

        assert!(store.history.borrow().is_none(), "saved only on persist");
        assert!(b.has_unsaved_changes());
        b.persist().unwrap();
        assert!(!b.has_unsaved_changes());
        assert_eq!(store.history.borrow().as_ref().unwrap().len(), 1);

        b.clear_history().unwrap();
        assert!(store.history.borrow().as_ref().unwrap().is_empty());
    }

    #[test]
    fn bookmarks_are_toggled_on_the_active_page_and_saved() {
        let store = Store::default();
        let mut b = browser_with(&store);
        assert!(!b.toggle_bookmark().unwrap());
        let id = b.open_tab(None).unwrap();
        assert!(!b.toggle_bookmark().unwrap(), "nothing loaded yet");
        finish(&mut b, id, "https://a.com/");
        assert!(b.toggle_bookmark().unwrap());
        assert!(b.is_active_bookmarked());
        assert_eq!(store.bookmarks.borrow().as_ref().unwrap().len(), 1);
        b.remove_bookmark(&addr("https://a.com/")).unwrap();
        assert!(!b.is_active_bookmarked());
        assert!(store.bookmarks.borrow().as_ref().unwrap().is_empty());
    }

    #[test]
    fn profile_errors_are_reported_and_leave_empty_data() {
        let mut b = Browser::new(
            FakeEngine::default(),
            Settings::default(),
            Profile {
                history: Box::new(FailingRepository),
                bookmarks: Box::new(FailingRepository),
                session: Box::new(FailingRepository),
                clock: Box::new(FixedClock),
            },
        );
        assert_eq!(b.load_profile().len(), 2);
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

        let saved = store.session.borrow().clone().unwrap();
        assert_eq!(
            saved.tabs,
            vec![addr("https://a.com/"), addr("https://b.com/")]
        );
        assert_eq!(saved.active, Some(0));

        let mut restored = browser_with(&store);
        assert_eq!(restored.restore_session().unwrap(), 2);
        let calls = take_calls(&mut restored);
        assert!(calls.contains(&Call::Open(1, "https://a.com/".into())));
        assert!(calls.contains(&Call::Open(2, "https://b.com/".into())));
        assert_eq!(calls.last(), Some(&Call::Show(Some(1))));
        assert_eq!(restored.session().active_id().map(TabId::get), Some(1));
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

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    fn take_calls(b: &mut Browser<FakeEngine>) -> Vec<Call> {
        std::mem::take(&mut b.engine_mut().calls)
    }

    #[test]
    fn open_tab_loads_the_home_page_and_shows_it() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        assert_eq!(
            take_calls(&mut b),
            vec![
                Call::Open(id.get(), "https://duckduckgo.com/".into()),
                Call::Show(Some(id.get())),
            ]
        );
        assert_eq!(b.session().active_id(), Some(id));
    }

    #[test]
    fn a_failed_open_leaves_no_tab() {
        let mut b = browser();
        b.engine_mut().fail_open = true;
        assert!(b.open_tab(None).is_err());
        assert!(b.session().is_empty());
    }

    #[test]
    fn closing_shows_the_next_active_tab() {
        let mut b = browser();
        let first = b.open_tab(None).unwrap();
        let second = b.open_tab(None).unwrap();
        take_calls(&mut b);
        b.close_tab(second);
        assert_eq!(
            take_calls(&mut b),
            vec![Call::Close(second.get()), Call::Show(Some(first.get()))]
        );
        b.close_active_tab();
        assert!(b.session().is_empty());
    }

    #[test]
    fn omnibox_navigates_the_active_tab() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        take_calls(&mut b);
        assert!(b.submit_omnibox("rust-lang.org").unwrap());
        assert!(!b.submit_omnibox("  ").unwrap());
        assert_eq!(
            take_calls(&mut b),
            vec![Call::Navigate(id.get(), "https://rust-lang.org/".into())]
        );
    }

    #[test]
    fn omnibox_without_tabs_opens_one() {
        let mut b = browser();
        b.submit_omnibox("hello").unwrap();
        assert_eq!(b.session().len(), 1);
    }

    #[test]
    fn back_and_forward_follow_the_mirrored_history() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        b.go_back();
        b.go_forward();
        take_calls(&mut b);
        for url in ["https://a.com/", "https://b.com/"] {
            b.handle(EngineEvent::LoadFinished {
                tab: id,
                address: addr(url),
            })
            .unwrap();
        }
        b.go_back();
        b.go_forward();
        assert_eq!(take_calls(&mut b), vec![Call::Back(id.get())]);
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
    fn new_window_requests_open_a_tab() {
        let mut b = browser();
        b.open_tab(None).unwrap();
        b.handle(EngineEvent::NewTabRequested {
            address: addr("https://popup.com/"),
        })
        .unwrap();
        assert_eq!(b.session().len(), 2);
    }

    #[test]
    fn cycling_and_reload_target_the_active_tab() {
        let mut b = browser();
        let first = b.open_tab(None).unwrap();
        let second = b.open_tab(None).unwrap();
        b.cycle_tabs(1);
        assert_eq!(b.session().active_id(), Some(first));
        take_calls(&mut b);
        b.reload();
        b.go_home().unwrap();
        b.set_viewport(Viewport::default());
        assert_eq!(
            take_calls(&mut b),
            vec![
                Call::Reload(first.get()),
                Call::Navigate(first.get(), "https://duckduckgo.com/".into()),
                Call::Viewport(Viewport::default()),
            ]
        );
        assert_ne!(first, second);
    }
}
