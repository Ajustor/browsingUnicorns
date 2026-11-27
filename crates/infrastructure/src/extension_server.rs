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
//!
//! As in Chrome, other origins (web pages, other extensions) only get the
//! files the manifest lists in `web_accessible_resources`, and never the
//! runtime nor the background page; no HTML is ever readable by them (no
//! CORS header on it), so the nonces it carries stay with the extension.
//! Who asks is told by the request's `Origin`, `Referer` and
//! `Sec-Fetch-Site` headers ([`Requester`]); a request with none of them
//! (the browser loading an extension page in a tab, the popup, the
//! background page) is served as the extension's own.

use std::collections::HashMap;
use std::fs;
use std::path::{Component, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use domain::{ExtensionId, Extensions, UrlPattern};
use serde_json::{Map, Value, json};

use crate::extensions::catalog::{ExtensionCatalog, ParsedExtension};
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
    /// Readable by any origin (`Access-Control-Allow-Origin: *`): a web
    /// accessible resource asked by another origin, never HTML.
    pub cors: bool,
}

/// Who asks for an extension's file: the request's headers, as the web
/// view hands them (any may be missing).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Requester {
    /// `Origin`.
    pub origin: Option<String>,
    /// `Referer`.
    pub referer: Option<String>,
    /// `Sec-Fetch-Site`.
    pub fetch_site: Option<String>,
}

/// Where a request comes from, as far as its headers tell.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Initiator {
    /// The extension's own pages.
    Own,
    /// Nothing says: the browser itself (a tab, the popup, the background
    /// page it loads), or a page of the extension hiding its referrer.
    Unknown,
    /// Another origin (its `scheme://host[:port]`, when known).
    Foreign(Option<String>),
}

/// `scheme://host[:port]` of `url`.
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    (!scheme.is_empty() && end > 0).then(|| format!("{scheme}://{}", &rest[..end]))
}

impl Requester {
    fn initiator(&self, origins: Origins, id: &ExtensionId) -> Initiator {
        let own = |origin: &str| {
            origins
                .parse(&format!("{origin}/"))
                .is_some_and(|(of, _)| &of == id)
        };
        let classify = |origin: String| {
            if own(&origin) {
                Initiator::Own
            } else {
                Initiator::Foreign(Some(origin))
            }
        };
        // "null": an opaque origin (a sandboxed frame...); the referrer
        // may still tell.
        if let Some(origin) = self
            .origin
            .as_deref()
            .map(str::trim)
            .filter(|o| !o.is_empty() && *o != "null")
        {
            return classify(origin.to_owned());
        }
        if let Some(origin) = self.referer.as_deref().and_then(origin_of) {
            return classify(origin);
        }
        match self.fetch_site.as_deref().map(str::trim) {
            Some("same-origin") => Initiator::Own,
            Some("cross-site" | "same-site") => Initiator::Foreign(None),
            _ => Initiator::Unknown,
        }
    }
}

