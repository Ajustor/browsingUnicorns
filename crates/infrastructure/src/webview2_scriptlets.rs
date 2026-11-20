//! Windows: the ad blocker's scriptlets, added at the start of each
//! top-level navigation (`NavigationStarting`), so they run before the
//! page's own scripts. A frame's own navigations are not seen, but a script
//! added for a host also runs in that host's frames (the script guards on
//! the hostname).
//!
//! Adding a script is asynchronous, and WebView2 does not wait for it: a
//! document created before the add completes runs without it (seen on
//! Twitch: the page was there 70 ms after `NavigationStarting`, the script
//! 450 ms after). So when the page of a navigation waiting for its site's
//! script commits (`ContentLoading`) before the add completes, it is
//! reloaded once the script is in. A reload keeps the history as the page
//! made it (a redirect's `location.replace`, a link's new entry), which
//! making the navigation again could not: what started it cannot be told
//! (`IsUserInitiated` and the page's user activation were seen set on pages
//! nobody touched). Neither can WebView2 wait for it: a deferred document
//! request stops the add from completing. Not reloaded: a navigation with a
//! body (a form's POST would be sent again) or through the history (it may
//! be a POST's result); its script runs from the next document.

use std::cell::RefCell;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_NAVIGATION_KIND_NEW_DOCUMENT, COREWEBVIEW2_NAVIGATION_KIND_RELOAD, ICoreWebView2,
    ICoreWebView2NavigationStartingEventArgs, ICoreWebView2NavigationStartingEventArgs3,
};
use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, ContentLoadingEventHandler,
    NavigationStartingEventHandler, take_pwstr,
};
use windows_core::{BOOL, HSTRING, Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::blocker::{BlockerChanges, BlockerHandle};
use crate::site_scripts::{self, SiteScripts};

/// A site's script on its way, and the tab's last navigation to the site
/// meanwhile.
#[derive(Debug, PartialEq)]
struct Waiting {
    host: String,
    /// The add it waits for.
    add: u64,
    /// That navigation's id.
    navigation: u64,
    /// Its page committed (before the script was in).
    committed: bool,
    /// It may be reloaded.
    reloadable: bool,
}

/// What to do once an add completed.
#[derive(Debug, PartialEq, Default)]
struct AfterAdd {
    /// An id to remove (a clear came in between).
    remove: Option<String>,
    /// The page committed without the script: reload it.
    reload: bool,
}

/// A tab's sites, with the ids WebView2 gave their scripts, and how many
/// times the blocker's changes cleared them (an id arriving after a clear
/// belongs to a script that must go).
#[derive(Default)]
struct Sites {
    scripts: SiteScripts<String>,
    changes: BlockerChanges,
    epoch: u64,
    /// Adds started, to tell their completions apart.
    adds: u64,
    /// The id of the tab's last navigation.
    current: u64,
    waiting: Option<Waiting>,
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

    /// Navigation `navigation` to `uri` starts (a redirect is the same
    /// navigation again); `reloadable`: its page may be reloaded. The
    /// script to add for it, numbered, if any.
    fn start(
        &mut self,
        uri: &str,
        navigation: u64,
        reloadable: bool,
        scriptlets: impl FnOnce(&str) -> String,
    ) -> Option<(String, u64)> {
        self.current = navigation;
        let host = site_scripts::web_host(uri);
        if let Some(script) = self.scripts.script_for(uri, scriptlets) {
            self.adds += 1;
            self.waiting = host.map(|host| Waiting {
                host,
                add: self.adds,
                navigation,
                committed: false,
                reloadable,
            });
            return Some((script, self.adds));
        }
        // Its site's script is on its way: this page is the one to watch.
        if let Some(w) = &mut self.waiting
            && host.as_deref() == Some(w.host.as_str())
        {
            w.navigation = navigation;
            w.committed = false;
            w.reloadable = reloadable;
        }
        None
    }

    /// The page of navigation `navigation` committed.
    fn committed(&mut self, navigation: u64) {
        if let Some(w) = &mut self.waiting
            && w.navigation == navigation
        {
            w.committed = true;
        }
    }

    /// Add `add`, for `host` at `epoch`, completed: `Ok(id)` or refused.
    fn after_add(
        &mut self,
        host: &str,
        add: u64,
        epoch: u64,
        result: Result<String, ()>,
    ) -> AfterAdd {
        let (remove, ok) = match result {
            Ok(id) => (self.added(id, epoch), true),
            Err(()) => {
                self.refused(host, epoch);
                (None, false)
            }
        };
        let reload = match self.waiting.take() {
            // Still on that site, its page there without the script.
            Some(w) if w.add == add => {
                ok && w.committed && w.reloadable && w.navigation == self.current
            }
            // A newer add's (after a clear): it decides.
            other => {
                self.waiting = other;
                false
            }
        };
        AfterAdd { remove, reload }
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

/// Whether a navigation's page may be reloaded: a new document or a reload
/// (not through the history, nor of an unknown kind), without a body, which
/// a reload would send again.
fn reloadable(kind: Option<Kind>, body: bool) -> bool {
    !body && matches!(kind, Some(Kind::NewDocument | Kind::Reload))
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
        // view's source is then still the previous page). The same address
        // again replaces the entry.
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
/// reloading a page that came before its script. A document is always
/// created after `NavigationStarting` (reloads and history included), so
/// new lists or settings are noticed there.
pub fn attach(view: &WebView, blocker: BlockerHandle) -> windows_core::Result<Scriptlets> {
    let webview = view.webview();
    let sites = Rc::new(RefCell::new(Sites::default()));
    let tab = Scriptlets {
        webview: webview.clone(),
        sites: sites.clone(),
        blocker: blocker.clone(),
    };
    let loading_sites = sites.clone();
    let loading = ContentLoadingEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut navigation = 0;
        unsafe { args.NavigationId(&mut navigation)? };
        loading_sites.borrow_mut().committed(navigation);
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_ContentLoading(&loading, &mut token)? };
    let handler = NavigationStartingEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        // The navigation's own address: a redirect starts its own.
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri)? };
        let uri = take_pwstr(uri);
        let mut navigation = 0;
        unsafe { args.NavigationId(&mut navigation)? };
        let body = has_body(&args);
        let reloadable = reloadable(kind(&args), body);
        let (stale, script, epoch) = {
            let mut sites = sites.borrow_mut();
            // One with a body is not made again.
            if body {
                sites.missed = None;
            } else {
                sites.navigated(&uri, &blocker);
            }
            let stale = sites.stale(&blocker);
            let script = sites.start(&uri, navigation, reloadable, |url| blocker.scriptlets(url));
            (stale, script, sites.epoch)
        };
        for id in stale {
            remove(&sender, id);
        }
        let Some((script, add)) = script else {
            return Ok(());
        };
        let host = site_scripts::web_host(&uri).unwrap_or_default();
        let (done_sites, webview, done_host) = (sites.clone(), sender.clone(), host.clone());
        let done = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |result, id| {
                let result = result.map(|()| id).map_err(|e| {
                    log::warn!("scriptlets: adding: {e}");
                });
                let after = done_sites
                    .borrow_mut()
                    .after_add(&done_host, add, epoch, result);
                if let Some(id) = after.remove {
                    remove(&webview, id);
                }
                if after.reload
                    && let Err(e) = unsafe { webview.Reload() }
                {
                    log::warn!("scriptlets: reloading: {e}");
                }
                Ok(())
            },
        ));
        if let Err(e) = unsafe {
            sender.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script.as_str()), &done)
        } {
            log::warn!("scriptlets: adding: {e}");
            sites.borrow_mut().after_add(&host, add, epoch, Err(()));
        }
        Ok(())
    }));
    unsafe { webview.add_NavigationStarting(&handler, &mut token)? };
    Ok(tab)
}

