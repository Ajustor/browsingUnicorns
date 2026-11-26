//! Windows: what WebView2 offers besides wry: saving passwords, the zoom
//! of each site, clearing what sites left.

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_BROWSING_DATA_KINDS, COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE,
    COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE, COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG,
    ICoreWebView2, ICoreWebView2_8, ICoreWebView2_13, ICoreWebView2_15, ICoreWebView2Profile2,
    ICoreWebView2Settings4,
};
use webview2_com::{
    ClearBrowsingDataCompletedHandler, ContainsFullScreenElementChangedEventHandler,
    FaviconChangedEventHandler, GetFaviconCompletedHandler,
    IsDocumentPlayingAudioChangedEventHandler, IsMutedChangedEventHandler,
    SourceChangedEventHandler, ZoomFactorChangedEventHandler, take_pwstr,
};
use windows_core::{Interface, PWSTR};
use wry::{WebView, WebViewExtWindows};

/// Lets the web view offer to save the passwords typed in `view`'s pages.
pub fn save_passwords(view: &WebView, on: bool) -> windows_core::Result<()> {
    let settings = unsafe { view.webview().Settings()? }.cast::<ICoreWebView2Settings4>()?;
    unsafe { settings.SetIsPasswordAutosaveEnabled(on) }
}

/// The host of `view`'s page.
fn host(source: &str) -> Option<String> {
    domain::WebAddress::parse(source)
        .ok()?
        .host()
        .map(str::to_owned)
}

/// Sets `view`'s zoom to `zoom_of` its site's on each new page, and calls
/// `on_zoom` with the site and the zoom when the user changes it
/// (Ctrl+wheel, Ctrl +/-).
pub fn follow_zoom(
    view: &WebView,
    zoom_of: impl Fn(&str) -> u32 + 'static,
    on_zoom: impl Fn(String, u32) + 'static,
) -> windows_core::Result<()> {
    let webview = view.webview();
    let controller = view.controller();
    let zoom_of = std::rc::Rc::new(zoom_of);
    let page = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
    {
        let (controller, zoom_of, page) = (controller.clone(), zoom_of.clone(), page.clone());
        let handler = SourceChangedEventHandler::create(Box::new(move |sender, _| {
            let Some(sender) = sender else { return Ok(()) };
            let mut source = PWSTR::null();
            unsafe { sender.Source(&mut source)? };
            let host = host(&take_pwstr(source));
            let percent = host.as_deref().map_or(100, |h| zoom_of(h));
            *page.borrow_mut() = host;
            unsafe { controller.SetZoomFactor(f64::from(percent) / 100.0) }
        }));
        let mut token = 0;
        unsafe { webview.add_SourceChanged(&handler, &mut token)? };
    }
    let handler = ZoomFactorChangedEventHandler::create(Box::new(move |sender, _| {
        let Some(sender) = sender else { return Ok(()) };
        let mut factor = 1.0;
        unsafe { sender.ZoomFactor(&mut factor)? };
        let percent = (factor * 100.0).round() as u32;
        // Its own change, on a new page, says nothing new.
        if let Some(host) = page.borrow().clone()
            && percent != zoom_of(&host)
        {
            on_zoom(host, percent);
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { controller.add_ZoomFactorChanged(&handler, &mut token) }
}

/// Calls `on_change` when `view`'s page puts an element in full screen (a
/// video) or takes it out.
pub fn follow_fullscreen(
    view: &WebView,
    on_change: impl Fn(bool) + 'static,
) -> windows_core::Result<()> {
    let handler =
        ContainsFullScreenElementChangedEventHandler::create(Box::new(move |sender, _| {
            let Some(sender) = sender else { return Ok(()) };
            let mut on = windows_core::BOOL::default();
            unsafe { sender.ContainsFullScreenElement(&mut on)? };
            on_change(on.as_bool());
            Ok(())
        }));
    let mut token = 0;
    unsafe {
        view.webview()
            .add_ContainsFullScreenElementChanged(&handler, &mut token)
    }
}

/// Calls `on_icon` with the PNG of `view`'s page icon each time it changes
/// (empty: the page has none).
pub fn follow_favicon(
    view: &WebView,
    on_icon: impl Fn(Vec<u8>) + 'static,
) -> windows_core::Result<()> {
    let on_icon = std::rc::Rc::new(on_icon);
    let handler = FaviconChangedEventHandler::create(Box::new(move |sender, _| {
        let Some(sender) = sender else { return Ok(()) };
        let on_icon = on_icon.clone();
        let done = GetFaviconCompletedHandler::create(Box::new(move |result, stream| {
            let png = match (result, stream) {
                (Ok(()), Some(stream)) => read_all(&stream),
                _ => Vec::new(),
            };
            on_icon(png);
            Ok(())
        }));
        unsafe {
            sender
                .cast::<ICoreWebView2_15>()?
                .GetFavicon(COREWEBVIEW2_FAVICON_IMAGE_FORMAT_PNG, &done)
        }
    }));
    let mut token = 0;
    unsafe {
        view.webview()
            .cast::<ICoreWebView2_15>()?
            .add_FaviconChanged(&handler, &mut token)
    }
}

/// Calls `on_audio` with whether `view`'s page plays sound and whether it
/// is muted, each time either changes.
pub fn follow_audio(
    view: &WebView,
    on_audio: impl Fn(bool, bool) + 'static,
) -> windows_core::Result<()> {
    let webview = view.webview().cast::<ICoreWebView2_8>()?;
    let on_audio = std::rc::Rc::new(on_audio);
    let report = move |sender: Option<ICoreWebView2>| -> windows_core::Result<()> {
        let Some(sender) = sender else { return Ok(()) };
        let sender = sender.cast::<ICoreWebView2_8>()?;
        let (mut playing, mut muted) =
            (windows_core::BOOL::default(), windows_core::BOOL::default());
        unsafe {
            sender.IsDocumentPlayingAudio(&mut playing)?;
            sender.IsMuted(&mut muted)?;
        }
        on_audio(playing.as_bool(), muted.as_bool());
        Ok(())
    };
    let report = std::rc::Rc::new(report);
    let mut token = 0;
    let r = report.clone();
    let playing = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |s, _| r(s)));
    unsafe { webview.add_IsDocumentPlayingAudioChanged(&playing, &mut token)? };
    let muted = IsMutedChangedEventHandler::create(Box::new(move |s, _| report(s)));
    unsafe { webview.add_IsMutedChanged(&muted, &mut token) }
}

/// Calls `on_crash(all)` when `view`'s page dies: its renderer (`false`),
/// or the browser process with every page (`true`). A frame's, the GPU's or
/// a helper's are left to WebView2, which starts them again.
pub fn follow_crashes(
    view: &WebView,
    on_crash: impl Fn(bool) + 'static,
) -> windows_core::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    };
    let handler = webview2_com::ProcessFailedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
        unsafe { args.ProcessFailedKind(&mut kind)? };
        if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
            on_crash(true);
        } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
            on_crash(false);
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { view.webview().add_ProcessFailed(&handler, &mut token) }
}

