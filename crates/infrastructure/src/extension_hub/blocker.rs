//! The ad blocker's page script: its cosmetic filters, nothing else.

use serde_json::{Map, Value};

use super::Hub;

impl Hub {
    pub(super) fn blocker_call(&self, op: &str, raw: &Map<String, Value>) -> Result<Value, String> {
        match op {
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
        }
    }
}

#[cfg(test)]
mod tests {
    use application::ports::PageSource;
    use domain::TabId;
    use serde_json::json;

    use super::super::tests::{Host, hub};
    use super::*;
    use crate::blocker::BlockerHandle;

    fn cosmetic(hub: &mut Hub, host: &mut Host, url: &str) -> Value {
        let m = json!({"t": crate::script_storage::owner_token("tok", "blocker"), "ch": "c", "id": 1, "owner": "blocker",
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
        let m = json!({"t": crate::script_storage::owner_token("tok", "blocker"), "ch": "c", "id": 1, "owner": "blocker",
            "op": "set", "items": {"a": 1}});
        hub.handle(&PageSource::Tab(TabId::new(1)), &m.to_string(), &mut host);
        assert!(host.delivered.pop().unwrap().1["error"].is_string());
    }
}
