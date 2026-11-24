//! What other browsers hold: Chromium's (Chrome, Edge, Brave, Vivaldi) and
//! Firefox's bookmarks and history read from their profile, the HTML
//! bookmarks file every browser exports (Safari...), and the CSV file of
//! passwords they export.

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

/// A page of another browser's history: its last visit (seconds since the
/// Unix epoch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedVisit {
    pub url: String,
    pub title: String,
    pub at: u64,
}

/// A sign-in from an exported passwords file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedLogin {
    pub url: String,
    pub username: String,
    pub password: String,
}

/// How a browser keeps its profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `Bookmarks` (JSON) and `History` (SQLite).
    Chromium,
    /// `places.sqlite`: bookmarks and history.
    Firefox,
}

/// A browser found on this computer, whose bookmarks and history can be
/// imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub name: &'static str,
    pub kind: Kind,
    /// Its (default) profile folder.
    pub profile: PathBuf,
}

/// The browsers installed here with a profile: Chromium's, then Firefox.
pub fn sources() -> Vec<Source> {
    let mut found = chromium_sources();
    found.extend(firefox_source());
    found
}

/// The Chromium browsers installed here with bookmarks or a history
/// (default profile).
fn chromium_sources() -> Vec<Source> {
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
            let mut profile = base.clone();
            profile.extend(parts.iter());
            profile.push("Default");
            let found = profile.join("Bookmarks").is_file() || profile.join("History").is_file();
            found.then_some(Source {
                name,
                kind: Kind::Chromium,
                profile,
            })
        })
        .collect()
}

/// Firefox's profile in use, if Firefox is installed here.
fn firefox_source() -> Option<Source> {
    let root = if cfg!(windows) {
        dirs::data_dir()?.join("Mozilla").join("Firefox")
    } else if cfg!(target_os = "macos") {
        dirs::data_dir()?.join("Firefox")
    } else {
        dirs::home_dir()?.join(".mozilla").join("firefox")
    };
    let ini = std::fs::read_to_string(root.join("profiles.ini")).ok()?;
    let (path, relative) = firefox_profile(&ini)?;
    let profile = if relative {
        root.join(path)
    } else {
        PathBuf::from(path)
    };
    profile.join("places.sqlite").is_file().then_some(Source {
        name: "Firefox",
        kind: Kind::Firefox,
        profile,
    })
}

/// The profile Firefox opens, from its `profiles.ini`: the one an
/// installation uses (`[Install…] Default=`), else the one marked
/// `Default=1`, else the first. Its path, and whether it is relative to
/// the file.
fn firefox_profile(ini: &str) -> Option<(String, bool)> {
    type Section = (String, Vec<(String, String)>);
    let mut sections: Vec<Section> = Vec::new();
    for line in ini.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_owned(), Vec::new()));
        } else if let (Some((k, v)), Some(s)) = (line.split_once('='), sections.last_mut()) {
            s.1.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let get = |s: &Section, key: &str| s.1.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    // (path, relative, marked default)
    let profiles: Vec<(String, bool, bool)> = sections
        .iter()
        .filter(|s| s.0.starts_with("Profile"))
        .filter_map(|s| {
            let path = get(s, "Path")?;
            let relative = get(s, "IsRelative").is_none_or(|v| v == "1");
            Some((path, relative, get(s, "Default").as_deref() == Some("1")))
        })
        .collect();
    let installed = sections
        .iter()
        .filter(|s| s.0.starts_with("Install"))
        .find_map(|s| get(s, "Default"));
    if let Some(path) = installed {
        let relative = profiles.iter().find(|p| p.0 == path).is_none_or(|p| p.1);
        return Some((path, relative));
    }
    profiles
        .iter()
        .find(|p| p.2)
        .or(profiles.first())
        .map(|p| (p.0.clone(), p.1))
}

/// The bookmarks of a browser found here.
pub fn read_bookmarks(source: &Source) -> Result<Vec<Imported>, String> {
    match source.kind {
        Kind::Chromium => {
            let file = source.profile.join("Bookmarks");
            if !file.is_file() {
                return Ok(Vec::new());
            }
            read_chromium(&file)
        }
        Kind::Firefox => sqlite::firefox_bookmarks(&source.profile.join("places.sqlite")),
    }
}

