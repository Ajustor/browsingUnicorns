use std::borrow::Cow;
#[cfg(windows)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(windows)]
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use application::ports::{
    EngineError, EngineEvent, ExtensionSupport, PageSource, Viewport, WebEngine,
};
use domain::{ExtensionId, Extensions, TabId, UserScripts, WebAddress};
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::http::Response;
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebContext, WebView, WebViewBuilder};

use crate::ParentWindow;
use crate::blocker::BlockerHandle;
use crate::extension_server::{ExtensionServer, Origins, SCHEME};
use crate::page_injection::{self, Handles};
use crate::page_scripts::{Bridge, Sources, page_scripts};

/// A key pressed with its modifiers; `key` is named like egui names it
/// (`T`, `Tab`, `Left`, `F5`...).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub key: String,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// A popup's natural width; `settled` once its page is loaded and two
/// measures agreed.
#[derive(Debug, Clone, Copy)]
struct PanelWidth {
    width: u32,
    settled: bool,
}

/// Called after an event is sent, so the UI wakes up to read it.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Where the engine keeps and finds things.
pub struct EngineFiles {
    /// What the pages store (cookies, cache...). It must be writable: by
    /// default WebView2 writes next to the executable, which fails once
    /// installed in Program Files.
    pub data_dir: PathBuf,
    /// The unpacked extensions (`<id>/` each).
    pub extensions_dir: PathBuf,
    /// The user scripts' code.
    pub user_scripts_dir: PathBuf,
    /// What extensions change at run time (rules, registered scripts).
    pub state_dir: PathBuf,
    /// Written into the page scripts, checked by the script storage.
    pub page_token: String,
    /// A private window: on macOS, where WKWebView ignores `data_dir`, the
    /// views keep their data in memory only.
    pub private: bool,
}

/// Whether the browser runs extensions itself (every platform, by
/// default). `BU_EXTENSIONS=native` hands them to WebView2 on Windows
/// instead: its own engine, but no popup next to the page and no way to
/// tell an extension which tab is active.
pub fn builtin_extensions() -> bool {
    !cfg!(windows) || std::env::var("BU_EXTENSIONS").map_or(true, |v| v != "native")
}

/// [`WebEngine`] backed by the system web view (WebView2 on Windows,
/// WKWebView on macOS, WebKitGTK on Linux), one child view per tab, plus
/// hidden views for the extensions' background pages and one for the popup
/// shown in the side panel.
///
/// Must live on the UI thread, like the window it is embedded in.
pub struct WryEngine {
    parent: ParentWindow,
    /// This device's preferences: download folder, passwords, zoom.
    device: std::rc::Rc<std::cell::RefCell<domain::DeviceSettings>>,
    views: HashMap<TabId, WebView>,
    shown: Option<TabId>,
    viewport: Viewport,
    events: Sender<EngineEvent>,
    waker: Waker,
    /// Where the web views keep cookies, cache and storage.
    context: WebContext,
    files: EngineFiles,
    /// Serves the extensions' files where the browser runs them itself.
    server: Option<Arc<ExtensionServer>>,
    /// Installed extensions and user scripts, for the page scripts.
    extensions: Extensions,
    user_scripts: UserScripts,
    /// The scripts injected into every page, and each view's handles.
    page_scripts: Vec<String>,
    injected: HashMap<TabId, Handles>,
    /// Background pages, by extension.
    backgrounds: HashMap<ExtensionId, WebView>,
    /// The extension popup, and its address.
    panel: Option<(String, WebView)>,
    /// Its content's size in physical pixels, as last measured.
    panel_size: Arc<std::sync::Mutex<Option<(u32, u32)>>>,
    panel_measured: Option<std::time::Instant>,
    /// Where the popup was last placed (moved only when it changes).
    panel_area: Option<Viewport>,
    /// The popup's natural width as last measured, and whether it settled.
    panel_width: Arc<std::sync::Mutex<Option<PanelWidth>>>,
    /// Extensions to hand to the web view profile once a view exists.
    #[cfg_attr(not(windows), allow(dead_code))]
    pending_extensions: Option<Extensions>,
    /// macOS, Linux: the extensions' network rules as WebKit content rules.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    rule_lists: crate::webkit_rules::RuleLists,
    /// The extensions handed to the profile so far: the only ones removed
    /// from it when the user removes them.
    #[cfg(windows)]
    managed_extensions: std::collections::HashSet<String>,
    /// Windows: the downloads under way, for their progress.
    #[cfg(windows)]
    downloads: crate::webview2_downloads::Operations,
    /// Windows: each tab's frames, whose page scripts WebView2 keeps apart.
    #[cfg(windows)]
    frames: HashMap<TabId, crate::webview2_frames::Frames>,
    /// Windows, extensions run by the browser: their network rules, and
    /// each tab's page (for the rules about the page making a request).
    #[cfg(windows)]
    blocking_rules: Arc<std::sync::RwLock<domain::RuleSet>>,
    #[cfg(windows)]
    tab_pages: HashMap<TabId, std::rc::Rc<std::cell::RefCell<String>>>,
    /// The built-in ad blocker.
    blocker: BlockerHandle,
    /// macOS, Linux: when its WebKit rules must be made again.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    blocker_changes: crate::blocker::BlockerChanges,
    /// Windows: what the ad blocker stopped on each tab's page.
    #[cfg(windows)]
    blocked: HashMap<TabId, Rc<std::cell::Cell<u32>>>,
    /// Windows: the browser's shortcuts, taken from the pages' keys, and
    /// those pressed since the last look.
    #[cfg(windows)]
    shortcuts: std::rc::Rc<std::cell::RefCell<std::collections::HashSet<KeyPress>>>,
    #[cfg(windows)]
    pressed: std::rc::Rc<std::cell::RefCell<Vec<KeyPress>>>,
    /// Windows: a page took the keyboard since the last look.
    #[cfg(windows)]
    page_focused: Rc<std::cell::Cell<bool>>,
    /// Windows: each tab's connection security, as last told.
    #[cfg(windows)]
    security: HashMap<TabId, Rc<RefCell<Option<crate::certificates::Certificates>>>>,
    /// Windows: each tab's ad blocker scriptlets.
    #[cfg(windows)]
    scriptlets: HashMap<TabId, crate::webview2_scriptlets::Scriptlets>,
    /// Windows: where the browser's tooltips and menus fall on the page,
    /// and the cut last made (tab, its viewport, its holes).
    #[cfg(windows)]
    page_holes: Vec<Viewport>,
    #[cfg(windows)]
    page_cut: Option<(TabId, Viewport, Vec<Viewport>)>,
}

