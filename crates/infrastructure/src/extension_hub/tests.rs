//! What the hub's tests share, and the tests across several APIs.

use std::fs;
use std::path::Path;

use application::ports::PageSource;
use domain::{ExtensionId, TabId};
use serde_json::{Value, json};

use super::*;
use crate::extensions::extension_dir;
use crate::extensions::state::ExtensionState;
use crate::script_storage::ScriptStorage;

pub(super) const EXT: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

#[derive(Default)]
pub(super) struct Host {
    pub(super) tabs: Vec<TabState>,
    pub(super) delivered: Vec<(PageSource, Value)>,
    pub(super) ran: Vec<(TabId, String)>,
    pub(super) changed: usize,
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

pub(super) fn hub(dir: &Path) -> Hub {
    Hub::new(
        ScriptStorage::new(dir.join("storage"), "tok".into()),
        Bridge::Ipc,
        dir.join("ext"),
        dir.join("state"),
    )
}

pub(super) fn msg(id: u64, ctx: &str, op: &str, extra: Value) -> String {
    let mut m = json!({"t": "tok", "ch": "c", "id": id, "owner": format!("ext:{EXT}"), "ctx": ctx, "op": op});
    if let (Value::Object(m), Value::Object(e)) = (&mut m, extra) {
        m.extend(e);
    }
    m.to_string()
}

pub(super) fn ext() -> ExtensionId {
    ExtensionId::parse(EXT).unwrap()
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
