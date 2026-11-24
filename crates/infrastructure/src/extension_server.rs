//! Serves the files of installed extensions to their own pages (background,
//! popup, options) and to `runtime.getURL`, where the web view has no
//! extension engine. Each extension gets its own origin:
//!
//! - macOS, Linux: `bu-ext://<id>/<path>` (a custom scheme of the web view);
//! - Windows: `https://bu-ext.<id>/<path>` (wry's form of custom schemes;
//!   https, so the pages are a secure context: `crypto.subtle`...);
//! - Android: `https://<id>.bu-ext.invalid/<path>` (`shouldInterceptRequest`).
//!
//! The extension pages' HTML gets the runtime (`chrome.*`) first. The runtime
//! holds the bridge token, so it is only served with a one-time nonce that
//! only the HTML the server just served carries: a web page cannot load it.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use domain::{ExtensionId, Extensions};
use serde_json::{Value, json};

use crate::extensions::{extension_dir, manifest};
use crate::page_scripts::{Bridge, extension_page_runtime};
use crate::script_storage::new_token;

/// The form of extension origins on this web view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origins {
    CustomScheme,
    WindowsHttps,
    Android,
}

/// The custom scheme registered with the desktop web view.
pub const SCHEME: &str = "bu-ext";

impl Origins {
    /// What the desktop web view uses on this platform.
    pub fn desktop() -> Self {
        if cfg!(windows) {
            Self::WindowsHttps
        } else {
            Self::CustomScheme
        }
    }

    /// `scheme://host` of extension `id`, no trailing slash.
    pub fn origin(self, id: &ExtensionId) -> String {
        match self {
            Self::CustomScheme => format!("{SCHEME}://{id}"),
            Self::WindowsHttps => format!("https://{SCHEME}.{id}"),
            Self::Android => format!("https://{id}.{SCHEME}.invalid"),
        }
    }

    /// The address of `path` in extension `id`.
    pub fn url(self, id: &ExtensionId, path: &str) -> String {
        format!("{}/{}", self.origin(id), path.trim_start_matches('/'))
    }

