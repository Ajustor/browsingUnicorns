//! Windows: the ad blocker's scriptlets, added at the start of each
//! top-level navigation (`NavigationStarting`), so they run before the
//! page's own scripts. A frame's own navigations are not seen, but a script
//! added for a host also runs in that host's frames (the script guards on
//! the hostname).
//!
//! Adding a script is asynchronous, and WebView2 does not wait for it: a
//! document created before the add completes runs without it (seen on
//! Twitch: the page was there 70 ms after `NavigationStarting`, the script
//! 450 ms after). So a navigation that needs a new script is cancelled, and
//! made again once the script is in. Not a navigation with a body (a form's
//! POST would come back as a GET) nor one through the history (it would
//! become a new entry): the script then runs from the next document.

use std::cell::RefCell;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD, COREWEBVIEW2_NAVIGATION_KIND_RELOAD,
    ICoreWebView2, ICoreWebView2ExecuteScriptCompletedHandler,
    ICoreWebView2NavigationStartingEventArgs, ICoreWebView2NavigationStartingEventArgs3,
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, NavigationStartingEventHandler, take_pwstr,
};
use windows_core::{BOOL, HSTRING, Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::blocker::{BlockerChanges, BlockerHandle};
use crate::site_scripts::{self, SiteScripts};

/// A tab's sites, with the ids WebView2 gave their scripts, and how many
/// times the blocker's changes cleared them (an id arriving after a clear
/// belongs to a script that must go).
#[derive(Default)]
struct Sites {
    scripts: SiteScripts<String>,
    changes: BlockerChanges,
    epoch: u64,
    /// Top-level navigations seen: a held one goes on only when none came
    /// since.
    navigations: u64,
    /// An address to let through once without a script (its add failed).
    pass: Option<String>,
    /// The address of the last navigation, when it came before the blocker
    /// had an engine.
    missed: Option<String>,
}

impl Sites {
    /// When `blocker` changed since the last navigation: forgets the sites,
    /// the ids of their scripts to remove.
    fn stale(&mut self, blocker: &BlockerHandle) -> Vec<String> {
        if !self.changes.changed(blocker) {
            return Vec::new();
        }
        self.epoch += 1;
        self.scripts.clear()
    }

    /// Keeps `id`, added at `epoch`; hands it back to remove when a clear
    /// came in between.
    fn added(&mut self, id: String, epoch: u64) -> Option<String> {
        if epoch != self.epoch {
            return Some(id);
        }
        self.scripts.keep(id);
        None
    }

    /// Forgets `host` after a refused add, unless a clear came in since
    /// `epoch` (the host is then already forgotten), so its next navigation
    /// tries again.
    fn refused(&mut self, host: &str, epoch: u64) {
        if epoch == self.epoch {
            self.scripts.forget(host);
        }
    }

    /// Counts a navigation; its number.
    fn begin(&mut self) -> u64 {
        self.navigations += 1;
        self.navigations
    }

    /// Whether navigation `seq` is still the last one.
    fn current(&self, seq: u64) -> bool {
        seq == self.navigations
    }

    /// Lets the next navigation to `uri` through without adding a script
    /// (made again after a failed add: it would fail and hold it again).
    fn let_through(&mut self, uri: String) {
        self.pass = Some(uri);
    }

    /// Whether a navigation to `uri` is the one to let through.
    fn passes(&mut self, uri: &str) -> bool {
        if self.pass.as_deref() == Some(uri) {
            self.pass = None;
            return true;
        }
        false
    }

    /// Notes a navigation to `uri`, and whether it came before the blocker
    /// had an engine.
    fn navigated(&mut self, uri: &str, blocker: &BlockerHandle) {
        self.missed = blocker.get().is_none().then(|| uri.to_owned());
    }

    /// The address of the navigation that came before the engine, to make
    /// again now that one is here (once).
    fn catch_up(&mut self, blocker: &BlockerHandle) -> Option<String> {
        if self.missed.is_none() || blocker.get().is_none() {
            return None;
        }
        self.missed.take()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    NewDocument,
    Reload,
    BackOrForward,
}

/// Whether a navigation needing a new script is held until it is in: not
/// through the history, nor with a body, which making it again would lose.
fn hold(kind: Kind, body: bool) -> bool {
    !body && kind != Kind::BackOrForward
}

/// A tab's scriptlets, for [`Scriptlets::catch_up`].
pub struct Scriptlets {
    webview: ICoreWebView2,
    sites: Rc<RefCell<Sites>>,
    blocker: BlockerHandle,
}

impl Scriptlets {
    /// Call each frame: the page loaded before the blocker had an engine
    /// (at start, the cached lists take a moment) loads again once it has
    /// one, when its site has scriptlets.
    pub fn catch_up(&self) {
        // That navigation's address: it may not have committed yet (the
        // view's source is then still the previous page).
        let Some(uri) = self.sites.borrow_mut().catch_up(&self.blocker) else {
            return;
        };
        if site_scripts::web_host(&uri).is_some()
            && !self.blocker.scriptlets(&uri).is_empty()
            && let Err(e) = unsafe { self.webview.Navigate(&HSTRING::from(uri.as_str())) }
        {
            log::warn!("scriptlets: loading again: {e}");
        }
    }
}

/// Adds the scriptlets of each site `view` navigates to, once per site,
/// holding the navigation until the script is in. A document is always
/// created after `NavigationStarting` (reloads and history included), so new
/// lists or settings are noticed there.
pub fn attach(view: &WebView, blocker: BlockerHandle) -> windows_core::Result<Scriptlets> {
    let webview = view.webview();
    let sites = Rc::new(RefCell::new(Sites::default()));
    let tab = Scriptlets {
        webview: webview.clone(),
        sites: sites.clone(),
        blocker: blocker.clone(),
    };
    let handler = NavigationStartingEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        // The navigation's own address: a redirect starts its own.
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri)? };
        let uri = take_pwstr(uri);
        let host = site_scripts::web_host(&uri);
        let body = has_body(&args);
        let (stale, script, epoch, seq) = {
            let mut sites = sites.borrow_mut();
            let seq = sites.begin();
            // One with a body is not made again.
            if body {
                sites.missed = None;
            } else {
                sites.navigated(&uri, &blocker);
            }
            let stale = sites.stale(&blocker);
            let script = if sites.passes(&uri) {
                None
            } else {
                sites
                    .scripts
                    .script_for(&uri, |url| blocker.scriptlets(url))
            };
            (stale, script, sites.epoch, seq)
        };
        for id in stale {
            remove(&sender, id);
        }
        let Some(script) = script else {
            return Ok(());
        };
        let held = hold(kind(&args), body);
        let (done_sites, webview, done_uri) = (sites.clone(), sender.clone(), uri.clone());
        let done = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |result, id| {
                let mut sites = done_sites.borrow_mut();
                let failed = match result {
                    Ok(()) => {
                        if let Some(id) = sites.added(id, epoch) {
                            remove(&webview, id);
                        }
                        false
                    }
                    Err(e) => {
                        log::warn!("scriptlets: adding: {e}");
                        if let Some(host) = &host {
                            sites.refused(host, epoch);
                        }
                        true
                    }
                };
                // Held, and the tab did not go elsewhere since: on its way.
                if held && sites.current(seq) {
                    if failed {
                        sites.let_through(done_uri.clone());
                    }
                    drop(sites);
                    go_again(&webview, &done_uri);
                }
                Ok(())
            },
        ));
        match unsafe {
            sender.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script.as_str()), &done)
        } {
            Ok(()) if held => unsafe { args.SetCancel(true)? },
            Ok(()) => {}
            Err(e) => {
                log::warn!("scriptlets: adding: {e}");
                if let Some(host) = site_scripts::web_host(&uri) {
                    sites.borrow_mut().refused(&host, epoch);
                }
            }
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NavigationStarting(&handler, &mut token)? };
    Ok(tab)
}

