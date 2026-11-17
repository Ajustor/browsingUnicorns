//! Windows: how far the downloads are, and cancelling them (wry only tells
//! when one starts and ends). Each tab hands its downloads' operations to a
//! list the engine reads on each frame.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use application::ports::DownloadProgress;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_DOWNLOAD_STATE, COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS, ICoreWebView2_4,
    ICoreWebView2DownloadOperation,
};
use webview2_com::{DownloadStartingEventHandler, take_pwstr};
use windows_core::{Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

/// The downloads under way, every tab's.
pub type Operations = Rc<RefCell<Vec<ICoreWebView2DownloadOperation>>>;

/// Adds the downloads `view` starts to `operations`. Registered after
/// wry's own handler, which chooses the file.
pub fn attach(view: &WebView, operations: Operations) -> windows_core::Result<()> {
    let webview = view.webview().cast::<ICoreWebView2_4>()?;
    let handler = DownloadStartingEventHandler::create(Box::new(move |_, args| {
        if let Some(args) = args {
            operations
                .borrow_mut()
                .push(unsafe { args.DownloadOperation()? });
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_DownloadStarting(&handler, &mut token) }
}

fn state(op: &ICoreWebView2DownloadOperation) -> Option<COREWEBVIEW2_DOWNLOAD_STATE> {
    let mut state = COREWEBVIEW2_DOWNLOAD_STATE::default();
    unsafe { op.State(&mut state) }.ok()?;
    Some(state)
}

fn file(op: &ICoreWebView2DownloadOperation) -> Option<PathBuf> {
    let mut path = PWSTR::null();
    unsafe { op.ResultFilePath(&mut path) }.ok()?;
    Some(PathBuf::from(take_pwstr(path)))
}

/// The downloads still under way (the others leave the list).
pub fn progress(operations: &Operations) -> Vec<DownloadProgress> {
    let mut operations = operations.borrow_mut();
    operations.retain(|op| state(op) == Some(COREWEBVIEW2_DOWNLOAD_STATE_IN_PROGRESS));
    operations
        .iter()
        .filter_map(|op| {
            let (mut received, mut total) = (0i64, 0i64);
            unsafe {
                op.BytesReceived(&mut received).ok()?;
                op.TotalBytesToReceive(&mut total).ok()?;
            }
            Some(DownloadProgress {
                file: file(op)?,
                received: received.max(0) as u64,
                // 0 when the server did not say.
                total: (total > 0).then_some(total as u64),
            })
        })
        .collect()
}

/// Cancels the download into `target`.
pub fn cancel(operations: &Operations, target: &Path) {
    for op in operations.borrow().iter() {
        if file(op).as_deref() == Some(target)
            && let Err(e) = unsafe { op.Cancel() }
        {
            log::warn!("cancel download {}: {e}", target.display());
        }
    }
}