    /// The extension and path (with its query) of an extension URL.
    /// `url` is also accepted in the `bu-ext://<id>/…` form wry hands its
    /// custom protocol handler on every platform.
    pub fn parse(self, url: &str) -> Option<(ExtensionId, String)> {
        let rest = url
            .strip_prefix(&format!("{SCHEME}://"))
            .or_else(|| match self {
                Self::WindowsHttps => url.strip_prefix(&format!("https://{SCHEME}.")),
                Self::Android => url.strip_prefix("https://"),
                Self::CustomScheme => None,
            })?;
        let (host, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        let host = match self {
            Self::Android if !url.starts_with(SCHEME) => {
                host.strip_suffix(&format!(".{SCHEME}.invalid"))?
            }
            _ => host,
        };
        Some((ExtensionId::parse(host)?, path.to_owned()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub mime: &'static str,
    pub body: Vec<u8>,
}

pub fn mime_type(path: &str) -> &'static str {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    match path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("mp3") => "audio/mpeg",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

/// What kind of page of the extension the runtime runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    Background,
    /// Popup, options and other pages of the extension.
    Page,
}

pub struct ExtensionServer {
    dir: PathBuf,
    token: String,
    origins: Origins,
    bridge: Bridge,
    extensions: RwLock<Extensions>,
    /// One-time nonces of the runtime, with their context.
    nonces: Mutex<HashMap<String, (Context, Instant)>>,
    /// Last version each extension's background saw: onInstalled.
    versions_file: PathBuf,
}

/// Nonces not used within this time are dropped.
const NONCE_LIFETIME: Duration = Duration::from_secs(120);
pub const BACKGROUND_PAGE: &str = "__bu_background.html";

/// Where the background page is served: in the folder of the extension's
/// background script (or page). A service worker's relative addresses, and
/// the chunks webpack finds from `self.location`, resolve from there.
fn background_path(background: &manifest::Background) -> String {
    let base = background
        .page
        .as_deref()
        .or(background.scripts.last().map(String::as_str))
        .unwrap_or("")
        .trim_start_matches('/');
    match base.rfind('/') {
        Some(slash) => format!("/{}/{BACKGROUND_PAGE}", &base[..slash]),
        None => format!("/{BACKGROUND_PAGE}"),
    }
}
const RUNTIME_PATH: &str = "/__bu/runtime.js";

fn safe_relative(path: &str) -> Option<PathBuf> {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let decoded = percent_decode(path.trim_start_matches('/'));
    let relative = PathBuf::from(decoded);
    relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(relative)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&text[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_param<'a>(path: &'a str, name: &str) -> Option<&'a str> {
    let query = path.split_once('?')?.1;
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
}

impl ExtensionServer {
    pub fn new(
        extensions_dir: impl Into<PathBuf>,
        versions_file: impl Into<PathBuf>,
        token: String,
        origins: Origins,
        bridge: Bridge,
    ) -> Arc<Self> {
        Arc::new(Self {
            dir: extensions_dir.into(),
            token,
            origins,
            bridge,
            extensions: RwLock::default(),
            nonces: Mutex::default(),
            versions_file: versions_file.into(),
        })
    }

    pub fn origins(&self) -> Origins {
        self.origins
    }

    pub fn set_extensions(&self, extensions: &Extensions) {
        *self.extensions.write().unwrap_or_else(|p| p.into_inner()) = extensions.clone();
    }

    /// The origins of the enabled extensions.
    pub fn enabled_origins(&self) -> Vec<String> {
        self.extensions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .items()
            .iter()
            .filter(|e| e.enabled)
            .map(|e| self.origins.origin(&e.id))
            .collect()
    }

    fn enabled(&self, id: &ExtensionId) -> bool {
        self.extensions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .is_some_and(|e| e.enabled)
    }

    fn nonce(&self, context: Context) -> String {
        let nonce = new_token();
        let mut nonces = self.nonces.lock().unwrap_or_else(|p| p.into_inner());
        nonces.retain(|_, (_, at)| at.elapsed() < NONCE_LIFETIME);
        nonces.insert(nonce.clone(), (context, Instant::now()));
        nonce
    }

    fn take_nonce(&self, nonce: &str) -> Option<Context> {
        let mut nonces = self.nonces.lock().unwrap_or_else(|p| p.into_inner());
        nonces
            .remove(nonce)
            .filter(|(_, at)| at.elapsed() < NONCE_LIFETIME)
            .map(|(c, _)| c)
    }

    /// `<script>` of the runtime, for an HTML page of the extension.
    fn runtime_tag(&self, context: Context) -> String {
        format!(
            "<script src=\"{RUNTIME_PATH}?n={}\"></script>",
            self.nonce(context)
        )
    }

    /// The address of the background page of `id`, if it has one.
    pub fn background_url(&self, id: &ExtensionId) -> Option<String> {
        let dir = extension_dir(&self.dir, id);
        manifest::background(&dir).map(|b| self.origins.url(id, &background_path(&b)))
    }

    /// "install", "update" or nothing, once per version.
    fn install_reason(&self, id: &ExtensionId, version: &str) -> Option<&'static str> {
        let mut seen: HashMap<String, String> = fs::read_to_string(&self.versions_file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let reason = match seen.get(id.as_str()) {
            None => Some("install"),
            Some(v) if v != version => Some("update"),
            Some(_) => None,
        };
        if reason.is_some() {
            seen.insert(id.to_string(), version.to_owned());
            if let Ok(text) = serde_json::to_string(&seen) {
                let _ = crate::files::write(&self.versions_file, &text);
            }
        }
        reason
    }

    /// The response to `url` (any extension origin form); `None`: 404.
    pub fn serve(&self, url: &str) -> Option<Resource> {
        let (id, path) = self.origins.parse(url)?;
        if !self.enabled(&id) {
            return None;
        }
        let dir = extension_dir(&self.dir, &id);
        let bare = path.split(['?', '#']).next().unwrap_or(&path);
        if bare == RUNTIME_PATH {
            let context = self.take_nonce(query_param(&path, "n")?)?;
            return Some(Resource {
                mime: mime_type(".js"),
                body: self.runtime(&id, &dir, context).into_bytes(),
            });
        }
        if bare.rsplit('/').next() == Some(BACKGROUND_PAGE)
            && let Some(background) =
                manifest::background(&dir).filter(|b| background_path(b) == bare)
        {
            let scripts: String = background
                .scripts
                .iter()
                .map(|s| {
                    let kind = if background.module {
                        " type=\"module\""
                    } else {
                        ""
                    };
                    format!(
                        "<script src=\"/{}\"{kind}></script>",
                        s.trim_start_matches('/')
                    )
                })
                .collect();
            let page = match &background.page {
                Some(page) => {
                    let html = fs::read_to_string(dir.join(safe_relative(page)?)).ok()?;
                    inject(&html, &self.runtime_tag(Context::Background))
                }
                None => format!(
                    "<!doctype html><meta charset=\"utf-8\">{}{scripts}",
                    self.runtime_tag(Context::Background)
                ),
            };
            return Some(Resource {
                mime: mime_type(".html"),
                body: page.into_bytes(),
            });
        }
        let file = dir.join(safe_relative(&path)?);
        let body = fs::read(&file).ok()?;
        let mime = mime_type(bare);
        if mime.starts_with("text/html") {
            let html = String::from_utf8_lossy(&body);
            return Some(Resource {
                mime,
                body: inject(&html, &self.runtime_tag(Context::Page)).into_bytes(),
            });
        }
        Some(Resource { mime, body })
    }

    fn runtime(&self, id: &ExtensionId, dir: &Path, context: Context) -> String {
        let (name, version) = self
            .extensions
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .map(|e| (e.name.clone(), e.version.clone()))
            .unwrap_or_default();
        let install = match context {
            Context::Background => self.install_reason(id, &version),
            Context::Page => None,
        };
        let manifest: Value = fs::read_to_string(dir.join("manifest.json"))
            .ok()
            .and_then(|t| serde_json::from_str(t.trim_start_matches('\u{feff}')).ok())
            .unwrap_or_else(|| json!({"name": name, "version": version}));
        let config = json!({
            "owner": format!("ext:{id}"),
            "context": match context { Context::Background => "background", Context::Page => "page" },
            "origin": self.origins.origin(id),
            "messages": Value::Object(manifest::messages(dir)),
            "manifest": manifest,
            "installReason": install,
        });
        extension_page_runtime(&self.token, self.bridge, &config.to_string())
    }
}

/// `tag` at the start of the document's `<head>` (or first thing).
fn inject(html: &str, tag: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let at = lower
        .find("<head")
        .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        .or_else(|| {
            lower
                .find("<html")
                .and_then(|i| lower[i..].find('>').map(|j| i + j + 1))
        })
        .unwrap_or(0);
    format!("{}{tag}{}", &html[..at], &html[at..])
}

#[cfg(test)]
mod tests {
    use domain::Extension;

    use super::*;

    const ID: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

    fn id() -> ExtensionId {
        ExtensionId::parse(ID).unwrap()
    }

    #[test]
    fn origins() {
        for o in [
            Origins::CustomScheme,
            Origins::WindowsHttps,
            Origins::Android,
        ] {
            let url = o.url(&id(), "/popup.html?x=1");
            assert_eq!(
                o.parse(&url),
                Some((id(), "/popup.html?x=1".into())),
                "{url}"
            );
            // wry's handler sees the custom scheme form.
            assert_eq!(
                o.parse(&format!("bu-ext://{ID}/a.js")),
                Some((id(), "/a.js".into()))
            );
        }
        assert_eq!(
            Origins::WindowsHttps.origin(&id()),
            format!("https://bu-ext.{ID}")
        );
        assert_eq!(Origins::Android.parse("https://example.com/"), None);
        assert_eq!(Origins::CustomScheme.parse("https://x/"), None);
    }

    fn server(dir: &Path, enabled: bool) -> Arc<ExtensionServer> {
        let ext = extension_dir(&dir.join("ext"), &id());
        fs::create_dir_all(ext.join("ui")).unwrap();
        fs::write(
            ext.join("manifest.json"),
            r#"{"name":"T","version":"1","background":{"service_worker":"sw.js"}}"#,
        )
        .unwrap();
        fs::write(ext.join("sw.js"), "self.x = 1;").unwrap();
        fs::write(
            ext.join("ui/popup.html"),
            "<html><HEAD><title>p</title></head><body>hi</body></html>",
        )
        .unwrap();
        let s = ExtensionServer::new(
            dir.join("ext"),
            dir.join("versions.json"),
            "tok".into(),
            Origins::CustomScheme,
            Bridge::Ipc,
        );
        s.set_extensions(&Extensions::from_items(vec![Extension {
            id: id(),
            name: "T".into(),
            version: "1".into(),
            description: String::new(),
            enabled,
            popup: Some("ui/popup.html".into()),
            options: None,
        }]));
        s
    }

    fn nonce_of(html: &str) -> String {
        html.split("runtime.js?n=").nth(1).unwrap()[..32].to_owned()
    }

    #[test]
    fn serves_pages_with_the_runtime_once() {
        let dir = tempfile::tempdir().unwrap();
        let s = server(dir.path(), true);
        let url = |p: &str| format!("bu-ext://{ID}{p}");

        let popup = s.serve(&url("/ui/popup.html")).unwrap();
        assert_eq!(popup.mime, "text/html; charset=utf-8");
        let html = String::from_utf8(popup.body).unwrap();
        assert!(
            html.starts_with("<html><HEAD><script src=\"/__bu/runtime.js?n="),
            "{html}"
        );
        let nonce = nonce_of(&html);
        let runtime = s
            .serve(&url(&format!("/__bu/runtime.js?n={nonce}")))
            .unwrap();
        let js = String::from_utf8(runtime.body).unwrap();
        assert!(js.contains("const TOKEN = \"tok\";"));
        assert!(js.contains("\"context\":\"page\""));
        assert!(
            s.serve(&url(&format!("/__bu/runtime.js?n={nonce}")))
                .is_none(),
            "one use"
        );
        assert!(s.serve(&url("/__bu/runtime.js?n=guess")).is_none());

        let bg_page = format!("/{BACKGROUND_PAGE}");
        let bg = String::from_utf8(s.serve(&url(&bg_page)).unwrap().body).unwrap();
        assert!(bg.contains("<script src=\"/sw.js\"></script>"), "{bg}");
        let js = String::from_utf8(
            s.serve(&url(&format!("/__bu/runtime.js?n={}", nonce_of(&bg))))
                .unwrap()
                .body,
        )
        .unwrap();
        assert!(js.contains("\"installReason\":\"install\""));
        // Installed once: no event on the next start.
        let bg = String::from_utf8(s.serve(&url(&bg_page)).unwrap().body).unwrap();
        let js = String::from_utf8(
            s.serve(&url(&format!("/__bu/runtime.js?n={}", nonce_of(&bg))))
                .unwrap()
                .body,
        )
        .unwrap();
        assert!(js.contains("\"installReason\":null"));

        assert_eq!(s.serve(&url("/sw.js")).unwrap().body, b"self.x = 1;");
        assert!(s.serve(&url("/../../versions.json")).is_none());
        assert!(s.serve(&url("/%2e%2e/x")).is_none());
        assert_eq!(
            s.background_url(&id()),
            Some(format!("bu-ext://{ID}/{BACKGROUND_PAGE}"))
        );
    }

    #[test]
    fn the_background_page_is_beside_its_worker() {
        let dir = tempfile::tempdir().unwrap();
        let s = server(dir.path(), true);
        let ext = extension_dir(&dir.path().join("ext"), &id());
        fs::create_dir_all(ext.join("background")).unwrap();
        fs::write(
            ext.join("manifest.json"),
            r#"{"name":"T","version":"1","background":{"service_worker":"background/sw.js"}}"#,
        )
        .unwrap();
        let page = format!("/background/{BACKGROUND_PAGE}");
        assert_eq!(
            s.background_url(&id()),
            Some(format!("bu-ext://{ID}{page}"))
        );
        let bg = String::from_utf8(s.serve(&format!("bu-ext://{ID}{page}")).unwrap().body).unwrap();
        assert!(
            bg.contains("<script src=\"/background/sw.js\"></script>"),
            "{bg}"
        );
        // Only there.
        assert!(
            s.serve(&format!("bu-ext://{ID}/{BACKGROUND_PAGE}"))
                .is_none()
        );
    }

    #[test]
    fn disabled_extensions_serve_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = server(dir.path(), false);
        assert!(s.serve(&format!("bu-ext://{ID}/sw.js")).is_none());
    }
}
