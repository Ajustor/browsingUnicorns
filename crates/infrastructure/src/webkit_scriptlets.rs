//! macOS, Linux: the ad blocker's scriptlets, added from wry's navigation
//! handler (WebKit's navigation decision), before the new document exists,
//! so they run before the page's own scripts. Frames' navigations come
//! through too: harmless, a script runs on its own host only.

use std::cell::RefCell;
use std::rc::Rc;

use wry::WebView;

use crate::blocker::{BlockerChanges, BlockerHandle};
use crate::site_scripts::SiteScripts;

/// A tab's sites, and its web view's scripts once the view exists. Adding
/// is synchronous: a script is in before the navigation goes on.
#[derive(Clone, Default)]
pub struct Sites(Rc<RefCell<State>>);

#[derive(Default)]
struct State {
    scripts: SiteScripts<platform::Handle>,
    changes: BlockerChanges,
    native: Option<platform::Native>,
}

impl Sites {
    /// Follows `view`, once built (before its first load).
    pub fn attach(&self, view: &WebView) {
        self.0.borrow_mut().native = platform::native(view);
    }

    /// From wry's navigation handler: when `blocker` changed since the last
    /// navigation, removes the scripts and forgets the sites; then adds
    /// `url`'s site's scriptlets.
    pub fn navigating(&self, url: &str, blocker: &BlockerHandle) {
        let mut state = self.0.borrow_mut();
        let state = &mut *state;
        let Some(native) = state.native.as_ref() else {
            return;
        };
        if state.changes.changed(blocker) {
            platform::remove(native, state.scripts.clear());
        }
        if let Some(script) = state.scripts.script_for(url, |u| blocker.scriptlets(u)) {
            let handle = platform::add(native, &script);
            state.scripts.keep(handle);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use webkit2gtk::{
        UserContentInjectedFrames, UserContentManager, UserContentManagerExt, UserScript,
        UserScriptInjectionTime, WebViewExt,
    };
    use wry::{WebView, WebViewExtUnix};

    pub type Native = UserContentManager;
    pub type Handle = UserScript;

    pub fn native(view: &WebView) -> Option<Native> {
        view.webview().user_content_manager()
    }

    pub fn add(manager: &Native, script: &str) -> Handle {
        let user_script = UserScript::new(
            script,
            UserContentInjectedFrames::AllFrames,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        );
        manager.add_script(&user_script);
        user_script
    }

    pub fn remove(manager: &Native, handles: Vec<Handle>) {
        for script in handles {
            manager.remove_script(&script);
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_foundation::NSString;
    use objc2_web_kit::{WKUserContentController, WKUserScript, WKUserScriptInjectionTime};
    use wry::{WebView, WebViewExtMacOS};

    use crate::site_scripts::MARKER;

    pub type Native = Retained<WKUserContentController>;

    /// Ours are found again by their [`MARKER`].
    pub struct Handle;

    pub fn native(view: &WebView) -> Option<Native> {
        Some(view.manager())
    }

    pub fn add(controller: &Native, script: &str) -> Handle {
        let Some(mtm) = MainThreadMarker::new() else {
            return Handle;
        };
        unsafe {
            let user_script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                WKUserScript::alloc(mtm),
                &NSString::from_str(script),
                WKUserScriptInjectionTime::AtDocumentStart,
                false,
            );
            controller.addUserScript(&user_script);
        }
        Handle
    }

    pub fn remove(controller: &Native, handles: Vec<Handle>) {
        if handles.is_empty() {
            return;
        }
        unsafe {
            // WebKit removes all the scripts or none: keep the others (wry's,
            // the page scripts).
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
        }
    }
}
