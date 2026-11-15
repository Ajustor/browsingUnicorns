//! Scripts the browser runs in pages: extensions' content scripts and user
//! scripts (the Greasemonkey format). Which pages they run on, and when.

use std::fmt;

/// When a script runs in the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunAt {
    /// Before the page's own scripts.
    DocumentStart,
    /// Once the document is parsed (`DOMContentLoaded`).
    DocumentEnd,
    /// Once the page is loaded, or soon after.
    #[default]
    DocumentIdle,
}

impl RunAt {
    /// `document_start` (extensions) or `document-start` (user scripts).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().replace('-', "_").as_str() {
            "document_start" => Some(Self::DocumentStart),
            "document_end" => Some(Self::DocumentEnd),
            "document_idle" | "document_body" => Some(Self::DocumentIdle),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DocumentStart => "document_start",
            Self::DocumentEnd => "document_end",
            Self::DocumentIdle => "document_idle",
        }
    }
}

/// Which addresses a script runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlPattern {
    /// A Chrome match pattern: `<all_urls>`, `*://*.example.com/path*`.
    Match {
        /// `None`: http and https.
        scheme: Option<String>,
        /// `None`: any host; `Some((domain, subdomains))`.
        host: Option<(String, bool)>,
        path: String,
    },
    /// Any URL (`<all_urls>`, `*`).
    All,
    /// A glob on the whole address (user scripts' `@include`).
    Glob(String),
    /// A regular expression (`@include /…/`), only checked in the page.
    Regex(String),
}

fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    // Iterative `*` matching with backtracking on the last star.
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if p < pattern.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else if let Some((sp, st)) = star {
            p = sp + 1;
            t = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == b'*')
}

fn escape_js_regex(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if "\\^$.|?+()[]{}/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn glob_js_regex(glob: &str) -> String {
    glob.split('*')
        .map(escape_js_regex)
        .collect::<Vec<_>>()
        .join(".*")
}

impl UrlPattern {
    /// A match pattern (`matches` of a manifest, `@match` of a user script).
    pub fn parse_match(pattern: &str) -> Option<Self> {
        let pattern = pattern.trim();
        if pattern == "<all_urls>" {
            return Some(Self::All);
        }
        let (scheme, rest) = pattern.split_once("://")?;
        let scheme = match scheme {
            "*" => None,
            "http" | "https" | "file" | "ftp" | "ws" | "wss" => Some(scheme.to_owned()),
            _ => return None,
        };
        let (host, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let host = match host {
            "*" => None,
            h if h.starts_with("*.") => Some((h[2..].to_ascii_lowercase(), true)),
            h if h.contains('*') => return None,
            h => Some((h.to_ascii_lowercase(), false)),
        };
        Some(Self::Match {
            scheme,
            host,
            path: path.to_owned(),
        })
    }

    /// A user script's `@include` / `@exclude`: a glob, or `/regex/`.
    pub fn parse_include(pattern: &str) -> Option<Self> {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return None;
        }
        if pattern == "*" {
            return Some(Self::All);
        }
        if pattern.len() > 2 && pattern.starts_with('/') && pattern.ends_with('/') {
            return Some(Self::Regex(pattern[1..pattern.len() - 1].to_owned()));
        }
        Some(Self::Glob(pattern.to_owned()))
    }

    /// Whether `url` matches; `None` for a regex (checked in the page only).
    pub fn matches(&self, url: &str) -> Option<bool> {
        match self {
            Self::All => Some(true),
            Self::Regex(_) => None,
            Self::Glob(g) => Some(glob_match(g.as_bytes(), url.as_bytes())),
            Self::Match { scheme, host, path } => {
                let Some((url_scheme, rest)) = url.split_once("://") else {
                    return Some(false);
                };
                let scheme_ok = match scheme {
                    None => matches!(url_scheme, "http" | "https"),
                    Some(s) => s == url_scheme,
                };
                let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                let url_host = rest[..end]
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                let url_path = if end < rest.len() { &rest[end..] } else { "/" };
                let url_path = url_path.split('#').next().unwrap_or(url_path);
                let host_ok = match host {
                    None => true,
                    Some((h, false)) => &url_host == h,
                    Some((h, true)) => {
                        &url_host == h
                            || (url_host.len() > h.len()
                                && url_host.ends_with(h.as_str())
                                && url_host.as_bytes()[url_host.len() - h.len() - 1] == b'.')
                    }
                };
                Some(scheme_ok && host_ok && glob_match(path.as_bytes(), url_path.as_bytes()))
            }
        }
    }

