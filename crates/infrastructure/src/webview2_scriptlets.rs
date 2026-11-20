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
//! reloaded once the script is in, if that is soon after (the user is not
//! using the page yet). A reload keeps the history as the page made it (a
//! redirect's `location.replace`, a link's new entry), which making the
//! navigation again could not: what started it cannot be told
//! (`IsUserInitiated` and the page's user activation were seen set on pages
//! nobody touched). Neither can WebView2 wait for it: a deferred document
//! request stops the add from completing. Not reloaded, its script running
//! from the next document: a navigation with a body (a form's POST would be
//! sent again), through the history (it may be a POST's result), or a
//! redirect's target with a query (a one-time link, a sign-in's return,
//! would be used twice; tokens on the redirecting address were used by the
//! first step, so `twitch.tv` to `www.twitch.tv` is reloaded).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

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

/// How long after its page committed a page may still be reloaded: later,
/// the user may be using it.
const RELOAD_WINDOW: Duration = Duration::from_millis(1500);

/// A site's script on its way, and the tab's last navigation to the site
/// meanwhile.
#[derive(Debug, PartialEq)]
struct Waiting {
    host: String,
    /// The add it waits for.
    add: u64,
    /// That navigation's id; `None` once it went elsewhere (a redirect to
    /// another site keeps its id).
    navigation: Option<u64>,
    /// When its page committed (before the script was in).
    committed: Option<Instant>,
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
    /// It did, but too long ago to reload.
    late: bool,
}

/// A navigation that came before the blocker had an engine.
#[derive(Debug, PartialEq)]
struct Missed {
    uri: String,
    navigation: u64,
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
    current: Option<u64>,
    /// The last page committed: its navigation, and when.
    last_committed: Option<(u64, Instant)>,
    waiting: Option<Waiting>,
    missed: Option<Missed>,
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

    /// Navigation `navigation` to `uri` starts (a server's redirect is the
    /// same navigation again, to its new address); `reloadable`: its page
    /// may be reloaded; `engine`: the blocker has one. The script to add
    /// for it, numbered, if any.
    fn start(
        &mut self,
        uri: &str,
        navigation: u64,
        reloadable: bool,
        engine: bool,
        scriptlets: impl FnOnce(&str) -> String,
    ) -> Option<(String, u64)> {
        let redirected = self.current == Some(navigation);
        self.current = Some(navigation);
        // A redirect's target with a query may be a one-time link (a
        // sign-in's return): its GET is not sent again. Tokens on the
        // redirecting address were used by the first step.
        let reloadable = reloadable && !(redirected && has_query(uri));
        self.missed = (!engine && reloadable).then(|| Missed {
            uri: uri.to_owned(),
            navigation,
        });
        let host = site_scripts::web_host(uri);
        if let Some(script) = self.scripts.script_for(uri, scriptlets) {
            return Some((script, self.wait(host, navigation, None, reloadable)));
        }
        if let Some(w) = &mut self.waiting {
            if host.as_deref() == Some(w.host.as_str()) {
                // Its site's script is on its way: this page is the one to
                // watch.
                w.navigation = Some(navigation);
                w.committed = None;
                w.reloadable = reloadable;
            } else if w.navigation == Some(navigation) {
                // Redirected to another site: not that site's page.
                w.navigation = None;
            }
        }
        None
    }

    /// Numbers an add for `host`, watching navigation `navigation`.
    fn wait(
        &mut self,
        host: Option<String>,
        navigation: u64,
        committed: Option<Instant>,
        reloadable: bool,
    ) -> u64 {
        self.adds += 1;
        self.waiting = host.map(|host| Waiting {
            host,
            add: self.adds,
            navigation: Some(navigation),
            committed,
            reloadable,
        });
        self.adds
    }

    /// The page of navigation `navigation` committed at `now` (not an error
    /// page: nothing to reload there).
    fn committed(&mut self, navigation: u64, error: bool, now: Instant) {
        if error {
            return;
        }
        self.last_committed = Some((navigation, now));
        if let Some(w) = &mut self.waiting
            && w.navigation == Some(navigation)
        {
            w.committed = Some(now);
        }
    }

