//! `scripting.*`: scripts and styles run in tabs, content scripts
//! registered at run time.

use std::fs;
use std::time::Instant;

use domain::ExtensionId;
use serde_json::{Map, Value, json};

use super::{Answer, Call, Hub, HubHost, Pending, tab_id, unsupported};
use crate::extensions::state::ExtensionState;
use crate::extensions::{extension_dir, manifest};
use crate::page_scripts::{self, Bridge};
use crate::script_storage::new_token;

impl Hub {
    pub(super) fn scripting_call(&mut self, call: &Call<'_>, host: &mut dyn HubHost) -> Answer {
        let Call {
            from,
            op,
            id,
            ch,
            raw,
            ..
        } = *call;
        let ext = call.ext;
        let field = |k: &str| call.field(k);
        match op {
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
                            match self.extension_file(ext, f) {
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
                    &self.injected_config(ext),
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
                    css.push_str(&self.extension_file(ext, f).unwrap_or_default());
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
                Some(self.content_scripts(ext, op, raw, host))
            }
            op => unsupported(op),
        }
    }

    /// An `executeScript` result: it comes with the one-time nonce of its
    /// call, not the token (the page could read it).
    pub(super) fn script_result(&mut self, raw: &mut Map<String, Value>, host: &mut dyn HubHost) {
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
}

#[cfg(test)]
mod tests {
    use application::ports::PageSource;
    use serde_json::json;

    use super::super::tests::{Host, ext, hub, msg};
    use super::*;

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
}