/// The history of a browser found here: each page once, at its last
/// visit, the most recent first (at most what this browser keeps).
pub fn read_history(source: &Source) -> Result<Vec<ImportedVisit>, String> {
    let limit = domain::BrowsingHistory::CAPACITY;
    match source.kind {
        Kind::Chromium => {
            let file = source.profile.join("History");
            if !file.is_file() {
                return Ok(Vec::new());
            }
            sqlite::chromium_history(&file, limit)
        }
        Kind::Firefox => sqlite::firefox_history(&source.profile.join("places.sqlite"), limit),
    }
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
        .ok_or_else(i18n::errors::infrastructure::no_bookmarks_in_file)?;
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

/// The sign-ins of a passwords file exported as CSV (Chrome, Edge, Firefox,
/// Bitwarden...): its columns are found by their header.
pub fn parse_passwords_csv(text: &str) -> Result<Vec<ImportedLogin>, String> {
    let mut rows = csv_rows(text.trim_start_matches('\u{feff}')).into_iter();
    let header: Vec<String> = rows
        .next()
        .unwrap_or_default()
        .iter()
        .map(|h| h.trim().to_ascii_lowercase())
        .collect();
    let column = |names: &[&str]| header.iter().position(|h| names.contains(&h.as_str()));
    let (Some(url), Some(user), Some(pass)) = (
        column(&["url", "login_uri", "website", "origin", "uri"]),
        column(&["username", "login_username", "user", "login", "email"]),
        column(&["password", "login_password"]),
    ) else {
        return Err(i18n::errors::infrastructure::no_passwords_in_file().to_owned());
    };
    Ok(rows
        .filter_map(|row| {
            let address = row.get(url)?.trim();
            let password = row.get(pass)?;
            if address.is_empty() || password.is_empty() {
                return None;
            }
            Some(ImportedLogin {
                url: address.to_owned(),
                username: row
                    .get(user)
                    .map(|u| u.trim().to_owned())
                    .unwrap_or_default(),
                password: password.clone(),
            })
        })
        .collect())
}

/// The rows of a CSV text (RFC 4180: quoted fields may hold commas, quotes
/// doubled and line breaks). Blank lines are skipped.
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            (true, '"') => quoted = false,
            (true, c) => field.push(c),
            (false, '"') => quoted = true,
            (false, ',') => row.push(std::mem::take(&mut field)),
            (false, '\r') => {}
            (false, '\n') => {
                row.push(std::mem::take(&mut field));
                if row.iter().any(|f| !f.is_empty()) {
                    rows.push(std::mem::take(&mut row));
                }
                row.clear();
            }
            (false, c) => field.push(c),
        }
    }
    row.push(field);
    if row.iter().any(|f| !f.is_empty()) {
        rows.push(row);
    }
    rows
}

/// Chromium's times: microseconds since 1601.
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
fn from_chromium_time(t: i64) -> u64 {
    const UNIX_EPOCH_SINCE_1601: i64 = 11_644_473_600;
    (t / 1_000_000 - UNIX_EPOCH_SINCE_1601).max(0) as u64
}

/// Firefox's places: its bookmarks' roots (by GUID) and the folder their
/// bookmarks go in (the toolbar's at the top, as the bar's of Chromium).
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
fn firefox_root_folder(guid: &str) -> Option<String> {
    use i18n::errors::infrastructure as t;
    match guid {
        "toolbar_____" => Some(String::new()),
        "menu________" => Some(t::firefox_menu().to_owned()),
        "unfiled_____" => Some(t::firefox_other().to_owned()),
        "mobile______" => Some(t::firefox_mobile().to_owned()),
        _ => None,
    }
}

/// A row of Firefox's `moz_bookmarks`, with its address.
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
struct Place {
    id: i64,
    /// 1: a bookmark, 2: a folder.
    kind: i64,
    parent: i64,
    position: i64,
    title: String,
    guid: String,
    url: Option<String>,
}

