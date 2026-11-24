//! Windows: the profile's cookies straight from WebView2's cookie manager.
//! wry's own cookie functions drop the leading dot of a domain cookie
//! (`.example.com`), which then would no longer reach the subdomains.

use std::sync::mpsc;

use domain::SiteCookie;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COOKIE_SAME_SITE_KIND, COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX,
    COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE, COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT,
    ICoreWebView2_2, ICoreWebView2Cookie,
};
use webview2_com::{GetCookiesCompletedHandler, take_pwstr, wait_with_pump};
use windows_core::{BOOL, HSTRING, Interface, PCWSTR, PWSTR};
use wry::{WebView, WebViewExtWindows};

fn text(get: impl FnOnce(&mut PWSTR) -> windows_core::Result<()>) -> windows_core::Result<String> {
    let mut raw = PWSTR::null();
    get(&mut raw)?;
    Ok(take_pwstr(raw))
}

fn flag(get: impl FnOnce(&mut BOOL) -> windows_core::Result<()>) -> windows_core::Result<bool> {
    let mut raw = BOOL::default();
    get(&mut raw)?;
    Ok(raw.as_bool())
}

fn read(c: &ICoreWebView2Cookie) -> windows_core::Result<SiteCookie> {
    unsafe {
        let mut same_site = COREWEBVIEW2_COOKIE_SAME_SITE_KIND::default();
        c.SameSite(&mut same_site)?;
        let mut expires = 0.0;
        c.Expires(&mut expires)?;
        let session = flag(|b| c.IsSession(b))?;
        Ok(SiteCookie {
            domain: text(|p| c.Domain(p))?,
            name: text(|p| c.Name(p))?,
            value: text(|p| c.Value(p))?,
            path: text(|p| c.Path(p))?,
            secure: flag(|b| c.IsSecure(b))?,
            http_only: flag(|b| c.IsHttpOnly(b))?,
            same_site: match same_site {
                COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT => "Strict",
                COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE => "None",
                _ => "Lax",
            }
            .into(),
            expires: (!session).then_some(expires as i64),
        })
    }
}

/// Every cookie of `view`'s profile. Waits for WebView2 (UI thread).
pub fn all(view: &WebView) -> windows_core::Result<Vec<SiteCookie>> {
    let webview = view.webview().cast::<ICoreWebView2_2>()?;
    let (tx, rx) = mpsc::channel();
    let handler = GetCookiesCompletedHandler::create(Box::new(move |result, list| {
        let cookies = result.and_then(|()| {
            let mut out = Vec::new();
            if let Some(list) = list {
                let mut count = 0;
                unsafe { list.Count(&mut count)? };
                for i in 0..count {
                    if let Ok(c) = unsafe { list.GetValueAtIndex(i) }.and_then(|c| read(&c)) {
                        out.push(c);
                    }
                }
            }
            Ok(out)
        });
        let _ = tx.send(cookies);
        Ok(())
    }));
    unsafe {
        webview
            .CookieManager()?
            .GetCookies(PCWSTR::null(), &handler)?
    };
    wait_with_pump(rx)
        .map_err(|e| windows_core::Error::new(windows_core::HRESULT(-1), e.to_string()))?
}

/// Sets `cookies` in `view`'s profile.
pub fn set(view: &WebView, cookies: &[SiteCookie]) -> windows_core::Result<()> {
    let webview = view.webview().cast::<ICoreWebView2_2>()?;
    let manager = unsafe { webview.CookieManager()? };
    for c in cookies {
        let result = unsafe {
            manager
                .CreateCookie(
                    &HSTRING::from(&c.name),
                    &HSTRING::from(&c.value),
                    &HSTRING::from(&c.domain),
                    &HSTRING::from(&c.path),
                )
                .and_then(|cookie| {
                    if let Some(expires) = c.expires {
                        cookie.SetExpires(expires as f64)?;
                    }
                    cookie.SetIsSecure(c.secure)?;
                    cookie.SetIsHttpOnly(c.http_only)?;
                    cookie.SetSameSite(match c.same_site.as_str() {
                        "Strict" => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_STRICT,
                        "None" => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_NONE,
                        _ => COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX,
                    })?;
                    manager.AddOrUpdateCookie(&cookie)
                })
        };
        if let Err(e) = result {
            log::warn!("cookie {} of {}: {e}", c.name, c.domain);
        }
    }
    Ok(())
}
