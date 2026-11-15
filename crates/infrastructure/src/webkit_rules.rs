//! Extensions' network rules on WebKit (macOS, Linux): converted to a content
//! rule list on a worker thread, compiled by WebKit, then attached to every
//! web view.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};

use domain::Extensions;
use wry::WebView;

use crate::extensions::{rule_set, safari};

/// The list's name in WebKit's store: compiling again replaces it.
const IDENTIFIER: &str = "browsingunicorns-extensions";

type Compiled = platform::Compiled;

#[derive(Default)]
enum State {
    #[default]
    Idle,
    /// The rules of generation `.0`, compiled (`None`: no rules).
    Ready(u64, Option<Compiled>),
}

pub struct RuleLists {
    extensions_dir: PathBuf,
    store_dir: PathBuf,
    generation: u64,
    tx: Sender<(u64, Option<String>)>,
    rx: Receiver<(u64, Option<String>)>,
    /// Filled by WebKit's completion callback, on the UI thread.
    compiled: Rc<RefCell<State>>,
    /// What the web views have.
    current: Option<Compiled>,
    waker: crate::wry_engine::Waker,
}

impl RuleLists {
    pub fn new(
        extensions_dir: PathBuf,
        store_dir: PathBuf,
        waker: crate::wry_engine::Waker,
    ) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            extensions_dir,
            store_dir,
            generation: 0,
            tx,
            rx,
            compiled: Rc::default(),
            current: None,
            waker,
        }
    }

    /// Converts the rules of `extensions` in the background.
    pub fn update(&mut self, extensions: &Extensions) {
        self.generation += 1;
        let (generation, dir, extensions, tx, waker) = (
            self.generation,
            self.extensions_dir.clone(),
            extensions.clone(),
            self.tx.clone(),
            self.waker.clone(),
        );
        std::thread::spawn(move || {
            let rules = rule_set(&dir, &extensions);
            let json = (!rules.is_empty()).then(|| {
                let (json, count) = safari::content_rule_list(&rules);
                log::info!("extensions: {count} WebKit rules");
                json
            });
            let _ = tx.send((generation, json));
            waker();
        });
    }

    /// Call once per frame. Hands converted rules to WebKit, and returns
    /// the compiled list when the web views should change to it.
    pub fn poll(&mut self) -> Option<Option<&Compiled>> {
        while let Ok((generation, json)) = self.rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            match json {
                None => *self.compiled.borrow_mut() = State::Ready(generation, None),
                Some(json) => {
                    let slot = self.compiled.clone();
                    let waker = self.waker.clone();
                    platform::compile(
                        &self.store_dir,
                        IDENTIFIER,
                        json,
                        Box::new(move |result| {
                            match result {
                                Ok(list) => {
                                    *slot.borrow_mut() = State::Ready(generation, Some(list))
                                }
                                Err(e) => log::warn!("extensions: WebKit refused the rules: {e}"),
                            }
                            waker();
                        }),
                    );
                }
            }
        }
        let state = std::mem::take(&mut *self.compiled.borrow_mut());
        match state {
            State::Ready(generation, list) if generation == self.generation => {
                self.current = list;
                Some(self.current.as_ref())
            }
            _ => None,
        }
    }

    /// Gives a new web view the current rules.
    pub fn attach(&self, view: &WebView) {
        if self.current.is_some() {
            platform::apply(view, self.current.as_ref());
        }
    }

    pub fn apply(view: &WebView, list: Option<&Compiled>) {
        platform::apply(view, list);
    }
}

type Done = Box<dyn FnOnce(Result<Compiled, String>)>;

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::Cell;
    use std::path::Path;

    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_foundation::{NSError, NSString};
    use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore};
    use wry::{WebView, WebViewExtMacOS};

    use super::Done;

    pub type Compiled = Retained<WKContentRuleList>;

    /// In WebKit's default store (the app's own container).
    pub fn compile(_store_dir: &Path, identifier: &str, json: String, done: Done) {
        let Some(mtm) = MainThreadMarker::new() else {
            done(Err("not on the main thread".into()));
            return;
        };
        let Some(store) = (unsafe { WKContentRuleListStore::defaultStore(mtm) }) else {
            done(Err("no content rule list store".into()));
            return;
        };
        let done = Cell::new(Some(done));
        let block =
            block2::RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
                let Some(done) = done.take() else { return };
                match unsafe { Retained::retain(list) } {
                    Some(list) => done(Ok(list)),
                    None => {
                        let message = unsafe { error.as_ref() }
                            .map(|e| e.localizedDescription().to_string())
                            .unwrap_or_else(|| "unknown error".into());
                        done(Err(message));
                    }
                }
            });
        unsafe {
            store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                Some(&NSString::from_str(identifier)),
                Some(&NSString::from_str(&json)),
                Some(&block),
            );
        }
    }

    pub fn apply(view: &WebView, list: Option<&Compiled>) {
        let manager = view.manager();
        unsafe {
            manager.removeAllContentRuleLists();
            if let Some(list) = list {
                manager.addContentRuleList(list);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::ffi::CString;
    use std::path::Path;
    use std::ptr;

    use glib::translate::{FromGlibPtrFull, ToGlibPtr};
    use webkit2gtk::WebViewExt;
    use webkit2gtk_sys as ffi;
    use wry::{WebView, WebViewExtUnix};

    use super::Done;

    /// A compiled `WebKitUserContentFilter` (reference counted by WebKit).
    pub struct Compiled(*mut ffi::WebKitUserContentFilter);

    impl Drop for Compiled {
        fn drop(&mut self) {
            unsafe { ffi::webkit_user_content_filter_unref(self.0) };
        }
    }

    unsafe extern "C" fn saved(
        store: *mut gobject_sys::GObject,
        result: *mut gio_sys::GAsyncResult,
        data: glib_sys::gpointer,
    ) {
        let done: Box<Done> = unsafe { Box::from_raw(data.cast()) };
        let mut error = ptr::null_mut();
        let filter = unsafe {
            ffi::webkit_user_content_filter_store_save_finish(store.cast(), result, &mut error)
        };
        if filter.is_null() {
            let message = if error.is_null() {
                "unknown error".to_owned()
            } else {
                unsafe { glib::Error::from_glib_full(error) }.to_string()
            };
            done(Err(message));
        } else {
            done(Ok(Compiled(filter)));
        }
        // The store was created for this one compilation.
        unsafe { gobject_sys::g_object_unref(store) };
    }

    pub fn compile(store_dir: &Path, identifier: &str, json: String, done: Done) {
        let (Ok(path), Ok(id)) = (
            CString::new(store_dir.to_string_lossy().as_bytes()),
            CString::new(identifier),
        ) else {
            done(Err("bad store path".into()));
            return;
        };
        let bytes = glib::Bytes::from_owned(json.into_bytes());
        let data: Box<Done> = Box::new(done);
        unsafe {
            let store = ffi::webkit_user_content_filter_store_new(path.as_ptr());
            ffi::webkit_user_content_filter_store_save(
                store,
                id.as_ptr(),
                bytes.to_glib_none().0,
                ptr::null_mut(),
                Some(saved),
                Box::into_raw(data).cast(),
            );
        }
    }

    pub fn apply(view: &WebView, list: Option<&Compiled>) {
        let Some(manager) = view.webview().user_content_manager() else {
            return;
        };
        let manager: *mut ffi::WebKitUserContentManager = manager.to_glib_none().0;
        unsafe {
            ffi::webkit_user_content_manager_remove_all_filters(manager);
            if let Some(list) = list {
                ffi::webkit_user_content_manager_add_filter(manager, list.0);
            }
        }
    }
}