/// A navigation's kind; `None` when WebView2 does not tell (older runtimes).
fn kind(args: &ICoreWebView2NavigationStartingEventArgs) -> Option<Kind> {
    let args = args
        .cast::<ICoreWebView2NavigationStartingEventArgs3>()
        .ok()?;
    let mut kind = Default::default();
    unsafe { args.NavigationKind(&mut kind) }.ok()?;
    Some(match kind {
        COREWEBVIEW2_NAVIGATION_KIND_NEW_DOCUMENT => Kind::NewDocument,
        COREWEBVIEW2_NAVIGATION_KIND_RELOAD => Kind::Reload,
        _ => Kind::BackOrForward,
    })
}

/// Whether a navigation carries a body (a form's POST): it has a
/// `Content-Type` (seen on POSTs, not on GETs). Unknown: taken as one.
fn has_body(args: &ICoreWebView2NavigationStartingEventArgs) -> bool {
    let Ok(headers) = (unsafe { args.RequestHeaders() }) else {
        return true;
    };
    let mut found = BOOL::default();
    unsafe { headers.Contains(&HSTRING::from("Content-Type"), &mut found) }
        .map_or(true, |()| found.as_bool())
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
    const NONE: fn(&str) -> String = |_| String::new();

    fn add_of(start: Option<(String, u64)>) -> u64 {
        start.expect("a script to add").1
    }

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
    fn a_page_there_before_its_script_is_reloaded() {
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, SCRIPT));
        sites.committed(1);
        let after = sites.after_add("a.test", add, sites.epoch, Ok("id".into()));
        assert!(after.reload);
        assert_eq!(after.remove, None);
        // Reloaded: the site has its script, nothing waits.
        assert_eq!(sites.start("https://a.test/", 2, true, SCRIPT), None);
        sites.committed(2);
        assert_eq!(sites.waiting, None);
    }

    #[test]
    fn a_page_not_there_yet_gets_the_script_without_a_reload() {
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, SCRIPT));
        let after = sites.after_add("a.test", add, sites.epoch, Ok("id".into()));
        assert!(!after.reload);
        sites.committed(1);
        assert_eq!(sites.waiting, None);
    }

    #[test]
    fn the_last_navigation_to_the_site_meanwhile_is_the_one_watched() {
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/1", 1, true, SCRIPT));
        sites.committed(1);
        // Again to the site (a second click) before the script is in.
        assert_eq!(sites.start("https://a.test/2", 2, true, SCRIPT), None);
        let after = sites.after_add("a.test", add, sites.epoch, Ok("id".into()));
        assert!(!after.reload, "the second page is not there yet");

        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/1", 1, true, SCRIPT));
        assert_eq!(sites.start("https://a.test/2", 2, true, SCRIPT), None);
        sites.committed(1);
        sites.committed(2);
        let after = sites.after_add("a.test", add, sites.epoch, Ok("id".into()));
        assert!(after.reload, "the second page came before the script");
    }

    #[test]
    fn a_tab_gone_elsewhere_is_not_reloaded() {
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, SCRIPT));
        sites.committed(1);
        assert_eq!(sites.start("https://b.test/", 2, true, NONE), None);
        sites.committed(2);
        let after = sites.after_add("a.test", add, sites.epoch, Ok("id".into()));
        assert!(!after.reload);
        assert_eq!(sites.waiting, None);
    }

    #[test]
    fn a_page_that_cannot_be_reloaded_waits_for_the_next_one() {
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, false, SCRIPT));
        sites.committed(1);
        let after = sites.after_add("a.test", add, sites.epoch, Ok("id".into()));
        assert!(!after.reload);
    }

    #[test]
    fn a_refused_script_reloads_nothing_and_is_asked_again() {
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, SCRIPT));
        sites.committed(1);
        let after = sites.after_add("a.test", add, sites.epoch, Err(()));
        assert_eq!(after, AfterAdd::default());
        assert!(sites.start("https://a.test/", 2, true, SCRIPT).is_some());
    }

    #[test]
    fn an_add_from_before_a_change_neither_stays_nor_decides_for_the_new_one() {
        let blocker = BlockerHandle::default();
        let mut sites = Sites::default();
        sites.stale(&blocker);
        let old_epoch = sites.epoch;
        let old = add_of(sites.start("https://a.test/1", 1, true, SCRIPT));
        sites.committed(1);
        // New lists, and the site again before the first add completed.
        blocker.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        sites.stale(&blocker);
        let new = add_of(sites.start("https://a.test/2", 2, true, SCRIPT));
        sites.committed(2);
        let after = sites.after_add("a.test", old, old_epoch, Ok("old".into()));
        assert_eq!(after.remove, Some("old".into()));
        assert!(!after.reload, "the new script is not in yet");
        let after = sites.after_add("a.test", new, sites.epoch, Ok("new".into()));
        assert_eq!(after.remove, None);
        assert!(after.reload);
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
    fn only_new_documents_and_reloads_without_a_body_are_reloaded() {
        assert!(reloadable(Some(Kind::NewDocument), false));
        assert!(reloadable(Some(Kind::Reload), false));
        assert!(!reloadable(Some(Kind::BackOrForward), false));
        assert!(!reloadable(None, false), "unknown kind");
        assert!(!reloadable(Some(Kind::NewDocument), true));
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