fn engine_error(e: wry::Error) -> EngineError {
    EngineError(e.to_string())
}

/// Where a download goes: `suggested` (the Downloads folder), the folder
/// chosen instead, or where the user says when asked (`None`: cancelled).
fn download_target(
    device: &domain::DeviceSettings,
    suggested: &std::path::Path,
) -> Option<std::path::PathBuf> {
    let name = suggested.file_name()?.to_owned();
    let dir = device
        .download_dir
        .clone()
        .or_else(|| suggested.parent().map(std::path::Path::to_path_buf));
    if device.ask_where_to_save {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save as")
            .set_file_name(name.to_string_lossy());
        if let Some(dir) = &dir {
            dialog = dialog.set_directory(dir);
        }
        return dialog.save_file();
    }
    match &device.download_dir {
        // Rebuilt from its parts: `C:/x` becomes `C:\x`, which WebView2
        // wants (a download into a path with `/` fails).
        Some(dir) => Some(free_name(
            &dir.components().collect::<std::path::PathBuf>(),
            &name,
        )),
        None => Some(suggested.to_path_buf()),
    }
}

/// `name` in `dir`, or `name (1)`, `name (2)`... when it is taken.
fn free_name(dir: &std::path::Path, name: &std::ffi::OsStr) -> std::path::PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let path = std::path::Path::new(name);
    let stem = path.file_stem().unwrap_or(name).to_string_lossy();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (1..)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|p| !p.exists())
        .expect("a free name")
}

fn bounds(v: Viewport) -> Rect {
    Rect {
        position: PhysicalPosition::new(v.x, v.y).into(),
        size: PhysicalSize::new(v.width.max(1), v.height.max(1)).into(),
    }
}

/// Reloads `view`'s page, fetching everything again.
#[cfg(windows)]
fn reload_from_origin(view: &WebView) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2CallDevToolsProtocolMethodCompletedHandler;
    use windows_core::HSTRING;
    use wry::WebViewExtWindows as _;
    unsafe {
        view.webview().CallDevToolsProtocolMethod(
            &HSTRING::from("Page.reload"),
            &HSTRING::from(r#"{"ignoreCache":true}"#),
            None::<&ICoreWebView2CallDevToolsProtocolMethodCompletedHandler>,
        )
    }
    .map_err(|e| e.to_string())
}

/// Reloads `view`'s page, fetching everything again.
#[cfg(target_os = "macos")]
fn reload_from_origin(view: &WebView) -> Result<(), String> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use wry::WebViewExtMacOS as _;
    let webview = view.webview();
    // -[WKWebView reloadFromOrigin]: end-to-end revalidation.
    let _navigation: *mut AnyObject = unsafe { msg_send![&*webview, reloadFromOrigin] };
    Ok(())
}

/// Reloads `view`'s page, fetching everything again.
#[cfg(target_os = "linux")]
fn reload_from_origin(view: &WebView) -> Result<(), String> {
    use webkit2gtk::WebViewExt as _;
    use wry::WebViewExtUnix as _;
    view.webview().reload_bypass_cache();
    Ok(())
}

/// The parts every view shares: the profile, the extensions' files, the
/// page scripts' bridge.
fn base_builder<'a>(
    context: &'a mut WebContext,
    server: Option<&Arc<ExtensionServer>>,
    private: bool,
    source: PageSource,
    emit: impl Fn(EngineEvent) + 'static,
) -> WebViewBuilder<'a> {
    // WebKitGTK registers schemes once per context.
    #[cfg(target_os = "linux")]
    let register = !context.is_custom_protocol_registered(SCHEME);
    #[cfg(not(target_os = "linux"))]
    let register = true;
    let builder = WebViewBuilder::new_with_web_context(context);
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows as _;
        // https://bu-ext.<id>: the extensions' pages are a secure context
        // (crypto.subtle...), as in Chrome.
        builder
            .with_browser_extensions_enabled(!builtin_extensions())
            .with_https_scheme(true)
    };
    // WKWebView uses the app's default data store whatever the context
    // says: a private window's views get a non-persistent one.
    #[cfg(target_os = "macos")]
    let builder = builder.with_incognito(private);
    #[cfg(not(target_os = "macos"))]
    let _ = private;
    let builder = builder
        .with_devtools(true)
        .with_ipc_handler(move |request| {
            emit(EngineEvent::PageMessage {
                from: source.clone(),
                body: request.into_body(),
            });
        });
    match server {
        Some(server) if register => {
            let server = server.clone();
            builder.with_custom_protocol(SCHEME.into(), move |_id, request| {
                let url = request.uri().to_string();
                match server.serve(&url) {
                    Some(resource) => Response::builder()
                        .header("Content-Type", resource.mime)
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Cow::Owned(resource.body)),
                    None => Response::builder()
                        .status(404)
                        .body(Cow::Borrowed(&b"not found"[..])),
                }
                .unwrap_or_else(|_| Response::new(Cow::Borrowed(&b""[..])))
            })
        }
        _ => builder,
    }
}