/// Whether the manifest lets `initiator` (another origin, when known)
/// load `path` (no leading slash, decoded): `web_accessible_resources`,
/// MV2's list of globs or MV3's objects (`resources` for the pages of
/// `matches`, or of the extensions of `extension_ids`).
fn web_accessible(
    manifest: &Map<String, Value>,
    path: &str,
    initiator: Option<&str>,
    origins: Origins,
) -> bool {
    let Some(entries) = manifest
        .get("web_accessible_resources")
        .and_then(Value::as_array)
    else {
        return false;
    };
    let strings = |value: Option<&Value>| -> Vec<String> {
        value
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let listed = |glob: &str| {
        UrlPattern::Glob(glob.trim().trim_start_matches('/').to_owned())
            .matches(path)
            .unwrap_or(false)
    };
    let initiator_extension = initiator
        .and_then(|o| origins.parse(&format!("{o}/")))
        .map(|(id, _)| id);
    entries.iter().any(|entry| match entry {
        Value::String(glob) => listed(glob),
        Value::Object(o) => {
            if !strings(o.get("resources")).iter().any(|g| listed(g)) {
                return false;
            }
            let by_page = strings(o.get("matches")).iter().any(|m| match initiator {
                Some(origin) => UrlPattern::parse_match(m)
                    .and_then(|p| p.matches(&format!("{origin}/")))
                    .unwrap_or(false),
                None => matches!(m.trim(), "<all_urls>" | "*://*/*"),
            });
            let by_extension = strings(o.get("extension_ids")).iter().any(|e| {
                e == "*"
                    || initiator_extension
                        .as_ref()
                        .is_some_and(|id| id.as_str() == e)
            });
            by_page || by_extension
        }
        _ => false,
    })
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
    /// Their manifests, parsed.
    catalog: Arc<ExtensionCatalog>,
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

pub(crate) fn safe_relative(path: &str) -> Option<PathBuf> {
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
        let dir = extensions_dir.into();
        Arc::new(Self {
            catalog: ExtensionCatalog::for_dir(&dir),
            dir,
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

    /// The installed extensions changed: their manifests are parsed again
    /// when next needed, by everyone sharing the [`ExtensionCatalog`].
    pub fn set_extensions(&self, extensions: &Extensions) {
        *self.extensions.write().unwrap_or_else(|p| p.into_inner()) = extensions.clone();
        self.catalog.installed_changed();
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
        let background = self.catalog.get(id)?.background()?;
        Some(self.origins.url(id, &background_path(&background)))
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

    /// The response to `url` (any extension origin form) for a request
    /// without headers: the browser's own, as the extension's pages get it.
    pub fn serve(&self, url: &str) -> Option<Resource> {
        self.serve_to(url, &Requester::default())
    }

    /// The response to `url` (any extension origin form) for `requester`;
    /// `None`: 404 (missing, or not for this requester).
    pub fn serve_to(&self, url: &str, requester: &Requester) -> Option<Resource> {
        let (id, path) = self.origins.parse(url)?;
        if !self.enabled(&id) {
            return None;
        }
        let bare = path.split(['?', '#']).next().unwrap_or(&path);
        let foreign = match requester.initiator(self.origins, &id) {
            Initiator::Own | Initiator::Unknown => false,
            Initiator::Foreign(origin) => {
                // The runtime and the background page are the extension's
                // alone; the rest only when the manifest says.
                let decoded = percent_decode(bare.trim_start_matches('/'));
                if bare == RUNTIME_PATH
                    || bare.rsplit('/').next() == Some(BACKGROUND_PAGE)
                    || !self.catalog.get(&id).is_some_and(|p| {
                        web_accessible(p.manifest(), &decoded, origin.as_deref(), self.origins)
                    })
                {
                    return None;
                }
                true
            }
        };
        let mut resource = self.resource(&id, &path, bare)?;
        resource.cors = foreign && !resource.mime.starts_with("text/html");
        Some(resource)
    }

    fn resource(&self, id: &ExtensionId, path: &str, bare: &str) -> Option<Resource> {
        let dir = extension_dir(&self.dir, id);
        if bare == RUNTIME_PATH {
            let context = self.take_nonce(query_param(path, "n")?)?;
            let parsed = self.catalog.get(id);
            return Some(Resource {
                mime: mime_type(".js"),
                body: self.runtime(id, parsed.as_deref(), context).into_bytes(),
                cors: false,
            });
        }
        if bare.rsplit('/').next() == Some(BACKGROUND_PAGE)
            && let Some(background) = self
                .catalog
                .get(id)
                .and_then(|p| p.background())
                .filter(|b| background_path(b) == bare)
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
                cors: false,
            });
        }
        let file = dir.join(safe_relative(path)?);
        let body = fs::read(&file).ok()?;
        let mime = mime_type(bare);
        if mime.starts_with("text/html") {
            let html = String::from_utf8_lossy(&body);
            return Some(Resource {
                mime,
                body: inject(&html, &self.runtime_tag(Context::Page)).into_bytes(),
                cors: false,
            });
        }
        Some(Resource {
            mime,
            body,
            cors: false,
        })
    }

    fn runtime(
        &self,
        id: &ExtensionId,
        parsed: Option<&ParsedExtension>,
        context: Context,
    ) -> String {
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
        let (manifest, messages) = match parsed {
            Some(p) => (json!(p.manifest()), json!(p.messages())),
            None => (json!({"name": name, "version": version}), json!({})),
        };
        let config = json!({
            "owner": format!("ext:{id}"),
            "context": match context { Context::Background => "background", Context::Page => "page" },
            "origin": self.origins.origin(id),
            "messages": messages,
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
    use std::path::Path;

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

    fn from_origin(origin: &str) -> Requester {
        Requester {
            origin: Some(origin.into()),
            ..Requester::default()
        }
    }

    fn from_page(url: &str) -> Requester {
        Requester {
            referer: Some(url.into()),
            ..Requester::default()
        }
    }

    #[test]
    fn other_origins_get_the_web_accessible_resources_only() {
        let dir = tempfile::tempdir().unwrap();
        let s = server(dir.path(), true);
        let ext = extension_dir(&dir.path().join("ext"), &id());
        fs::create_dir_all(ext.join("img")).unwrap();
        fs::write(ext.join("img/a.png"), b"png").unwrap();
        fs::write(ext.join("ui/menu.html"), "<html><head></head></html>").unwrap();
        fs::write(
            ext.join("manifest.json"),
            r#"{"name":"T","version":"1","manifest_version":3,
                "background":{"service_worker":"sw.js"},
                "web_accessible_resources":[
                  {"resources":["img/*.png"],"matches":["https://*.bank.test/*"]},
                  {"resources":["ui/menu.html"],"matches":["<all_urls>"]}]}"#,
        )
        .unwrap();
        let url = |p: &str| format!("bu-ext://{ID}{p}");
        let page = "https://www.bank.test";
        let evil = "https://evil.test";

        // The attack: a web page reads the background page, takes its
        // nonce, then the runtime and its token.
        let bg = format!("/{BACKGROUND_PAGE}");
        assert!(s.serve_to(&url(&bg), &from_origin(evil)).is_none());
        assert!(
            s.serve_to(&url(&bg), &from_page("https://evil.test/x"))
                .is_none()
        );
        let own_bg = String::from_utf8(s.serve(&url(&bg)).unwrap().body).unwrap();
        let runtime = url(&format!("/__bu/runtime.js?n={}", nonce_of(&own_bg)));
        assert!(s.serve_to(&runtime, &from_origin(evil)).is_none());
        // The nonce is still unused: the extension's own page gets it.
        let own = from_page(&format!("bu-ext://{ID}/{BACKGROUND_PAGE}"));
        assert!(!s.serve_to(&runtime, &own).unwrap().cors);

        // Not listed: nothing to tell the extension is there.
        assert!(s.serve_to(&url("/sw.js"), &from_origin(evil)).is_none());
        assert!(
            s.serve_to(&url("/ui/popup.html"), &from_origin(evil))
                .is_none()
        );
        let cross_site = Requester {
            fetch_site: Some("cross-site".into()),
            ..Requester::default()
        };
        assert!(s.serve_to(&url("/sw.js"), &cross_site).is_none());
        // Listed for some sites only.
        assert!(s.serve_to(&url("/img/a.png"), &from_origin(evil)).is_none());
        let png = s.serve_to(&url("/img/a.png"), &from_origin(page)).unwrap();
        assert_eq!((png.body.as_slice(), png.cors), (&b"png"[..], true));
        assert!(
            s.serve_to(
                &url("/img/a.png"),
                &from_page("https://www.bank.test/login")
            )
            .is_some()
        );
        // A page for every site, framed (Bitwarden's inline menu), but
        // never readable: its runtime's nonce stays in the frame.
        let menu = s
            .serve_to(&url("/ui/menu.html"), &from_origin(evil))
            .unwrap();
        assert!(!menu.cors);
        assert!(
            String::from_utf8(menu.body)
                .unwrap()
                .contains("runtime.js?n=")
        );

        // The extension's own pages, and the browser's requests, get all.
        assert!(
            s.serve_to(&url("/sw.js"), &from_origin(&format!("bu-ext://{ID}")))
                .is_some()
        );
        assert!(s.serve_to(&url("/sw.js"), &own).is_some());
        let same_origin = Requester {
            origin: Some("null".into()),
            fetch_site: Some("same-origin".into()),
            ..Requester::default()
        };
        assert!(s.serve_to(&url("/sw.js"), &same_origin).is_some());
        assert!(!s.serve(&url("/sw.js")).unwrap().cors);
    }

    #[test]
    fn manifest_v2_lists_globs_for_everyone() {
        let manifest: Value =
            serde_json::from_str(r#"{"web_accessible_resources": ["/fonts/*", "inject.js"]}"#)
                .unwrap();
        let m = manifest.as_object().unwrap();
        let o = Origins::WindowsHttps;
        assert!(web_accessible(
            m,
            "fonts/a/b.woff2",
            Some("https://x.test"),
            o
        ));
        assert!(web_accessible(m, "inject.js", None, o));
        assert!(!web_accessible(
            m,
            "background.js",
            Some("https://x.test"),
            o
        ));
        // MV3: other extensions, by id.
        let manifest: Value = serde_json::from_str(&format!(
            r#"{{"web_accessible_resources": [{{"resources": ["*"], "extension_ids": ["{ID}"]}}]}}"#
        ))
        .unwrap();
        let m = manifest.as_object().unwrap();
        assert!(web_accessible(m, "a.js", Some(&o.origin(&id())), o));
        assert!(!web_accessible(m, "a.js", Some("https://x.test"), o));
        assert!(!web_accessible(m, "a.js", None, o));
    }

    #[test]
    fn disabled_extensions_serve_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let s = server(dir.path(), false);
        assert!(s.serve(&format!("bu-ext://{ID}/sw.js")).is_none());
    }
}