    /// Add `add`, for `host` at `epoch`, completed at `now`: `Ok(id)` or
    /// refused.
    fn after_add(
        &mut self,
        host: &str,
        add: u64,
        epoch: u64,
        result: Result<String, ()>,
        now: Instant,
    ) -> AfterAdd {
        let (remove, ok) = match result {
            Ok(id) => (self.added(id, epoch), true),
            Err(()) => {
                self.refused(host, epoch);
                (None, false)
            }
        };
        let mut after = AfterAdd {
            remove,
            ..AfterAdd::default()
        };
        match self.waiting.take() {
            // Still on that site, its page there without the script (which
            // stays: not cleared since).
            Some(w) if w.add == add => {
                if let (true, None, true, true, Some(at)) = (
                    ok,
                    &after.remove,
                    w.reloadable,
                    w.navigation.is_some() && w.navigation == self.current,
                    w.committed,
                ) {
                    after.reload = now.saturating_duration_since(at) <= RELOAD_WINDOW;
                    after.late = !after.reload;
                }
            }
            // A newer add's (after a clear): it decides.
            other => self.waiting = other,
        }
        after
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

    /// Once the blocker has an engine (`engine`): the script for the
    /// navigation that came before it, numbered, waiting to reload its page
    /// (once).
    fn catch_up(
        &mut self,
        engine: bool,
        scriptlets: impl FnOnce(&str) -> String,
    ) -> Option<(String, String, u64)> {
        if !engine {
            return None;
        }
        let missed = self.missed.take()?;
        let host = site_scripts::web_host(&missed.uri)?;
        let script = self.scripts.script_for(&missed.uri, scriptlets)?;
        // Its page may be there already.
        let committed = self
            .last_committed
            .filter(|(n, _)| *n == missed.navigation)
            .map(|(_, at)| at);
        let add = self.wait(Some(host.clone()), missed.navigation, committed, true);
        Some((script, host, add))
    }
}

/// Whether `uri` has a query string.
fn has_query(uri: &str) -> bool {
    uri.split('#')
        .next()
        .and_then(|u| u.split_once('?'))
        .is_some_and(|(_, query)| !query.is_empty())
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
    /// (at start, the cached lists take a moment) gets its site's script
    /// once the engine is here, and is reloaded as a page that came before
    /// its script would be.
    pub fn catch_up(&self) {
        let (stale, caught, epoch) = {
            let mut sites = self.sites.borrow_mut();
            if sites.missed.is_none() || self.blocker.get().is_none() {
                return;
            }
            let stale = sites.stale(&self.blocker);
            let caught = sites.catch_up(true, |url| self.blocker.scriptlets(url));
            (stale, caught, sites.epoch)
        };
        for id in stale {
            remove(&self.webview, id);
        }
        if let Some((script, host, add)) = caught {
            add_script(&self.webview, &self.sites, script, host, add, epoch);
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
        let mut error = BOOL::default();
        unsafe { args.IsErrorPage(&mut error)? };
        loading_sites
            .borrow_mut()
            .committed(navigation, error.as_bool(), Instant::now());
        Ok(())
    }));
    let mut loading_token = 0;
    unsafe { webview.add_ContentLoading(&loading, &mut loading_token)? };
    let handler = NavigationStartingEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        // This step's address: a server's redirect raises the event again,
        // with the same navigation id and the new address.
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri)? };
        let uri = take_pwstr(uri);
        let mut navigation = 0;
        unsafe { args.NavigationId(&mut navigation)? };
        let reloadable = reloadable(kind(&args), has_body(&args));
        let (stale, script, epoch) = {
            let mut sites = sites.borrow_mut();
            let stale = sites.stale(&blocker);
            let engine = blocker.get().is_some();
            let script = sites.start(&uri, navigation, reloadable, engine, |url| {
                blocker.scriptlets(url)
            });
            (stale, script, sites.epoch)
        };
        for id in stale {
            remove(&sender, id);
        }
        if let Some((script, add)) = script {
            let host = site_scripts::web_host(&uri).unwrap_or_default();
            add_script(&sender, &sites, script, host, add, epoch);
        }
        Ok(())
    }));
    let mut starting_token = 0;
    unsafe { webview.add_NavigationStarting(&handler, &mut starting_token)? };
    Ok(tab)
}

