//! `storage.*`: the script storage, and its change events sent to every
//! page of the owner.

use serde_json::{Value, json};

use super::{Hub, HubHost};
use crate::script_storage::Message;

impl Hub {
    pub(super) fn storage_call(
        &mut self,
        owner: &str,
        message: Message,
        host: &mut dyn HubHost,
    ) -> Result<Value, String> {
        self.storage.apply(message).map(|(result, changes)| {
            if let Some((changes, area)) = changes {
                let event =
                    json!({"event": "storage", "owner": owner, "changes": changes, "area": area});
                self.broadcast(owner, event, host);
            }
            result
        })
    }
}

#[cfg(test)]
mod tests {
    use application::ports::PageSource;
    use serde_json::json;

    use super::super::HubHost;
    use super::super::tests::{Host, ext, hub, msg};

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
}
