//! What an extension changes at run time, kept in the profile
//! (`<profile>/extension-state/<id>.json`): its enabled rulesets, dynamic
//! and session rules (`declarativeNetRequest`), and the content scripts it
//! registered (`scripting.registerContentScripts`). Session rules are
//! dropped when the browser starts.

use std::fs;
use std::path::Path;

use domain::ExtensionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::manifest::ContentScript;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExtensionState {
    /// `None`: the manifest's choice.
    pub enabled_rulesets: Option<Vec<String>>,
    pub dynamic_rules: Vec<Value>,
    pub session_rules: Vec<Value>,
    /// As `scripting.registerContentScripts` was given them.
    pub content_scripts: Vec<Value>,
}

fn file(dir: &Path, id: &ExtensionId) -> std::path::PathBuf {
    dir.join(format!("{id}.json"))
}

impl ExtensionState {
    pub fn load(dir: &Path, id: &ExtensionId) -> Self {
        fs::read_to_string(file(dir, id))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, dir: &Path, id: &ExtensionId) -> Result<(), String> {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        crate::files::write(&file(dir, id), &text).map_err(|e| e.to_string())
    }

    /// Drops every extension's session rules (at start-up).
    pub fn end_sessions(dir: &Path) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".json").and_then(ExtensionId::parse) else {
                continue;
            };
            let mut state = Self::load(dir, &id);
            if !state.session_rules.is_empty() {
                state.session_rules.clear();
                let _ = state.save(dir, &id);
            }
        }
    }

    pub fn forget(dir: &Path, id: &ExtensionId) {
        let _ = fs::remove_file(file(dir, id));
    }

    /// The registered content scripts, as the manifest's are read.
    pub fn registered_scripts(&self) -> Vec<ContentScript> {
        self.content_scripts
            .iter()
            .filter_map(|s| {
                let strings = |key: &str| -> Vec<String> {
                    s.get(key)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let matches: Vec<_> = strings("matches")
                    .iter()
                    .filter_map(|p| domain::UrlPattern::parse_match(p))
                    .collect();
                if matches.is_empty() {
                    return None;
                }
                Some(ContentScript {
                    matches,
                    excludes: strings("excludeMatches")
                        .iter()
                        .filter_map(|p| domain::UrlPattern::parse_match(p))
                        .collect(),
                    js: strings("js"),
                    css: strings("css"),
                    run_at: s
                        .get("runAt")
                        .and_then(Value::as_str)
                        .and_then(domain::RunAt::parse)
                        .unwrap_or_default(),
                    all_frames: s.get("allFrames").and_then(Value::as_bool).unwrap_or(false),
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn round_trip_and_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let id = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        assert_eq!(
            ExtensionState::load(dir.path(), &id),
            ExtensionState::default()
        );
        let state = ExtensionState {
            enabled_rulesets: Some(vec!["a".into()]),
            dynamic_rules: vec![json!({"id": 1})],
            session_rules: vec![json!({"id": 2})],
            content_scripts: vec![
                json!({"id": "cs", "matches": ["<all_urls>"], "js": ["x.js"], "runAt": "document_start", "allFrames": true}),
            ],
        };
        state.save(dir.path(), &id).unwrap();
        assert_eq!(ExtensionState::load(dir.path(), &id), state);
        let scripts = state.registered_scripts();
        assert_eq!(scripts[0].js, vec!["x.js"]);
        assert!(scripts[0].all_frames);
        ExtensionState::end_sessions(dir.path());
        let after = ExtensionState::load(dir.path(), &id);
        assert!(after.session_rules.is_empty());
        assert_eq!(after.dynamic_rules.len(), 1);
    }
}
