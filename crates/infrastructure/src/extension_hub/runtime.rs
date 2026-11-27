//! `runtime.*` messaging: the pages saying hello and ready,
//! `runtime.sendMessage` and `tabs.sendMessage`, and their answers.

use std::time::Instant;

use application::ports::PageSource;
use domain::ExtensionId;
use serde_json::{Value, json};

use super::{
    Answer, Call, Hub, HubHost, MAX_QUEUED, NEVER_READY, NO_RECEIVER, Pending, Queued, UNTAKEN,
    tab_id, unsupported,
};

impl Hub {
    pub(super) fn runtime_call(&mut self, call: &Call<'_>, host: &mut dyn HubHost) -> Answer {
        let Call {
            from,
            ctx,
            owner,
            id,
            ch,
            ..
        } = *call;
        let ext = call.ext;
        let field = |k: &str| call.field(k);
        match call.op {
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
            "blur" => {
                if from == &PageSource::Panel {
                    self.panel_blurred = true;
                }
                Some(Ok(Value::Null))
            }
            "ready" => {
                if let (PageSource::Background(_), Some(ext)) = (from, ext) {
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
                    && self.expects_background(ext)
                {
                    let sender = self.sender(from, ext, host);
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
                let to = self.message_targets(from, owner, ext);
                let sender = self.sender(from, ext, host);
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
            op => unsupported(op),
        }
    }

    /// Answers the messages nobody will: not taken on by a listener within
    /// [`UNTAKEN`], or waiting for a background page that is not starting.
    pub(super) fn expire(&mut self, now: Instant, host: &mut dyn HubHost) {
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
    pub(super) fn sender(&self, from: &PageSource, ext: &ExtensionId, host: &dyn HubHost) -> Value {
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

    /// Whether `ext` has a background page (its manifest says).
    fn expects_background(&self, ext: &ExtensionId) -> bool {
        self.catalog
            .get(ext)
            .is_some_and(|p| p.background().is_some())
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
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::tests::{Host, ext, hub, msg};
    use super::*;
    use crate::extensions::extension_dir;

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
}