/// Mutes `view`'s page, or lets it play sound again.
pub fn set_muted(view: &WebView, muted: bool) {
    let result = view
        .webview()
        .cast::<ICoreWebView2_8>()
        .and_then(|w| unsafe { w.SetIsMuted(muted) });
    if let Err(e) = result {
        log::debug!("mute: {e}");
    }
}

/// What is left to read in `stream`.
fn read_all(stream: &windows::Win32::System::Com::IStream) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let mut read = 0u32;
        let result = unsafe {
            stream.Read(
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                Some(&mut read),
            )
        };
        if result.is_err() || read == 0 {
            break;
        }
        out.extend_from_slice(&buffer[..read as usize]);
    }
    out
}

/// Sets `view`'s zoom, in percent.
pub fn set_zoom(view: &WebView, percent: u32) {
    if let Err(e) = unsafe { view.controller().SetZoomFactor(f64::from(percent) / 100.0) } {
        log::debug!("zoom: {e}");
    }
}

/// Removes the cookies and storage of every site and/or the cache, from
/// the profile `view` belongs to.
pub fn clear_site_data(view: &WebView, cookies: bool, cache: bool) -> windows_core::Result<()> {
    let mut kinds = COREWEBVIEW2_BROWSING_DATA_KINDS(0);
    if cookies {
        kinds.0 |= COREWEBVIEW2_BROWSING_DATA_KINDS_ALL_SITE.0;
    }
    if cache {
        kinds.0 |= COREWEBVIEW2_BROWSING_DATA_KINDS_DISK_CACHE.0;
    }
    let profile = unsafe { view.webview().cast::<ICoreWebView2_13>()?.Profile()? }
        .cast::<ICoreWebView2Profile2>()?;
    let done = ClearBrowsingDataCompletedHandler::create(Box::new(|result| {
        if let Err(e) = result {
            log::warn!("clear browsing data: {e}");
        }
        Ok(())
    }));
    unsafe { profile.ClearBrowsingData(kinds, &done) }
}
