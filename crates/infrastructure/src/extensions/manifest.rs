//! An unpacked extension's `manifest.json`: what the browser shows of it,
//! and its `key`.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub popup: Option<String>,
    pub options: Option<String>,
}

fn load(dir: &Path) -> Result<Map<String, Value>, String> {
    let text =
        fs::read_to_string(dir.join("manifest.json")).map_err(|e| format!("manifest.json: {e}"))?;
    match serde_json::from_str(text.trim_start_matches('\u{feff}')) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(i18n::errors::infrastructure::manifest_not_an_object().into()),
        Err(e) => Err(format!("manifest.json: {e}")),
    }
}

fn text<'a>(map: &'a Map<String, Value>, path: &[&str]) -> Option<&'a str> {
    let (last, parents) = path.split_last()?;
    let mut map = map;
    for key in parents {
        map = map.get(*key)?.as_object()?;
    }
    map.get(*last)?.as_str().filter(|s| !s.is_empty())
}

/// Replaces `__MSG_name__` with its text in the default locale.
fn localize(dir: &Path, manifest: &Map<String, Value>, value: &str) -> String {
    let Some(key) = value
        .strip_prefix("__MSG_")
        .and_then(|v| v.strip_suffix("__"))
    else {
        return value.to_owned();
    };
    let locale = text(manifest, &["default_locale"]).unwrap_or("en");
    let messages = fs::read_to_string(dir.join("_locales").join(locale).join("messages.json"))
        .ok()
        .and_then(|t| {
            serde_json::from_str::<Map<String, Value>>(t.trim_start_matches('\u{feff}')).ok()
        });
    messages
        .and_then(|m| {
            m.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .and_then(|(_, v)| v.get("message")?.as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| key.to_owned())
}

pub fn read(dir: &Path) -> Result<ManifestInfo, String> {
    let m = load(dir)?;
    let name =
        text(&m, &["name"]).ok_or_else(i18n::errors::infrastructure::manifest_without_name)?;
    Ok(ManifestInfo {
        name: localize(dir, &m, name),
        version: text(&m, &["version"]).unwrap_or("").to_owned(),
        description: text(&m, &["description"])
            .map(|d| localize(dir, &m, d))
            .unwrap_or_default(),
        popup: [
            &["action", "default_popup"][..],
            &["browser_action", "default_popup"],
            &["page_action", "default_popup"],
        ]
        .iter()
        .find_map(|p| text(&m, p))
        .map(str::to_owned),
        options: text(&m, &["options_ui", "page"])
            .or_else(|| text(&m, &["options_page"]))
            .map(str::to_owned),
    })
}

/// A `content_scripts` entry of the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentScript {
    pub matches: Vec<domain::UrlPattern>,
    pub excludes: Vec<domain::UrlPattern>,
    /// Paths relative to the extension's root.
    pub js: Vec<String>,
    pub css: Vec<String>,
    pub run_at: domain::RunAt,
    pub all_frames: bool,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The manifest's content scripts (those declared statically).
pub fn content_scripts(dir: &Path) -> Vec<ContentScript> {
    let Ok(m) = load(dir) else {
        return Vec::new();
    };
    let Some(entries) = m.get("content_scripts").and_then(Value::as_array) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|e| {
            let matches: Vec<_> = strings(e.get("matches"))
                .iter()
                .filter_map(|p| domain::UrlPattern::parse_match(p))
                .collect();
            if matches.is_empty() {
                return None;
            }
            let mut excludes: Vec<_> = strings(e.get("exclude_matches"))
                .iter()
                .filter_map(|p| domain::UrlPattern::parse_match(p))
                .collect();
            excludes.extend(
                strings(e.get("exclude_globs"))
                    .iter()
                    .filter_map(|p| domain::UrlPattern::parse_include(p)),
            );
            Some(ContentScript {
                matches,
                excludes,
                js: strings(e.get("js")),
                css: strings(e.get("css")),
                run_at: e
                    .get("run_at")
                    .and_then(Value::as_str)
                    .and_then(domain::RunAt::parse)
                    .unwrap_or_default(),
                all_frames: e
                    .get("all_frames")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect()
}

/// The extension's toolbar icon (relative path), the size closest above
/// 32 px: `action.default_icon`, `browser_action.default_icon`, `icons`.
pub fn action_icon(dir: &Path) -> Option<String> {
    let m = load(dir).ok()?;
    let candidates = [
        m.get("action").and_then(|a| a.get("default_icon")),
        m.get("browser_action").and_then(|a| a.get("default_icon")),
        m.get("page_action").and_then(|a| a.get("default_icon")),
        m.get("icons"),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|icon| match icon {
            Value::String(path) => Some(path.clone()),
            Value::Object(sizes) => {
                let mut sized: Vec<(u32, &str)> = sizes
                    .iter()
                    .filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?)))
                    .collect();
                sized.sort_by_key(|(size, _)| (*size < 32, size.abs_diff(32)));
                sized.first().map(|(_, p)| (*p).to_owned())
            }
            _ => None,
        })
}

/// The `background` of a manifest: MV3's service worker (run as a page
/// script here), MV2's scripts or page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Background {
    pub scripts: Vec<String>,
    pub page: Option<String>,
    /// `"type": "module"` (MV3).
    pub module: bool,
}

pub fn background(dir: &Path) -> Option<Background> {
    let m = load(dir).ok()?;
    let bg = m.get("background")?.as_object()?;
    let mut scripts = strings(bg.get("scripts"));
    if let Some(worker) = bg.get("service_worker").and_then(Value::as_str) {
        scripts.push(worker.to_owned());
    }
    let page = bg.get("page").and_then(Value::as_str).map(str::to_owned);
    (!scripts.is_empty() || page.is_some()).then(|| Background {
        scripts,
        page,
        module: bg.get("type").and_then(Value::as_str) == Some("module"),
    })
}

/// The default locale's `messages.json`, keys in lower case
/// (`chrome.i18n.getMessage`).
pub fn messages(dir: &Path) -> Map<String, Value> {
    let locale = load(dir)
        .ok()
        .and_then(|m| text(&m, &["default_locale"]).map(str::to_owned))
        .unwrap_or_else(|| "en".into());
    fs::read_to_string(dir.join("_locales").join(locale).join("messages.json"))
        .ok()
        .and_then(|t| {
            serde_json::from_str::<Map<String, Value>>(t.trim_start_matches('\u{feff}')).ok()
        })
        .map(|m| m.into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect())
        .unwrap_or_default()
}

/// Whether the manifest asks for `permission` (`permissions` or
/// `optional_permissions`, which this browser grants as asked).
pub fn has_permission(dir: &Path, permission: &str) -> bool {
    let Ok(m) = load(dir) else {
        return false;
    };
    ["permissions", "optional_permissions"].iter().any(|key| {
        m.get(*key)
            .and_then(Value::as_array)
            .is_some_and(|list| list.iter().any(|p| p.as_str() == Some(permission)))
    })
}

/// Whether the extension in `dir` came from a store (Chrome Web Store, Edge
/// Add-ons): their packages declare where updates come from. Unpacked
/// folders loaded by hand don't, and are left as they are.
pub fn from_store(dir: &Path) -> bool {
    load(dir).ok().is_some_and(|m| {
        text(&m, &["update_url"])
            .is_some_and(|u| u.contains("clients2.google.com") || u.contains("edge.microsoft.com"))
    })
}

/// The `version` of the manifest in a package's `zip`.
pub fn version_in_zip(zip: &[u8]) -> Option<String> {
    use std::io::Read as _;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).ok()?;
    let mut entry = archive.by_name("manifest.json").ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    let m: Map<String, Value> = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    text_owned(&m, "version")
}

