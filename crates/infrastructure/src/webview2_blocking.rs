//! The extensions' network rules on Windows, when the browser runs the
//! extensions itself: every request of a tab goes through
//! `WebResourceRequested`, which the shared matcher answers (block, or serve
//! the extension's stand-in file).

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use domain::{Request, ResourceType, RuleSet, Verdict};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_TEXT_TRACK,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET,
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST, ICoreWebView2_2,
};
use webview2_com::{WebResourceRequestedEventHandler, take_pwstr};
use windows::Win32::UI::Shell::SHCreateMemStream;
use windows_core::{HSTRING, Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::extension_server::mime_type;
use crate::extensions::extension_dir;

fn resource_type(context: COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> Option<ResourceType> {
    Some(match context {
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => ResourceType::Stylesheet,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => ResourceType::Image,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_TEXT_TRACK => {
            ResourceType::Media
        }
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => ResourceType::Font,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => ResourceType::Script,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST
        | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH
        | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_EVENT_SOURCE => ResourceType::XmlHttpRequest,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => ResourceType::WebSocket,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => ResourceType::Ping,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_CSP_VIOLATION_REPORT => ResourceType::CspReport,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => ResourceType::SubFrame,
        _ => return None,
    })
}

/// `path` of extension `id`'s folder, refusing `..`.
fn stand_in(dir: &Path, id: &domain::ExtensionId, path: &str) -> Option<Vec<u8>> {
    let relative = path.trim_start_matches('/');
    if relative.split(['/', '\\']).any(|p| p == "..") {
        return None;
    }
    fs::read(extension_dir(dir, id).join(relative)).ok()
}

/// Filters every request of `view` through `rules`. `page` is the address
/// of the tab's page (its top-level navigations), kept up to date by the
/// engine.
pub fn attach(
    view: &WebView,
    rules: Arc<RwLock<RuleSet>>,
    extensions_dir: PathBuf,
    page: Rc<RefCell<String>>,
) -> windows_core::Result<()> {
    let webview = view.webview();
    let environment = unsafe { webview.cast::<ICoreWebView2_2>()?.Environment()? };
    unsafe {
        webview.AddWebResourceRequestedFilter(
            &HSTRING::from("*"),
            COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
        )?;
    }
    let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let rules = rules.read().unwrap_or_else(|p| p.into_inner());
        if rules.is_empty() {
            return Ok(());
        }
        let request = unsafe { args.Request()? };
        let mut uri = PWSTR::null();
        unsafe { request.Uri(&mut uri)? };
        let url = take_pwstr(uri);
        if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("ws")) {
            return Ok(());
        }
        let mut method = PWSTR::null();
        unsafe { request.Method(&mut method)? };
        let method = take_pwstr(method);
        let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT::default();
        unsafe { args.ResourceContext(&mut context)? };
        let page = page.borrow().clone();
        // The tab's own page: a top-level navigation, left alone unless a
        // rule names main frames.
        let main_frame = context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT && url == page;
        let (resource_type, document) = if main_frame {
            (Some(ResourceType::MainFrame), None)
        } else {
            (resource_type(context), Some(page.as_str()))
        };
        let initiator = document
            .and_then(|d| Request::new(d, None, None, "GET"))
            .map(|r| r.host);
        let Some(request) = Request::new(&url, initiator, resource_type, &method) else {
            return Ok(());
        };
        let response = match rules.decide(&request, document) {
            Verdict::Allow => return Ok(()),
            Verdict::Block => unsafe {
                environment.CreateWebResourceResponse(
                    None,
                    403,
                    &HSTRING::from("Blocked"),
                    &HSTRING::new(),
                )?
            },
            Verdict::Redirect { extension, path } => {
                let Some(body) = stand_in(&extensions_dir, &extension, &path) else {
                    return Ok(());
                };
                let stream = unsafe { SHCreateMemStream(Some(&body)) };
                let headers = format!("Content-Type: {}", mime_type(&path));
                unsafe {
                    environment.CreateWebResourceResponse(
                        stream.as_ref(),
                        200,
                        &HSTRING::from("OK"),
                        &HSTRING::from(headers),
                    )?
                }
            }
        };
        unsafe { args.SetResponse(&response) }
    }));
    let mut token = 0;
    unsafe { webview.add_WebResourceRequested(&handler, &mut token) }
}
