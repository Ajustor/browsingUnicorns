//! The developer tools in a side panel on Windows. WebView2 only opens them
//! in a window of their own; docked, they are their front end (served by
//! the runtime on its local debugging port) in a view of ours, attached to
//! the page's DevTools target.

use std::cell::RefCell;
use std::net::{Ipv4Addr, TcpListener};
use std::rc::Rc;
use std::sync::OnceLock;

use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
use windows_core::HSTRING;
use wry::{WebView, WebViewExtWindows as _};

/// The runtime's local debugging port: picked once, every view of the
/// process shares the runtime (and must give it the same arguments).
pub fn port() -> Option<u16> {
    static PORT: OnceLock<Option<u16>> = OnceLock::new();
    *PORT.get_or_init(|| {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
        Some(listener.local_addr().ok()?.port())
    })
}

/// The web view runtime's arguments: wry's defaults, and the debugging
/// server on `port` (local only), its front end allowed to connect.
pub fn browser_args(port: u16) -> String {
    format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
         --remote-debugging-port={port} \
         --remote-allow-origins=http://127.0.0.1:{port}"
    )
}

/// The front end's address, inspecting `target`.
pub fn front_end_url(port: u16, target: &str) -> String {
    format!(
        "http://127.0.0.1:{port}/devtools/inspector.html?ws=127.0.0.1:{port}/devtools/page/{target}"
    )
}

/// The DevTools target id of `view`'s page: `slot` gets it, `wake` tells.
pub fn target_id(
    view: &WebView,
    slot: Rc<RefCell<Option<Result<String, String>>>>,
    wake: impl Fn() + 'static,
) {
    let answer = slot.clone();
    let handler =
        CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
            *answer.borrow_mut() = Some(
                result
                    .map_err(|e| e.message())
                    .and_then(|()| parse_target(&json)),
            );
            wake();
            Ok(())
        }));
    let call = unsafe {
        view.webview().CallDevToolsProtocolMethod(
            &HSTRING::from("Target.getTargetInfo"),
            &HSTRING::from("{}"),
            &handler,
        )
    };
    if let Err(e) = call {
        *slot.borrow_mut() = Some(Err(e.message()));
    }
}

/// `Target.getTargetInfo`'s answer: `{"targetInfo": {"targetId": ...}}`.
fn parse_target(json: &str) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    value
        .pointer("/targetInfo/targetId")
        .and_then(|id| id.as_str())
        // Goes into a URL path.
        .filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(str::to_owned)
        .ok_or_else(|| "no DevTools target".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_target_id() {
        let json =
            r#"{"targetInfo": {"targetId": "85C0FA390DFA38FEB4E8656A75766E4B", "type": "page"}}"#;
        assert_eq!(
            parse_target(json).unwrap(),
            "85C0FA390DFA38FEB4E8656A75766E4B"
        );
        assert!(parse_target(r#"{"targetInfo": {"targetId": "../x"}}"#).is_err());
        assert!(parse_target("{}").is_err());
    }

    #[test]
    fn the_front_end_inspects_the_target() {
        assert_eq!(
            front_end_url(9333, "ABC"),
            "http://127.0.0.1:9333/devtools/inspector.html?ws=127.0.0.1:9333/devtools/page/ABC"
        );
    }
}
