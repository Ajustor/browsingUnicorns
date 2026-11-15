//! Keeps the extensions of the WebView2 profile in line with the user's
//! list: adds the missing ones, removes the others, enables or disables.
//!
//! WebView2 keeps installed extensions in its profile (the web views' data
//! folder), shared by every tab. The calls are asynchronous: their callbacks
//! run later on the UI thread.

use std::collections::HashSet;
use std::path::PathBuf;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2, ICoreWebView2_13, ICoreWebView2BrowserExtension, ICoreWebView2Profile7,
};
use webview2_com::{
    BrowserExtensionEnableCompletedHandler, BrowserExtensionRemoveCompletedHandler,
    ProfileAddBrowserExtensionCompletedHandler, ProfileGetBrowserExtensionsCompletedHandler,
    take_pwstr,
};
use windows_core::{BOOL, HSTRING, Interface, PWSTR};

/// An extension the profile should have.
pub struct Wanted {
    pub id: String,
    pub path: PathBuf,
    pub enabled: bool,
}

fn logged(what: String) -> impl FnOnce(windows_core::Result<()>) -> windows_core::Result<()> {
    move |result| {
        match result {
            Ok(()) => log::info!("extensions: {what}"),
            Err(e) => log::warn!("extensions: {what}: {e}"),
        }
        Ok(())
    }
}

fn enable(
    ext: &ICoreWebView2BrowserExtension,
    id: &str,
    enabled: bool,
) -> windows_core::Result<()> {
    let what = format!("{} {id}", if enabled { "enabled" } else { "disabled" });
    let handler = BrowserExtensionEnableCompletedHandler::create(Box::new(logged(what)));
    unsafe { ext.Enable(enabled, &handler) }
}

fn id_of(ext: &ICoreWebView2BrowserExtension) -> windows_core::Result<String> {
    let mut id = PWSTR::null();
    unsafe { ext.Id(&mut id)? };
    Ok(take_pwstr(id))
}

/// `managed`: the extensions this app installed. Only those are removed
/// when no longer wanted; the profile also holds built-in ones (PDF viewer,
/// Web Store) that must stay.
pub fn reconcile(
    webview: &ICoreWebView2,
    wanted: Vec<Wanted>,
    managed: HashSet<String>,
) -> windows_core::Result<()> {
    let profile = unsafe {
        webview
            .cast::<ICoreWebView2_13>()?
            .Profile()?
            .cast::<ICoreWebView2Profile7>()?
    };
    let add_to = profile.clone();
    let handler =
        ProfileGetBrowserExtensionsCompletedHandler::create(Box::new(move |result, list| {
            result?;
            let mut installed = HashSet::new();
            if let Some(list) = list {
                let mut count = 0;
                unsafe { list.Count(&mut count)? };
                for i in 0..count {
                    let ext = unsafe { list.GetValueAtIndex(i)? };
                    let id = id_of(&ext)?;
                    match wanted.iter().find(|w| w.id == id) {
                        None if !managed.contains(&id) => {}
                        None => {
                            let handler = BrowserExtensionRemoveCompletedHandler::create(Box::new(
                                logged(format!("removed {id}")),
                            ));
                            unsafe { ext.Remove(&handler)? };
                        }
                        Some(w) => {
                            let mut on = BOOL::default();
                            unsafe { ext.IsEnabled(&mut on)? };
                            if on.as_bool() != w.enabled {
                                enable(&ext, &id, w.enabled)?;
                            }
                        }
                    }
                    installed.insert(id);
                }
            }
            for w in wanted.into_iter().filter(|w| !installed.contains(&w.id)) {
                let Wanted { id, path, enabled } = w;
                let handler = ProfileAddBrowserExtensionCompletedHandler::create(Box::new(
                    move |result, ext| {
                        match (result, ext) {
                            (Ok(()), Some(ext)) => {
                                let added = id_of(&ext).unwrap_or_default();
                                if added != id {
                                    log::warn!("extensions: {id} was added as {added}");
                                }
                                log::info!("extensions: added {added}");
                                if !enabled {
                                    enable(&ext, &added, false)?;
                                }
                            }
                            (Err(e), _) => log::warn!("extensions: adding {id}: {e}"),
                            (Ok(()), None) => {}
                        }
                        Ok(())
                    },
                ));
                unsafe { add_to.AddBrowserExtension(&HSTRING::from(path.as_path()), &handler)? };
            }
            Ok(())
        }));
    unsafe { profile.GetBrowserExtensions(&handler) }
}