    /// The pattern as a JavaScript regular expression literal, tested in
    /// the page against `location.href`.
    pub fn to_js_regex(&self) -> String {
        match self {
            Self::All => "/^(https?|file|ftp|wss?):/".into(),
            Self::Regex(r) => format!("/{r}/"),
            Self::Glob(g) => format!("/^{}$/", glob_js_regex(g)),
            Self::Match { scheme, host, path } => {
                let scheme = match scheme {
                    None => "https?".to_owned(),
                    Some(s) => escape_js_regex(s),
                };
                let host = match host {
                    None => "[^/?#]*".to_owned(),
                    Some((h, false)) => format!("{}(:\\d+)?", escape_js_regex(h)),
                    Some((h, true)) => format!("([^/?#]*\\.)?{}(:\\d+)?", escape_js_regex(h)),
                };
                format!("/^{scheme}:\\/\\/{host}{}(#.*)?$/i", glob_js_regex(path))
            }
        }
    }
}

/// A user script's id: letters, digits and `-`, from its name and namespace.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserScriptId(String);

impl UserScriptId {
    pub fn parse(id: &str) -> Option<Self> {
        (!id.is_empty()
            && id.len() <= 64
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'))
        .then(|| Self(id.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UserScriptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The `==UserScript==` block of a user script.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserScriptMeta {
    pub name: String,
    pub namespace: String,
    pub version: String,
    pub description: String,
    pub matches: Vec<UrlPattern>,
    pub excludes: Vec<UrlPattern>,
    pub run_at: RunAt,
    pub grants: Vec<String>,
    /// Scripts it needs, loaded before it (`@require`).
    pub requires: Vec<String>,
    pub no_frames: bool,
}

impl UserScriptMeta {
    /// `None` when the code has no metadata block or no name.
    pub fn parse(code: &str) -> Option<Self> {
        let mut meta = Self {
            // Greasemonkey's default for user scripts.
            run_at: RunAt::DocumentEnd,
            ..Self::default()
        };
        let mut inside = false;
        for line in code.lines() {
            let line = line.trim();
            if line.starts_with("// ==UserScript==") {
                inside = true;
                continue;
            }
            if line.starts_with("// ==/UserScript==") {
                break;
            }
            if !inside {
                continue;
            }
            let Some(entry) = line.strip_prefix("//") else {
                continue;
            };
            let Some(entry) = entry.trim_start().strip_prefix('@') else {
                continue;
            };
            let (key, value) = match entry.split_once(char::is_whitespace) {
                Some((k, v)) => (k, v.trim()),
                None => (entry, ""),
            };
            match key {
                "name" if meta.name.is_empty() => meta.name = value.to_owned(),
                "namespace" => meta.namespace = value.to_owned(),
                "version" => meta.version = value.to_owned(),
                "description" if meta.description.is_empty() => {
                    meta.description = value.to_owned();
                }
                "match" => meta.matches.extend(UrlPattern::parse_match(value)),
                "include" => meta.matches.extend(UrlPattern::parse_include(value)),
                "exclude" => meta.excludes.extend(UrlPattern::parse_include(value)),
                "exclude-match" => meta.excludes.extend(UrlPattern::parse_match(value)),
                "run-at" => meta.run_at = RunAt::parse(value).unwrap_or(meta.run_at),
                "grant" if !value.is_empty() => meta.grants.push(value.to_owned()),
                "require" if !value.is_empty() => meta.requires.push(value.to_owned()),
                "noframes" => meta.no_frames = true,
                _ => {}
            }
        }
        (inside && !meta.name.is_empty()).then_some(meta)
    }

    /// Whether the script runs on `url` (regexes count as a match).
    pub fn runs_on(&self, url: &str) -> bool {
        self.matches.iter().any(|p| p.matches(url) != Some(false))
            && !self.excludes.iter().any(|p| p.matches(url) == Some(true))
    }
}

/// An installed user script; its code is kept by the infrastructure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserScript {
    pub id: UserScriptId,
    pub meta: UserScriptMeta,
    /// Where it was installed from (updates).
    pub source_url: String,
    pub enabled: bool,
}

/// The installed user scripts, in installation order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserScripts {
    items: Vec<UserScript>,
}

impl UserScripts {
    pub fn from_items(items: Vec<UserScript>) -> Self {
        let mut scripts = Self::default();
        for s in items {
            scripts.install(s);
        }
        scripts
    }

    pub fn items(&self) -> &[UserScript] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: &UserScriptId) -> Option<&UserScript> {
        self.items.iter().find(|s| &s.id == id)
    }

    /// Adds `script`, or updates it in place (keeping it on or off).
    pub fn install(&mut self, mut script: UserScript) {
        match self.items.iter_mut().find(|s| s.id == script.id) {
            Some(existing) => {
                script.enabled = existing.enabled;
                *existing = script;
            }
            None => self.items.push(script),
        }
    }

    pub fn remove(&mut self, id: &UserScriptId) -> bool {
        let before = self.items.len();
        self.items.retain(|s| &s.id != id);
        self.items.len() != before
    }

    pub fn set_enabled(&mut self, id: &UserScriptId, enabled: bool) -> bool {
        match self.items.iter_mut().find(|s| &s.id == id) {
            Some(s) if s.enabled != enabled => {
                s.enabled = enabled;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str) -> UrlPattern {
        UrlPattern::parse_match(p).unwrap()
    }

    #[test]
    fn match_patterns() {
        assert_eq!(m("<all_urls>").matches("https://a.b/"), Some(true));
        assert_eq!(
            m("*://*.example.com/*").matches("https://example.com/x"),
            Some(true)
        );
        assert_eq!(
            m("*://*.example.com/*").matches("http://a.example.com:8080/x"),
            Some(true)
        );
        assert_eq!(
            m("*://*.example.com/*").matches("https://badexample.com/"),
            Some(false)
        );
        assert_eq!(
            m("*://*.example.com/*").matches("ftp://example.com/"),
            Some(false)
        );
        assert_eq!(
            m("https://example.com/news/*").matches("https://example.com/news/1?x"),
            Some(true)
        );
        assert_eq!(
            m("https://example.com/news/*").matches("https://example.com/other"),
            Some(false)
        );
        assert_eq!(
            m("https://example.com/*").matches("https://example.com"),
            Some(true)
        );
        assert!(UrlPattern::parse_match("https://ex*ample.com/").is_none());
        assert!(UrlPattern::parse_match("chrome://settings/*").is_none());
    }

    #[test]
    fn includes() {
        let g = UrlPattern::parse_include("https://*.wikipedia.org/wiki/*").unwrap();
        assert_eq!(g.matches("https://fr.wikipedia.org/wiki/Rust"), Some(true));
        assert_eq!(g.matches("https://fr.wikipedia.org/w/index"), Some(false));
        let r = UrlPattern::parse_include("/^https?://x\\.org/").unwrap();
        assert_eq!(r.matches("https://x.org/"), None);
        assert_eq!(r.to_js_regex(), "/^https?://x\\.org/");
    }

    #[test]
    fn js_regexes() {
        assert_eq!(
            m("*://*.example.com/a*").to_js_regex(),
            "/^https?:\\/\\/([^/?#]*\\.)?example\\.com(:\\d+)?\\/a.*(#.*)?$/i"
        );
        assert_eq!(
            UrlPattern::parse_include("https://x.org/*")
                .unwrap()
                .to_js_regex(),
            "/^https:\\/\\/x\\.org\\/.*$/"
        );
    }

    #[test]
    fn user_script_metadata() {
        let code = "// ==UserScript==\n\
            // @name        Wide Wikipedia\n\
            // @namespace   https://example.org\n\
            // @version     1.2\n\
            // @description Full width articles\n\
            // @match       https://*.wikipedia.org/wiki/*\n\
            // @include     /^https://example\\.org/\n\
            // @exclude     https://*.wikipedia.org/wiki/Special:*\n\
            // @run-at      document-start\n\
            // @grant       GM_addStyle\n\
            // @grant       GM.getValue\n\
            // @require     https://cdn.example/lib.js\n\
            // @noframes\n\
            // ==/UserScript==\n\
            GM_addStyle('body{max-width:none}');";
        let meta = UserScriptMeta::parse(code).unwrap();
        assert_eq!(meta.name, "Wide Wikipedia");
        assert_eq!(meta.version, "1.2");
        assert_eq!(meta.matches.len(), 2);
        assert_eq!(meta.excludes.len(), 1);
        assert_eq!(meta.run_at, RunAt::DocumentStart);
        assert_eq!(meta.grants, vec!["GM_addStyle", "GM.getValue"]);
        assert_eq!(meta.requires, vec!["https://cdn.example/lib.js"]);
        assert!(meta.no_frames);
        assert!(meta.runs_on("https://fr.wikipedia.org/wiki/Rust"));
        assert!(!meta.runs_on("https://fr.wikipedia.org/wiki/Special:Random"));
        assert!(meta.runs_on("https://example.org/x"), "a regex might match");
        assert!(UserScriptMeta::parse("alert(1)").is_none());
        assert_eq!(
            UserScriptMeta::parse("// ==UserScript==\n// @name x\n// ==/UserScript==")
                .unwrap()
                .run_at,
            RunAt::DocumentEnd
        );
    }

    #[test]
    fn user_script_list() {
        let script = |enabled| UserScript {
            id: UserScriptId::parse("wide-wikipedia-1a2b").unwrap(),
            meta: UserScriptMeta {
                name: "x".into(),
                ..UserScriptMeta::default()
            },
            source_url: "https://greasyfork.org/x.user.js".into(),
            enabled,
        };
        let mut list = UserScripts::default();
        list.install(script(true));
        let id = list.items()[0].id.clone();
        assert!(list.set_enabled(&id, false));
        list.install(script(true));
        assert!(!list.items()[0].enabled, "an update keeps it off");
        assert!(list.remove(&id));
        assert!(UserScriptId::parse("Bad Id").is_none());
    }
}