/// Adds `script` (add `add`, for `host`, at `epoch`) to `webview`; once it
/// is in, reloads the page that came before it when [`Sites::after_add`]
/// says so.
fn add_script(
    webview: &ICoreWebView2,
    sites: &Rc<RefCell<Sites>>,
    script: String,
    host: String,
    add: u64,
    epoch: u64,
) {
    let (done_sites, done_webview, done_host) = (sites.clone(), webview.clone(), host.clone());
    let done =
        AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(move |result, id| {
            let result = result.map(|()| id).map_err(|e| {
                log::warn!("scriptlets: adding: {e}");
            });
            let after =
                done_sites
                    .borrow_mut()
                    .after_add(&done_host, add, epoch, result, Instant::now());
            if let Some(id) = after.remove {
                remove(&done_webview, id);
            }
            if after.late {
                // Rare (a page loaded long before the engine came): seen.
                log::info!(
                    "scriptlets: {done_host}'s page came too long before its script, not reloaded"
                );
            }
            if after.reload
                && let Err(e) = unsafe { done_webview.Reload() }
            {
                log::warn!("scriptlets: reloading: {e}");
            }
            Ok(())
        }));
    if let Err(e) = unsafe {
        webview.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script.as_str()), &done)
    } {
        log::warn!("scriptlets: adding: {e}");
        sites
            .borrow_mut()
            .after_add(&host, add, epoch, Err(()), Instant::now());
    }
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

    /// Completes `add` for `host`, `ms` after `t0`.
    fn done(sites: &mut Sites, host: &str, add: u64, t0: Instant, ms: u64) -> AfterAdd {
        let epoch = sites.epoch;
        sites.after_add(
            host,
            add,
            epoch,
            Ok(format!("id{add}")),
            t0 + Duration::from_millis(ms),
        )
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
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        let after = done(&mut sites, "a.test", add, t0, 100);
        assert!(after.reload);
        assert_eq!(after.remove, None);
        // Reloaded: the site has its script, nothing waits.
        assert_eq!(sites.start("https://a.test/", 2, true, true, SCRIPT), None);
        assert_eq!(sites.waiting, None);
    }

    #[test]
    fn a_page_not_there_yet_gets_the_script_without_a_reload() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, true, SCRIPT));
        assert!(!done(&mut sites, "a.test", add, t0, 100).reload);
        sites.committed(1, false, t0);
        assert_eq!(sites.waiting, None);
    }

    #[test]
    fn a_page_used_for_a_while_is_not_reloaded() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        let after = done(&mut sites, "a.test", add, t0, 1600);
        assert!(!after.reload);
        assert!(after.late);
    }

    #[test]
    fn an_error_page_is_not_reloaded() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, true, SCRIPT));
        sites.committed(1, true, t0);
        assert!(!done(&mut sites, "a.test", add, t0, 100).reload);
    }

    #[test]
    fn the_last_navigation_to_the_site_meanwhile_is_the_one_watched() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/1", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        // Again to the site (a second click) before the script is in.
        assert_eq!(sites.start("https://a.test/2", 2, true, true, SCRIPT), None);
        assert!(
            !done(&mut sites, "a.test", add, t0, 100).reload,
            "the second page is not there yet"
        );

        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/1", 1, true, true, SCRIPT));
        assert_eq!(sites.start("https://a.test/2", 2, true, true, SCRIPT), None);
        sites.committed(1, false, t0);
        sites.committed(2, false, t0);
        assert!(
            done(&mut sites, "a.test", add, t0, 100).reload,
            "the second page came before the script"
        );
    }

    #[test]
    fn a_tab_gone_elsewhere_is_not_reloaded() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        assert_eq!(sites.start("https://b.test/", 2, true, true, NONE), None);
        sites.committed(2, false, t0);
        assert!(!done(&mut sites, "a.test", add, t0, 100).reload);
        assert_eq!(sites.waiting, None);
    }

    #[test]
    fn a_redirect_to_another_site_is_not_reloaded_for_the_first() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        // youtube.com, then its server's redirect (same navigation) to a
        // site that needs no add (www.youtube.com, already in).
        let add = add_of(sites.start("https://a.test/", 1, true, true, SCRIPT));
        assert_eq!(sites.start("https://b.test/", 1, true, true, NONE), None);
        sites.committed(1, false, t0);
        assert!(!done(&mut sites, "a.test", add, t0, 100).reload);
    }

    #[test]
    fn a_redirect_target_with_a_query_is_not_reloaded() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        // A sign-in's return: another site, then this one, same navigation.
        assert_eq!(
            sites.start("https://login.test/", 1, true, true, NONE),
            None
        );
        let add = add_of(sites.start("https://app.test/callback?code=x", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        assert!(!done(&mut sites, "app.test", add, t0, 100).reload);
    }

    #[test]
    fn a_redirect_target_without_a_query_is_reloaded() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        // twitch.tv, redirected to www.twitch.tv (same navigation), whose
        // script is added on the second step.
        assert_eq!(sites.start("https://twitch.tv/", 1, true, true, NONE), None);
        let add = add_of(sites.start("https://www.twitch.tv/", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        assert!(done(&mut sites, "www.twitch.tv", add, t0, 100).reload);
    }

    #[test]
    fn a_query_is_told() {
        assert!(has_query("https://a.test/cb?code=x"));
        assert!(!has_query("https://a.test/"));
        assert!(!has_query("https://a.test/?"));
        assert!(!has_query("https://a.test/#a?b"));
    }

    #[test]
    fn a_page_that_cannot_be_reloaded_waits_for_the_next_one() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, false, true, SCRIPT));
        sites.committed(1, false, t0);
        assert!(!done(&mut sites, "a.test", add, t0, 100).reload);
    }

    #[test]
    fn a_refused_script_reloads_nothing_and_is_asked_again() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        let epoch = sites.epoch;
        let after = sites.after_add("a.test", add, epoch, Err(()), t0);
        assert_eq!(after, AfterAdd::default());
        assert!(
            sites
                .start("https://a.test/", 2, true, true, SCRIPT)
                .is_some()
        );
    }

    #[test]
    fn an_add_from_before_a_change_neither_stays_nor_decides_for_the_new_one() {
        let t0 = Instant::now();
        let blocker = BlockerHandle::default();
        let mut sites = Sites::default();
        sites.stale(&blocker);
        let old_epoch = sites.epoch;
        let old = add_of(sites.start("https://a.test/1", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        // New lists, and the site again before the first add completed.
        blocker.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        sites.stale(&blocker);
        let new = add_of(sites.start("https://a.test/2", 2, true, true, SCRIPT));
        sites.committed(2, false, t0);
        let after = sites.after_add("a.test", old, old_epoch, Ok("old".into()), t0);
        assert_eq!(after.remove, Some("old".into()));
        assert!(!after.reload, "the new script is not in yet");
        let after = done(&mut sites, "a.test", new, t0, 100);
        assert_eq!(after.remove, None);
        assert!(after.reload);
    }

    #[test]
    fn a_stale_add_for_the_watched_page_does_not_reload_it() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        let add = add_of(sites.start("https://a.test/", 1, true, true, SCRIPT));
        sites.committed(1, false, t0);
        // A clear came in, and no navigation since to start a new add.
        let old_epoch = sites.epoch;
        sites.epoch += 1;
        let after = sites.after_add("a.test", add, old_epoch, Ok("id".into()), t0);
        assert_eq!(after.remove, Some("id".into()));
        assert!(!after.reload);
    }

    #[test]
    fn a_page_loaded_before_the_engine_is_caught_up_once_with_one_reload() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        assert_eq!(sites.start("https://a.test/", 1, true, false, NONE), None);
        assert_eq!(sites.start("https://b.test/", 2, true, false, NONE), None);
        sites.committed(2, false, t0);
        assert_eq!(sites.catch_up(false, SCRIPT), None, "no engine yet");
        let (_, host, add) = sites.catch_up(true, SCRIPT).expect("b's script");
        assert_eq!(host, "b.test");
        assert_eq!(sites.catch_up(true, SCRIPT), None, "once");
        // Its page is there already: reloaded once the script is in.
        assert!(done(&mut sites, "b.test", add, t0, 100).reload);
    }

    #[test]
    fn a_page_not_there_when_caught_up_is_watched_like_any() {
        let t0 = Instant::now();
        let mut sites = Sites::default();
        assert_eq!(sites.start("https://a.test/", 1, true, false, NONE), None);
        let (_, _, add) = sites.catch_up(true, SCRIPT).expect("a's script");
        sites.committed(1, false, t0);
        assert!(done(&mut sites, "a.test", add, t0, 100).reload);
    }

    #[test]
    fn pages_loaded_with_the_engine_or_not_reloadable_are_not_caught_up() {
        let mut sites = Sites::default();
        assert_eq!(sites.start("https://a.test/", 1, true, true, NONE), None);
        assert_eq!(sites.catch_up(true, SCRIPT), None);
        assert_eq!(sites.start("https://b.test/", 2, false, false, NONE), None);
        assert_eq!(sites.catch_up(true, SCRIPT), None);
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