/// A navigation's kind (a new document when WebView2 does not tell).
fn kind(args: &ICoreWebView2NavigationStartingEventArgs) -> Kind {
    let Ok(args) = args.cast::<ICoreWebView2NavigationStartingEventArgs3>() else {
        return Kind::NewDocument;
    };
    let mut kind = Default::default();
    if unsafe { args.NavigationKind(&mut kind) }.is_err() {
        return Kind::NewDocument;
    }
    match kind {
        COREWEBVIEW2_NAVIGATION_KIND_RELOAD => Kind::Reload,
        COREWEBVIEW2_NAVIGATION_KIND_BACK_OR_FORWARD => Kind::BackOrForward,
        _ => Kind::NewDocument,
    }
}

/// Whether a navigation carries a body (a form's POST): it has a
/// `Content-Type`. Unknown: taken as one, never to lose it.
fn has_body(args: &ICoreWebView2NavigationStartingEventArgs) -> bool {
    let Ok(headers) = (unsafe { args.RequestHeaders() }) else {
        return true;
    };
    let mut found = BOOL::default();
    unsafe { headers.Contains(&HSTRING::from("Content-Type"), &mut found) }
        .map_or(true, |()| found.as_bool())
}

/// Makes a held navigation to `uri` again. From the blank page a new tab
/// starts on, in its place: once the tab's first navigation is cancelled,
/// that page is in the history, and Back would lead to it.
fn go_again(webview: &ICoreWebView2, uri: &str) {
    let mut source = PWSTR::null();
    let blank =
        unsafe { webview.Source(&mut source) }.is_ok() && take_pwstr(source) == "about:blank";
    let result = if blank {
        let uri = serde_json::to_string(uri).expect("a string");
        let script = HSTRING::from(format!("location.replace({uri})"));
        unsafe {
            webview.ExecuteScript(&script, None::<&ICoreWebView2ExecuteScriptCompletedHandler>)
        }
    } else {
        unsafe { webview.Navigate(&HSTRING::from(uri)) }
    };
    if let Err(e) = result {
        log::warn!("scriptlets: navigating again: {e}");
    }
}

