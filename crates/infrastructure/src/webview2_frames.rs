//! Windows: the page scripts in a tab's frames. WebView2 hands what a
//! frame posts (`chrome.webview.postMessage`) to that frame's own
//! `WebMessageReceived`, which wry doesn't listen to, and runs scripts in
//! the top document only: an extension page in a frame (Bitwarden's inline
//! menu) could neither ask nor be answered. Each frame of the tab is kept,
//! its messages go where the tab's go, and the answers reach it too.

use std::cell::RefCell;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_4, ICoreWebView2ExecuteScriptCompletedHandler, ICoreWebView2Frame2,
};
use webview2_com::{
    FrameCreatedEventHandler, FrameDestroyedEventHandler, FrameWebMessageReceivedEventHandler,
    take_pwstr,
};
use windows_core::{HSTRING, Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

/// A tab's frames (its top document's children), as they come and go.
pub type Frames = Rc<RefCell<Vec<ICoreWebView2Frame2>>>;

/// Keeps `view`'s frames in `frames`; what their scripts post goes to
/// `on_message`, with the address of the frame's document (WebView2's,
/// not the frame's say).
pub fn attach(
    view: &WebView,
    frames: Frames,
    on_message: impl Fn(String, String) + 'static,
) -> windows_core::Result<()> {
    let webview = view.webview().cast::<ICoreWebView2_4>()?;
    let on_message = Rc::new(on_message);
    let handler = FrameCreatedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let frame = unsafe { args.Frame()? };
        let frame2 = frame.cast::<ICoreWebView2Frame2>()?;
        let on_message = on_message.clone();
        let received = FrameWebMessageReceivedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut text = PWSTR::null();
                // Not a string: not the page scripts' (they post JSON text).
                if unsafe { args.TryGetWebMessageAsString(&mut text) }.is_ok() {
                    let text = take_pwstr(text);
                    let mut source = PWSTR::null();
                    // No address: nothing it could be but a web page's.
                    let url = match unsafe { args.Source(&mut source) } {
                        Ok(()) => take_pwstr(source),
                        Err(_) => "about:blank".to_owned(),
                    };
                    on_message(url, text);
                }
            }
            Ok(())
        }));
        let mut token = 0;
        unsafe { frame2.add_WebMessageReceived(&received, &mut token)? };
        let (list, gone) = (frames.clone(), frame2.clone());
        let destroyed = FrameDestroyedEventHandler::create(Box::new(move |_, _| {
            list.borrow_mut().retain(|f| f != &gone);
            Ok(())
        }));
        unsafe { frame.add_Destroyed(&destroyed, &mut token)? };
        frames.borrow_mut().push(frame2);
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_FrameCreated(&handler, &mut token) }
}

/// Runs `script` in each of `frames`.
pub fn run(frames: &Frames, script: &str) {
    let script = HSTRING::from(script);
    for frame in frames.borrow().iter() {
        if let Err(e) = unsafe {
            frame.ExecuteScript(&script, None::<&ICoreWebView2ExecuteScriptCompletedHandler>)
        } {
            log::debug!("script in a frame: {e}");
        }
    }
}
