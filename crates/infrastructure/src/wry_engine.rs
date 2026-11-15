use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use application::ports::{EngineError, EngineEvent, Viewport, WebEngine};
use domain::{TabId, WebAddress};
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebView, WebViewBuilder};

use crate::ParentWindow;

/// Called after an event is sent, so the UI wakes up to read it.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// [`WebEngine`] backed by the system web view (WebView2 on Windows,
/// WKWebView on macOS, WebKitGTK on Linux), one child view per tab.
///
/// Must live on the UI thread, like the window it is embedded in.
pub struct WryEngine {
    parent: ParentWindow,
    views: HashMap<TabId, WebView>,
    shown: Option<TabId>,
    viewport: Viewport,
    events: Sender<EngineEvent>,
    waker: Waker,
}

fn engine_error(e: wry::Error) -> EngineError {
    EngineError(e.to_string())
}

fn bounds(v: Viewport) -> Rect {
    Rect {
        position: PhysicalPosition::new(v.x, v.y).into(),
        size: PhysicalSize::new(v.width.max(1), v.height.max(1)).into(),
    }
}

impl WryEngine {
    pub fn new(parent: ParentWindow, events: Sender<EngineEvent>, waker: Waker) -> Self {
        #[cfg(target_os = "linux")]
        if let Err(e) = gtk::init() {
            log::error!("GTK initialisation failed: {e}");
        }
        Self {
            parent,
            views: HashMap::new(),
            shown: None,
            viewport: Viewport::default(),
            events,
            waker,
        }
    }

    /// Advances the platform's own event loop where the web view needs it
    /// (GTK on Linux). Call once per UI frame.
    pub fn pump(&self) {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
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

    fn with_view(&self, tab: TabId, action: &str, f: impl FnOnce(&WebView) -> wry::Result<()>) {
        if let Some(view) = self.views.get(&tab)
            && let Err(e) = f(view)
        {
            log::warn!("{action} in tab {tab}: {e}");
        }
    }
}

impl WebEngine for WryEngine {
    fn open_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        let emit = self.emitter();
        let on_load = emit.clone();
        let on_title = emit.clone();
        let view = WebViewBuilder::new()
            .with_url(address.as_str())
            .with_bounds(bounds(self.viewport))
            .with_visible(false)
            .with_devtools(cfg!(debug_assertions))
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
            .with_new_window_req_handler(move |url, _features| {
                if let Ok(address) = WebAddress::parse(&url) {
                    emit(EngineEvent::NewTabRequested { address });
                }
                NewWindowResponse::Deny
            })
            .build_as_child(&self.parent)
            .map_err(engine_error)?;
        self.views.insert(tab, view);
        Ok(())
    }

    fn close_view(&mut self, tab: TabId) {
        // Dropping the view destroys it.
        self.views.remove(&tab);
        if self.shown == Some(tab) {
            self.shown = None;
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

    fn set_viewport(&mut self, viewport: Viewport) {
        if self.viewport == viewport {
            return;
        }
        self.viewport = viewport;
        // Hidden views are resized when shown.
        if let Some(tab) = self.shown {
            self.with_view(tab, "resize", |v| v.set_bounds(bounds(viewport)));
        }
    }
}