fn remove(webview: &ICoreWebView2, id: String) {
    if let Err(e) = unsafe { webview.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id)) } {
        log::warn!("scriptlets: removing: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocker::BlockerPolicy;

    const SCRIPT: fn(&str) -> String = |_| "x();".into();

    #[test]
    fn a_change_hands_the_scripts_back_and_forgets_the_sites() {
        let blocker = BlockerHandle::default();
        let mut sites = Sites::default();
        assert!(sites.stale(&blocker).is_empty());
        assert!(
            sites
                .scripts
                .script_for("https://site.test/", SCRIPT)
                .is_some()
        );
        assert_eq!(sites.added("a".into(), sites.epoch), None);
        assert!(sites.stale(&blocker).is_empty());
        assert_eq!(sites.scripts.script_for("https://site.test/", SCRIPT), None);

        blocker.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        assert_eq!(sites.stale(&blocker), vec!["a".to_string()]);
        assert!(
            sites
                .scripts
                .script_for("https://site.test/", SCRIPT)
                .is_some()
        );
    }

    #[test]
    fn a_held_navigation_goes_on_when_none_came_since() {
        let mut sites = Sites::default();
        let first = sites.begin();
        assert!(sites.current(first));
        let second = sites.begin();
        assert!(!sites.current(first));
        assert!(sites.current(second));
    }

    #[test]
    fn a_refused_address_goes_through_once_without_a_script() {
        let mut sites = Sites::default();
        sites.begin();
        sites.let_through("https://a.test/".into());
        assert!(!sites.passes("https://b.test/"));
        assert!(sites.passes("https://a.test/"));
        assert!(!sites.passes("https://a.test/"));
    }

    #[test]
    fn a_page_loaded_before_the_engine_is_caught_up_once() {
        let blocker = BlockerHandle::default();
        let mut sites = Sites::default();
        sites.navigated("https://a.test/", &blocker);
        sites.navigated("https://b.test/", &blocker);
        assert_eq!(sites.catch_up(&blocker), None, "no engine yet");
        blocker.set(crate::blocker::Blocker::from_lists(&["||ads.test^"]).unwrap());
        assert_eq!(sites.catch_up(&blocker).as_deref(), Some("https://b.test/"));
        assert_eq!(sites.catch_up(&blocker), None);
        // A page loaded with the engine needs nothing.
        sites.navigated("https://c.test/", &blocker);
        assert_eq!(sites.catch_up(&blocker), None);
    }

    #[test]
    fn only_new_documents_and_reloads_without_a_body_are_held() {
        assert!(hold(Kind::NewDocument, false));
        assert!(hold(Kind::Reload, false));
        assert!(!hold(Kind::BackOrForward, false));
        assert!(!hold(Kind::NewDocument, true));
    }

    #[test]
    fn an_id_from_before_a_change_is_handed_back() {
        let blocker = BlockerHandle::default();
        let mut sites = Sites::default();
        sites.stale(&blocker);
        let epoch = sites.epoch;
        blocker.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        sites.stale(&blocker);
        assert_eq!(sites.added("late".into(), epoch), Some("late".into()));
        assert!(sites.stale(&blocker).is_empty());
    }
}
