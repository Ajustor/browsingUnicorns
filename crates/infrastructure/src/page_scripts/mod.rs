//! The scripts the browser injects into pages, built from the extensions'
//! content scripts and the user scripts.
//!
//! Each content script entry and each user script becomes one injected
//! script: a copy of [`RUNTIME`] (bridge to Rust, `chrome.*`, `GM_*`) and the
//! code, wrapped in a function so nothing leaks to the page. They are
//! injected separately so a script that does not parse breaks only itself;
//! each one checks the address and runs at its `run_at`.

use std::fs;
use std::path::Path;

use domain::{Extensions, UrlPattern, UserScripts};
use serde_json::{Value, json};

use crate::extension_server::Origins;
use crate::extensions::state::ExtensionState;
use crate::extensions::{extension_dir, manifest};
use crate::script_storage::new_token;
use crate::user_scripts::read_code;

pub mod credentials;

const RUNTIME: &str = include_str!("runtime.js");

/// Starts every injected script: platforms find theirs by it.
pub const MARKER: &str = "/*browsing-unicorns page script*/";

/// How the scripts reach Rust.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bridge {
    /// wry's `window.ipc.postMessage`; replies come through `__buReply`.
    Ipc,
    /// The Android app's `BUBridge` JavaScript interface (synchronous).
    Android,
}

impl Bridge {
    fn name(self) -> &'static str {
        match self {
            Self::Ipc => "ipc",
            Self::Android => "android",
        }
    }
}

pub struct Sources<'a> {
    pub extensions_dir: &'a Path,
    /// `None` where the web view runs extensions itself (WebView2).
    pub extensions: Option<&'a Extensions>,
    pub user_scripts_dir: &'a Path,
    pub user_scripts: &'a UserScripts,
    pub token: &'a str,
    pub bridge: Bridge,
    /// Where the extensions' files are served (`runtime.getURL`).
    pub origins: Origins,
    /// What extensions registered at run time.
    pub state_dir: Option<&'a Path>,
    /// Add the ad blocker's cosmetic filters (asked from the hub).
    pub blocker: bool,
    /// Add the password manager's script (reports the sign-ins sent).
    pub credentials: bool,
}

/// A JavaScript expression, true in the extensions' own pages wherever
/// they are served ([`Origins`]): as Chrome on `chrome-extension://`, no
/// page script runs there (they would take the messages meant for the
/// extension's page, a password manager's popup opened in a tab).
const IN_EXTENSION_PAGE: &str = r#"(location.protocol === "bu-ext:" || /^bu-ext\.[a-p]{32}$|^[a-p]{32}\.bu-ext\.invalid$/.test(location.hostname))"#;

/// `script` (starting with [`MARKER`]), but not in an extension's page.
fn outside_extension_pages(script: String) -> String {
    match script.strip_prefix(MARKER) {
        Some(rest) => format!("{MARKER}if (!{IN_EXTENSION_PAGE}) {rest}"),
        None => script,
    }
}

/// The owner the ad blocker's page script speaks as.
pub const BLOCKER_OWNER: &str = "blocker";

/// `re("source", "flags")` of a pattern.
fn regex(pattern: &UrlPattern) -> String {
    let literal = pattern.to_js_regex();
    let (source, flags) = literal[1..].rsplit_once('/').unwrap_or((&literal[1..], ""));
    format!("re({}, {})", json!(source), json!(flags))
}

fn regexes(patterns: &[UrlPattern]) -> String {
    format!(
        "[{}]",
        patterns.iter().map(regex).collect::<Vec<_>>().join(", ")
    )
}

fn wrap(token: &str, bridge: Bridge, call: &str) -> String {
    format!(
        "{MARKER}(function () {{\nconst TOKEN = {};\nconst CHANNEL = {};\nconst BRIDGE = {};\n{RUNTIME}\n{call}\n}})();\n",
        json!(token),
        json!(new_token()),
        json!(bridge.name()),
    )
}

/// What an extension runs in a tab (`scripting.executeScript`): `body` (a
/// function body) with the extension's `chrome.*`, as its content scripts
/// get it. An expression whose value is what `body` returns. `config` is
/// the extension's `owner`, `origin`, `messages` and `manifest`, as JSON.
pub fn injected_script(token: &str, bridge: Bridge, config: &str, body: &str) -> String {
    format!(
        "(function () {{\nconst TOKEN = {};\nconst CHANNEL = {};\nconst BRIDGE = {};\n{RUNTIME}\n\
         return injected({config}, function (chrome, browser) {{\n{body}\n}});\n}})()\n\
         //# sourceURL={EXTENSION_SCRIPT}-injected.js\n",
        json!(token),
        json!(new_token()),
        json!(bridge.name()),
    )
}

/// The name an extension's scripts run under in a page (`//# sourceURL`):
/// the runtime knows a field value they set by it (see `isolateFieldValues`
/// in `runtime.js`).
const EXTENSION_SCRIPT: &str = "bu-extension-script";

