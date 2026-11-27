//! Fakes and helpers shared by the browser's tests.

use std::sync::{Arc, Mutex};

pub(crate) use domain::{
    Bookmarks, BrowsingHistory, Credentials, DeviceId, SeenCookies, SessionSnapshot, Settings,
    SiteCookie, TabId, WebAddress,
};

pub(crate) use super::Browser;
pub(crate) use crate::ports::{
    Clock, EngineError, EngineEvent, Profile, Repository, RepositoryError, Viewport, WebEngine,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Call {
    Open(u64, String),
    Close(u64),
    Show(Option<u64>),
    Navigate(u64, String),
    Back(u64),
    Forward(u64),
    Reload(u64),
    Viewport(Viewport),
    Fill(u64, String, String),
    SetCookies(Vec<String>),
    Script(u64, String),
    Print(u64),
}

#[derive(Default)]
pub(crate) struct FakeEngine {
    pub(crate) calls: Vec<Call>,
    pub(crate) fail_open: bool,
    pub(crate) cookies: Option<Vec<SiteCookie>>,
}

impl WebEngine for FakeEngine {
    fn open_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        if self.fail_open {
            return Err(EngineError("boom".into()));
        }
        self.calls.push(Call::Open(tab.get(), address.to_string()));
        Ok(())
    }
    fn open_private_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        self.open_view(tab, address)
    }
    fn close_view(&mut self, tab: TabId) {
        self.calls.push(Call::Close(tab.get()));
    }
    fn show(&mut self, tab: Option<TabId>) {
        self.calls.push(Call::Show(tab.map(TabId::get)));
    }
    fn can_split(&self) -> bool {
        true
    }
    fn run_in_tab(&mut self, tab: TabId, script: &str) {
        self.calls.push(Call::Script(tab.get(), script.to_owned()));
    }
    fn print(&mut self, tab: TabId) {
        self.calls.push(Call::Print(tab.get()));
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
    fn fill_credentials(&mut self, tab: TabId, origin: &str, username: &str, _: &str) {
        self.calls
            .push(Call::Fill(tab.get(), origin.into(), username.into()));
    }
    fn site_cookies(&mut self) -> Option<Vec<SiteCookie>> {
        self.cookies.clone()
    }
    fn set_site_cookies(&mut self, cookies: &[SiteCookie]) {
        self.calls.push(Call::SetCookies(
            cookies.iter().map(|c| c.value.clone()).collect(),
        ));
        if let Some(all) = &mut self.cookies {
            all.extend_from_slice(cookies);
        }
    }
}

/// A repository whose content the test can read and preset.
pub(crate) struct MemoryRepository<T>(Arc<Mutex<Option<T>>>);

impl<T: Clone + Default + Send> Repository<T> for MemoryRepository<T> {
    fn load(&self) -> Result<T, RepositoryError> {
        Ok(self.0.lock().unwrap().clone().unwrap_or_default())
    }
    fn save(&self, value: &T) -> Result<(), RepositoryError> {
        *self.0.lock().unwrap() = Some(value.clone());
        Ok(())
    }
}

pub(crate) struct FailingRepository;

impl<T> Repository<T> for FailingRepository {
    fn load(&self) -> Result<T, RepositoryError> {
        Err(RepositoryError("disk".into()))
    }
    fn save(&self, _: &T) -> Result<(), RepositoryError> {
        Err(RepositoryError("disk".into()))
    }
}

pub(crate) struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> domain::Timestamp {
        1_000
    }
}

pub(crate) type Cell<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub(crate) struct Store {
    pub(crate) settings: Cell<Settings>,
    pub(crate) history: Cell<BrowsingHistory>,
    pub(crate) bookmarks: Cell<Bookmarks>,
    pub(crate) session: Cell<SessionSnapshot>,
    pub(crate) credentials: Cell<Credentials>,
    pub(crate) seen_cookies: Cell<SeenCookies>,
}

impl Store {
    pub(crate) fn profile(&self) -> Profile {
        Profile {
            settings: Box::new(MemoryRepository(self.settings.clone())),
            history: Box::new(MemoryRepository(self.history.clone())),
            bookmarks: Box::new(MemoryRepository(self.bookmarks.clone())),
            session: Box::new(MemoryRepository(self.session.clone())),
            credentials: Box::new(MemoryRepository(self.credentials.clone())),
            seen_cookies: Box::new(MemoryRepository(self.seen_cookies.clone())),
            clock: Box::new(FixedClock),
        }
    }
}

pub(crate) fn browser_with(store: &Store) -> Browser<FakeEngine> {
    Browser::new(FakeEngine::default(), Settings::default(), store.profile())
}

pub(crate) fn browser() -> Browser<FakeEngine> {
    browser_with(&Store::default())
}

pub(crate) fn finish(b: &mut Browser<FakeEngine>, tab: TabId, url: &str) {
    b.handle(EngineEvent::LoadFinished {
        tab,
        address: addr(url),
    })
    .unwrap();
}

pub(crate) fn device(id: &str) -> DeviceId {
    DeviceId::new(id).unwrap()
}

pub(crate) fn signed_in_browser() -> (Store, Browser<FakeEngine>, TabId) {
    let store = Store::default();
    let mut settings = Settings::default();
    settings.device.save_passwords = true;
    let mut b = Browser::new(FakeEngine::default(), settings, store.profile());
    let id = b.open_tab(None).unwrap();
    finish(&mut b, id, "https://a.com/login");
    (store, b, id)
}

pub(crate) fn addr(s: &str) -> WebAddress {
    WebAddress::parse(s).unwrap()
}

pub(crate) fn take_calls(b: &mut Browser<FakeEngine>) -> Vec<Call> {
    std::mem::take(&mut b.engine_mut().calls)
}