/// Firefox's bookmarks from its `moz_bookmarks` rows, folders kept.
#[cfg_attr(not(feature = "desktop"), allow(dead_code))]
fn firefox_tree(mut places: Vec<Place>) -> Vec<Imported> {
    places.sort_by_key(|p| (p.parent, p.position));
    let mut out = Vec::new();
    let roots: Vec<(i64, String)> = places
        .iter()
        .filter_map(|p| Some((p.id, firefox_root_folder(&p.guid)?)))
        .collect();
    // Toolbar first, then the menu, other and mobile bookmarks.
    let order = [
        "toolbar_____",
        "menu________",
        "unfiled_____",
        "mobile______",
    ];
    let mut roots: Vec<(usize, i64, String)> = roots
        .into_iter()
        .map(|(id, folder)| {
            let guid = &places.iter().find(|p| p.id == id).map(|p| p.guid.as_str());
            let rank = order
                .iter()
                .position(|g| Some(*g) == *guid)
                .unwrap_or(order.len());
            (rank, id, folder)
        })
        .collect();
    roots.sort();
    fn walk(places: &[Place], parent: i64, folder: &str, out: &mut Vec<Imported>, depth: usize) {
        if depth > 64 {
            return;
        }
        for p in places.iter().filter(|p| p.parent == parent) {
            match (p.kind, &p.url) {
                (1, Some(url)) if !url.starts_with("place:") => out.push(Imported {
                    url: url.clone(),
                    title: p.title.clone(),
                    folder: folder.to_owned(),
                }),
                (2, _) => walk(places, p.id, &join(folder, &p.title), out, depth + 1),
                _ => {}
            }
        }
    }
    for (_, id, folder) in roots {
        walk(&places, id, &folder, &mut out, 0);
    }
    out
}

/// Reading other browsers' SQLite files (desktop only).
#[cfg(feature = "desktop")]
mod sqlite {
    use std::path::{Path, PathBuf};

    use rusqlite::{Connection, OpenFlags};

    use super::{Imported, ImportedVisit, Place};

