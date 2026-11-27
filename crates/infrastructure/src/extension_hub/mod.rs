//! The browser side of the extension APIs the runtime (`page_scripts`)
//! cannot answer alone: storage (and its change events), messages between
//! content scripts, background pages and the popup, tabs, `scripting` and
//! `declarativeNetRequest`. Shared by the desktop and Android: what touches
//! the tabs or the web views goes through [`HubHost`].
//!
//! Each API namespace has its handler, in its own module: [`Hub::handle`]
//! checks the message, then hands it on by its `op`.

mod blocker;
mod dnr;
mod fetch;
mod native;
mod ports;
mod runtime;
mod scripting;
mod storage;
mod tabs;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use application::Browser;
use application::ports::{PageSource, WebEngine};
use domain::{ExtensionId, Extensions, TabId, WebAddress};
use serde_json::{Map, Value, json};

use crate::blocker::BlockerHandle;
use crate::extension_fetch::Fetcher;
use crate::extension_server::Origins;
use crate::extensions::state::ExtensionState;
use crate::native_messaging::NativeMessaging;
use crate::page_scripts::{BLOCKER_OWNER, Bridge};
use crate::script_storage::{Message, ScriptStorage, valid_owner};

/// A tab, as the extension APIs describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabState {
    pub id: TabId,
    pub url: String,
    pub title: String,
    pub active: bool,
    pub loading: bool,
}

/// What the hub needs from the browser.
pub trait HubHost {
    fn tabs(&self) -> Vec<TabState>;
    fn create_tab(&mut self, url: Option<&str>, active: bool) -> Option<TabId>;
    /// A tab for an extension's popup window (`windows.create` of type
    /// `popup`): opened from the active tab, which it goes back to once
    /// closed.
    fn create_window_tab(&mut self, url: Option<&str>, active: bool) -> Option<TabId> {
        self.create_tab(url, active)
    }
    fn update_tab(&mut self, tab: TabId, url: Option<&str>, active: bool);
    fn remove_tab(&mut self, tab: TabId);
    fn reload_tab(&mut self, tab: TabId);
    /// Sends `message` to the page scripts of `to`.
    fn deliver(&mut self, to: &PageSource, message: &str);
    fn run_in_tab(&mut self, tab: TabId, script: &str);
    /// Extension `id` changed its rules or registered scripts.
    fn extension_changed(&mut self, id: &ExtensionId);
}

/// [`HubHost`] on a [`Browser`]: what the desktop and Android use.
pub struct BrowserHub<'a, E: WebEngine> {
    pub browser: &'a mut Browser<E>,
    /// The installed extensions (to rebuild their rules and scripts).
    pub extensions: &'a Extensions,
}

impl<E: WebEngine> HubHost for BrowserHub<'_, E> {
    fn tabs(&self) -> Vec<TabState> {
        let session = self.browser.session();
        let active = session.active_id();
        session
            .tabs()
            .iter()
            .map(|t| TabState {
                id: t.id(),
                url: t
                    .address()
                    .map_or_else(|| "about:blank".to_owned(), |a| a.to_string()),
                title: t.title().to_owned(),
                active: Some(t.id()) == active,
                loading: t.is_loading(),
            })
            .collect()
    }

    fn create_tab(&mut self, url: Option<&str>, active: bool) -> Option<TabId> {
        let previous = self.browser.session().active_id();
        let address = url.and_then(|u| WebAddress::parse(u).ok());
        let tab = self.browser.open_tab(address).ok()?;
        if !active && let Some(previous) = previous {
            self.browser.activate(previous);
        }
        Some(tab)
    }

    fn create_window_tab(&mut self, url: Option<&str>, active: bool) -> Option<TabId> {
        let (Some(opener), Some(address)) = (
            self.browser.session().active_id(),
            url.and_then(|u| WebAddress::parse(u).ok()),
        ) else {
            return self.create_tab(url, active);
        };
        let tab = self.browser.open_link_tab(Some(opener), address).ok()?;
        // It goes with this run: not reopened as a tab next time.
        self.browser.keep_out_of_session(tab);
        if active {
            self.browser.activate(tab);
        }
        Some(tab)
    }

    fn update_tab(&mut self, tab: TabId, url: Option<&str>, active: bool) {
        if let Some(address) = url.and_then(|u| WebAddress::parse(u).ok()) {
            let _ = self.browser.load_in_tab(tab, &address);
        }
        if active {
            self.browser.activate(tab);
        }
    }

    fn remove_tab(&mut self, tab: TabId) {
        self.browser.close_tab(tab);
    }

    fn reload_tab(&mut self, tab: TabId) {
        self.browser.reload_tab(tab);
    }

    fn deliver(&mut self, to: &PageSource, message: &str) {
        self.browser.engine_mut().reply_to_page(to, message);
    }

    fn run_in_tab(&mut self, tab: TabId, script: &str) {
        self.browser.engine_mut().run_in_tab(tab, script);
    }

    fn extension_changed(&mut self, _id: &ExtensionId) {
        self.browser.engine_mut().set_extensions(self.extensions);
    }
}

