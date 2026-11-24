//! Windows: the pages' Content Security Policy lets the built-in
//! extensions' frames in (see [`crate::csp`]). Documents are paused once
//! their headers arrive (DevTools' `Fetch` domain), the policy rewritten
//! when it would block them.

use std::rc::Rc;

use serde_json::{Value, json};
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;
use webview2_com::{
    CallDevToolsProtocolMethodCompletedHandler, DevToolsProtocolEventReceivedEventHandler,
    take_pwstr,
};
use windows_core::{HSTRING, PWSTR};
use wry::{WebView, WebViewExtWindows as _};

/// Rewrites the policy of `view`'s documents with the origins `origins`
/// gives at the time.
pub fn attach(
    view: &WebView,
    origins: impl Fn() -> Vec<String> + 'static,
) -> windows_core::Result<()> {
    let webview = view.webview();
    let origins = Rc::new(origins);
    let sender = webview.clone();
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut json = PWSTR::null();
        unsafe { args.ParameterObjectAsJson(&mut json)? };
        let Ok(paused) = serde_json::from_str::<Value>(&take_pwstr(json)) else {
            return Ok(());
        };
        resume(&sender, &paused, &origins());
        Ok(())
    }));
    unsafe {
        let receiver =
            webview.GetDevToolsProtocolEventReceiver(&HSTRING::from("Fetch.requestPaused"))?;
        receiver.add_DevToolsProtocolEventReceived(&handler, &mut 0)?;
    }
    let patterns = json!({ "patterns": [
        { "resourceType": "Document", "requestStage": "Response" },
    ]});
    call(&webview, "Fetch.enable", &patterns, |_| {})
}

/// Lets the paused document go on, its policy rewritten if need be.
fn resume(webview: &ICoreWebView2, paused: &Value, origins: &[String]) {
    let Some(id) = paused["requestId"].as_str() else {
        return;
    };
    let headers = paused["responseHeaders"].as_array();
    let rewritten = headers.and_then(|headers| {
        let mut changed = false;
        let headers: Vec<Value> = headers
            .iter()
            .map(|h| {
                let (Some(name), Some(value)) = (h["name"].as_str(), h["value"].as_str()) else {
                    return h.clone();
                };
                if !name.eq_ignore_ascii_case("content-security-policy") {
                    return h.clone();
                }
                // Several headers may come joined by newlines.
                let lines: Vec<String> = value
                    .split('\n')
                    .map(|line| match crate::csp::allow_frames(line, origins) {
                        Some(l) => {
                            changed = true;
                            l
                        }
                        None => line.to_owned(),
                    })
                    .collect();
                json!({ "name": name, "value": lines.join("\n") })
            })
            .collect();
        changed.then_some(headers)
    });
    let status = paused["responseStatusCode"].as_u64().unwrap_or(0);
    // An error or a redirect: no document to protect.
    let document = paused.get("responseErrorReason").is_none() && !(300..400).contains(&status);
    match rewritten {
        Some(headers) if document => fulfill(webview, id, status, headers),
        _ => go_on(webview, id),
    }
}

/// Lets the paused request go on as it is.
fn go_on(webview: &ICoreWebView2, id: &str) {
    if let Err(e) = call(
        webview,
        "Fetch.continueRequest",
        &json!({ "requestId": id }),
        |_| {},
    ) {
        log::warn!("paused page: {e}");
    }
}

/// Answers the paused request with its own body under `headers`.
/// (`Fetch.continueResponse` would change the headers too late: the
/// policy is read before.) Whatever fails, the page goes on unchanged.
fn fulfill(webview: &ICoreWebView2, id: &str, status: u64, headers: Vec<Value>) {
    let (wv, id_owned) = (webview.clone(), id.to_owned());
    let sent = call(
        webview,
        "Fetch.getResponseBody",
        &json!({ "requestId": id }),
        move |got| {
            let Some(got) = got else {
                return go_on(&wv, &id_owned);
            };
            let Some(body) = got["body"].as_str() else {
                return go_on(&wv, &id_owned);
            };
            let body = if got["base64Encoded"].as_bool() == Some(true) {
                body.to_owned()
            } else {
                data_encoding::BASE64.encode(body.as_bytes())
            };
            // The body comes decoded, whole.
            let headers: Vec<Value> = headers
                .into_iter()
                .filter(|h| {
                    let name = h["name"].as_str().unwrap_or("");
                    !name.eq_ignore_ascii_case("content-encoding")
                        && !name.eq_ignore_ascii_case("content-length")
                })
                .collect();
            let params = json!({
                "requestId": id_owned,
                "responseCode": status,
                "responseHeaders": headers,
                "body": body,
            });
            let (retry, rid) = (wv.clone(), id_owned.clone());
            let sent = call(&wv, "Fetch.fulfillRequest", &params, move |done| {
                if done.is_none() {
                    go_on(&retry, &rid);
                }
            });
            if sent.is_err() {
                go_on(&wv, &id_owned);
            }
        },
    );
    if let Err(e) = sent {
        log::warn!("page policy: {e}");
        go_on(webview, id);
    }
}

/// Calls DevTools' `method`; `done` gets its result, `None` if it failed.
fn call(
    webview: &ICoreWebView2,
    method: &str,
    params: &Value,
    done: impl FnOnce(Option<Value>) + 'static,
) -> windows_core::Result<()> {
    unsafe {
        webview.CallDevToolsProtocolMethod(
            &HSTRING::from(method),
            &HSTRING::from(params.to_string()),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
                done(result.ok().and_then(|()| serde_json::from_str(&json).ok()));
                Ok(())
            })),
        )
    }
}
