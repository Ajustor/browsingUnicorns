//! The browser side of the extension APIs the runtime (`page_scripts`)
//! cannot answer alone: storage (and its change events), messages between
//! content scripts, background pages and the popup, tabs, `scripting` and
//! `declarativeNetRequest`. Shared by the desktop and Android: what touches
//! the tabs or the web views goes through [`HubHost`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use application::Browser;
use application::ports::{PageSource, WebEngine};
use domain::{ExtensionId, Extensions, TabId, UrlPattern, WebAddress};
use serde_json::{Map, Value, json};

use crate::blocker::BlockerHandle;
use crate::extension_fetch::{self, FetchRequest, Fetcher};
use crate::extension_server::Origins;
use crate::extensions::state::ExtensionState;
use crate::extensions::{dnr, extension_dir, manifest};
use crate::native_messaging::{self, FromHost, NativeMessaging};
use crate::page_scripts::{self, BLOCKER_OWNER, Bridge};
use crate::script_storage::{Message, ScriptStorage, new_token, valid_owner};

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

    fn update_tab(&mut self, tab: TabId, url: Option<&str>, active: bool) {
        if let Some(address) = url.and_then(|u| WebAddress::parse(u).ok()) {
            let _ = self.browser.engine_mut().navigate(tab, &address);
        }
        if active {
            self.browser.activate(tab);
        }
    }

    fn remove_tab(&mut self, tab: TabId) {
        self.browser.close_tab(tab);
    }

    fn reload_tab(&mut self, tab: TabId) {
        self.browser.engine_mut().reload(tab);
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

fn tab_json(t: &TabState, index: usize, window: u64) -> Value {
    json!({
        "id": t.id.get(),
        "index": index,
        "windowId": window,
        "active": t.active,
        "highlighted": t.active,
        "selected": t.active,
        "pinned": false,
        "audible": false,
        "discarded": false,
        "autoDiscardable": true,
        "incognito": false,
        "url": t.url,
        "pendingUrl": t.url,
        "title": t.title,
        "status": if t.loading { "loading" } else { "complete" },
        "favIconUrl": "",
        "groupId": -1,
    })
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

    /// `page` went away: its ports close, the other ends are told.
    fn close_ports_of(&mut self, page: &PageSource, host: &mut dyn HubHost) {
        // The messages only it could answer: answered "nothing", as in
        // Chrome when the receiving page goes away.
        let orphans: Vec<u64> = self
            .pending
            .iter_mut()
            .filter_map(|(rid, p)| {
                let before = p.receivers.len();
                p.receivers.retain(|r| r != page);
                (before > 0 && p.receivers.is_empty()).then_some(*rid)
            })
            .collect();
        for rid in orphans {
            if let Some(p) = self.pending.remove(&rid) {
                Self::reply(host, &p.to, p.id, &p.ch, Ok(Value::Null));
            }
        }
        let mut tell = Vec::new();
        self.ports.retain(|id, port| {
            if &port.opener == page {
                for r in &port.receivers {
                    tell.push((r.clone(), port.owner.clone(), *id));
                }
                return false;
            }
            let before = port.receivers.len();
            port.receivers.retain(|r| r != page);
            if port.receivers.is_empty() && before > 0 {
                tell.push((port.opener.clone(), port.owner.clone(), *id));
                return false;
            }
            true
        });
        for (to, owner, id) in tell {
            let event = json!({"event": "portDisconnect", "owner": owner, "port": id});
            host.deliver(&to, &event.to_string());
        }
    }

    /// Answers the messages nobody will: not taken on by a listener within
    /// [`UNTAKEN`], or waiting for a background page that is not starting.
    fn expire(&mut self, now: Instant, host: &mut dyn HubHost) {
        let lost: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, p)| !p.taken && now.duration_since(p.since) >= UNTAKEN)
            .map(|(rid, _)| *rid)
            .collect();
        for rid in lost {
            if let Some(p) = self.pending.remove(&rid) {
                Self::reply(host, &p.to, p.id, &p.ch, Ok(Value::Null));
            }
        }
        for queue in self.queued.values_mut() {
            let (old, keep): (Vec<_>, Vec<_>) = std::mem::take(queue)
                .into_iter()
                .partition(|q| now.duration_since(q.since) >= NEVER_READY);
            *queue = keep;
            for q in old {
                Self::reply(host, &q.from, q.id, &q.ch, Err(NO_RECEIVER.into()));
            }
        }
        self.queued.retain(|_, q| !q.is_empty());
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
        while let Ok((to, id, ch, result)) = self.fetched_rx.try_recv() {
            Self::reply(host, &to, id, &ch, result);
        }
        for (port, event) in self.native.poll() {
            let Some((to, owner)) = self.native_ports.get(&port).cloned() else {
                continue;
            };
            let event = match event {
                FromHost::Message(msg) => {
                    json!({"event": "native", "owner": owner, "port": port, "msg": msg})
                }
                FromHost::Closed(error) => {
                    self.native_ports.remove(&port);
                    json!({"event": "nativeDisconnect", "owner": owner, "port": port, "error": error})
                }
            };
            host.deliver(&to, &event.to_string());
        }
    }

    /// Closes the native ports of the pages in `from`.
    fn close_native_ports(&mut self, from: impl Fn(&PageSource, &str) -> bool) {
        let ports: Vec<u64> = self
            .native_ports
            .iter()
            .filter(|(_, (page, owner))| from(page, owner))
            .map(|(port, _)| *port)
            .collect();
        for port in ports {
            self.native_ports.remove(&port);
            self.native.disconnect(port);
        }
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
            let nonce = raw.get("nonce").and_then(Value::as_str).unwrap_or("");
            if let Some(p) = self.scripts.remove(nonce) {
                let value = raw.remove("value").unwrap_or(Value::Null);
                Self::reply(
                    host,
                    &p.to,
                    p.id,
                    &p.ch,
                    Ok(json!([{"frameId": 0, "result": value}])),
                );
            }
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
            let result = match message.op.as_str() {
                "blocker.cosmetic" => {
                    let url = raw.get("url").and_then(Value::as_str).unwrap_or("");
                    Ok(Value::String(
                        self.blocker
                            .as_ref()
                            .map(|b| b.cosmetic_css(url))
                            .unwrap_or_default(),
                    ))
                }
                op => Err(format!("unknown operation {op}")),
            };
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
            let result = self.storage.apply(message).map(|(result, changes)| {
                if let Some((changes, area)) = changes {
                    let event = json!({"event": "storage", "owner": owner, "changes": changes, "area": area});
                    self.broadcast(&owner, event, host);
                }
                result
            });
            Self::reply(host, from, id, &ch, result);
            return;
        }
        let op = message.op.clone();
        let result = self.call(from, &ctx, &owner, &op, id, &ch, &raw, host);
        if let Some(result) = result {
            Self::reply(host, from, id, &ch, result);
        }
    }

    /// `None`: the answer comes later.
    #[allow(clippy::too_many_arguments)]
    fn call(
        &mut self,
        from: &PageSource,
        ctx: &str,
        owner: &str,
        op: &str,
        id: u64,
        ch: &Option<String>,
        raw: &Map<String, Value>,
        host: &mut dyn HubHost,
    ) -> Option<Result<Value, String>> {
        let ext = owner.strip_prefix("ext:").and_then(ExtensionId::parse);
        let field = |k: &str| raw.get(k).cloned().unwrap_or(Value::Null);
        match op {
            "hello" => {
                match (from, ctx) {
                    (PageSource::Tab(tab), "content") => {
                        self.tab_owners
                            .entry(*tab)
                            .or_default()
                            .insert(owner.to_owned());
                    }
                    (PageSource::Background(_), _) => {
                        // A background page (re)loading: not listening yet,
                        // whatever its previous load said. Messages wait
                        // for its "ready" rather than reaching no listener.
                        self.backgrounds.insert(owner.to_owned());
                        self.ready.remove(owner);
                    }
                    (PageSource::Panel, _) => self.panel = Some(owner.to_owned()),
                    (PageSource::Tab(tab), "page") => {
                        self.page_tabs.insert(*tab, owner.to_owned());
                    }
                    _ => {}
                }
                Some(Ok(Value::Null))
            }
            "net.fetch" => {
                // As in Chrome: the extension's own pages, to the hosts its
                // permissions cover. Anything else: the page's own fetch.
                let ext = ext?;
                let url = field("url").as_str().unwrap_or("").to_owned();
                let fetcher = self.fetcher.clone().filter(|_| {
                    ctx != "content"
                        && extension_fetch::allowed(
                            &extension_dir(&self.extensions_dir, &ext),
                            &url,
                        )
                });
                let Some(fetcher) = fetcher else {
                    return Some(Err("not permitted".into()));
                };
                let request = FetchRequest {
                    url,
                    method: field("method")
                        .as_str()
                        .unwrap_or("GET")
                        .to_ascii_uppercase(),
                    headers: field("headers")
                        .as_array()
                        .map(|pairs| {
                            pairs
                                .iter()
                                .filter_map(|p| {
                                    Some((
                                        p.get(0)?.as_str()?.to_owned(),
                                        p.get(1)?.as_str()?.to_owned(),
                                    ))
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    body: field("body")
                        .as_str()
                        .and_then(|b| data_encoding::BASE64.decode(b.as_bytes()).ok()),
                };
                let (tx, wake, to, ch) = (
                    self.fetched_tx.clone(),
                    self.wake.clone(),
                    from.clone(),
                    ch.clone(),
                );
                std::thread::spawn(move || {
                    let result = fetcher(request).map(|r| {
                        json!({
                            "url": r.url,
                            "status": r.status,
                            "statusText": r.status_text,
                            "headers": r.headers,
                            "body": data_encoding::BASE64.encode(&r.body),
                        })
                    });
                    if tx.send((to, id, ch, result)).is_ok() {
                        wake();
                    }
                });
                None
            }
            "port.connect" => {
                let ext = ext?;
                // tabs.connect: the content scripts of a tab. runtime.connect:
                // the extension's pages, the caller excepted.
                let receivers: Vec<PageSource> = match tab_id(&field("tabId")) {
                    Some(tab) => self
                        .tab_owners
                        .get(&tab)
                        .filter(|o| o.contains(owner))
                        .map(|_| vec![PageSource::Tab(tab)])
                        .unwrap_or_default(),
                    None => {
                        let mut pages = Vec::new();
                        if self.backgrounds.contains(owner) {
                            pages.push(PageSource::Background(ext.clone()));
                        }
                        if self.panel.as_deref() == Some(owner) {
                            pages.push(PageSource::Panel);
                        }
                        pages.extend(self.page_tabs_of(owner));
                        pages
                    }
                };
                let receivers: Vec<PageSource> =
                    receivers.into_iter().filter(|r| r != from).collect();
                if receivers.is_empty() {
                    return Some(Err(
                        "Could not establish connection. Receiving end does not exist.".into(),
                    ));
                }
                let id = self.next_port;
                self.next_port += 1;
                let sender = self.sender(from, &ext, host);
                let event = json!({"event": "connect", "owner": owner, "port": id,
                    "name": field("name"), "sender": sender})
                .to_string();
                for r in &receivers {
                    host.deliver(r, &event);
                }
                self.ports.insert(
                    id,
                    Port {
                        owner: owner.to_owned(),
                        opener: from.clone(),
                        receivers,
                    },
                );
                Some(Ok(Value::from(id)))
            }
            "port.post" => {
                let id = field("port").as_u64()?;
                let Some(port) = self.ports.get(&id).filter(|p| p.owner == owner) else {
                    return Some(Err("Attempting to use a disconnected port object".into()));
                };
                let to: Vec<PageSource> = if &port.opener == from {
                    port.receivers.clone()
                } else if port.receivers.contains(from) {
                    vec![port.opener.clone()]
                } else {
                    return Some(Err("Attempting to use a disconnected port object".into()));
                };
                let event =
                    json!({"event": "port", "owner": owner, "port": id, "msg": field("msg")})
                        .to_string();
                for dest in &to {
                    host.deliver(dest, &event);
                }
                Some(Ok(Value::Null))
            }
            "port.disconnect" => {
                let id = field("port").as_u64()?;
                let mut tell = Vec::new();
                if let Some(port) = self.ports.get_mut(&id).filter(|p| p.owner == owner) {
                    if &port.opener == from {
                        tell.append(&mut port.receivers);
                    } else {
                        port.receivers.retain(|r| r != from);
                        if port.receivers.is_empty() {
                            tell.push(port.opener.clone());
                        }
                    }
                    if port.receivers.is_empty() {
                        self.ports.remove(&id);
                    }
                }
                let event =
                    json!({"event": "portDisconnect", "owner": owner, "port": id}).to_string();
                for dest in &tell {
                    host.deliver(dest, &event);
                }
                Some(Ok(Value::Null))
            }
            "native.connect" => {
                let ext = ext?;
                // As in Chrome: extension pages only, and only with the
                // permission.
                if ctx == "content"
                    || !manifest::has_permission(
                        &extension_dir(&self.extensions_dir, &ext),
                        "nativeMessaging",
                    )
                {
                    return Some(Err(native_messaging::NOT_FOUND.into()));
                }
                let name = field("app");
                let result = self
                    .native
                    .connect(name.as_str().unwrap_or(""), &ext)
                    .map(|port| {
                        self.native_ports
                            .insert(port, (from.clone(), owner.to_owned()));
                        Value::from(port)
                    });
                Some(result)
            }
            "native.post" => {
                let port = field("port").as_u64()?;
                if self.native_ports.get(&port).map(|(_, o)| o.as_str()) != Some(owner) {
                    return Some(Err("Attempting to use a disconnected port object".into()));
                }
                Some(self.native.post(port, &field("msg")).map(|()| Value::Null))
            }
            "native.disconnect" => {
                let port = field("port").as_u64()?;
                if self.native_ports.get(&port).map(|(_, o)| o.as_str()) == Some(owner) {
                    self.native_ports.remove(&port);
                    self.native.disconnect(port);
                }
                Some(Ok(Value::Null))
            }
            "blur" => {
                if from == &PageSource::Panel {
                    self.panel_blurred = true;
                }
                Some(Ok(Value::Null))
            }
            "ready" => {
                if let (PageSource::Background(_), Some(ext)) = (from, ext.as_ref()) {
                    self.ready.insert(owner.to_owned());
                    for q in self.queued.remove(owner).unwrap_or_default() {
                        let to = self.message_targets(&q.from, owner, ext);
                        if let Some(result) = self
                            .send_event(&q.from, q.id, &q.ch, owner, &to, q.payload, q.sender, host)
                        {
                            Self::reply(host, &q.from, q.id, &q.ch, result);
                        }
                    }
                }
                Some(Ok(Value::Null))
            }
            "sendMessage" => {
                let ext = ext?;
                if !matches!(from, PageSource::Background(_))
                    && !self.ready.contains(owner)
                    && self.expects_background(owner, &ext)
                {
                    let sender = self.sender(from, &ext, host);
                    let queue = self.queued.entry(owner.to_owned()).or_default();
                    if queue.len() >= MAX_QUEUED {
                        let old = queue.remove(0);
                        Self::reply(
                            host,
                            &old.from,
                            old.id,
                            &old.ch,
                            Err(
                                "Could not establish connection. Receiving end does not exist."
                                    .into(),
                            ),
                        );
                    }
                    queue.push(Queued {
                        from: from.clone(),
                        id,
                        ch: ch.clone(),
                        payload: field("payload"),
                        sender,
                        since: Instant::now(),
                    });
                    return None;
                }
                let to = self.message_targets(from, owner, &ext);
                let sender = self.sender(from, &ext, host);
                self.send_event(from, id, ch, owner, &to, field("payload"), sender, host)
            }
            "tabs.sendMessage" => {
                let ext = ext?;
                let tab = tab_id(&field("tabId"));
                let Some(tab) =
                    tab.filter(|t| self.tab_owners.get(t).is_some_and(|o| o.contains(owner)))
                else {
                    return Some(Err(
                        "Could not establish connection. Receiving end does not exist.".into(),
                    ));
                };
                let sender = json!({"id": ext.to_string(), "origin": ""});
                self.send_event(
                    from,
                    id,
                    ch,
                    owner,
                    &[PageSource::Tab(tab)],
                    field("payload"),
                    sender,
                    host,
                )
            }
            "ack" => {
                let rid = field("rid").as_u64()?;
                if let Some(p) = self.pending.get_mut(&rid) {
                    p.taken = true;
                }
                None
            }
            "respond" => {
                let rid = field("rid").as_u64()?;
                let none = field("none").as_bool().unwrap_or(false);
                let p = self.pending.get_mut(&rid)?;
                if none {
                    p.waiting = p.waiting.saturating_sub(1);
                    if p.waiting > 0 {
                        return None;
                    }
                }
                let p = self.pending.remove(&rid)?;
                let payload = if none { Value::Null } else { field("payload") };
                Self::reply(host, &p.to, p.id, &p.ch, Ok(payload));
                None
            }
            "tabs.query" => Some(Ok(self.query_tabs(&field("query"), host))),
            "tabs.get" => {
                let tab = tab_id(&field("tabId"));
                let tabs = host.tabs();
                Some(
                    tabs.iter()
                        .enumerate()
                        .find(|(_, t)| Some(t.id) == tab)
                        .map(|(i, t)| self.tab_json(t, i))
                        .ok_or_else(|| "No tab with this id.".to_owned()),
                )
            }
            op if op.starts_with("windows.") => Some(self.windows_call(from, &ext, op, raw, host)),
            "tabs.create" => {
                let props = field("props");
                let url = props
                    .get("url")
                    .and_then(Value::as_str)
                    .map(|u| self.resolve(&ext, u));
                let active = props.get("active").and_then(Value::as_bool).unwrap_or(true);
                let tab = host.create_tab(url.as_deref(), active);
                Some(Ok(self.describe(tab, host)))
            }
            "tabs.update" => {
                let props = field("props");
                let tab = tab_id(&field("tabId")).or_else(|| self.active_tab(host))?;
                let url = props
                    .get("url")
                    .and_then(Value::as_str)
                    .map(|u| self.resolve(&ext, u));
                let active = props
                    .get("active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                host.update_tab(tab, url.as_deref(), active);
                Some(Ok(self.describe(Some(tab), host)))
            }
            "tabs.remove" => {
                for t in field("tabIds")
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(tab_id)
                {
                    host.remove_tab(t);
                }
                Some(Ok(Value::Null))
            }
            "tabs.reload" => {
                if let Some(tab) = tab_id(&field("tabId")).or_else(|| self.active_tab(host)) {
                    host.reload_tab(tab);
                }
                Some(Ok(Value::Null))
            }
            "scripting.executeScript" => {
                let ext = ext?;
                let tab = tab_id(&field("tabId")).or_else(|| self.active_tab(host))?;
                // The files, or the function, run with the extension's
                // `chrome.*` (Bitwarden's autofill is injected so).
                let body = match field("code").as_str() {
                    Some(code) => format!("return ({code});"),
                    None => {
                        let mut body = String::new();
                        for f in field("files")
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                        {
                            match self.extension_file(&ext, f) {
                                Some(text) => {
                                    body.push_str(&format!("(function () {{\n{text}\n}})();\n"))
                                }
                                None => return Some(Err(format!("Could not load file: '{f}'."))),
                            }
                        }
                        body
                    }
                };
                let code = page_scripts::injected_script(
                    &self.token,
                    self.bridge,
                    &self.injected_config(&ext),
                    &body,
                );
                let nonce = new_token();
                self.scripts.insert(
                    nonce.clone(),
                    Pending {
                        to: from.clone(),
                        id,
                        ch: ch.clone(),
                        waiting: 1,
                        receivers: Vec::new(),
                        since: Instant::now(),
                        taken: true,
                    },
                );
                host.run_in_tab(tab, &self.result_script(&code, &nonce));
                None
            }
            "scripting.insertCSS" => {
                let ext = ext?;
                let tab = tab_id(&field("tabId")).or_else(|| self.active_tab(host))?;
                let mut css = field("css").as_str().unwrap_or("").to_owned();
                for f in field("files")
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    css.push_str(&self.extension_file(&ext, f).unwrap_or_default());
                }
                let script = format!(
                    "(() => {{ const s = document.createElement('style'); s.textContent = {}; \
                     (document.head || document.documentElement).appendChild(s); }})()",
                    json!(css)
                );
                host.run_in_tab(tab, &script);
                Some(Ok(Value::Null))
            }
            "scripting.registerContentScripts"
            | "scripting.updateContentScripts"
            | "scripting.unregisterContentScripts"
            | "scripting.getRegisteredContentScripts" => {
                let ext = ext?;
                Some(self.content_scripts(&ext, op, raw, host))
            }
            "dnr.getEnabledRulesets"
            | "dnr.updateEnabledRulesets"
            | "dnr.getRules"
            | "dnr.updateRules" => {
                let ext = ext?;
                Some(self.dnr(&ext, op, raw, host))
            }
            other => Some(Err(format!("{other} is not supported"))),
        }
    }

    /// Sends `payload` to `to` as a `runtime.onMessage` event; the first
    /// answer goes back to the caller (`null` when nobody answers).
    #[allow(clippy::too_many_arguments)]
    fn send_event(
        &mut self,
        from: &PageSource,
        id: u64,
        ch: &Option<String>,
        owner: &str,
        to: &[PageSource],
        payload: Value,
        sender: Value,
        host: &mut dyn HubHost,
    ) -> Option<Result<Value, String>> {
        if to.is_empty() {
            return Some(Ok(Value::Null));
        }
        let rid = self.next_rid;
        self.next_rid += 1;
        self.pending.insert(
            rid,
            Pending {
                to: from.clone(),
                id,
                ch: ch.clone(),
                waiting: to.len(),
                receivers: to.to_vec(),
                since: Instant::now(),
                taken: false,
            },
        );
        let event = json!({"event": "message", "owner": owner, "rid": rid, "payload": payload, "sender": sender})
            .to_string();
        for dest in to {
            host.deliver(dest, &event);
        }
        None
    }

    /// `sender` of `runtime.onMessage`: the tab as it is now.
    fn sender(&self, from: &PageSource, ext: &ExtensionId, host: &dyn HubHost) -> Value {
        let mut sender = json!({"id": ext.to_string()});
        // The extension's own pages: their origin, as Chrome tells it
        // (extensions check it to trust the message).
        if matches!(from, PageSource::Background(_) | PageSource::Panel) {
            let origin = self.origins().origin(ext);
            sender["url"] = format!("{origin}/").into();
            sender["origin"] = origin.into();
        }
        let tabs = host.tabs();
        if let PageSource::Tab(tab) = from
            && let Some((i, t)) = tabs.iter().enumerate().find(|(_, t)| t.id == *tab)
        {
            sender["tab"] = self.tab_json(t, i);
            sender["url"] = t.url.clone().into();
            sender["frameId"] = 0.into();
            sender["origin"] = t
                .url
                .split('/')
                .take(3)
                .collect::<Vec<_>>()
                .join("/")
                .into();
        }
        sender
    }

    fn tab_json(&self, t: &TabState, index: usize) -> Value {
        tab_json(t, index, self.window_of(t.id))
    }

    fn window_of(&self, tab: TabId) -> u64 {
        self.popups.get(&tab).map_or(MAIN_WINDOW, |(w, _)| *w)
    }

    /// The window `id` as `windows.*` describes it (`None`: no such window).
    fn window_json(&self, id: u64, populate: bool, host: &dyn HubHost) -> Option<Value> {
        let kind = if id == MAIN_WINDOW {
            "normal".to_owned()
        } else {
            self.popups.values().find(|(w, _)| *w == id)?.1.clone()
        };
        let all = host.tabs();
        let mut window = json!({
            "id": id,
            "focused": all.iter().any(|t| t.active && self.window_of(t.id) == id),
            "type": kind,
            "state": "normal",
            "incognito": false,
            "alwaysOnTop": false,
            "left": 0,
            "top": 0,
            "width": 1280,
            "height": 800,
        });
        if populate {
            window["tabs"] = all
                .iter()
                .enumerate()
                .filter(|(_, t)| self.window_of(t.id) == id)
                .map(|(i, t)| self.tab_json(t, i))
                .collect();
        }
        Some(window)
    }

    /// The window of the page asking (`windows.getCurrent`).
    fn current_window(&self, from: &PageSource) -> u64 {
        match from {
            PageSource::Tab(tab) => self.window_of(*tab),
            _ => MAIN_WINDOW,
        }
    }

    /// `windows.*`: the tabs `windows.create` opened are windows of their own,
    /// gone (`windows.onRemoved`) when the tab closes.
    fn windows_call(
        &mut self,
        from: &PageSource,
        ext: &Option<ExtensionId>,
        op: &str,
        raw: &Map<String, Value>,
        host: &mut dyn HubHost,
    ) -> Result<Value, String> {
        let field = |k: &str| raw.get(k).cloned().unwrap_or(Value::Null);
        let populate = field("info")
            .get("populate")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // WINDOW_ID_CURRENT (-2) is no u64: the asking page's window.
        let id = field("windowId")
            .as_u64()
            .unwrap_or_else(|| self.current_window(from));
        let missing = |id: u64| format!("No window with id: {id}.");
        match op {
            "windows.get" => self
                .window_json(id, populate, host)
                .ok_or_else(|| missing(id)),
            "windows.getCurrent" => {
                let id = self.current_window(from);
                self.window_json(id, populate, host)
                    .ok_or_else(|| missing(id))
            }
            "windows.getLastFocused" => {
                let id = self
                    .active_tab(host)
                    .map_or(MAIN_WINDOW, |t| self.window_of(t));
                self.window_json(id, populate, host)
                    .ok_or_else(|| missing(id))
            }
            "windows.getAll" => {
                let mut ids = vec![MAIN_WINDOW];
                ids.extend(self.popups.values().map(|(w, _)| *w));
                ids.sort_unstable();
                Ok(ids
                    .into_iter()
                    .filter_map(|id| self.window_json(id, populate, host))
                    .collect())
            }
            "windows.create" => {
                let data = field("data");
                let url = match data.get("url") {
                    Some(Value::String(u)) => Some(u.as_str()),
                    Some(Value::Array(list)) => list.first().and_then(Value::as_str),
                    _ => None,
                }
                .map(|u| self.resolve(ext, u));
                let focused = data.get("focused").and_then(Value::as_bool).unwrap_or(true);
                let kind = data
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("normal")
                    .to_owned();
                let tab = host
                    .create_tab(url.as_deref(), focused)
                    .ok_or("The window could not be created.")?;
                let id = self.next_window;
                self.next_window += 1;
                self.popups.insert(tab, (id, kind));
                self.window_json(id, true, host).ok_or_else(|| missing(id))
            }
            "windows.update" => {
                let focused = field("info").get("focused").and_then(Value::as_bool);
                if focused == Some(true) {
                    let tab = self
                        .popups
                        .iter()
                        .find(|(_, (w, _))| *w == id)
                        .map(|(t, _)| *t);
                    if let Some(tab) = tab {
                        host.update_tab(tab, None, true);
                    }
                }
                self.window_json(id, false, host).ok_or_else(|| missing(id))
            }
            "windows.remove" => {
                // The browser's own window stays: an extension does not close it.
                if id == MAIN_WINDOW {
                    return Ok(Value::Null);
                }
                let tabs: Vec<TabId> = self
                    .popups
                    .iter()
                    .filter(|(_, (w, _))| *w == id)
                    .map(|(t, _)| *t)
                    .collect();
                if tabs.is_empty() {
                    return Err(missing(id));
                }
                for tab in tabs {
                    host.remove_tab(tab);
                }
                Ok(Value::Null)
            }
            op => Err(format!("unknown operation {op}")),
        }
    }

    /// Whether `owner` has a background page (the manifest says, once).
    fn expects_background(&mut self, owner: &str, ext: &ExtensionId) -> bool {
        let dir = &self.extensions_dir;
        *self
            .has_background
            .entry(owner.to_owned())
            .or_insert_with(|| manifest::background(&extension_dir(dir, ext)).is_some())
    }

    /// Where a `runtime.sendMessage` from `from` goes: the background page,
    /// the popup and the extension's pages in tabs, but the sender.
    fn message_targets(
        &self,
        from: &PageSource,
        owner: &str,
        ext: &ExtensionId,
    ) -> Vec<PageSource> {
        let mut to = Vec::new();
        if !matches!(from, PageSource::Background(_)) && self.backgrounds.contains(owner) {
            to.push(PageSource::Background(ext.clone()));
        }
        if from != &PageSource::Panel && self.panel.as_deref() == Some(owner) {
            to.push(PageSource::Panel);
        }
        to.extend(self.page_tabs_of(owner).into_iter().filter(|t| t != from));
        to
    }

    fn active_tab(&self, host: &dyn HubHost) -> Option<TabId> {
        host.tabs().into_iter().find(|t| t.active).map(|t| t.id)
    }

    fn describe(&self, tab: Option<TabId>, host: &dyn HubHost) -> Value {
        host.tabs()
            .iter()
            .enumerate()
            .find(|(_, t)| Some(t.id) == tab)
            .map(|(i, t)| self.tab_json(t, i))
            .unwrap_or(Value::Null)
    }

    /// A relative URL of an extension is a file of the extension.
    fn resolve(&self, ext: &Option<ExtensionId>, url: &str) -> String {
        match ext {
            Some(id)
                if !url.contains("://")
                    && !url.starts_with("about:")
                    && !url.starts_with("data:") =>
            {
                match self.bridge {
                    Bridge::Ipc => crate::extension_server::Origins::desktop(),
                    Bridge::Android => crate::extension_server::Origins::Android,
                }
                .url(id, url)
            }
            _ => url.to_owned(),
        }
    }

    fn query_tabs(&self, query: &Value, host: &dyn HubHost) -> Value {
        let urls: Vec<UrlPattern> = match query.get("url") {
            Some(Value::String(u)) => UrlPattern::parse_match(u).into_iter().collect(),
            Some(Value::Array(list)) => list
                .iter()
                .filter_map(Value::as_str)
                .filter_map(UrlPattern::parse_match)
                .collect(),
            _ => Vec::new(),
        };
        let wants = |key: &str| query.get(key).and_then(Value::as_bool);
        let status = query.get("status").and_then(Value::as_str);
        Value::Array(
            host.tabs()
                .iter()
                .enumerate()
                .filter(|(_, t)| wants("active").is_none_or(|a| a == t.active))
                .filter(|(_, t)| wants("highlighted").is_none_or(|a| a == t.active))
                .filter(|(_, t)| {
                    urls.is_empty() || urls.iter().any(|p| p.matches(&t.url) == Some(true))
                })
                .filter(|(_, t)| {
                    status.is_none_or(|s| s == if t.loading { "loading" } else { "complete" })
                })
                // No tab is pinned.
                .filter(|_| query.get("pinned").and_then(Value::as_bool) != Some(true))
                .filter(|(_, t)| {
                    query
                        .get("windowId")
                        .and_then(Value::as_u64)
                        .is_none_or(|w| w == self.window_of(t.id))
                })
                .filter(|(_, t)| {
                    query
                        .get("windowType")
                        .and_then(Value::as_str)
                        .is_none_or(|k| {
                            k == self
                                .popups
                                .get(&t.id)
                                .map_or("normal", |(_, kind)| kind.as_str())
                        })
                })
                .map(|(i, t)| self.tab_json(t, i))
                .collect(),
        )
    }

    /// What [`page_scripts::injected_script`] needs to know of `ext`.
    fn injected_config(&self, ext: &ExtensionId) -> String {
        let dir = extension_dir(&self.extensions_dir, ext);
        let (name, version) = manifest::read(&dir)
            .map(|m| (m.name, m.version))
            .unwrap_or_default();
        json!({
            "owner": format!("ext:{ext}"),
            "origin": self.origins().origin(ext),
            "messages": manifest::messages(&dir),
            "manifest": {"name": name, "version": version},
        })
        .to_string()
    }

    fn extension_file(&self, ext: &ExtensionId, path: &str) -> Option<String> {
        let relative = path.trim_start_matches('/');
        if relative.split(['/', '\\']).any(|p| p == "..") {
            return None;
        }
        fs::read_to_string(extension_dir(&self.extensions_dir, ext).join(relative)).ok()
    }

    /// `code` (an expression) run in a page, its result sent back.
    fn result_script(&self, code: &str, nonce: &str) -> String {
        let post = match self.bridge {
            Bridge::Ipc => "window.ipc && window.ipc.postMessage(m)",
            Bridge::Android => "window.BUBridge && window.BUBridge.call(m)",
        };
        format!(
            "(async () => {{ let r = null; try {{ r = await ({code}); }} catch (e) {{ console.error(e); }} \
             let m; try {{ m = JSON.stringify({{op: 'scriptResult', nonce: {}, value: r === undefined ? null : r}}); }} \
             catch (e) {{ m = JSON.stringify({{op: 'scriptResult', nonce: {}, value: null}}); }} {post}; }})()",
            json!(nonce),
            json!(nonce)
        )
    }

    fn content_scripts(
        &mut self,
        ext: &ExtensionId,
        op: &str,
        raw: &Map<String, Value>,
        host: &mut dyn HubHost,
    ) -> Result<Value, String> {
        let mut state = ExtensionState::load(&self.state_dir, ext);
        let id_of = |s: &Value| s.get("id").and_then(Value::as_str).map(str::to_owned);
        let ids: Option<Vec<String>> = raw.get("ids").and_then(Value::as_array).map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        });
        let given: Vec<Value> = raw
            .get("scripts")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        match op {
            "scripting.getRegisteredContentScripts" => {
                return Ok(Value::Array(
                    state
                        .content_scripts
                        .into_iter()
                        .filter(|s| {
                            ids.as_ref()
                                .is_none_or(|ids| id_of(s).is_some_and(|i| ids.contains(&i)))
                        })
                        .collect(),
                ));
            }
            "scripting.registerContentScripts" => {
                for s in given {
                    let sid = id_of(&s).ok_or("a content script needs an id")?;
                    if state
                        .content_scripts
                        .iter()
                        .any(|e| id_of(e).as_ref() == Some(&sid))
                    {
                        return Err(format!("Duplicate script ID '{sid}'"));
                    }
                    state.content_scripts.push(s);
                }
            }
            "scripting.updateContentScripts" => {
                for s in given {
                    let sid = id_of(&s).ok_or("a content script needs an id")?;
                    let existing = state
                        .content_scripts
                        .iter_mut()
                        .find(|e| id_of(e).as_ref() == Some(&sid))
                        .ok_or_else(|| format!("No script with ID '{sid}'"))?;
                    if let (Value::Object(e), Value::Object(u)) = (existing, s) {
                        e.extend(u);
                    }
                }
            }
            _ => {
                state.content_scripts.retain(|s| match &ids {
                    None => false,
                    Some(ids) => !id_of(s).is_some_and(|i| ids.contains(&i)),
                });
            }
        }
        state.save(&self.state_dir, ext)?;
        host.extension_changed(ext);
        Ok(Value::Null)
    }

    fn dnr(
        &mut self,
        ext: &ExtensionId,
        op: &str,
        raw: &Map<String, Value>,
        host: &mut dyn HubHost,
    ) -> Result<Value, String> {
        let dir = extension_dir(&self.extensions_dir, ext);
        let mut state = ExtensionState::load(&self.state_dir, ext);
        let enabled = state
            .enabled_rulesets
            .clone()
            .unwrap_or_else(|| dnr::default_rulesets(&dir));
        let session = raw.get("kind").and_then(Value::as_str) == Some("session");
        match op {
            "dnr.getEnabledRulesets" => return Ok(json!(enabled)),
            "dnr.getRules" => {
                let list = if session {
                    &state.session_rules
                } else {
                    &state.dynamic_rules
                };
                let ids: Option<Vec<u64>> = raw
                    .get("ids")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_u64).collect());
                return Ok(Value::Array(
                    list.iter()
                        .filter(|r| {
                            ids.as_ref().is_none_or(|ids| {
                                r.get("id")
                                    .and_then(Value::as_u64)
                                    .is_some_and(|i| ids.contains(&i))
                            })
                        })
                        .cloned()
                        .collect(),
                ));
            }
            "dnr.updateEnabledRulesets" => {
                let options = raw.get("options").cloned().unwrap_or(Value::Null);
                let list = |k: &str| -> Vec<String> {
                    options
                        .get(k)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let all = dnr::all_rulesets(&dir);
                let mut now = enabled;
                now.retain(|r| !list("disableRulesetIds").contains(r));
                for r in list("enableRulesetIds") {
                    if !all.contains(&r) {
                        return Err(format!("Invalid ruleset id: {r}."));
                    }
                    if !now.contains(&r) {
                        now.push(r);
                    }
                }
                state.enabled_rulesets = Some(now);
            }
            _ => {
                let options = raw.get("options").cloned().unwrap_or(Value::Null);
                let remove: Vec<u64> = options
                    .get("removeRuleIds")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_u64).collect())
                    .unwrap_or_default();
                let add = options
                    .get("addRules")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let list = if session {
                    &mut state.session_rules
                } else {
                    &mut state.dynamic_rules
                };
                list.retain(|r| {
                    !r.get("id")
                        .and_then(Value::as_u64)
                        .is_some_and(|i| remove.contains(&i))
                });
                for r in add {
                    let rid = r
                        .get("id")
                        .and_then(Value::as_u64)
                        .ok_or("a rule needs an id")?;
                    if list
                        .iter()
                        .any(|e| e.get("id").and_then(Value::as_u64) == Some(rid))
                    {
                        return Err(format!("Rule with id {rid} does not have a unique ID."));
                    }
                    list.push(r);
                }
            }
        }
        state.save(&self.state_dir, ext)?;
        host.extension_changed(ext);
        Ok(Value::Null)
    }

    /// Compares the tabs with the last time and sends `tabs.on*` events to
    /// the background pages. Call once per frame (cheap when nothing moved).
    pub fn tabs_changed(&mut self, host: &mut dyn HubHost) {
        self.expire(Instant::now(), host);
        for page in std::mem::take(&mut self.closed_pages) {
            self.close_ports_of(&page, host);
        }
        let now = host.tabs();
        if now == self.last_tabs {
            return;
        }
        let before = std::mem::replace(&mut self.last_tabs, now.clone());
        let mut events: Vec<(&str, Value)> = Vec::new();
        for (i, t) in now.iter().enumerate() {
            match before.iter().find(|b| b.id == t.id) {
                None => events.push(("onCreated", json!([self.tab_json(t, i)]))),
                Some(b) => {
                    let mut change = Map::new();
                    if b.loading != t.loading {
                        change.insert(
                            "status".into(),
                            (if t.loading { "loading" } else { "complete" }).into(),
                        );
                    }
                    if b.url != t.url {
                        change.insert("url".into(), t.url.clone().into());
                    }
                    if b.title != t.title {
                        change.insert("title".into(), t.title.clone().into());
                    }
                    if !change.is_empty() {
                        events.push((
                            "onUpdated",
                            json!([t.id.get(), change, self.tab_json(t, i)]),
                        ));
                    }
                    if t.loading && !b.loading {
                        // A new page: its content scripts say hello again.
                        self.tab_owners.remove(&t.id);
                        self.page_tabs.remove(&t.id);
                        self.closed_pages.push(PageSource::Tab(t.id));
                    }
                    if t.active && !b.active {
                        events.push((
                            "onActivated",
                            json!([{"tabId": t.id.get(), "windowId": self.window_of(t.id)}]),
                        ));
                    }
                }
            }
        }
        let mut windows_removed = Vec::new();
        for b in &before {
            if !now.iter().any(|t| t.id == b.id) {
                self.tab_owners.remove(&b.id);
                self.page_tabs.remove(&b.id);
                self.closed_pages.push(PageSource::Tab(b.id));
                let popup = self.popups.remove(&b.id).map(|(w, _)| w);
                events.push((
                    "onRemoved",
                    json!([
                        b.id.get(),
                        {"windowId": popup.unwrap_or(MAIN_WINDOW), "isWindowClosing": popup.is_some()}
                    ]),
                ));
                windows_removed.extend(popup);
            }
        }
        for owner in &self.backgrounds {
            let Some(id) = owner.strip_prefix("ext:").and_then(ExtensionId::parse) else {
                continue;
            };
            for (name, args) in &events {
                let msg = json!({"event": "tabs", "owner": owner, "name": name, "args": args});
                host.deliver(&PageSource::Background(id.clone()), &msg.to_string());
            }
            for window in &windows_removed {
                let msg = json!({"event": "windows", "owner": owner, "name": "onRemoved", "args": [window]});
                host.deliver(&PageSource::Background(id.clone()), &msg.to_string());
            }
        }
    }

    pub fn extensions_dir(&self) -> &Path {
        &self.extensions_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXT: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

    #[derive(Default)]
    struct Host {
        tabs: Vec<TabState>,
        delivered: Vec<(PageSource, Value)>,
        ran: Vec<(TabId, String)>,
        changed: usize,
    }

    impl HubHost for Host {
        fn tabs(&self) -> Vec<TabState> {
            self.tabs.clone()
        }
        fn create_tab(&mut self, url: Option<&str>, active: bool) -> Option<TabId> {
            let id = TabId::new(self.tabs.len() as u64 + 1);
            self.tabs.push(TabState {
                id,
                url: url.unwrap_or("about:blank").into(),
                title: String::new(),
                active,
                loading: true,
            });
            Some(id)
        }
        fn update_tab(&mut self, tab: TabId, url: Option<&str>, _active: bool) {
            if let (Some(t), Some(u)) = (self.tabs.iter_mut().find(|t| t.id == tab), url) {
                t.url = u.into();
            }
        }
        fn remove_tab(&mut self, tab: TabId) {
            self.tabs.retain(|t| t.id != tab);
        }
        fn reload_tab(&mut self, _tab: TabId) {}
        fn deliver(&mut self, to: &PageSource, message: &str) {
            self.delivered
                .push((to.clone(), serde_json::from_str(message).unwrap()));
        }
        fn run_in_tab(&mut self, tab: TabId, script: &str) {
            self.ran.push((tab, script.to_owned()));
        }
        fn extension_changed(&mut self, _id: &ExtensionId) {
            self.changed += 1;
        }
    }

    fn hub(dir: &Path) -> Hub {
        Hub::new(
            ScriptStorage::new(dir.join("storage"), "tok".into()),
            Bridge::Ipc,
            dir.join("ext"),
            dir.join("state"),
        )
    }

    fn msg(id: u64, ctx: &str, op: &str, extra: Value) -> String {
        let mut m = json!({"t": "tok", "ch": "c", "id": id, "owner": format!("ext:{EXT}"), "ctx": ctx, "op": op});
        if let (Value::Object(m), Value::Object(e)) = (&mut m, extra) {
            m.extend(e);
        }
        m.to_string()
    }

    fn ext() -> ExtensionId {
        ExtensionId::parse(EXT).unwrap()
    }

    fn cosmetic(hub: &mut Hub, host: &mut Host, url: &str) -> Value {
        let m = json!({"t": "tok", "ch": "c", "id": 1, "owner": "blocker", "ctx": "content",
            "op": "blocker.cosmetic", "url": url});
        hub.handle(&PageSource::Tab(TabId::new(1)), &m.to_string(), host);
        host.delivered.pop().unwrap().1
    }

    #[test]
    fn the_blocker_script_gets_the_sites_cosmetic_filters() {
        use crate::blocker::{Blocker, BlockerPolicy};

        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        // No blocker yet: nothing to hide.
        assert_eq!(
            cosmetic(&mut hub, &mut host, "https://a.test/")["result"],
            ""
        );

        let blocker = BlockerHandle::default();
        blocker.set(
            Blocker::from_lists(&["a.test##.ad
||ads.test^
"])
            .unwrap(),
        );
        hub.set_blocker(blocker.clone());
        assert_eq!(
            cosmetic(&mut hub, &mut host, "https://a.test/")["result"],
            ".ad{display:none!important}"
        );
        assert_eq!(
            cosmetic(&mut hub, &mut host, "https://b.test/")["result"],
            ""
        );

        blocker.set_policy(BlockerPolicy {
            enabled: true,
            allowlist: vec!["a.test".into()],
        });
        assert_eq!(
            cosmetic(&mut hub, &mut host, "https://a.test/")["result"],
            ""
        );
        blocker.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        assert_eq!(
            cosmetic(&mut hub, &mut host, "https://a.test/")["result"],
            ""
        );
    }

    #[test]
    fn the_blocker_owner_has_no_other_operation() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let m = json!({"t": "tok", "ch": "c", "id": 1, "owner": "blocker", "ctx": "content",
            "op": "set", "items": {"a": 1}});
        hub.handle(&PageSource::Tab(TabId::new(1)), &m.to_string(), &mut host);
        assert!(host.delivered.pop().unwrap().1["error"].is_string());
    }

    #[test]
    fn extension_pages_fetch_their_hosts_without_cors() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let ext_dir = extension_dir(&dir.path().join("ext"), &ext());
        fs::create_dir_all(&ext_dir).unwrap();
        fs::write(
            ext_dir.join("manifest.json"),
            r#"{"host_permissions": ["https://api.test/*"]}"#,
        )
        .unwrap();
        hub.set_fetcher(Arc::new(|r: FetchRequest| {
            Ok(extension_fetch::FetchResponse {
                url: r.url,
                status: 200,
                status_text: "OK".into(),
                headers: vec![("content-type".into(), "text/plain".into())],
                body: format!(
                    "{} {}",
                    r.method,
                    String::from_utf8(r.body.unwrap_or_default()).unwrap()
                )
                .into_bytes(),
            })
        }));
        let mut host = Host::default();
        let bg = PageSource::Background(ext());
        let body = data_encoding::BASE64.encode(b"hi");

        hub.handle(
            &bg,
            &msg(
                1,
                "background",
                "net.fetch",
                json!({"url": "https://api.test/x", "method": "post", "body": body}),
            ),
            &mut host,
        );
        // Made off the UI thread: wait for it.
        for _ in 0..100 {
            hub.pump_native(&mut host);
            if !host.delivered.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (to, reply) = host.delivered.pop().unwrap();
        assert_eq!(to, bg);
        assert_eq!(reply["result"]["status"], 200);
        let got = data_encoding::BASE64
            .decode(reply["result"]["body"].as_str().unwrap().as_bytes())
            .unwrap();
        assert_eq!(got, b"POST hi");

        // Another host, or a content script: the page's own fetch.
        hub.handle(
            &bg,
            &msg(
                2,
                "background",
                "net.fetch",
                json!({"url": "https://evil.test/"}),
            ),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["error"], "not permitted");
        hub.handle(
            &PageSource::Tab(TabId::new(1)),
            &msg(
                3,
                "content",
                "net.fetch",
                json!({"url": "https://api.test/x"}),
            ),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["error"], "not permitted");
    }

    #[test]
    fn ports_between_popup_and_background() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let (bg, popup) = (PageSource::Background(ext()), PageSource::Panel);

        // No background yet: nobody to connect to.
        hub.handle(
            &popup,
            &msg(1, "page", "port.connect", json!({"name": "x"})),
            &mut host,
        );
        assert!(host.delivered.pop().unwrap().1["error"].is_string());

        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        hub.handle(&popup, &msg(1, "page", "hello", json!({})), &mut host);
        host.delivered.clear();

        hub.handle(
            &popup,
            &msg(2, "page", "port.connect", json!({"name": "session"})),
            &mut host,
        );
        let id = host.delivered.pop().unwrap().1["result"].as_u64().unwrap();
        let (to, event) = host.delivered.pop().unwrap();
        assert_eq!(to, bg);
        assert_eq!(event["event"], "connect");
        assert_eq!(event["name"], "session");
        assert_eq!(event["port"], id);
        // The popup is the extension's own page: its origin says so.
        assert!(
            event["sender"]["origin"]
                .as_str()
                .unwrap()
                .contains(&ext().to_string())
        );

        // Both ways.
        hub.handle(
            &popup,
            &msg(3, "page", "port.post", json!({"port": id, "msg": {"a": 1}})),
            &mut host,
        );
        host.delivered.pop();
        let (to, event) = host.delivered.pop().unwrap();
        assert_eq!(
            (to, event["event"].clone(), event["msg"].clone()),
            (bg.clone(), json!("port"), json!({"a": 1}))
        );
        hub.handle(
            &bg,
            &msg(
                4,
                "background",
                "port.post",
                json!({"port": id, "msg": "ok"}),
            ),
            &mut host,
        );
        host.delivered.pop();
        let (to, event) = host.delivered.pop().unwrap();
        assert_eq!((to, event["msg"].clone()), (popup.clone(), json!("ok")));

        // The popup closes: the background's end disconnects.
        hub.panel_closed();
        host.delivered.clear();
        hub.tabs_changed(&mut host);
        assert!(
            host.delivered
                .iter()
                .any(|(to, e)| to == &bg && e["event"] == "portDisconnect" && e["port"] == id)
        );
        hub.handle(
            &bg,
            &msg(5, "background", "port.post", json!({"port": id, "msg": 1})),
            &mut host,
        );
        assert!(host.delivered.pop().unwrap().1["error"].is_string());
    }

    /// The browser just started: a page's message waits for the background
    /// page's listeners, as Chrome wakes its worker up for it.
    #[test]
    fn messages_wait_for_the_background_page() {
        let dir = tempfile::tempdir().unwrap();
        let ext_dir = extension_dir(&dir.path().join("ext"), &ext());
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.json"),
            r#"{"name": "E", "version": "1", "manifest_version": 3, "background": {"service_worker": "bg.js"}}"#,
        )
        .unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let tab = host.create_tab(Some("https://news.test/"), true).unwrap();
        let (bg, page) = (PageSource::Background(ext()), PageSource::Tab(tab));
        hub.handle(
            &page,
            &msg(1, "content", "sendMessage", json!({"payload": "early"})),
            &mut host,
        );
        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        assert!(
            !host
                .delivered
                .iter()
                .any(|(to, m)| to == &bg && m["event"] == "message")
        );
        host.delivered.clear();
        hub.handle(&bg, &msg(2, "background", "ready", json!({})), &mut host);
        let event = host
            .delivered
            .iter()
            .find(|(to, m)| to == &bg && m["event"] == "message")
            .map(|(_, m)| m.clone())
            .expect("the message reached the background page");
        assert_eq!(event["payload"], "early");
        assert_eq!(event["sender"]["tab"]["id"], tab.get());
    }

    #[test]
    fn a_restarted_background_page_gets_the_messages_once_ready() {
        let dir = tempfile::tempdir().unwrap();
        let ext_dir = extension_dir(&dir.path().join("ext"), &ext());
        std::fs::create_dir_all(&ext_dir).unwrap();
        std::fs::write(
            ext_dir.join("manifest.json"),
            r#"{"name": "E", "version": "1", "manifest_version": 3, "background": {"service_worker": "bg.js"}}"#,
        )
        .unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let tab = host.create_tab(Some("https://news.test/"), true).unwrap();
        hub.tabs_changed(&mut host);
        let (bg, page) = (PageSource::Background(ext()), PageSource::Tab(tab));
        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        hub.handle(&bg, &msg(2, "background", "ready", json!({})), &mut host);
        hub.handle(&page, &msg(1, "content", "hello", json!({})), &mut host);
        host.delivered.clear();

        // Taken on, then the page stops (an update restarts it): "nothing".
        hub.handle(
            &page,
            &msg(2, "content", "sendMessage", json!({"payload": 1})),
            &mut host,
        );
        let rid = host.delivered.pop().unwrap().1["rid"].as_u64().unwrap();
        hub.handle(
            &bg,
            &msg(3, "background", "ack", json!({"rid": rid})),
            &mut host,
        );
        hub.background_closed(&ext());
        hub.tabs_changed(&mut host);
        assert_eq!(
            host.delivered.pop().unwrap(),
            (page.clone(), json!({"id": 2, "ch": "c", "result": null}))
        );

        // While it starts again: kept for it until it listens.
        hub.handle(
            &page,
            &msg(3, "content", "sendMessage", json!({"payload": 2})),
            &mut host,
        );
        hub.handle(&bg, &msg(4, "background", "hello", json!({})), &mut host);
        let to_bg = |host: &Host| {
            host.delivered
                .iter()
                .filter(|(to, m)| to == &bg && m["event"] == "message")
                .map(|(_, m)| m["payload"].clone())
                .collect::<Vec<_>>()
        };
        assert!(to_bg(&host).is_empty());
        hub.handle(&bg, &msg(5, "background", "ready", json!({})), &mut host);
        assert_eq!(to_bg(&host), [json!(2)]);

        // A page reloading by itself is not ready either until it says so.
        hub.handle(&bg, &msg(6, "background", "hello", json!({})), &mut host);
        host.delivered.clear();
        hub.handle(
            &page,
            &msg(4, "content", "sendMessage", json!({"payload": 3})),
            &mut host,
        );
        assert!(host.delivered.is_empty());
    }

    #[test]
    fn messages_between_content_scripts_and_background() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let tab = host.create_tab(Some("https://news.test/"), true).unwrap();
        hub.tabs_changed(&mut host);
        let (bg, page) = (PageSource::Background(ext()), PageSource::Tab(tab));

        // Nobody listens yet: undefined.
        hub.handle(
            &page,
            &msg(1, "content", "sendMessage", json!({"payload": "hi"})),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["result"], Value::Null);

        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        hub.handle(&page, &msg(1, "content", "hello", json!({})), &mut host);
        host.delivered.clear();

        hub.handle(
            &page,
            &msg(2, "content", "sendMessage", json!({"payload": {"q": 1}})),
            &mut host,
        );
        let (to, event) = host.delivered.pop().unwrap();
        assert_eq!(to, bg);
        assert_eq!(event["event"], "message");
        assert_eq!(event["payload"], json!({"q": 1}));
        assert_eq!(event["sender"]["tab"]["id"], tab.get());
        let rid = event["rid"].as_u64().unwrap();
        hub.handle(
            &bg,
            &msg(
                5,
                "background",
                "respond",
                json!({"rid": rid, "payload": 42}),
            ),
            &mut host,
        );
        let (to, reply) = host.delivered.pop().unwrap();
        assert_eq!(to, page);
        assert_eq!(reply, json!({"id": 2, "ch": "c", "result": 42}));

        // The background asks the tab.
        hub.handle(
            &bg,
            &msg(
                6,
                "background",
                "tabs.sendMessage",
                json!({"tabId": tab.get(), "payload": "ping"}),
            ),
            &mut host,
        );
        let (to, event) = host.delivered.pop().unwrap();
        assert_eq!(to, page);
        let rid = event["rid"].as_u64().unwrap();
        hub.handle(
            &page,
            &msg(7, "content", "respond", json!({"rid": rid, "none": true})),
            &mut host,
        );
        assert_eq!(
            host.delivered.pop().unwrap().1,
            json!({"id": 6, "ch": "c", "result": null})
        );
        // A tab without the extension's scripts.
        hub.handle(
            &bg,
            &msg(
                8,
                "background",
                "tabs.sendMessage",
                json!({"tabId": 99, "payload": 1}),
            ),
            &mut host,
        );
        assert!(
            host.delivered.pop().unwrap().1["error"]
                .as_str()
                .unwrap()
                .contains("Receiving end")
        );
    }

    #[test]
    fn unanswered_messages_expire() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let tab = host.create_tab(Some("https://news.test/"), true).unwrap();
        hub.tabs_changed(&mut host);
        let (bg, page) = (PageSource::Background(ext()), PageSource::Tab(tab));
        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        hub.handle(&page, &msg(1, "content", "hello", json!({})), &mut host);
        host.delivered.clear();
        let later = Instant::now() + UNTAKEN + Duration::from_secs(1);

        // Lost on the way (the page was reloading): "no answer", not a wait.
        hub.handle(
            &page,
            &msg(2, "content", "sendMessage", json!({"payload": 1})),
            &mut host,
        );
        host.delivered.clear();
        assert!(hub.waiting());
        hub.expire(later, &mut host);
        assert_eq!(
            host.delivered.pop().unwrap(),
            (page.clone(), json!({"id": 2, "ch": "c", "result": null}))
        );
        assert!(!hub.waiting());

        // Taken on by an asynchronous listener: waited for.
        hub.handle(
            &page,
            &msg(3, "content", "sendMessage", json!({"payload": 2})),
            &mut host,
        );
        let rid = host.delivered.pop().unwrap().1["rid"].as_u64().unwrap();
        hub.handle(
            &bg,
            &msg(4, "background", "ack", json!({"rid": rid})),
            &mut host,
        );
        hub.expire(later, &mut host);
        assert!(host.delivered.is_empty());
        hub.handle(
            &bg,
            &msg(
                5,
                "background",
                "respond",
                json!({"rid": rid, "payload": 42}),
            ),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["result"], 42);
    }

    #[test]
    fn storage_changes_reach_every_context() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let tab = host.create_tab(Some("https://a.test/"), true).unwrap();
        let bg = PageSource::Background(ext());
        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        hub.handle(
            &PageSource::Tab(tab),
            &msg(1, "content", "hello", json!({})),
            &mut host,
        );
        host.delivered.clear();
        hub.handle(
            &bg,
            &msg(2, "background", "set", json!({"items": {"k": 1}})),
            &mut host,
        );
        let events: Vec<_> = host
            .delivered
            .iter()
            .filter(|(_, m)| m["event"] == "storage")
            .collect();
        assert_eq!(events.len(), 2, "the tab and the background");
        assert_eq!(events[0].1["changes"], json!({"k": {"newValue": 1}}));
        // Same value again: no event.
        host.delivered.clear();
        hub.handle(
            &bg,
            &msg(3, "background", "set", json!({"items": {"k": 1}})),
            &mut host,
        );
        assert!(host.delivered.iter().all(|(_, m)| m["event"].is_null()));
    }

    #[test]
    fn tabs_api_and_events() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let bg = PageSource::Background(ext());
        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        host.create_tab(Some("https://a.test/x"), false);
        host.create_tab(Some("https://b.test/"), true);
        hub.handle(
            &bg,
            &msg(
                2,
                "background",
                "tabs.query",
                json!({"query": {"active": true}}),
            ),
            &mut host,
        );
        let tabs = host.delivered.pop().unwrap().1["result"].clone();
        assert_eq!(tabs.as_array().unwrap().len(), 1);
        assert_eq!(tabs[0]["url"], "https://b.test/");
        hub.handle(
            &bg,
            &msg(
                3,
                "background",
                "tabs.query",
                json!({"query": {"url": "*://a.test/*"}}),
            ),
            &mut host,
        );
        assert_eq!(
            host.delivered.pop().unwrap().1["result"][0]["url"],
            "https://a.test/x"
        );

        hub.handle(
            &bg,
            &msg(
                4,
                "background",
                "tabs.create",
                json!({"props": {"url": "options.html"}}),
            ),
            &mut host,
        );
        let created = host.delivered.pop().unwrap().1["result"].clone();
        assert!(
            created["url"]
                .as_str()
                .unwrap()
                .ends_with(&format!("{EXT}/options.html"))
        );

        host.delivered.clear();
        hub.tabs_changed(&mut host);
        let names: Vec<_> = host
            .delivered
            .iter()
            .map(|(_, m)| m["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names.iter().filter(|n| *n == "onCreated").count(), 3);
        host.delivered.clear();
        host.tabs[0].loading = false;
        host.tabs[0].title = "A".into();
        host.remove_tab(TabId::new(2));
        hub.tabs_changed(&mut host);
        let names: Vec<_> = host
            .delivered
            .iter()
            .map(|(_, m)| m["name"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, vec!["onUpdated", "onRemoved"]);
    }

    /// Bitwarden's passkey window: it waits for `windows.onRemoved` to give
    /// the page back its own WebAuthn.
    #[test]
    fn created_windows_close_with_their_tab() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let bg = PageSource::Background(ext());
        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        host.create_tab(Some("https://a.test/"), true);
        hub.tabs_changed(&mut host);
        hub.handle(
            &bg,
            &msg(
                2,
                "background",
                "windows.create",
                json!({"data": {"url": "popup/index.html#/fido2", "type": "popup"}}),
            ),
            &mut host,
        );
        let window = host.delivered.pop().unwrap().1["result"].clone();
        assert_eq!(window["type"], "popup");
        let id = window["id"].as_u64().unwrap();
        assert_ne!(id, MAIN_WINDOW);
        assert_eq!(window["tabs"][0]["windowId"], id);
        let tab = window["tabs"][0]["id"].as_u64().unwrap();

        hub.handle(
            &bg,
            &msg(
                3,
                "background",
                "tabs.query",
                json!({"query": {"windowType": "popup"}}),
            ),
            &mut host,
        );
        let popups = host.delivered.pop().unwrap().1["result"].clone();
        assert_eq!(popups.as_array().unwrap().len(), 1);
        // The popup's own page finds its window.
        hub.handle(
            &PageSource::Tab(TabId::new(tab)),
            &msg(4, "page", "windows.getCurrent", json!({"info": {}})),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["result"]["id"], id);
        // The browser's window is not the extension's to close.
        hub.handle(
            &bg,
            &msg(5, "background", "windows.remove", json!({"windowId": 1})),
            &mut host,
        );
        assert_eq!(host.tabs.len(), 2);

        hub.tabs_changed(&mut host);
        host.delivered.clear();
        host.remove_tab(TabId::new(tab));
        hub.tabs_changed(&mut host);
        let closed: Vec<_> = host
            .delivered
            .iter()
            .filter(|(_, m)| m["event"] == "windows")
            .map(|(_, m)| (m["name"].clone(), m["args"].clone()))
            .collect();
        assert_eq!(closed, vec![(json!("onRemoved"), json!([id]))]);
        hub.handle(
            &bg,
            &msg(6, "background", "windows.get", json!({"windowId": id})),
            &mut host,
        );
        assert!(host.delivered.pop().unwrap().1["error"].is_string());
    }

    #[test]
    fn an_extension_page_in_a_tab_is_one_of_its_pages() {
        // Bitwarden's passkey window: its popup page, in a tab
        // `windows.create` opened, waits for its background's messages.
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let url = format!("https://bu-ext.{EXT}/popup/index.html");
        let tab = host.create_tab(Some(&url), true).unwrap();
        hub.tabs_changed(&mut host);
        host.tabs[0].loading = false;
        hub.tabs_changed(&mut host);
        let (bg, page) = (PageSource::Background(ext()), PageSource::Tab(tab));
        hub.handle(&bg, &msg(1, "background", "hello", json!({})), &mut host);
        hub.handle(&bg, &msg(2, "background", "ready", json!({})), &mut host);
        hub.handle(&page, &msg(1, "page", "hello", json!({})), &mut host);
        host.delivered.clear();

        // runtime.sendMessage from the background reaches it.
        hub.handle(
            &bg,
            &msg(3, "background", "sendMessage", json!({"payload": "pick"})),
            &mut host,
        );
        let (to, event) = host.delivered.pop().unwrap();
        assert_eq!(
            (to, event["event"].clone()),
            (page.clone(), json!("message"))
        );
        assert_eq!(event["payload"], "pick");
        // ...and its own messages reach the background, not itself.
        host.delivered.clear();
        hub.handle(
            &page,
            &msg(2, "page", "sendMessage", json!({"payload": "ok"})),
            &mut host,
        );
        let to: Vec<PageSource> = host.delivered.iter().map(|(t, _)| t.clone()).collect();
        assert_eq!(to, vec![bg.clone()]);

        // storage.onChanged too.
        host.delivered.clear();
        hub.handle(
            &bg,
            &msg(4, "background", "set", json!({"items": {"k": 1}})),
            &mut host,
        );
        assert!(
            host.delivered
                .iter()
                .any(|(to, e)| to == &page && e["event"] == "storage")
        );

        // runtime.connect from the background finds it.
        host.delivered.clear();
        hub.handle(
            &bg,
            &msg(5, "background", "port.connect", json!({"name": "s"})),
            &mut host,
        );
        assert!(
            host.delivered
                .iter()
                .any(|(to, e)| to == &page && e["event"] == "connect")
        );

        // The tab goes to another page: no longer the extension's.
        host.tabs[0].loading = true;
        hub.tabs_changed(&mut host);
        host.delivered.clear();
        hub.handle(
            &bg,
            &msg(6, "background", "sendMessage", json!({"payload": "x"})),
            &mut host,
        );
        assert!(!host.delivered.iter().any(|(to, _)| to == &page));
    }

    #[test]
    fn execute_script_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let tab = host.create_tab(Some("https://a.test/"), true).unwrap();
        let bg = PageSource::Background(ext());
        hub.handle(
            &bg,
            &msg(
                1,
                "background",
                "scripting.executeScript",
                json!({"tabId": tab.get(), "code": "document.title"}),
            ),
            &mut host,
        );
        let (ran_in, script) = host.ran.pop().unwrap();
        assert_eq!(ran_in, tab);
        assert!(script.contains("return (document.title);"));
        assert!(
            script.contains("return injected("),
            "with the extension's chrome.*"
        );
        let nonce = script.split("nonce: \"").nth(1).unwrap()[..32].to_owned();
        // A guessed nonce does nothing; the right one answers once.
        hub.handle(
            &PageSource::Tab(tab),
            &json!({"op": "scriptResult", "nonce": "x", "value": 1}).to_string(),
            &mut host,
        );
        assert!(host.delivered.is_empty());
        hub.handle(
            &PageSource::Tab(tab),
            &json!({"op": "scriptResult", "nonce": nonce, "value": "T"}).to_string(),
            &mut host,
        );
        assert_eq!(
            host.delivered.pop().unwrap().1["result"],
            json!([{"frameId": 0, "result": "T"}])
        );
    }

    #[test]
    fn registered_scripts_and_rules_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let ext_dir = extension_dir(&dir.path().join("ext"), &ext());
        fs::create_dir_all(&ext_dir).unwrap();
        fs::write(
            ext_dir.join("manifest.json"),
            r#"{"name":"x","declarative_net_request":{"rule_resources":[{"id":"a","enabled":true,"path":"a.json"},{"id":"b","enabled":false,"path":"b.json"}]}}"#,
        )
        .unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let bg = PageSource::Background(ext());
        let scripts = json!({"scripts": [{"id": "cs", "matches": ["<all_urls>"], "js": ["c.js"]}]});
        hub.handle(
            &bg,
            &msg(
                1,
                "background",
                "scripting.registerContentScripts",
                scripts.clone(),
            ),
            &mut host,
        );
        hub.handle(
            &bg,
            &msg(2, "background", "scripting.registerContentScripts", scripts),
            &mut host,
        );
        assert!(
            host.delivered.pop().unwrap().1["error"]
                .as_str()
                .unwrap()
                .contains("Duplicate")
        );
        hub.handle(
            &bg,
            &msg(
                3,
                "background",
                "scripting.getRegisteredContentScripts",
                json!({}),
            ),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["result"][0]["id"], "cs");

        hub.handle(
            &bg,
            &msg(4, "background", "dnr.getEnabledRulesets", json!({})),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["result"], json!(["a"]));
        hub.handle(
            &bg,
            &msg(
                5,
                "background",
                "dnr.updateEnabledRulesets",
                json!({"options": {"enableRulesetIds": ["b"], "disableRulesetIds": ["a"]}}),
            ),
            &mut host,
        );
        hub.handle(
            &bg,
            &msg(6, "background", "dnr.getEnabledRulesets", json!({})),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["result"], json!(["b"]));
        let rule =
            json!({"id": 1, "action": {"type": "block"}, "condition": {"urlFilter": "||x.test^"}});
        hub.handle(
            &bg,
            &msg(
                7,
                "background",
                "dnr.updateRules",
                json!({"kind": "dynamic", "options": {"addRules": [rule]}}),
            ),
            &mut host,
        );
        hub.handle(
            &bg,
            &msg(8, "background", "dnr.getRules", json!({"kind": "dynamic"})),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["result"][0]["id"], 1);
        assert_eq!(host.changed, 3);
        let state = ExtensionState::load(&dir.path().join("state"), &ext());
        assert_eq!(state.registered_scripts()[0].js, vec!["c.js"]);
    }
}