/// A request waiting for answers (messages) or a result (scripts).
struct Pending {
    to: PageSource,
    id: u64,
    ch: Option<String>,
    /// Receivers that did not answer yet.
    waiting: usize,
    /// Where it went: when they all close, nobody will answer.
    receivers: Vec<PageSource>,
    /// When it was sent, and whether a receiver took it on (an
    /// asynchronous listener): see [`UNTAKEN`].
    since: Instant,
    taken: bool,
}

pub struct Hub {
    storage: ScriptStorage,
    token: String,
    bridge: Bridge,
    extensions_dir: PathBuf,
    state_dir: PathBuf,
    next_rid: u64,
    pending: HashMap<u64, Pending>,
    /// `executeScript` results, by one-time nonce.
    scripts: HashMap<String, Pending>,
    /// Owners (`ext:<id>`) with content scripts in each tab.
    tab_owners: HashMap<TabId, HashSet<String>>,
    backgrounds: HashSet<String>,
    panel: Option<String>,
    /// Tabs showing an extension's own page (a window it opened, as
    /// Bitwarden's passkey window): by owner, as one of its pages.
    page_tabs: HashMap<TabId, String>,
    /// The popup lost the focus: the user clicked elsewhere.
    panel_blurred: bool,
    /// The tabs as last seen, for `tabs.on*` events.
    last_tabs: Vec<TabState>,
    /// Ports to native hosts (`runtime.connectNative`), and whose they are.
    native: NativeMessaging,
    native_ports: HashMap<u64, (PageSource, String)>,
    /// Ports between an extension's pages and scripts (`runtime.connect`,
    /// `tabs.connect`), by id.
    ports: HashMap<u64, Port>,
    next_port: u64,
    /// Pages gone since the last frame: their ports close.
    closed_pages: Vec<PageSource>,
    /// Makes the extension pages' requests to their permitted hosts (no
    /// fetcher: they get none, the page falls back to its own `fetch`).
    fetcher: Option<Fetcher>,
    /// Finished requests: who asked, and the answer.
    fetched_tx: Sender<FetchDone>,
    fetched_rx: Receiver<FetchDone>,
    /// Wakes the UI when something finished in the background.
    wake: Arc<dyn Fn() + Send + Sync>,
    /// The ad blocker, for its cosmetic filters.
    blocker: Option<BlockerHandle>,
    /// Tabs opened by `windows.create`: each is a window of its own for the
    /// extensions (its id, its type), closed with the tab.
    popups: HashMap<TabId, (u64, String)>,
    /// The size (points) a popup window asked for, by tab.
    window_sizes: HashMap<TabId, (u32, u32)>,
    next_window: u64,
    /// Background pages whose scripts ran: their listeners are in.
    ready: HashSet<String>,
    /// Messages for a background page not ready yet (the browser just
    /// started): Chrome wakes the worker up, here they wait for it.
    queued: HashMap<String, Vec<Queued>>,
    /// Whether each extension has a background page, by owner.
    has_background: HashMap<String, bool>,
}

/// A `runtime.sendMessage` waiting for its extension's background page.
struct Queued {
    from: PageSource,
    id: u64,
    ch: Option<String>,
    payload: Value,
    sender: Value,
    since: Instant,
}

/// At most this many messages wait for a background page (one that never
/// starts would keep them all).
const MAX_QUEUED: usize = 64;

/// A message no listener took on by then never reached one (its page was
/// loading, reloading or gone): the sender gets "no answer", as in Chrome,
/// rather than waiting forever.
const UNTAKEN: Duration = Duration::from_secs(3);

/// A background page not ready by then is not starting: the messages
/// waiting for it fail, as in Chrome when nothing receives them.
const NEVER_READY: Duration = Duration::from_secs(10);

const NO_RECEIVER: &str = "Could not establish connection. Receiving end does not exist.";

/// The browser's window, holding every tab not opened as a window.
const MAIN_WINDOW: u64 = 1;

type FetchDone = (PageSource, u64, Option<String>, Result<Value, String>);

/// A port: the page that opened it, the pages that got `onConnect`.
struct Port {
    owner: String,
    opener: PageSource,
    receivers: Vec<PageSource>,
}

fn tab_id(value: &Value) -> Option<TabId> {
    value.as_u64().map(TabId::new)
}