fn text_owned(m: &Map<String, Value>, key: &str) -> Option<String> {
    m.get(key)?.as_str().map(str::to_owned)
}

/// The publisher's public key (base64 DER) the manifest declares, if any.
pub fn key(dir: &Path) -> Option<String> {
    let m = load(dir).ok()?;
    text(&m, &["key"]).map(str::to_owned)
}

/// Writes the publisher's public key (base64 DER) into the manifest: an
/// unpacked extension then has the same id as in the Web Store.
pub fn set_key(dir: &Path, key_base64: &str) -> Result<(), String> {
    let mut m = load(dir)?;
    m.insert("key".into(), Value::String(key_base64.to_owned()));
    let text = serde_json::to_string_pretty(&m).map_err(|e| e.to_string())?;
    fs::write(dir.join("manifest.json"), text).map_err(|e| format!("manifest.json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_localized_manifest() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("manifest.json"),
            "\u{feff}{\"name\":\"__MSG_extName__\",\"version\":\"1.2\",\"default_locale\":\"fr\",\
             \"description\":\"Plain\",\"action\":{\"default_popup\":\"popup.html\"},\
             \"options_ui\":{\"page\":\"options.html\"}}",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("_locales/fr")).unwrap();
        fs::write(
            dir.path().join("_locales/fr/messages.json"),
            r#"{"extname":{"message":"Bloqueur"}}"#,
        )
        .unwrap();
        let info = read(dir.path()).unwrap();
        assert_eq!(
            info,
            ManifestInfo {
                name: "Bloqueur".into(),
                version: "1.2".into(),
                description: "Plain".into(),
                popup: Some("popup.html".into()),
                options: Some("options.html".into()),
            }
        );
        set_key(dir.path(), "S0VZ").unwrap();
        assert_eq!(load(dir.path()).unwrap()["key"], "S0VZ");
        assert_eq!(read(dir.path()).unwrap().name, "Bloqueur");
    }

    #[test]
    fn mv2_popup_and_options_page() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("manifest.json"),
            r#"{"name":"Old","browser_action":{"default_popup":"p.html"},"options_page":"o.html"}"#,
        )
        .unwrap();
        let info = read(dir.path()).unwrap();
        assert_eq!(info.popup.as_deref(), Some("p.html"));
        assert_eq!(info.options.as_deref(), Some("o.html"));
    }

    #[test]
    fn content_scripts_and_messages() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("manifest.json"),
            r#"{"name":"x","default_locale":"en","content_scripts":[
                {"matches":["*://*.example.com/*"],"exclude_matches":["*://example.com/admin/*"],
                 "js":["a.js","b.js"],"css":["s.css"],"run_at":"document_start","all_frames":true},
                {"matches":["chrome://*/*"],"js":["never.js"]}]}"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("_locales/en")).unwrap();
        fs::write(
            dir.path().join("_locales/en/messages.json"),
            r#"{"Hello":{"message":"Hi $1"}}"#,
        )
        .unwrap();
        let scripts = content_scripts(dir.path());
        assert_eq!(scripts.len(), 1, "unsupported schemes are left out");
        assert_eq!(scripts[0].js, vec!["a.js", "b.js"]);
        assert_eq!(scripts[0].run_at, domain::RunAt::DocumentStart);
        assert!(scripts[0].all_frames);
        assert_eq!(scripts[0].excludes.len(), 1);
        assert_eq!(messages(dir.path())["hello"]["message"], "Hi $1");
    }

    #[test]
    fn toolbar_icons() {
        let dir = tempfile::tempdir().unwrap();
        let write = |m: &str| fs::write(dir.path().join("manifest.json"), m).unwrap();
        write(
            r#"{"name":"x","action":{"default_icon":{"16":"16.png","32":"32.png","64":"64.png"}}}"#,
        );
        assert_eq!(action_icon(dir.path()).as_deref(), Some("32.png"));
        write(r#"{"name":"x","browser_action":{"default_icon":"b.png"}}"#);
        assert_eq!(action_icon(dir.path()).as_deref(), Some("b.png"));
        write(r#"{"name":"x","icons":{"16":"16.png","48":"48.png","128":"128.png"}}"#);
        assert_eq!(action_icon(dir.path()).as_deref(), Some("48.png"));
        write(r#"{"name":"x"}"#);
        assert_eq!(action_icon(dir.path()), None);
    }
}
