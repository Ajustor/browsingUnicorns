//! Windows: the address a page moves to without loading a new document
//! (`history.pushState`, `replaceState`, a `#fragment`), as YouTube does
//! from video to video. wry reports loads only, so the address bar would
//! keep the page's first address.

use webview2_com::SourceChangedEventHandler;
use webview2_com::take_pwstr;
use windows_core::BOOL;
use windows_core::PWSTR;
use wry::{WebView, WebViewExtWindows};

/// Calls `on_change` with the address `view` moved to within its document.
pub fn attach(view: &WebView, on_change: impl Fn(String) + 'static) -> windows_core::Result<()> {
    let webview = view.webview();
    let handler = SourceChangedEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        let mut new_document = BOOL::default();
        unsafe { args.IsNewDocument(&mut new_document)? };
        // A new document: its load is reported already.
        if new_document.as_bool() {
            return Ok(());
        }
        let mut source = PWSTR::null();
        unsafe { sender.Source(&mut source)? };
        on_change(take_pwstr(source));
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_SourceChanged(&handler, &mut token) }
}
