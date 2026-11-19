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

use crate::blocker::BlockerHandle;
use crate::site_scripts::SiteScripts;

/// A tab's sites, with the ids WebView2 gave their scripts.
pub type Sites = Rc<RefCell<SiteScripts<String>>>;

/// Adds the scriptlets of each site `view` navigates to, once per site.
pub fn attach(view: &WebView, blocker: BlockerHandle, sites: Sites) -> windows_core::Result<()> {
    let webview = view.webview();
    let handler = NavigationStartingEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        // The navigation's own address: a redirect starts its own.
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri)? };
        let uri = take_pwstr(uri);
        let Some(script) = sites
            .borrow_mut()
            .script_for(&uri, |url| blocker.scriptlets(url))
        else {
            return Ok(());
        };
        let ids = sites.clone();
        // An id arriving after a `clear` is kept past it: its script stays
        // until the next clear (it only runs on its own host).
        let done = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |result, id| {
                match result {
                    Ok(()) => ids.borrow_mut().keep(id),
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

/// Removes `sites`' scripts from `view` and forgets its sites (the ad
/// blocker's lists or settings changed).
pub fn clear(view: &WebView, sites: &Sites) {
    let webview = view.webview();
    for id in sites.borrow_mut().clear() {
        if let Err(e) = unsafe { webview.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id)) } {
            log::warn!("scriptlets: removing: {e}");
        }
    }
}