impl Hub {
    pub fn new(
        storage: ScriptStorage,
        bridge: Bridge,
        extensions_dir: impl Into<PathBuf>,
        state_dir: impl Into<PathBuf>,
    ) -> Self {
        let state_dir = state_dir.into();
        ExtensionState::end_sessions(&state_dir);
        let (fetched_tx, fetched_rx) = mpsc::channel();
        Self {
            token: storage.token().to_owned(),
            storage,
            bridge,
            extensions_dir: extensions_dir.into(),
            state_dir,
            next_rid: 1,
            pending: HashMap::new(),
            scripts: HashMap::new(),
            tab_owners: HashMap::new(),
            backgrounds: HashSet::new(),
            panel: None,
            page_tabs: HashMap::new(),
            panel_blurred: false,
            last_tabs: Vec::new(),
            native: NativeMessaging::default(),
            native_ports: HashMap::new(),
            ports: HashMap::new(),
            next_port: 1,
            closed_pages: Vec::new(),
            fetcher: None,
            fetched_tx,
            fetched_rx,
            wake: Arc::new(|| {}),
            blocker: None,
            popups: HashMap::new(),
            window_sizes: HashMap::new(),
            next_window: MAIN_WINDOW + 1,
            ready: HashSet::new(),
            queued: HashMap::new(),
            has_background: HashMap::new(),
        }
    }

    /// The ad blocker whose cosmetic filters the pages ask
    /// (`blocker.cosmetic`).
    pub fn set_blocker(&mut self, blocker: BlockerHandle) {
        self.blocker = Some(blocker);
    }

    /// Where the extensions' pages are served (their `origin`).
    fn origins(&self) -> Origins {
        match self.bridge {
            Bridge::Ipc => Origins::desktop(),
            Bridge::Android => Origins::Android,
        }
    }

    /// Whether messages wait for an answer: the caller then calls
    /// [`Self::tabs_changed`] again a moment later, for them to expire.
    pub fn waiting(&self) -> bool {
        self.pending.values().any(|p| !p.taken) || !self.queued.is_empty()
    }

    /// Called when a native host sends something or a request finishes:
    /// the UI then calls [`Self::pump_native`].
    pub fn set_native_waker(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let native = wake.clone();
        self.native.set_waker(move || native());
        self.wake = wake;
    }

    /// How the extension pages' requests to their permitted hosts are made.
    pub fn set_fetcher(&mut self, fetcher: Fetcher) {
        self.fetcher = Some(fetcher);
    }

    /// Hands what native hosts sent to the pages holding their ports, and
    /// finished requests to the pages that made them.
    pub fn pump_native(&mut self, host: &mut dyn HubHost) {
        self.deliver_fetched(host);
        self.poll_native(host);
    }

    /// Forgets what extension or user script `owner` stored.
    pub fn forget(&mut self, owner: &str) {
        self.storage.forget(owner);
        if let Some(id) = owner.strip_prefix("ext:").and_then(ExtensionId::parse) {
            ExtensionState::forget(&self.state_dir, &id);
        }
        self.backgrounds.remove(owner);
        self.ready.remove(owner);
        self.queued.remove(owner);
        self.has_background.remove(owner);
        self.close_native_ports(|_, o| o == owner);
        self.ports.retain(|_, p| p.owner != owner);
    }

    /// A background page went away (extension disabled or removed).
    pub fn background_closed(&mut self, id: &ExtensionId) {
        let owner = format!("ext:{id}");
        self.backgrounds.remove(&owner);
        self.ready.remove(&owner);
        self.has_background.remove(&owner);
        let page = PageSource::Background(id.clone());
        self.close_native_ports(|p, _| *p == page);
        self.closed_pages.push(page);
    }

    /// The popup closed.
    pub fn panel_closed(&mut self) {
        if self.panel.is_some() {
            self.closed_pages.push(PageSource::Panel);
        }
        self.panel = None;
        self.panel_blurred = false;
        self.close_native_ports(|p, _| *p == PageSource::Panel);
    }

    /// Whether the popup lost the focus since the last call (it closes).
    pub fn take_panel_blur(&mut self) -> bool {
        std::mem::take(&mut self.panel_blurred)
    }

    fn reply(
        host: &mut dyn HubHost,
        to: &PageSource,
        id: u64,
        ch: &Option<String>,
        result: Result<Value, String>,
    ) {
        let mut msg = match result {
            Ok(r) => json!({"id": id, "result": r}),
            Err(e) => json!({"id": id, "error": e}),
        };
        if let Some(ch) = ch {
            msg["ch"] = ch.clone().into();
        }
        host.deliver(to, &msg.to_string());
    }

