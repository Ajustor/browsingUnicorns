//! Bookmarks of other browsers: Chromium's (Chrome, Edge, Brave, Vivaldi)
//! read from their profile, and the HTML file every browser exports
//! (Firefox, Safari...).

use std::path::{Path, PathBuf};

use serde_json::Value;

/// A bookmark found in another browser, with its folder (names separated
/// by `/`, empty at the top).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Imported {
    pub url: String,
    pub title: String,
    pub folder: String,
}

/// A browser found on this computer, whose bookmarks can be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub name: &'static str,
    /// Its `Bookmarks` file.
    pub file: PathBuf,
}

/// The Chromium browsers installed here with bookmarks (default profile).
pub fn chromium_sources() -> Vec<Source> {
    // Where each keeps its "User Data", under the per-user data folder.
    let browsers: &[(&str, &[&str])] = if cfg!(windows) {
        &[
            ("Chrome", &["Google", "Chrome", "User Data"]),
            ("Edge", &["Microsoft", "Edge", "User Data"]),
            ("Brave", &["BraveSoftware", "Brave-Browser", "User Data"]),
            ("Vivaldi", &["Vivaldi", "User Data"]),
        ]
    } else if cfg!(target_os = "macos") {
        &[
            ("Chrome", &["Google", "Chrome"]),
            ("Edge", &["Microsoft Edge"]),
            ("Brave", &["BraveSoftware", "Brave-Browser"]),
            ("Vivaldi", &["Vivaldi"]),
        ]
    } else {
        &[
            ("Chrome", &["google-chrome"]),
            ("Chromium", &["chromium"]),
            ("Edge", &["microsoft-edge"]),
            ("Brave", &["BraveSoftware", "Brave-Browser"]),
            ("Vivaldi", &["vivaldi"]),
        ]
    };
    let base = if cfg!(windows) {
        dirs::data_local_dir()
    } else {
        dirs::config_dir()
    };
    let Some(base) = base else {
        return Vec::new();
    };
    browsers
        .iter()
        .filter_map(|(name, parts)| {
            let mut file = base.clone();
            file.extend(parts.iter());
            file.push("Default");
            file.push("Bookmarks");
            file.is_file().then_some(Source { name, file })
        })
        .collect()
}

/// The bookmarks of a Chromium `Bookmarks` file.
pub fn read_chromium(file: &Path) -> Result<Vec<Imported>, String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    parse_chromium(&text)
}

fn parse_chromium(text: &str) -> Result<Vec<Imported>, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let roots = json
        .get("roots")
        .and_then(Value::as_object)
        .ok_or("no bookmarks in this file")?;
    let mut out = Vec::new();
    // The bar's bookmarks at the top; "Other" and "Mobile" in folders
    // named after them, as the browser shows them.
    for (key, root) in roots {
        let folder = match key.as_str() {
            "bookmark_bar" => String::new(),
            _ => root
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(key)
                .to_owned(),
        };
        walk(root, &folder, &mut out);
    }
    Ok(out)
}

fn walk(node: &Value, folder: &str, out: &mut Vec<Imported>) {
    let Some(children) = node.get("children").and_then(Value::as_array) else {
        return;
    };
    for child in children {
        let name = child.get("name").and_then(Value::as_str).unwrap_or("");
        match child.get("type").and_then(Value::as_str) {
            Some("url") => {
                if let Some(url) = child.get("url").and_then(Value::as_str) {
                    out.push(Imported {
                        url: url.to_owned(),
                        title: name.to_owned(),
                        folder: folder.to_owned(),
                    });
                }
            }
            Some("folder") => walk(child, &join(folder, name), out),
            _ => {}
        }
    }
}

fn join(folder: &str, name: &str) -> String {
    let name = name.replace('/', "-");
    if folder.is_empty() {
        name
    } else {
        format!("{folder}/{name}")
    }
}

