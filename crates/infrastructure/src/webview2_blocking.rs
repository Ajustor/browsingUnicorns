//! The network filters on Windows: every request of a tab goes through
//! `WebResourceRequested`, answered first by the extensions' rules (when
//! the browser runs the extensions itself: block, or serve the extension's
//! stand-in file), then by the built-in ad blocker.

use std::cell::{Cell, RefCell};
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

use crate::blocker::BlockerHandle;
use crate::extension_server::{mime_type, safe_relative};
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

/// `path` of extension `id`'s folder: plain names only, so that neither
/// `..` nor an absolute path (`C:/…`, `\\server\share`) leaves it.
fn stand_in(dir: &Path, id: &domain::ExtensionId, path: &str) -> Option<Vec<u8>> {
    fs::read(extension_dir(dir, id).join(safe_relative(path)?)).ok()
}

/// Filters every request of `view` through `rules`, then `blocker`. `page`
/// is the address of the tab's page (its top-level navigations), kept up to
/// date by the engine; `blocked` counts the requests the blocker stopped,
/// `on_blocked` is called after each.
pub fn attach(
    view: &WebView,
    rules: Arc<RwLock<RuleSet>>,
    extensions_dir: PathBuf,
    page: Rc<RefCell<String>>,
    blocker: BlockerHandle,
    blocked: Rc<Cell<u32>>,
    on_blocked: impl Fn() + 'static,
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
        if rules.is_empty() && blocker.get().is_none() {
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
        let verdict = if rules.is_empty() {
            Verdict::Allow
        } else {
            let initiator = document
                .and_then(|d| Request::new(d, None, None, "GET"))
                .map(|r| r.host);
            match Request::new(&url, initiator, resource_type, &method) {
                Some(request) => rules.decide(&request, document),
                None => Verdict::Allow,
            }
        };
        drop(rules);
        let response = match verdict {
            // The extensions let it through: the ad blocker's turn.
            Verdict::Allow => {
                if main_frame || !blocker.should_block(&url, &page, resource_type) {
                    return Ok(());
                }
                blocked.set(blocked.get().saturating_add(1));
                on_blocked();
                if resource_type == Some(ResourceType::Script) {
                    // An empty script rather than an error, as uBlock
                    // Origin does: loaders (RequireJS) fail every module
                    // after one that does not load, and the page breaks
                    // (a cookie banner whose buttons do nothing).
                    let stream = unsafe { SHCreateMemStream(Some(b"")) };
                    unsafe {
                        environment.CreateWebResourceResponse(
                            stream.as_ref(),
                            200,
                            &HSTRING::from("OK"),
                            &HSTRING::from(
                                "Content-Type: application/javascript\r\nAccess-Control-Allow-Origin: *",
                            ),
                        )?
                    }
                } else {
                    unsafe {
                        environment.CreateWebResourceResponse(
                            None,
                            403,
                            &HSTRING::from("Blocked"),
                            &HSTRING::new(),
                        )?
                    }
                }
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stand_ins_stay_in_the_extension_folder() {
        let root = tempfile::tempdir().unwrap();
        let id = domain::ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        let dir = root.path().join("extensions");
        fs::create_dir_all(extension_dir(&dir, &id).join("war")).unwrap();
        fs::write(extension_dir(&dir, &id).join("war/noop.js"), "ok").unwrap();
        let outside = root.path().join("secret.txt");
        fs::write(&outside, "secret").unwrap();

        assert_eq!(stand_in(&dir, &id, "/war/noop.js").unwrap(), b"ok");
        let absolute = outside.to_string_lossy().into_owned();
        for path in [
            absolute.as_str(),
            "C:/Windows/win.ini",
            "/C:/Windows/win.ini",
            "\\\\server\\x",
            "//server/x",
            "/war/../../secret.txt",
            "/war/%2e%2e/%2e%2e/secret.txt",
            "a/../../b",
        ] {
            assert_eq!(stand_in(&dir, &id, path), None, "{path}");
        }
    }
}