    /// Every place the scripts of `owner` run.
    fn endpoints(&self, owner: &str) -> Vec<PageSource> {
        let mut out: Vec<PageSource> = self
            .tab_owners
            .iter()
            .filter(|(_, owners)| owners.contains(owner))
            .map(|(tab, _)| PageSource::Tab(*tab))
            .collect();
        if self.backgrounds.contains(owner)
            && let Some(id) = owner.strip_prefix("ext:").and_then(ExtensionId::parse)
        {
            out.push(PageSource::Background(id));
        }
        if self.panel.as_deref() == Some(owner) {
            out.push(PageSource::Panel);
        }
        for tab in self.page_tabs_of(owner) {
            if !out.contains(&tab) {
                out.push(tab);
            }
        }
        out
    }

    /// The tabs showing a page of `owner`.
    fn page_tabs_of(&self, owner: &str) -> Vec<PageSource> {
        self.page_tabs
            .iter()
            .filter(|(_, o)| o.as_str() == owner)
            .map(|(tab, _)| PageSource::Tab(*tab))
            .collect()
    }

    fn broadcast(&self, owner: &str, event: Value, host: &mut dyn HubHost) {
        let text = event.to_string();
        for to in self.endpoints(owner) {
            host.deliver(&to, &text);
        }
    }

    /// A message of page scripts running in `from`.
    pub fn handle(&mut self, from: &PageSource, body: &str, host: &mut dyn HubHost) {
        let Ok(Value::Object(mut raw)) = serde_json::from_str::<Value>(body) else {
            return;
        };
        // An executeScript result: no token (the page could read it), a nonce.
        if raw.get("op").and_then(Value::as_str) == Some("scriptResult") {
            self.script_result(&mut raw, host);
            return;
        }
        let Ok(message) = serde_json::from_value::<Message>(Value::Object(raw.clone())) else {
            return;
        };
        if message.t != self.token {
            return;
        }
        let (id, ch, owner) = (message.id, message.ch.clone(), message.owner.clone());
        // The ad blocker's script: its cosmetic filters, nothing else.
        if owner == BLOCKER_OWNER {
            let result = self.blocker_call(&message.op, &raw);
            Self::reply(host, from, id, &ch, result);
            return;
        }
        if !valid_owner(&owner) {
            return;
        }
        let ctx = raw
            .get("ctx")
            .and_then(Value::as_str)
            .unwrap_or("content")
            .to_owned();
        if ScriptStorage::is_storage_op(&message.op) {
            let result = self.storage_call(&owner, message, host);
            Self::reply(host, from, id, &ch, result);
            return;
        }
        let ext = owner.strip_prefix("ext:").and_then(ExtensionId::parse);
        let call = Call {
            from,
            ctx: &ctx,
            owner: &owner,
            ext: ext.as_ref(),
            op: &message.op,
            id,
            ch: &ch,
            raw: &raw,
        };
        if let Some(result) = self.call(&call, host) {
            Self::reply(host, from, id, &ch, result);
        }
    }

    /// Hands `call` to the handler of its API namespace (the `op` before
    /// its dot). `None`: the answer comes later.
    fn call(&mut self, call: &Call<'_>, host: &mut dyn HubHost) -> Answer {
        let namespace = call.op.split_once('.').map_or("", |(ns, _)| ns);
        match (namespace, call.op) {
            ("", _) | (_, "tabs.sendMessage") => self.runtime_call(call, host),
            ("port", _) => self.port_call(call, host),
            ("native", _) => self.native_call(call),
            ("net", _) => self.fetch_call(call),
            ("tabs" | "windows", _) => self.tabs_call(call, host),
            ("scripting", _) => self.scripting_call(call, host),
            ("dnr", _) => self.dnr_call(call, host),
            _ => unsupported(call.op),
        }
    }

    pub fn extensions_dir(&self) -> &Path {
        &self.extensions_dir
    }
}

/// A call of an extension's page scripts, as the handlers get it.
#[derive(Clone, Copy)]
struct Call<'a> {
    from: &'a PageSource,
    /// What the page says it is: "content", "background" or "page".
    ctx: &'a str,
    /// `ext:<id>` or `us:<id>`.
    owner: &'a str,
    /// The extension of `owner` (`None`: a user script).
    ext: Option<&'a ExtensionId>,
    op: &'a str,
    id: u64,
    ch: &'a Option<String>,
    /// The whole message.
    raw: &'a Map<String, Value>,
}

impl Call<'_> {
    fn field(&self, key: &str) -> Value {
        self.raw.get(key).cloned().unwrap_or(Value::Null)
    }
}

/// A handler's answer. `None`: it comes later (or never).
type Answer = Option<Result<Value, String>>;

fn unsupported(op: &str) -> Answer {
    Some(Err(format!("{op} is not supported")))
}
