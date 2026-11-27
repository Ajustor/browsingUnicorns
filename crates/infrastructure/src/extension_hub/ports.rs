//! Ports between an extension's pages and scripts (`runtime.connect`,
//! `tabs.connect`).

use application::ports::PageSource;
use serde_json::{Value, json};

use super::{Answer, Call, Hub, HubHost, Port, tab_id, unsupported};

impl Hub {
    pub(super) fn port_call(&mut self, call: &Call<'_>, host: &mut dyn HubHost) -> Answer {
        let Call { from, owner, .. } = *call;
        let ext = call.ext;
        let field = |k: &str| call.field(k);
        match call.op {
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
                let sender = self.sender(from, ext, host);
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
            op => unsupported(op),
        }
    }

    /// `page` went away: its ports close, the other ends are told.
    pub(super) fn close_ports_of(&mut self, page: &PageSource, host: &mut dyn HubHost) {
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
}

#[cfg(test)]
mod tests {
    use super::super::tests::{Host, ext, hub, msg};
    use super::*;

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
}
