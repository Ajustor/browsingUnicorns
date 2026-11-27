//! `tabs.*` and `windows.*`, and the `tabs.on*` / `windows.on*` events.

use std::time::Instant;

use application::ports::PageSource;
use domain::{ExtensionId, TabId, UrlPattern};
use serde_json::{Map, Value, json};

use super::{Answer, Call, Hub, HubHost, MAIN_WINDOW, TabState, tab_id, unsupported};
use crate::page_scripts::Bridge;

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

impl Hub {
    pub(super) fn tabs_call(&mut self, call: &Call<'_>, host: &mut dyn HubHost) -> Answer {
        let Call { from, raw, .. } = *call;
        let ext = call.ext;
        let field = |k: &str| call.field(k);
        match call.op {
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
            op if op.starts_with("windows.") => Some(self.windows_call(from, ext, op, raw, host)),
            "tabs.create" => {
                let props = field("props");
                let url = props
                    .get("url")
                    .and_then(Value::as_str)
                    .map(|u| self.resolve(ext, u));
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
                    .map(|u| self.resolve(ext, u));
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
            op => unsupported(op),
        }
    }

    pub(super) fn tab_json(&self, t: &TabState, index: usize) -> Value {
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
        ext: Option<&ExtensionId>,
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
                // A popup (a passkey prompt...) floats over its opener's
                // page; a normal window is a tab.
                let popup = matches!(kind.as_str(), "popup" | "panel");
                let tab = if popup {
                    host.create_window_tab(url.as_deref(), focused)
                } else {
                    host.create_tab(url.as_deref(), focused)
                }
                .ok_or("The window could not be created.")?;
                let size = |k: &str| data.get(k).and_then(Value::as_u64).map(|v| v as u32);
                if popup && let (Some(w), Some(h)) = (size("width"), size("height")) {
                    self.window_sizes.insert(tab, (w, h));
                }
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

    /// The extensions' popup windows (tabs shown floating over their
    /// opener's page), with the size they asked for.
    pub fn popup_windows(&self) -> Vec<(TabId, Option<(u32, u32)>)> {
        self.popups
            .iter()
            .filter(|(_, (_, kind))| matches!(kind.as_str(), "popup" | "panel"))
            .map(|(tab, _)| (*tab, self.window_sizes.get(tab).copied()))
            .collect()
    }

    pub(super) fn active_tab(&self, host: &dyn HubHost) -> Option<TabId> {
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
    fn resolve(&self, ext: Option<&ExtensionId>, url: &str) -> String {
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
                self.window_sizes.remove(&b.id);
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
}

#[cfg(test)]
mod tests {
    use domain::TabId;
    use serde_json::json;

    use super::super::tests::{EXT, Host, ext, hub, msg};
    use super::*;

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
    fn popup_windows_are_listed_with_their_size() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let mut host = Host::default();
        let bg = PageSource::Background(ext());
        hub.handle(
            &bg,
            &msg(1, "background", "windows.create", json!({"data": {"url": "https://a.test/", "type": "popup", "width": 380, "height": 630}})),
            &mut host,
        );
        hub.handle(
            &bg,
            &msg(
                2,
                "background",
                "windows.create",
                json!({"data": {"url": "https://b.test/"}}),
            ),
            &mut host,
        );
        let windows = hub.popup_windows();
        assert_eq!(windows, vec![(TabId::new(1), Some((380, 630)))]);
        hub.tabs_changed(&mut host);
        host.tabs.clear();
        hub.tabs_changed(&mut host);
        assert!(hub.popup_windows().is_empty());
    }
}