    /// Runs `query` on the database `file` of a browser that may be open:
    /// read as it is on disk, without locking it. When it has a write-ahead
    /// log (Firefox), a copy of both is read, for the latest changes.
    fn read<T>(
        file: &Path,
        query: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, String> {
        let failed = |e: &dyn std::fmt::Display| {
            i18n::errors::infrastructure::cannot_read_browser_data(&file.display(), e)
        };
        let wal = PathBuf::from(format!("{}-wal", file.display()));
        if wal.metadata().is_ok_and(|m| m.len() > 0) {
            let dir = std::env::temp_dir().join(format!(
                "browsingUnicorns-import-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            ));
            let result = (|| {
                std::fs::create_dir_all(&dir).map_err(|e| failed(&e))?;
                let copy = dir.join("db.sqlite");
                std::fs::copy(file, &copy).map_err(|e| failed(&e))?;
                std::fs::copy(&wal, dir.join("db.sqlite-wal")).map_err(|e| failed(&e))?;
                let db = Connection::open(&copy).map_err(|e| failed(&e))?;
                let out = query(&db).map_err(|e| failed(&e));
                drop(db);
                out
            })();
            let _ = std::fs::remove_dir_all(&dir);
            return result;
        }
        let db = Connection::open_with_flags(
            uri(file),
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| failed(&e))?;
        query(&db).map_err(|e| failed(&e))
    }

    /// `file` as an SQLite URI, opened as an unchanging file (no lock).
    fn uri(file: &Path) -> String {
        let path = file.to_string_lossy().replace('\\', "/");
        let mut out = String::from(if path.starts_with('/') {
            "file://"
        } else {
            "file:///"
        });
        for b in path.bytes() {
            match b {
                b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'/'
                | b':'
                | b'-'
                | b'_'
                | b'.'
                | b'~' => out.push(b as char),
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out.push_str("?immutable=1");
        out
    }

    pub fn chromium_history(file: &Path, limit: usize) -> Result<Vec<ImportedVisit>, String> {
        read(file, |db| {
            let mut statement = db.prepare(
                "SELECT url, title, last_visit_time FROM urls \
                 WHERE hidden = 0 AND last_visit_time > 0 \
                 ORDER BY last_visit_time DESC LIMIT ?1",
            )?;
            let rows = statement.query_map([limit as i64], |r| {
                Ok(ImportedVisit {
                    url: r.get(0)?,
                    title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    at: super::from_chromium_time(r.get(2)?),
                })
            })?;
            rows.collect()
        })
    }

    pub fn firefox_history(file: &Path, limit: usize) -> Result<Vec<ImportedVisit>, String> {
        read(file, |db| {
            let mut statement = db.prepare(
                "SELECT url, title, last_visit_date FROM moz_places \
                 WHERE hidden = 0 AND last_visit_date IS NOT NULL \
                 ORDER BY last_visit_date DESC LIMIT ?1",
            )?;
            let rows = statement.query_map([limit as i64], |r| {
                Ok(ImportedVisit {
                    url: r.get(0)?,
                    title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    at: (r.get::<_, i64>(2)? / 1_000_000).max(0) as u64,
                })
            })?;
            rows.collect()
        })
    }

    pub fn firefox_bookmarks(file: &Path) -> Result<Vec<Imported>, String> {
        let places = read(file, |db| {
            let mut statement = db.prepare(
                "SELECT b.id, b.type, b.parent, b.position, b.title, b.guid, p.url \
                 FROM moz_bookmarks b LEFT JOIN moz_places p ON p.id = b.fk",
            )?;
            let rows = statement.query_map([], |r| {
                Ok(Place {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    parent: r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    position: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                    title: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    guid: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    url: r.get(6)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        Ok(super::firefox_tree(places))
    }
}

/// Without SQLite (Android): nothing to read.
#[cfg(not(feature = "desktop"))]
mod sqlite {
    use std::path::Path;

    use super::{Imported, ImportedVisit};

    fn unavailable() -> String {
        "not available here".to_owned()
    }

    pub fn chromium_history(_: &Path, _: usize) -> Result<Vec<ImportedVisit>, String> {
        Err(unavailable())
    }

    pub fn firefox_history(_: &Path, _: usize) -> Result<Vec<ImportedVisit>, String> {
        Err(unavailable())
    }

    pub fn firefox_bookmarks(_: &Path) -> Result<Vec<Imported>, String> {
        Err(unavailable())
    }
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

    #[test]
    fn firefox_opens_the_installs_profile() {
        let ini = "[Install308046B0AF4A39CB]\nDefault=Profiles/abc.default-release\nLocked=1\n\n\
                   [Profile1]\nName=default\nIsRelative=1\nPath=Profiles/xyz.default\nDefault=1\n\n\
                   [Profile0]\nName=default-release\nIsRelative=1\nPath=Profiles/abc.default-release\n";
        assert_eq!(
            firefox_profile(ini),
            Some(("Profiles/abc.default-release".to_owned(), true))
        );
        let ini = "[Profile0]\nName=a\nIsRelative=1\nPath=Profiles/a\n\n\
                   [Profile1]\nName=b\nIsRelative=0\nPath=/home/me/b\nDefault=1\n";
        assert_eq!(firefox_profile(ini), Some(("/home/me/b".to_owned(), false)));
        assert_eq!(firefox_profile("[General]\nVersion=2\n"), None);
    }

    #[test]
    fn firefox_bookmarks_keep_their_folders() {
        let place =
            |id, kind, parent, position, title: &str, guid: &str, url: Option<&str>| Place {
                id,
                kind,
                parent,
                position,
                title: title.to_owned(),
                guid: guid.to_owned(),
                url: url.map(str::to_owned),
            };
        let found = firefox_tree(vec![
            place(1, 2, 0, 0, "", "root________", None),
            place(2, 2, 1, 0, "menu", "menu________", None),
            place(3, 2, 1, 1, "toolbar", "toolbar_____", None),
            place(10, 1, 2, 0, "In the menu", "a", Some("https://menu.org/")),
            place(11, 1, 3, 1, "Second", "b", Some("https://second.org/")),
            place(12, 1, 3, 0, "First", "c", Some("https://first.org/")),
            place(13, 2, 3, 2, "Dev", "d", None),
            place(14, 1, 13, 0, "Docs", "e", Some("https://docs.rs/")),
            place(15, 1, 3, 3, "Recent", "f", Some("place:sort=8")),
        ]);
        let summary: Vec<(&str, &str)> = found
            .iter()
            .map(|b| (b.title.as_str(), b.folder.as_str()))
            .collect();
        let menu = i18n::errors::infrastructure::firefox_menu();
        assert_eq!(
            summary,
            [
                ("First", ""),
                ("Second", ""),
                ("Docs", "Dev"),
                ("In the menu", menu)
            ]
        );
    }

    #[test]
    fn chromium_times_are_since_1601() {
        // 2024-01-01T00:00:00Z.
        assert_eq!(from_chromium_time(13_348_540_800_000_000), 1_704_067_200);
        assert_eq!(from_chromium_time(0), 0);
    }

    #[test]
    fn password_files_of_each_browser() {
        // Chrome and Edge.
        let chrome = "name,url,username,password,note\n\
                      site,https://site.org/login,me@mail.org,\"p,a\"\"ss\",\n\
                      empty,https://nopass.org/,me,,\n";
        assert_eq!(
            parse_passwords_csv(chrome).unwrap(),
            [ImportedLogin {
                url: "https://site.org/login".into(),
                username: "me@mail.org".into(),
                password: "p,a\"ss".into(),
            }]
        );
        // Firefox, with a byte order mark and Windows line ends.
        let firefox = "\u{feff}\"url\",\"username\",\"password\",\"httpRealm\"\r\n\
                       \"https://a.org\",\"\",\"secret\",\"\"\r\n";
        let found = parse_passwords_csv(firefox).unwrap();
        assert_eq!(found[0].url, "https://a.org");
        assert_eq!(found[0].username, "");
        // Bitwarden: a line break inside a note.
        let bitwarden = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
                         ,,login,B,\"two\nlines\",,0,https://b.org,bob,pw,\n";
        let found = parse_passwords_csv(bitwarden).unwrap();
        assert_eq!(
            (
                found[0].url.as_str(),
                found[0].username.as_str(),
                found[0].password.as_str()
            ),
            ("https://b.org", "bob", "pw")
        );
        assert!(parse_passwords_csv("a,b\n1,2\n").is_err());
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn history_is_read_from_a_chromium_database() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("History");
        let db = rusqlite::Connection::open(&file).unwrap();
        db.execute_batch(
            "CREATE TABLE urls (id INTEGER PRIMARY KEY, url TEXT, title TEXT, \
             visit_count INTEGER, last_visit_time INTEGER, hidden INTEGER);
             INSERT INTO urls VALUES (1, 'https://old.org/', 'Old', 1, 13348540800000000, 0);
             INSERT INTO urls VALUES (2, 'https://new.org/', NULL, 3, 13348540860000000, 0);
             INSERT INTO urls VALUES (3, 'https://hidden.org/', 'H', 1, 13348540900000000, 1);",
        )
        .unwrap();
        drop(db);
        let found = sqlite::chromium_history(&file, 10).unwrap();
        let summary: Vec<(&str, &str, u64)> = found
            .iter()
            .map(|v| (v.url.as_str(), v.title.as_str(), v.at))
            .collect();
        assert_eq!(
            summary,
            [
                ("https://new.org/", "", 1_704_067_260),
                ("https://old.org/", "Old", 1_704_067_200)
            ]
        );
        assert_eq!(sqlite::chromium_history(&file, 1).unwrap().len(), 1);
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn firefox_places_are_read_with_their_write_ahead_log() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("places.sqlite");
        // Firefox open: its latest changes still in the log.
        let db = rusqlite::Connection::open(&file).unwrap();
        db.pragma_update(None, "journal_mode", "wal").unwrap();
        db.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
        db.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, title TEXT, \
             hidden INTEGER, last_visit_date INTEGER);
             CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER, \
             parent INTEGER, position INTEGER, title TEXT, guid TEXT);
             INSERT INTO moz_places VALUES (1, 'https://a.org/', 'A', 0, 1704067200000000);
             INSERT INTO moz_places VALUES (2, 'https://never.org/', 'N', 0, NULL);
             INSERT INTO moz_bookmarks VALUES (1, 2, NULL, 0, 0, '', 'root________');
             INSERT INTO moz_bookmarks VALUES (2, 2, NULL, 1, 0, 'toolbar', 'toolbar_____');
             INSERT INTO moz_bookmarks VALUES (3, 1, 2, 2, 0, 'Never', 'x');",
        )
        .unwrap();
        assert!(
            dir.path()
                .join("places.sqlite-wal")
                .metadata()
                .unwrap()
                .len()
                > 0
        );
        let history = sqlite::firefox_history(&file, 10).unwrap();
        assert_eq!(
            history,
            [ImportedVisit {
                url: "https://a.org/".into(),
                title: "A".into(),
                at: 1_704_067_200
            }]
        );
        let bookmarks = sqlite::firefox_bookmarks(&file).unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].url, "https://never.org/");
        drop(db);
    }
}
