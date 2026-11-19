//! Windows: the ad blocker's scriptlets, added at the start of each
//! top-level navigation (`NavigationStarting`), before the new document
//! exists, so they run before the page's own scripts. Frames are left out.

use std::cell::RefCell;
use std::rc::Rc;

use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, NavigationStartingEventHandler, take_pwstr,
};
use windows_core::{HSTRING, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::blocker::{BlockerChanges, BlockerHandle};
use crate::site_scripts::SiteScripts;

/// A tab's sites, with the ids WebView2 gave their scripts, and how many
/// times the blocker's changes cleared them (an id arriving after a clear
/// belongs to a script that must go).
#[derive(Default)]
struct Sites {
    scripts: SiteScripts<String>,
    changes: BlockerChanges,
    epoch: u64,
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
}

/// Adds the scriptlets of each site `view` navigates to, once per site. A
/// document is always created after `NavigationStarting` (reloads and
/// history included), so new lists or settings are noticed there.
pub fn attach(view: &WebView, blocker: BlockerHandle) -> windows_core::Result<()> {
    let webview = view.webview();
    let sites = Rc::new(RefCell::new(Sites::default()));
    let handler = NavigationStartingEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        // The navigation's own address: a redirect starts its own.
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri)? };
        let uri = take_pwstr(uri);
        let (stale, script, epoch) = {
            let mut sites = sites.borrow_mut();
            let stale = sites.stale(&blocker);
            let script = sites.scripts.script_for(&uri, |url| blocker.scriptlets(url));
            (stale, script, sites.epoch)
        };
        for id in stale {
            remove(&sender, id);
        }
        let Some(script) = script else {
            return Ok(());
        };
        let (ids, webview) = (sites.clone(), sender.clone());
        let done = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |result, id| {
                match result {
                    Ok(()) => {
                        let late = ids.borrow_mut().added(id, epoch);
                        if let Some(id) = late {
                            remove(&webview, id);
                        }
                    }
                    Err(e) => log::warn!("scriptlets: adding: {e}"),
                }
                Ok(())
            },
        ));
        if let Err(e) = unsafe {
            sender.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script.as_str()), &done)
        } {
            log::warn!("scriptlets: adding: {e}");
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NavigationStarting(&handler, &mut token) }
}

fn remove(webview: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2, id: String) {
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
        assert!(sites.scripts.script_for("https://site.test/", SCRIPT).is_some());
        assert_eq!(sites.added("a".into(), sites.epoch), None);
        assert!(sites.stale(&blocker).is_empty());
        assert_eq!(sites.scripts.script_for("https://site.test/", SCRIPT), None);

        blocker.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        assert_eq!(sites.stale(&blocker), vec!["a".to_string()]);
        assert!(sites.scripts.script_for("https://site.test/", SCRIPT).is_some());
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