impl WryEngine {
    pub fn new(
        parent: ParentWindow,
        files: EngineFiles,
        events: Sender<EngineEvent>,
        waker: Waker,
        blocker: BlockerHandle,
    ) -> Self {
        #[cfg(target_os = "linux")]
        if let Err(e) = gtk::init() {
            log::error!("GTK initialisation failed: {e}");
        }
        let server = builtin_extensions().then(|| {
            ExtensionServer::new(
                files.extensions_dir.clone(),
                files.state_dir.join("versions.json"),
                files.page_token.clone(),
                Origins::desktop(),
                Bridge::Ipc,
            )
        });
        Self {
            parent,
            views: HashMap::new(),
            device: Default::default(),
            shown: None,
            viewport: Viewport::default(),
            events,
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            rule_lists: crate::webkit_rules::RuleLists::new(
                files.extensions_dir.clone(),
                files.state_dir.clone(),
                files.data_dir.join("content-filters"),
                waker.clone(),
            ),
            waker,
            context: WebContext::new(Some(files.data_dir.clone())),
            files,
            server,
            extensions: Extensions::default(),
            user_scripts: UserScripts::default(),
            page_scripts: Vec::new(),
            injected: HashMap::new(),
            backgrounds: HashMap::new(),
            panel: None,
            panel_size: Arc::default(),
            panel_measured: None,
            panel_area: None,
            panel_width: Arc::default(),
            pending_extensions: None,
            #[cfg(windows)]
            managed_extensions: Default::default(),
            #[cfg(windows)]
            downloads: Default::default(),
            #[cfg(windows)]
            frames: HashMap::new(),
            #[cfg(windows)]
            blocking_rules: Arc::default(),
            #[cfg(windows)]
            tab_pages: HashMap::new(),
            blocker,
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            blocker_changes: Default::default(),
            #[cfg(windows)]
            blocked: HashMap::new(),
            #[cfg(windows)]
            shortcuts: Default::default(),
            #[cfg(windows)]
            pressed: Default::default(),
            #[cfg(windows)]
            page_focused: Default::default(),
            #[cfg(windows)]
            security: HashMap::new(),
            #[cfg(windows)]
            scriptlets: HashMap::new(),
            #[cfg(windows)]
            page_holes: Vec::new(),
            #[cfg(windows)]
            page_cut: None,
        }
    }

    /// Cuts the shown page where the browser's tooltips and menus fall
    /// (the page left whole without them), when that changed.
    #[cfg(windows)]
    fn cut_page(&mut self) {
        use wry::WebViewExtWindows as _;
        let wanted = self
            .shown
            .filter(|_| !self.page_holes.is_empty())
            .map(|tab| (tab, self.viewport, self.page_holes.clone()));
        if wanted == self.page_cut {
            return;
        }
        // Another tab, or none: the one cut before is made whole again.
        if let Some((old, ..)) = self.page_cut
            && wanted.as_ref().is_none_or(|(tab, ..)| *tab != old)
            && let Some(view) = self.views.get(&old)
        {
            crate::webview2_region::cut(view.hwnd(), self.viewport, &[]);
        }
        if let Some((tab, viewport, holes)) = &wanted
            && let Some(view) = self.views.get(tab)
        {
            crate::webview2_region::cut(view.hwnd(), *viewport, holes);
        }
        // Only what was cut: a view that appears later is cut then.
        self.page_cut = wanted.filter(|(tab, ..)| self.views.contains_key(tab));
    }

