//! The browser side of the extension APIs the runtime (`page_scripts`)
//! cannot answer alone: storage (and its change events), messages between
//! content scripts, background pages and the popup, tabs, `scripting` and
//! `declarativeNetRequest`. Shared by the desktop and Android: what touches
//! the tabs or the web views goes through [`HubHost`].

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use application::Browser;
use application::ports::{PageSource, WebEngine};
use domain::{ExtensionId, Extensions, TabId, UrlPattern, WebAddress};
use serde_json::{Map, Value, json};

use crate::extensions::state::ExtensionState;
use crate::extensions::{dnr, extension_dir};
use crate::page_scripts::Bridge;
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
    /// The tabs as last seen, for `tabs.on*` events.
    last_tabs: Vec<TabState>,
}

fn tab_json(t: &TabState, index: usize) -> Value {
    json!({
        "id": t.id.get(),
        "index": index,
        "windowId": 1,
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
            last_tabs: Vec::new(),
        }
    }

    /// Forgets what extension or user script `owner` stored.
    pub fn forget(&mut self, owner: &str) {
        self.storage.forget(owner);
        if let Some(id) = owner.strip_prefix("ext:").and_then(ExtensionId::parse) {
            ExtensionState::forget(&self.state_dir, &id);
        }
        self.backgrounds.remove(owner);
    }

    /// A background page went away (extension disabled or removed).
    pub fn background_closed(&mut self, id: &ExtensionId) {
        self.backgrounds.remove(&format!("ext:{id}"));
    }

    /// The popup in the side panel closed.
    pub fn panel_closed(&mut self) {
        self.panel = None;
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
        out
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
        if message.t != self.token || !valid_owner(&message.owner) {
            return;
        }
        let (id, ch, owner) = (message.id, message.ch.clone(), message.owner.clone());
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
                        self.backgrounds.insert(owner.to_owned());
                    }
                    (PageSource::Panel, _) => self.panel = Some(owner.to_owned()),
                    _ => {}
                }
                Some(Ok(Value::Null))
            }
            "sendMessage" => {
                let ext = ext?;
                let mut to = Vec::new();
                if !matches!(from, PageSource::Background(_)) && self.backgrounds.contains(owner) {
                    to.push(PageSource::Background(ext.clone()));
                }
                if from != &PageSource::Panel && self.panel.as_deref() == Some(owner) {
                    to.push(PageSource::Panel);
                }
                let sender = Self::sender(from, &ext, host);
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
                        .map(|(i, t)| tab_json(t, i))
                        .ok_or_else(|| "No tab with this id.".to_owned()),
                )
            }
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
                let code = match field("code").as_str() {
                    Some(code) => code.to_owned(),
                    None => {
                        let mut code = String::new();
                        for f in field("files")
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                        {
                            match self.extension_file(&ext, f) {
                                Some(text) => {
                                    code.push_str(&format!("(function () {{\n{text}\n}})();\n"))
                                }
                                None => return Some(Err(format!("Could not load file: '{f}'."))),
                            }
                        }
                        format!("(function () {{ {code} }})()")
                    }
                };
                let nonce = new_token();
                self.scripts.insert(
                    nonce.clone(),
                    Pending {
                        to: from.clone(),
                        id,
                        ch: ch.clone(),
                        waiting: 1,
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
    fn sender(from: &PageSource, ext: &ExtensionId, host: &dyn HubHost) -> Value {
        let mut sender = json!({"id": ext.to_string()});
        let tabs = host.tabs();
        if let PageSource::Tab(tab) = from
            && let Some((i, t)) = tabs.iter().enumerate().find(|(_, t)| t.id == *tab)
        {
            sender["tab"] = tab_json(t, i);
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

    fn active_tab(&self, host: &dyn HubHost) -> Option<TabId> {
        host.tabs().into_iter().find(|t| t.active).map(|t| t.id)
    }

    fn describe(&self, tab: Option<TabId>, host: &dyn HubHost) -> Value {
        host.tabs()
            .iter()
            .enumerate()
            .find(|(_, t)| Some(t.id) == tab)
            .map(|(i, t)| tab_json(t, i))
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
                .map(|(i, t)| tab_json(t, i))
                .collect(),
        )
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
        let now = host.tabs();
        if now == self.last_tabs {
            return;
        }
        let before = std::mem::replace(&mut self.last_tabs, now.clone());
        let mut events: Vec<(&str, Value)> = Vec::new();
        for (i, t) in now.iter().enumerate() {
            match before.iter().find(|b| b.id == t.id) {
                None => events.push(("onCreated", json!([tab_json(t, i)]))),
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
                        events.push(("onUpdated", json!([t.id.get(), change, tab_json(t, i)])));
                    }
                    if t.loading && !b.loading {
                        // A new page: its content scripts say hello again.
                        self.tab_owners.remove(&t.id);
                    }
                    if t.active && !b.active {
                        events.push(("onActivated", json!([{"tabId": t.id.get(), "windowId": 1}])));
                    }
                }
            }
        }
        for b in &before {
            if !now.iter().any(|t| t.id == b.id) {
                self.tab_owners.remove(&b.id);
                events.push((
                    "onRemoved",
                    json!([b.id.get(), {"windowId": 1, "isWindowClosing": false}]),
                ));
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
        assert!(script.contains("await (document.title)"));
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
