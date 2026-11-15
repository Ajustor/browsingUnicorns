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
        Ok(_) => Err("manifest.json is not an object".into()),
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
    let name = text(&m, &["name"]).ok_or("the manifest has no name")?;
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
}