    /// Advances the platform's own event loop where the web view needs it
    /// (GTK on Linux), and hands compiled content rules to the views. Call
    /// once per UI frame.
    pub fn pump(&mut self) {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            // New lists or settings: the ad blocker's rules again.
            if self.blocker_changes.changed(&self.blocker) {
                let (policy, dir) = (self.blocker.policy(), crate::blocker_dir());
                self.rule_lists
                    .update_blocker(move || crate::blocker::webkit_rules_in(&dir?, &policy));
            }
            if let Some(lists) = self.rule_lists.poll() {
                for view in self.views.values() {
                    crate::webkit_rules::RuleLists::apply(view, &lists);
                }
            }
        }
        // A page loaded before the ad blocker's lists: again with them.
        #[cfg(windows)]
        for scriptlets in self.scriptlets.values() {
            scriptlets.catch_up();
        }
        self.measure_panel();
    }

    /// Measures the popup's content now and then (popups change size).
    fn measure_panel(&mut self) {
        let Some((_, view)) = &self.panel else {
            return;
        };
        if self
            .panel_measured
            .is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(400))
        {
            return;
        }
        self.panel_measured = Some(std::time::Instant::now());
        // While it loads: the width the content wants (no wrapping), found
        // by laying the page out at `max-content` for a moment. Once the
        // page is loaded and two such widths agree, only its height at that
        // width: reading it changes nothing (doing the width trick all the
        // time made pages relayout, and blink).
        const MEASURE: &str = "(() => { const d = document.documentElement; if (!d) return '';            const old = d.style.width; d.style.width = 'max-content';            const w = Math.ceil(d.getBoundingClientRect().width); d.style.width = old;            const h = Math.ceil(Math.max(d.scrollHeight, document.body ? document.body.scrollHeight : 0));            const r = window.devicePixelRatio || 1;            const ready = document.readyState === 'complete' ? 1 : 0;            return [Math.min(Math.max(w, 120), 800) * r, Math.min(Math.max(h, 40), 600) * r, ready].map(Math.round).join(','); })()";
        const HEIGHT: &str = "(() => { const d = document.documentElement; if (!d) return '';            const h = Math.ceil(Math.max(d.scrollHeight, document.body ? document.body.scrollHeight : 0));            return String(Math.round(Math.min(Math.max(h, 40), 600) * (window.devicePixelRatio || 1))); })()";
        let size = self.panel_size.clone();
        let settled = self.panel_width.clone();
        let known_width = settled
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .filter(|w| w.settled)
            .map(|w| w.width);
        let script = if known_width.is_some() {
            HEIGHT
        } else {
            MEASURE
        };
        let waker = self.waker.clone();
        let _ = view.evaluate_script_with_callback(script, move |result| {
            let text = result.trim_matches('"');
            let parts: Vec<u32> = text
                .split(',')
                .filter_map(|p| p.parse::<u32>().ok())
                .collect();
            let measured = match (known_width, parts.as_slice()) {
                (Some(w), [h]) => Some((w, *h)),
                (None, [w, h, ready]) => {
                    let mut width = settled.lock().unwrap_or_else(|p| p.into_inner());
                    let same = width.is_some_and(|last| last.width == *w);
                    *width = Some(PanelWidth {
                        width: *w,
                        settled: same && *ready == 1,
                    });
                    Some((*w, *h))
                }
                _ => None,
            };
            if let Some(measured) = measured {
                let mut slot = size.lock().unwrap_or_else(|p| p.into_inner());
                if *slot != Some(measured) {
                    *slot = Some(measured);
                    waker();
                }
            }
        });
    }

    /// The popup's content size in physical pixels, once measured.
    pub fn panel_size(&self) -> Option<(u32, u32)> {
        *self.panel_size.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn emitter(&self) -> impl Fn(EngineEvent) + Clone + 'static {
        let events = self.events.clone();
        let waker = self.waker.clone();
        move |event| {
            // The receiver only goes away when the window closes.
            if events.send(event).is_ok() {
                waker();
            }
        }
    }

    /// Hands the extensions waiting for a view to the web view profile.
    #[cfg(windows)]
    fn apply_extensions(&mut self) {
        use wry::WebViewExtWindows as _;

        if builtin_extensions() {
            return;
        }
        let Some(view) = self.views.values().next() else {
            return;
        };
        let Some(extensions) = self.pending_extensions.take() else {
            return;
        };
        self.managed_extensions
            .extend(extensions.items().iter().map(|e| e.id.to_string()));
        let wanted = extensions
            .items()
            .iter()
            .map(|e| crate::webview2_extensions::Wanted {
                id: e.id.to_string(),
                path: crate::extensions::extension_dir(&self.files.extensions_dir, &e.id),
                enabled: e.enabled,
            })
            .collect();
        if let Err(e) = crate::webview2_extensions::reconcile(
            &view.webview(),
            wanted,
            self.managed_extensions.clone(),
        ) {
            log::warn!("extensions: {e}");
        }
    }

    /// Builds the page scripts again and hands them to every view.
    fn rebuild_page_scripts(&mut self) {
        let sources = Sources {
            extensions_dir: &self.files.extensions_dir,
            // WebView2 runs the extensions' content scripts itself.
            extensions: (self.extension_support() != ExtensionSupport::Full)
                .then_some(&self.extensions),
            user_scripts_dir: &self.files.user_scripts_dir,
            user_scripts: &self.user_scripts,
            token: &self.files.page_token,
            bridge: Bridge::Ipc,
            origins: Origins::desktop(),
            state_dir: Some(&self.files.state_dir),
            blocker: true,
        };
        self.page_scripts = page_scripts(&sources);
        for (tab, view) in &self.views {
            let handles = self.injected.entry(*tab).or_default();
            page_injection::replace(view, &self.page_scripts, handles);
        }
    }

    /// Starts the background pages of the enabled extensions, stops the
    /// others.
    fn sync_backgrounds(&mut self) {
        let Some(server) = self.server.clone() else {
            return;
        };
        let wanted: HashMap<ExtensionId, String> = self
            .extensions
            .items()
            .iter()
            .filter(|e| e.enabled)
            .filter_map(|e| Some((e.id.clone(), server.background_url(&e.id)?)))
            .collect();
        self.backgrounds.retain(|id, _| wanted.contains_key(id));
        for (id, url) in wanted {
            if self.backgrounds.contains_key(&id) {
                continue;
            }
            let emit = self.emitter();
            let view = base_builder(
                &mut self.context,
                self.server.as_ref(),
                self.files.private,
                PageSource::Background(id.clone()),
                emit,
            )
            .with_bounds(bounds(Viewport::default()))
            .with_visible(false)
            .with_url(&url)
            .build_as_child(&self.parent);
            match view {
                Ok(view) => {
                    log::info!("extensions: background page of {id} started");
                    self.backgrounds.insert(id, view);
                }
                Err(e) => log::warn!("extensions: background page of {id}: {e}"),
            }
        }
    }

    fn with_view(&self, tab: TabId, action: &str, f: impl FnOnce(&WebView) -> wry::Result<()>) {
        if let Some(view) = self.views.get(&tab)
            && let Err(e) = f(view)
        {
            log::warn!("{action} in tab {tab}: {e}");
        }
    }

    fn view_of(&self, source: &PageSource) -> Option<&WebView> {
        match source {
            PageSource::Tab(tab) => self.views.get(tab),
            PageSource::Background(id) => self.backgrounds.get(id),
            PageSource::Panel => self.panel.as_ref().map(|(_, v)| v),
        }
    }

    /// Whether extension `id`'s background page runs (it can be inspected).
    pub fn has_background(&self, id: &ExtensionId) -> bool {
        self.backgrounds.contains_key(id)
    }

    /// The keys the browser takes from the pages (its shortcuts). Windows
    /// only: elsewhere a page with the focus keeps its keys.
    pub fn set_shortcuts(&mut self, keys: impl IntoIterator<Item = KeyPress>) {
        #[cfg(windows)]
        {
            *self.shortcuts.borrow_mut() = keys.into_iter().collect();
        }
        #[cfg(not(windows))]
        drop(keys.into_iter());
    }

    /// Gives the keyboard back to the window (from the page that has it),
    /// for the address bar.
    pub fn focus_window(&self) {
        if let Some(view) = self.shown.and_then(|tab| self.views.get(&tab))
            && let Err(e) = view.focus_parent()
        {
            log::warn!("focus: {e}");
        }
    }

    /// How `tab`'s page connection is secured, once the engine told.
    /// Windows only: elsewhere the system's web view doesn't tell.
    pub fn site_security(&self, tab: TabId) -> Option<crate::certificates::Certificates> {
        #[cfg(windows)]
        return self.security.get(&tab).and_then(|s| s.borrow().clone());
        #[cfg(not(windows))]
        {
            let _ = tab;
            Some(Err("this system's web view doesn't tell".into()))
        }
    }

    /// What the ad blocker stopped on `tab`'s page. Windows only: WebKit
    /// doesn't tell what its content rules block.
    pub fn blocked_count(&self, tab: TabId) -> Option<u32> {
        #[cfg(windows)]
        return self.blocked.get(&tab).map(|b| b.get());
        #[cfg(not(windows))]
        {
            let _ = tab;
            None
        }
    }

    /// Whether a page took the keyboard since the last call (the window's
    /// own fields must then let it go).
    pub fn take_page_focused(&mut self) -> bool {
        #[cfg(windows)]
        return self.page_focused.replace(false);
        #[cfg(not(windows))]
        false
    }

    /// The shortcuts pressed in a page since the last call.
    pub fn take_key_presses(&mut self) -> Vec<KeyPress> {
        #[cfg(windows)]
        return std::mem::take(&mut *self.pressed.borrow_mut());
        #[cfg(not(windows))]
        Vec::new()
    }

    /// The address of `path` of extension `id`, where its files are served.
    pub fn extension_url(&self, id: &ExtensionId, path: &str) -> Option<String> {
        self.server.as_ref().map(|s| s.origins().url(id, path))
    }
}

