//! Content rule lists on WebKit (macOS, Linux): the extensions' network
//! rules and the ad blocker's, each converted on a worker thread, compiled
//! by WebKit, then attached to every web view.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};

use domain::Extensions;
use wry::WebView;

use crate::extensions::{rule_set, safari};
use crate::wry_engine::Waker;

/// The lists' names in WebKit's store: compiling again replaces them.
const EXTENSIONS: &str = "browsingunicorns-extensions";
const BLOCKER: &str = "bu-blocker";

type Compiled = platform::Compiled;

#[derive(Default)]
enum State {
    #[default]
    Idle,
    /// The rules of generation `.0`, compiled (`None`: no rules).
    Ready(u64, Option<Compiled>),
}

/// One content rule list: the latest conversion asked wins.
struct Slot {
    identifier: &'static str,
    generation: u64,
    /// The generation whose compiled rules are wanted: an earlier one when
    /// the latest conversion gave the same rules (not compiled again).
    wanted: u64,
    /// The generation and hash of the last rules handed to WebKit.
    sent: Option<(u64, u64)>,
    tx: Sender<(u64, Option<String>)>,
    rx: Receiver<(u64, Option<String>)>,
    /// Filled by WebKit's completion callback, on the UI thread.
    compiled: Rc<RefCell<State>>,
    /// What the web views have.
    current: Option<Compiled>,
}

fn hash_of(json: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    json.hash(&mut hasher);
    hasher.finish()
}

impl Slot {
    fn new(identifier: &'static str) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            identifier,
            generation: 0,
            wanted: 0,
            sent: None,
            tx,
            rx,
            compiled: Rc::default(),
            current: None,
        }
    }

    /// Runs `convert` (the list as JSON, `None` for no rules) in the
    /// background.
    fn update(&mut self, waker: &Waker, convert: impl FnOnce() -> Option<String> + Send + 'static) {
        self.generation += 1;
        let (generation, tx, waker) = (self.generation, self.tx.clone(), waker.clone());
        std::thread::spawn(move || {
            let json = convert();
            let _ = tx.send((generation, json));
            waker();
        });
    }

    /// Hands converted rules to WebKit; whether `current` changed.
    fn poll(&mut self, store_dir: &Path, waker: &Waker) -> bool {
        while let Ok((generation, json)) = self.rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            match json {
                None => {
                    self.wanted = generation;
                    self.sent = None;
                    *self.compiled.borrow_mut() = State::Ready(generation, None);
                }
                // The same rules as last handed to WebKit: theirs are wanted,
                // compiled or on their way (compiling is WebKit's costly part).
                Some(json) if self.sent.is_some_and(|(_, hash)| hash == hash_of(&json)) => {
                    if let Some((sent, _)) = self.sent {
                        self.wanted = sent;
                    }
                }
                Some(json) => {
                    self.wanted = generation;
                    self.sent = Some((generation, hash_of(&json)));
                    let slot = self.compiled.clone();
                    let waker = waker.clone();
                    let identifier = self.identifier;
                    platform::compile(
                        store_dir,
                        identifier,
                        json,
                        Box::new(move |result| {
                            match result {
                                Ok(list) => {
                                    *slot.borrow_mut() = State::Ready(generation, Some(list))
                                }
                                Err(e) => {
                                    log::warn!("{identifier}: WebKit refused the rules: {e}")
                                }
                            }
                            waker();
                        }),
                    );
                }
            }
        }
        let state = std::mem::take(&mut *self.compiled.borrow_mut());
        match state {
            State::Ready(generation, list) if generation == self.wanted => {
                self.current = list;
                true
            }
            _ => false,
        }
    }
}

pub struct RuleLists {
    extensions_dir: PathBuf,
    state_dir: PathBuf,
    store_dir: PathBuf,
    extensions: Slot,
    blocker: Slot,
    waker: Waker,
}

impl RuleLists {
    pub fn new(
        extensions_dir: PathBuf,
        state_dir: PathBuf,
        store_dir: PathBuf,
        waker: Waker,
    ) -> Self {
        Self {
            extensions_dir,
            state_dir,
            store_dir,
            extensions: Slot::new(EXTENSIONS),
            blocker: Slot::new(BLOCKER),
            waker,
        }
    }

    /// Converts the rules of `extensions` in the background.
    pub fn update(&mut self, extensions: &Extensions) {
        let (dir, state_dir, extensions) = (
            self.extensions_dir.clone(),
            self.state_dir.clone(),
            extensions.clone(),
        );
        self.extensions.update(&self.waker, move || {
            let rules = rule_set(&dir, Some(&state_dir), &extensions);
            (!rules.is_empty()).then(|| {
                let (json, count) = safari::content_rule_list(&rules);
                log::info!("extensions: {count} WebKit rules");
                json
            })
        });
    }

    /// The ad blocker's rules, from `convert` in the background (`None`:
    /// none).
    pub fn update_blocker(&mut self, convert: impl FnOnce() -> Option<String> + Send + 'static) {
        self.blocker.update(&self.waker, convert);
    }

    /// Call once per frame. Hands converted rules to WebKit, and returns
    /// the compiled lists when the web views should change to them.
    pub fn poll(&mut self) -> Option<Vec<&Compiled>> {
        let extensions = self.extensions.poll(&self.store_dir, &self.waker);
        let blocker = self.blocker.poll(&self.store_dir, &self.waker);
        (extensions || blocker).then(|| self.lists())
    }

    fn lists(&self) -> Vec<&Compiled> {
        self.extensions
            .current
            .iter()
            .chain(self.blocker.current.iter())
            .collect()
    }

    /// Gives a new web view the current rules.
    pub fn attach(&self, view: &WebView) {
        let lists = self.lists();
        if !lists.is_empty() {
            platform::apply(view, &lists);
        }
    }

    pub fn apply(view: &WebView, lists: &[&Compiled]) {
        platform::apply(view, lists);
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

    pub fn apply(view: &WebView, lists: &[&Compiled]) {
        let manager = view.manager();
        unsafe {
            manager.removeAllContentRuleLists();
            for list in lists.iter().copied() {
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

    pub fn apply(view: &WebView, lists: &[&Compiled]) {
        let Some(manager) = view.webview().user_content_manager() else {
            return;
        };
        let manager: *mut ffi::WebKitUserContentManager = manager.to_glib_none().0;
        unsafe {
            ffi::webkit_user_content_manager_remove_all_filters(manager);
            for list in lists {
                ffi::webkit_user_content_manager_add_filter(manager, list.0);
            }
        }
    }
}
