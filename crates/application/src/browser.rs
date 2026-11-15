use domain::{Session, Settings, TabId, WebAddress, resolve_omnibox};

use crate::BrowserError;
use crate::ports::{EngineEvent, Viewport, WebEngine};

/// The browser window's use cases.
pub struct Browser<E: WebEngine> {
    session: Session,
    settings: Settings,
    engine: E,
}

impl<E: WebEngine> Browser<E> {
    pub fn new(engine: E, settings: Settings) -> Self {
        Self {
            session: Session::new(),
            settings,
            engine,
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
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
        let Some(address) = resolve_omnibox(input, &self.settings.search_engine) else {
            return Ok(false);
        };
        match self.session.active_id() {
            Some(id) => self.engine.navigate(id, &address)?,
            None => {
                self.open_tab(Some(address))?;
            }
        }
        Ok(true)
    }

    pub fn go_home(&mut self) -> Result<(), BrowserError> {
        let home = self.settings.home_page.clone();
        match self.session.active_id() {
            Some(id) => self.engine.navigate(id, &home)?,
            None => {
                self.open_tab(Some(home))?;
            }
        }
        Ok(())
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
                    t.load_finished(address);
                }
            }
            EngineEvent::TitleChanged { tab, title } => {
                if let Some(t) = self.session.tab_mut(tab) {
                    t.set_title(title);
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
    use super::*;
    use crate::ports::EngineError;

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

    fn browser() -> Browser<FakeEngine> {
        Browser::new(FakeEngine::default(), Settings::default())
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