impl WebEngine for WryEngine {
    fn open_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        let emit = self.emitter();
        let on_load = emit.clone();
        let on_title = emit.clone();
        let on_message = emit.clone();
        let on_download = emit.clone();
        let on_downloaded = emit.clone();
        #[cfg(windows)]
        let on_frame_message = emit.clone();
        #[cfg(windows)]
        let on_moved = emit.clone();
        #[cfg(windows)]
        let on_zoom = emit.clone();
        #[cfg(windows)]
        let on_fullscreen = emit.clone();
        #[cfg(windows)]
        let on_favicon = emit.clone();
        #[cfg(windows)]
        let on_audio = emit.clone();
        let device_for_downloads = self.device.clone();
        // Downloads the browser stopped before they began.
        let refused = std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashSet::new()));
        let refused_by_start = refused.clone();
        let builder = base_builder(
            &mut self.context,
            self.server.as_ref(),
            self.files.private,
            PageSource::Tab(tab),
            on_message,
        );
        // The page the tab shows, for the network rules, and what the ad
        // blocker stopped on it (counted again on each new page).
        #[cfg(windows)]
        let page = std::rc::Rc::new(std::cell::RefCell::new(address.to_string()));
        #[cfg(windows)]
        let blocked = Rc::new(std::cell::Cell::new(0));
        #[cfg(windows)]
        let builder = {
            let (page, blocked) = (page.clone(), blocked.clone());
            builder.with_navigation_handler(move |url| {
                *page.borrow_mut() = url;
                blocked.set(0);
                true
            })
        };
        // The ad blocker's scriptlets, at the start of each navigation.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let sites = crate::webkit_scriptlets::Sites::default();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let builder = {
            let (sites, blocker) = (sites.clone(), self.blocker.clone());
            builder.with_navigation_handler(move |url| {
                sites.navigating(&url, &blocker);
                true
            })
        };
        // No address yet: the page scripts must be in before the first load.
        let view = builder
            .with_bounds(bounds(self.viewport))
            .with_visible(false)
            .with_on_page_load_handler(move |event, url| {
                let Ok(address) = WebAddress::parse(&url) else {
                    return;
                };
                on_load(match event {
                    PageLoadEvent::Started => EngineEvent::LoadStarted { tab, address },
                    PageLoadEvent::Finished => EngineEvent::LoadFinished { tab, address },
                });
            })
            .with_document_title_changed_handler(move |title| {
                on_title(EngineEvent::TitleChanged { tab, title });
            })
            // Into the user's Downloads folder (WebView2 picks a free name),
            // the one chosen, or where the user says.
            .with_download_started_handler(move |url, file| {
                // A store's "Add to Chrome": the web view would only save
                // the package (and fail); the browser installs it.
                if let Some(id) = file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| domain::ExtensionId::of_store_package(&url, n))
                {
                    on_download(EngineEvent::ExtensionOffered { id });
                    refused_by_start.borrow_mut().insert(url);
                    return false;
                }
                let Some(target) = download_target(&device_for_downloads.borrow(), file) else {
                    refused_by_start.borrow_mut().insert(url);
                    return false;
                };
                *file = target;
                on_download(EngineEvent::DownloadStarted {
                    url,
                    file: file.clone(),
                });
                true
            })
            .with_download_completed_handler(move |url, file, ok| {
                // Refused above: the web view reports it failed, but there
                // is no download to show.
                if refused.borrow_mut().remove(&url) {
                    return;
                }
                on_downloaded(EngineEvent::DownloadFinished { url, file, ok });
            })
            .with_new_window_req_handler(move |url, features| {
                // `window.open` with a size: a popup (a sign-in, a payment),
                // which talks to its opener (`window.opener`) and closes
                // itself. WebView2's own window keeps that link; a tab
                // would break it. Links and plain `window.open`: a tab.
                #[cfg(windows)]
                if features.size.is_some() {
                    return NewWindowResponse::Allow;
                }
                #[cfg(not(windows))]
                let _ = features;
                if let Ok(address) = WebAddress::parse(&url) {
                    emit(EngineEvent::NewTabRequested {
                        opener: Some(tab),
                        address,
                    });
                }
                NewWindowResponse::Deny
            })
            .build_as_child(&self.parent)
            .map_err(engine_error)?;
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        self.rule_lists.attach(&view);
        // Before the first load, which comes through the navigation handler.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        sites.attach(&view);
        // On every tab: the ad blocker, and the extensions' rules when the
        // browser runs them (empty otherwise).
        #[cfg(windows)]
        {
            let waker = self.waker.clone();
            if let Err(e) = crate::webview2_blocking::attach(
                &view,
                self.blocking_rules.clone(),
                self.files.extensions_dir.clone(),
                page.clone(),
                self.blocker.clone(),
                blocked.clone(),
                move || waker(),
            ) {
                log::warn!("network rules in tab {tab}: {e}");
            }
            self.tab_pages.insert(tab, page);
            self.blocked.insert(tab, blocked);
        }
        #[cfg(windows)]
        if let Err(e) = crate::webview2_downloads::attach(&view, self.downloads.clone()) {
            log::warn!("downloads of tab {tab}: {e}");
        }
        // The page scripts of its frames talk to the browser like its own.
        #[cfg(windows)]
        {
            let frames = crate::webview2_frames::Frames::default();
            if let Err(e) = crate::webview2_frames::attach(&view, frames.clone(), move |body| {
                on_frame_message(EngineEvent::PageMessage {
                    from: PageSource::Tab(tab),
                    body,
                });
            }) {
                log::warn!("frames of tab {tab}: {e}");
            }
            self.frames.insert(tab, frames);
        }
        // This device's passwords and zoom by site.
        #[cfg(windows)]
        {
            let device = self.device.borrow();
            if let Err(e) = crate::webview2_settings::save_passwords(&view, device.save_passwords) {
                log::warn!("passwords in tab {tab}: {e}");
            }
            drop(device);
            let device = self.device.clone();
            if let Err(e) = crate::webview2_settings::follow_zoom(
                &view,
                move |host| device.borrow().zoom_of(host),
                move |host, percent| on_zoom(EngineEvent::ZoomChanged { host, percent }),
            ) {
                log::warn!("zoom of tab {tab}: {e}");
            }
            if let Err(e) = crate::webview2_settings::follow_fullscreen(&view, move |on| {
                on_fullscreen(EngineEvent::FullScreenChanged { tab, on });
            }) {
                log::warn!("full screen of tab {tab}: {e}");
            }
            if let Err(e) = crate::webview2_settings::follow_favicon(&view, move |png| {
                on_favicon(EngineEvent::FaviconChanged { tab, png });
            }) {
                log::warn!("icon of tab {tab}: {e}");
            }
            if let Err(e) = crate::webview2_settings::follow_audio(&view, move |playing, muted| {
                on_audio(EngineEvent::AudioChanged {
                    tab,
                    playing,
                    muted,
                });
            }) {
                log::warn!("sound of tab {tab}: {e}");
            }
        }
        // Within the page (`history.pushState`): the address bar follows.
        #[cfg(windows)]
        if let Err(e) = crate::webview2_address::attach(&view, move |url| {
            if let Ok(address) = WebAddress::parse(&url) {
                on_moved(EngineEvent::LoadFinished { tab, address });
            }
        }) {
            log::warn!("address of tab {tab}: {e}");
        }
        #[cfg(windows)]
        match crate::webview2_scriptlets::attach(&view, self.blocker.clone()) {
            Ok(scriptlets) => {
                self.scriptlets.insert(tab, scriptlets);
            }
            Err(e) => log::warn!("scriptlets of tab {tab}: {e}"),
        }
        #[cfg(windows)]
        {
            let waker = self.waker.clone();
            if let Err(e) = crate::webview2_keys::attach(
                &view,
                self.shortcuts.clone(),
                self.pressed.clone(),
                move || waker(),
            ) {
                log::warn!("shortcuts in tab {tab}: {e}");
            }
            let (focused, waker) = (self.page_focused.clone(), self.waker.clone());
            if let Err(e) = crate::webview2_keys::on_focus(&view, move || {
                focused.set(true);
                waker();
            }) {
                log::warn!("focus of tab {tab}: {e}");
            }
        }
        #[cfg(windows)]
        {
            let slot = Rc::default();
            let waker = self.waker.clone();
            if let Err(e) = crate::certificates::watch(&view, Rc::clone(&slot), move || waker()) {
                log::warn!("security of tab {tab}: {e}");
            }
            self.security.insert(tab, slot);
        }
        let mut handles = Handles::default();
        page_injection::replace(&view, &self.page_scripts, &mut handles);
        view.load_url(address.as_str()).map_err(engine_error)?;
        self.injected.insert(tab, handles);
        self.views.insert(tab, view);
        #[cfg(windows)]
        self.apply_extensions();
        Ok(())
    }

    fn close_view(&mut self, tab: TabId) {
        // Dropping the view destroys it.
        self.views.remove(&tab);
        self.injected.remove(&tab);
        #[cfg(windows)]
        self.tab_pages.remove(&tab);
        #[cfg(windows)]
        self.blocked.remove(&tab);
        #[cfg(windows)]
        self.security.remove(&tab);
        #[cfg(windows)]
        self.scriptlets.remove(&tab);
        #[cfg(windows)]
        self.frames.remove(&tab);
        if self.shown == Some(tab) {
            self.shown = None;
        }
        #[cfg(windows)]
        if self.page_cut.as_ref().is_some_and(|(cut, ..)| *cut == tab) {
            // Its window is gone with it.
            self.page_cut = None;
        }
    }

    fn show(&mut self, tab: Option<TabId>) {
        if self.shown == tab {
            return;
        }
        if let Some(old) = self.shown {
            self.with_view(old, "hide", |v| v.set_visible(false));
        }
        if let Some(new) = tab {
            let viewport = self.viewport;
            self.with_view(new, "show", |v| {
                v.set_bounds(bounds(viewport))?;
                v.set_visible(true)
            });
        }
        self.shown = tab;
        #[cfg(windows)]
        self.cut_page();
    }

    fn navigate(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        match self.views.get(&tab) {
            Some(view) => view.load_url(address.as_str()).map_err(engine_error),
            None => Err(EngineError(format!("no view for tab {tab}"))),
        }
    }

    fn go_back(&mut self, tab: TabId) {
        self.with_view(tab, "back", |v| v.evaluate_script("history.back()"));
    }

    fn go_forward(&mut self, tab: TabId) {
        self.with_view(tab, "forward", |v| v.evaluate_script("history.forward()"));
    }

    fn reload(&mut self, tab: TabId) {
        self.with_view(tab, "reload", WebView::reload);
    }

    fn reload_bypassing_cache(&mut self, tab: TabId) {
        let Some(view) = self.views.get(&tab) else {
            return;
        };
        if let Err(e) = reload_from_origin(view) {
            log::warn!("reload of tab {tab} without the cache: {e}");
            self.reload(tab);
        }
    }

    fn set_viewport(&mut self, viewport: Viewport) {
        if self.viewport == viewport {
            return;
        }
        self.viewport = viewport;
        // Hidden views are resized when shown.
        if let Some(tab) = self.shown {
            self.with_view(tab, "resize", |v| v.set_bounds(bounds(viewport)));
        }
        #[cfg(windows)]
        self.cut_page();
    }

    fn set_page_holes(&mut self, holes: &[Viewport]) {
        #[cfg(windows)]
        if self.page_holes != holes {
            self.page_holes = holes.to_vec();
            self.cut_page();
        }
        #[cfg(not(windows))]
        let _ = holes;
    }

    fn extension_support(&self) -> ExtensionSupport {
        if !builtin_extensions() {
            ExtensionSupport::Full
        } else if cfg!(any(windows, target_os = "macos", target_os = "linux")) {
            ExtensionSupport::Partial
        } else {
            ExtensionSupport::None
        }
    }

    fn set_extensions(&mut self, extensions: &Extensions) {
        if self.extension_support() == ExtensionSupport::None {
            return;
        }
        self.pending_extensions = Some(extensions.clone());
        #[cfg(windows)]
        self.apply_extensions();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        self.rule_lists.update(extensions);
        #[cfg(windows)]
        if builtin_extensions() {
            // Read off the UI thread: big rulesets take a moment.
            let (rules, dir, state, list) = (
                self.blocking_rules.clone(),
                self.files.extensions_dir.clone(),
                self.files.state_dir.clone(),
                extensions.clone(),
            );
            std::thread::spawn(move || {
                let set = crate::extensions::rule_set(&dir, Some(&state), &list);
                *rules.write().unwrap_or_else(|p| p.into_inner()) = set;
            });
        }
        self.extensions = extensions.clone();
        if let Some(server) = &self.server {
            server.set_extensions(extensions);
        }
        if self.extension_support() == ExtensionSupport::Partial {
            self.rebuild_page_scripts();
            self.sync_backgrounds();
        }
    }

    fn set_user_scripts(&mut self, scripts: &UserScripts) {
        self.user_scripts = scripts.clone();
        self.rebuild_page_scripts();
    }

    fn reply_to_page(&mut self, to: &PageSource, message: &str) {
        let script = format!("window.__buReply && window.__buReply({message})");
        if let Some(view) = self.view_of(to)
            && let Err(e) = view.evaluate_script(&script)
        {
            log::warn!("page message to {to:?}: {e}");
        }
        // Answers and port, storage events reach the tab's frames too (each
        // frame's scripts take only their own). Not the messages and
        // connections for the tab's content scripts: those are the top
        // document's, answered once.
        #[cfg(windows)]
        if let PageSource::Tab(tab) = to
            && let Some(frames) = self.frames.get(tab)
            && !matches!(
                serde_json::from_str::<serde_json::Value>(message)
                    .ok()
                    .as_ref()
                    .and_then(|m| m.get("event"))
                    .and_then(serde_json::Value::as_str),
                Some("message" | "connect")
            )
        {
            crate::webview2_frames::run(frames, &script);
        }
    }

    fn run_in_tab(&mut self, tab: TabId, script: &str) {
        self.with_view(tab, "script", |v| v.evaluate_script(script));
    }

    fn open_devtools(&mut self, page: &PageSource) {
        match self.view_of(page) {
            Some(view) => view.open_devtools(),
            None => log::warn!("devtools: no page {page:?}"),
        }
    }

    #[cfg(windows)]
    fn downloads(&mut self) -> Vec<application::ports::DownloadProgress> {
        crate::webview2_downloads::progress(&self.downloads)
    }

    #[cfg(windows)]
    fn can_cancel_downloads(&self) -> bool {
        true
    }

    fn apply_device_settings(&mut self, settings: &domain::DeviceSettings) {
        *self.device.borrow_mut() = settings.clone();
        #[cfg(windows)]
        for (tab, view) in &self.views {
            if let Err(e) = crate::webview2_settings::save_passwords(view, settings.save_passwords)
            {
                log::warn!("passwords in tab {tab}: {e}");
            }
            let host = self
                .tab_pages
                .get(tab)
                .and_then(|p| WebAddress::parse(&p.borrow()).ok())
                .and_then(|a| a.host().map(str::to_owned));
            if let Some(host) = host {
                crate::webview2_settings::set_zoom(view, settings.zoom_of(&host));
            }
        }
    }

    fn set_muted(&mut self, tab: TabId, muted: bool) {
        #[cfg(windows)]
        if let Some(view) = self.views.get(&tab) {
            crate::webview2_settings::set_muted(view, muted);
        }
        #[cfg(not(windows))]
        let _ = (tab, muted);
    }

    fn can_save_passwords(&self) -> bool {
        cfg!(windows)
    }

    fn can_clear_site_data(&self) -> bool {
        cfg!(windows)
    }

    fn clear_site_data(&mut self, cookies: bool, cache: bool) {
        #[cfg(windows)]
        if let Some(view) = self.views.values().next()
            && let Err(e) = crate::webview2_settings::clear_site_data(view, cookies, cache)
        {
            log::warn!("clear browsing data: {e}");
        }
        #[cfg(not(windows))]
        let _ = (cookies, cache);
    }

    #[cfg(windows)]
    fn cancel_download(&mut self, file: &std::path::Path) {
        crate::webview2_downloads::cancel(&self.downloads, file);
    }

    fn show_panel_page(&mut self, url: Option<&str>, area: Viewport) {
        let Some(url) = url else {
            self.panel = None;
            self.panel_area = None;
            return;
        };
        // Called every frame: the same popup only moves (or takes its
        // measured size). Its size is forgotten only for a new popup, or
        // it would fall back to the default size and be measured again
        // each frame (it blinked).
        if let Some((current, view)) = &self.panel
            && current == url
        {
            if self.panel_area != Some(area) {
                self.panel_area = Some(area);
                if let Err(e) = view.set_bounds(bounds(area)) {
                    log::warn!("panel: {e}");
                }
            }
            return;
        }
        self.panel_area = Some(area);
        *self.panel_size.lock().unwrap_or_else(|p| p.into_inner()) = None;
        *self.panel_width.lock().unwrap_or_else(|p| p.into_inner()) = None;
        self.panel_measured = None;
        let emit = self.emitter();
        let view = base_builder(
            &mut self.context,
            self.server.as_ref(),
            self.files.private,
            PageSource::Panel,
            emit,
        )
        .with_bounds(bounds(area))
        .with_url(url)
        .build_as_child(&self.parent);
        match view {
            Ok(view) => {
                // Its blur (a click elsewhere) closes it.
                let _ = view.focus();
                self.panel = Some((url.to_owned(), view));
            }
            Err(e) => log::warn!("panel: {e}"),
        }
    }
}