/// The bookmarks of an exported HTML file (the "Netscape" format every
/// browser writes): `<DT><H3>` opens a folder whose `<DL>` follows,
/// `<DT><A HREF>` is a bookmark.
pub fn parse_html(text: &str) -> Vec<Imported> {
    let mut out = Vec::new();
    // The folder of each list open around the current point (the file's
    // own list and the bookmarks bar's have none: theirs go at the top),
    // and the one named last, whose list comes next.
    let mut stack: Vec<String> = Vec::new();
    let mut named: Option<String> = None;
    let lower = text.to_ascii_lowercase();
    let mut i = 0;
    while let Some(start) = lower[i..].find('<').map(|p| p + i) {
        let Some(end) = lower[start..].find('>').map(|p| p + start) else {
            break;
        };
        let tag = &lower[start + 1..end];
        let name = tag.split_whitespace().next().unwrap_or("");
        i = end + 1;
        match name {
            "h3" => {
                let close = lower[i..].find("</h3>").map_or(text.len(), |p| p + i);
                let bar = tag.contains("personal_toolbar_folder");
                named = Some(if bar {
                    String::new()
                } else {
                    decode(&text[i..close])
                });
                i = close;
            }
            "dl" => stack.push(named.take().unwrap_or_default()),
            "/dl" => {
                stack.pop();
            }
            "a" => {
                let Some(url) = attribute(&text[start + 1..end], "href") else {
                    continue;
                };
                let close = lower[i..].find("</a>").map_or(text.len(), |p| p + i);
                let title = decode(&text[i..close]);
                i = close;
                let folder = stack
                    .iter()
                    .filter(|f| !f.is_empty())
                    .fold(String::new(), |acc, f| join(&acc, f));
                out.push(Imported {
                    url: decode(&url),
                    title,
                    folder,
                });
            }
            _ => {}
        }
    }
    out
}

/// The value of attribute `name` in a tag's text.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let at = lower.find(&format!("{name}="))? + name.len() + 1;
    let rest = &tag[at..];
    let value = match rest.chars().next()? {
        q @ ('"' | '\'') => rest[1..].split(q).next()?,
        _ => rest.split_whitespace().next()?,
    };
    Some(value.to_owned())
}

/// HTML text: entities decoded, blanks collapsed.
fn decode(html: &str) -> String {
    let text = html
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&amp;", "&");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_bookmarks_keep_their_folders() {
        let text = r#"{"roots": {
            "bookmark_bar": {"name": "Bookmarks bar", "type": "folder", "children": [
                {"type": "url", "name": "Rust", "url": "https://rust-lang.org/"},
                {"type": "folder", "name": "Work", "children": [
                    {"type": "url", "name": "Docs", "url": "https://docs.rs/"}
                ]}
            ]},
            "other": {"name": "Other bookmarks", "type": "folder", "children": [
                {"type": "url", "name": "News", "url": "https://lemonde.fr/"}
            ]}
        }}"#;
        let found = parse_chromium(text).unwrap();
        let summary: Vec<(&str, &str)> = found
            .iter()
            .map(|b| (b.title.as_str(), b.folder.as_str()))
            .collect();
        assert_eq!(
            summary,
            [("Rust", ""), ("Docs", "Work"), ("News", "Other bookmarks")]
        );
    }

    #[test]
    fn html_exports_keep_their_folders() {
        let text = r#"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks Menu</H1>
<DL><p>
    <DT><A HREF="https://menu.org/" ADD_DATE="1">In the menu</A>
    <DT><H3 ADD_DATE="1" PERSONAL_TOOLBAR_FOLDER="true">Bookmarks Toolbar</H3>
    <DL><p>
        <DT><A HREF="https://bar.org/?a=1&amp;b=2">On the bar</A>
        <DT><H3>Dev</H3>
        <DL><p>
            <DT><A HREF="https://docs.rs/">Docs &amp; crates</A>
        </DL><p>
    </DL><p>
    <DT><H3>Recipes</H3>
    <DL><p>
        <DT><A HREF='https://food.org/'>Soup</A>
    </DL><p>
</DL>"#;
        let found = parse_html(text);
        let summary: Vec<(&str, &str, &str)> = found
            .iter()
            .map(|b| (b.url.as_str(), b.title.as_str(), b.folder.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("https://menu.org/", "In the menu", ""),
                ("https://bar.org/?a=1&b=2", "On the bar", ""),
                ("https://docs.rs/", "Docs & crates", "Dev"),
                ("https://food.org/", "Soup", "Recipes"),
            ]
        );
    }
}
