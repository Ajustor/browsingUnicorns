//! Injects the page scripts ([`crate::page_scripts`]) into a web view, at
//! document start in every frame, and replaces them when they change. They
//! apply from the view's next page load.

use wry::WebView;

pub use platform::Handles;

/// Replaces the scripts `handles` injected into `view` by `scripts`.
pub fn replace(view: &WebView, scripts: &[String], handles: &mut Handles) {
    platform::replace(view, scripts, handles);
}

#[cfg(windows)]
mod platform {
    use std::cell::RefCell;
    use std::rc::Rc;

    use webview2_com::AddScriptToExecuteOnDocumentCreatedCompletedHandler;
    use windows_core::HSTRING;
    use wry::{WebView, WebViewExtWindows};

    /// The ids WebView2 gave the scripts (they arrive asynchronously).
    #[derive(Default)]
    pub struct Handles(Rc<RefCell<Vec<String>>>);

    pub fn replace(view: &WebView, scripts: &[String], handles: &mut Handles) {
        let webview = view.webview();
        for id in handles.0.borrow_mut().drain(..) {
            if let Err(e) =
                unsafe { webview.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id)) }
            {
                log::warn!("page scripts: removing: {e}");
            }
        }
        for script in scripts {
            let ids = handles.0.clone();
            let handler = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
                move |result, id| {
                    match result {
                        Ok(()) => ids.borrow_mut().push(id),
                        Err(e) => log::warn!("page scripts: adding: {e}"),
                    }
                    Ok(())
                },
            ));
            if let Err(e) = unsafe {
                webview
                    .AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script.as_str()), &handler)
            } {
                log::warn!("page scripts: adding: {e}");
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_foundation::NSString;
    use objc2_web_kit::{WKUserScript, WKUserScriptInjectionTime};
    use wry::{WebView, WebViewExtMacOS};

    use crate::page_scripts::MARKER;

    /// Ours are found again by their [`MARKER`].
    #[derive(Default)]
    pub struct Handles(());

    pub fn replace(view: &WebView, scripts: &[String], _handles: &mut Handles) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let controller = view.manager();
        unsafe {
            // WebKit removes all the scripts or none: keep the others (wry's).
            let others: Vec<Retained<WKUserScript>> = controller
                .userScripts()
                .to_vec()
                .into_iter()
                .filter(|s| !s.source().to_string().starts_with(MARKER))
                .collect();
            controller.removeAllUserScripts();
            for s in &others {
                controller.addUserScript(s);
            }
            for script in scripts {
                let user_script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                    WKUserScript::alloc(mtm),
                    &NSString::from_str(script),
                    WKUserScriptInjectionTime::AtDocumentStart,
                    false,
                );
                controller.addUserScript(&user_script);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use webkit2gtk::{
        UserContentInjectedFrames, UserContentManagerExt, UserScript, UserScriptInjectionTime,
        WebViewExt,
    };
    use wry::{WebView, WebViewExtUnix};

    #[derive(Default)]
    pub struct Handles(Vec<UserScript>);

    pub fn replace(view: &WebView, scripts: &[String], handles: &mut Handles) {
        let Some(manager) = view.webview().user_content_manager() else {
            return;
        };
        for old in handles.0.drain(..) {
            manager.remove_script(&old);
        }
        for script in scripts {
            let user_script = UserScript::new(
                script,
                UserContentInjectedFrames::AllFrames,
                UserScriptInjectionTime::Start,
                &[],
                &[],
            );
            manager.add_script(&user_script);
            handles.0.push(user_script);
        }
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    use wry::WebView;

    #[derive(Default)]
    pub struct Handles(());

    pub fn replace(_view: &WebView, _scripts: &[String], _handles: &mut Handles) {}
}