/// The runtime of an extension page (background, popup, options), with
/// its configuration (`owner`, `context`, `origin`, `messages`, `manifest`,
/// `installReason`) as JSON.
pub fn extension_page_runtime(token: &str, bridge: Bridge, config: &str) -> String {
    wrap(token, bridge, &format!("extensionPage({config});"))
}

/// The files' contents, one after the other, `separator` between them.
fn read_files(root: &Path, files: &[String], separator: &str) -> Option<String> {
    let mut out = String::new();
    for f in files {
        let relative = f.trim_start_matches('/');
        if relative.split(['/', '\\']).any(|part| part == "..") {
            return None;
        }
        out.push_str(&fs::read_to_string(root.join(relative)).ok()?);
        out.push_str(separator);
    }
    Some(out)
}

/// The scripts to inject into every page, in order.
pub fn page_scripts(sources: &Sources<'_>) -> Vec<String> {
    let mut out = Vec::new();
    if sources.blocker {
        out.push(wrap(sources.token, sources.bridge, "blockerCosmetic();"));
    }
    if sources.credentials {
        out.push(credentials::capture_script(sources.bridge));
    }
    for e in sources
        .extensions
        .map(|list| list.items())
        .unwrap_or_default()
        .iter()
        .filter(|e| e.enabled)
    {
        let dir = extension_dir(sources.extensions_dir, &e.id);
        let mut entries = manifest::content_scripts(&dir);
        if let Some(state_dir) = sources.state_dir {
            entries.extend(ExtensionState::load(state_dir, &e.id).registered_scripts());
        }
        if entries.is_empty() {
            continue;
        }
        let messages = Value::Object(manifest::messages(&dir));
        for cs in entries {
            let (Some(js), Some(css_text)) = (
                read_files(&dir, &cs.js, "\n;\n"),
                read_files(&dir, &cs.css, "\n"),
            ) else {
                log::warn!("extensions: content script files of {} missing", e.name);
                continue;
            };
            let css: Vec<String> = if cs.css.is_empty() {
                Vec::new()
            } else {
                vec![css_text]
            };
            let config = format!(
                "{{owner: {}, origin: {}, match: {}, exclude: {}, runAt: {}, allFrames: {}, css: {}, messages: {}, manifest: {}}}",
                json!(format!("ext:{}", e.id)),
                json!(sources.origins.origin(&e.id)),
                regexes(&cs.matches),
                regexes(&cs.excludes),
                json!(cs.run_at.as_str()),
                cs.all_frames,
                json!(css),
                messages,
                json!({"name": e.name, "version": e.version}),
            );
            let code = if cs.js.is_empty() {
                "null".to_owned()
            } else {
                format!("function (chrome, browser) {{\n{js}\n}}")
            };
            let mut script = wrap(
                sources.token,
                sources.bridge,
                &format!("contentScript({config}, {code});"),
            );
            script.push_str(&format!("//# sourceURL={EXTENSION_SCRIPT}-{}.js\n", e.id));
            out.push(script);
        }
    }
    for s in sources.user_scripts.items().iter().filter(|s| s.enabled) {
        let Some((required, code)) = read_code(sources.user_scripts_dir, &s.id) else {
            log::warn!("user scripts: code of {} missing", s.meta.name);
            continue;
        };
        let m = &s.meta;
        let config = format!(
            "{{owner: {}, match: {}, exclude: {}, runAt: {}, noFrames: {}, grants: {}, info: {}, handlerVersion: {}}}",
            json!(format!("us:{}", s.id)),
            regexes(&m.matches),
            regexes(&m.excludes),
            json!(m.run_at.as_str()),
            m.no_frames,
            json!(m.grants),
            json!({"name": m.name, "namespace": m.namespace, "version": m.version, "description": m.description}),
            json!(env!("CARGO_PKG_VERSION")),
        );
        out.push(wrap(
            sources.token,
            sources.bridge,
            &format!(
                "userScript({config}, function (GM, GM_info, GM_addStyle, GM_getValue, GM_setValue, \
                 GM_deleteValue, GM_listValues, GM_xmlhttpRequest, GM_openInTab, GM_setClipboard, \
                 GM_registerMenuCommand, GM_unregisterMenuCommand, GM_notification, GM_log, unsafeWindow) {{\n\
                 {required}\n{code}\n}});"
            ),
        ));
    }
    out.into_iter().map(outside_extension_pages).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use domain::{Extension, ExtensionId};

    use super::*;
    use crate::user_scripts::UserScriptFolder;

    #[test]
    fn scripts_of_extensions_and_user_scripts() {
        let root = tempfile::tempdir().unwrap();
        let id = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        let dir = extension_dir(&root.path().join("ext"), &id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("manifest.json"),
            r#"{"name":"Dark","content_scripts":[{"matches":["<all_urls>"],"js":["c.js"],"css":["c.css"]}]}"#,
        )
        .unwrap();
        fs::write(
            dir.join("c.js"),
            "document.body.dataset.dark = '1';\n\
             chrome.storage.local.set({a: 41}).then(() => chrome.storage.local.get('a'))\
             .then((r) => { document.body.dataset.a = r.a + 1; });",
        )
        .unwrap();
        fs::write(dir.join("c.css"), "body{background:#000}").unwrap();
        let extensions = Extensions::from_items(vec![Extension {
            id,
            name: "Dark".into(),
            version: "1".into(),
            description: String::new(),
            enabled: true,
            popup: None,
            options: None,
        }]);

        let us_dir = root.path().join("us");
        let folder = UserScriptFolder::new(&us_dir, Arc::new(|_: &str| Err("offline".into())));
        let script = folder
            .save(
                "https://x/wide.user.js",
                "// ==UserScript==\n// @name Wide\n// @match https://*.wikipedia.org/*\n// @include /^https://x\\.org//\n// @grant GM_setValue\n// @grant GM_getValue\n// ==/UserScript==\n\
                 GM_addStyle('x');\n\
                 GM_setValue('n', GM_getValue('n', 0) + 1);\n\
                 document.body.dataset.n = GM_getValue('n');",
            )
            .unwrap();
        let user_scripts = UserScripts::from_items(vec![script]);

        let ext_dir = root.path().join("ext");
        let sources = |with_extensions: bool| Sources {
            extensions_dir: &ext_dir,
            extensions: with_extensions.then_some(&extensions),
            user_scripts_dir: &us_dir,
            user_scripts: &user_scripts,
            token: "tok",
            bridge: Bridge::Ipc,
            origins: Origins::CustomScheme,
            state_dir: None,
            blocker: false,
            credentials: false,
        };
        let scripts = page_scripts(&sources(true));
        // `node --check` them: BU_DUMP_PAGE_SCRIPTS=<dir> cargo test …
        if let Ok(out) = std::env::var("BU_DUMP_PAGE_SCRIPTS") {
            for (i, s) in scripts.iter().enumerate() {
                fs::write(Path::new(&out).join(format!("script{i}.js")), s).unwrap();
            }
        }
        assert_eq!(scripts.len(), 2);
        assert!(scripts.iter().all(|s| s.starts_with(MARKER)));
        assert!(
            scripts[0].contains("contentScript({owner: \"ext:ddkjiahejlhfcafbddmgiahcphecmpfh\"")
        );
        assert!(scripts[0].contains("document.body.dataset.dark"));
        assert!(scripts[0].contains("body{background:#000}"));
        assert!(scripts[1].contains("userScript({owner: \"us:wide-"));
        assert!(
            scripts[1].contains(r#"re("^https://x\\.org/", "")"#),
            "{}",
            scripts[1]
        );
        assert!(scripts[1].contains("const TOKEN = \"tok\";"));
        // WebView2 runs the extensions itself: only the user script.
        assert_eq!(page_scripts(&sources(false)).len(), 1);
        // The ad blocker's cosmetic filters come first.
        let with_blocker = page_scripts(&Sources {
            blocker: true,
            credentials: false,
            ..sources(false)
        });
        assert_eq!(with_blocker.len(), 2);
        assert!(with_blocker[0].contains("\nblockerCosmetic();\n"));
    }

    #[test]
    fn no_page_script_runs_in_the_extensions_own_pages() {
        // As Chrome on chrome-extension://: Bitwarden's popup opened in a
        // tab (https://bu-ext.<id> on Windows) got Bitwarden's own content
        // scripts, which took its background's messages.
        let root = tempfile::tempdir().unwrap();
        let scripts = page_scripts(&Sources {
            extensions_dir: root.path(),
            extensions: None,
            user_scripts_dir: root.path(),
            user_scripts: &UserScripts::default(),
            token: "tok",
            bridge: Bridge::Ipc,
            origins: Origins::WindowsHttps,
            state_dir: None,
            blocker: true,
            credentials: true,
        });
        assert_eq!(scripts.len(), 2);
        let guard = format!("{MARKER}if (!{IN_EXTENSION_PAGE}) ");
        assert!(scripts.iter().all(|s| s.starts_with(&guard)));

        let node = std::process::Command::new("node")
            .arg("-e")
            .arg(format!(
                "const cases = [\
                   ['https:', 'bu-ext.nngceckbapebfimnlniiiahkandclblb', true],\
                   ['https:', 'nngceckbapebfimnlniiiahkandclblb.bu-ext.invalid', true],\
                   ['bu-ext:', 'nngceckbapebfimnlniiiahkandclblb', true],\
                   ['https:', 'www.paypal.com', false],\
                   ['https:', 'bu-ext.example.com', false],\
                   ['http:', 'bu-ext.test', false]];\
                 for (const [protocol, hostname, expected] of cases) {{\
                   const location = {{ protocol, hostname }};\
                   if ({IN_EXTENSION_PAGE} !== expected) {{ console.log(hostname); process.exit(1); }}\
                 }}"
            ))
            .output();
        // Without node, the shape above is all that is checked.
        if let Ok(out) = node {
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
        }
    }
}
