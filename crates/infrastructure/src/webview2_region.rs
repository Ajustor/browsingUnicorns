//! The shown page's window cut where the browser's tooltips and menus fall:
//! the browser's own surface, under it, shows through the holes.

use application::ports::Viewport;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, DeleteObject, RGN_DIFF, RGN_ERROR, SetWindowRgn,
};

/// Gives `window` (the page's container, `size` its area in physical
/// pixels) the region "everything but `holes`" (relative to its top-left
/// corner); no holes: no region, the whole window.
pub fn cut(window: HWND, size: Viewport, holes: &[Viewport]) {
    if holes.is_empty() {
        // SAFETY: a window of ours, on the UI thread.
        unsafe { SetWindowRgn(window, None, true) };
        return;
    }
    // SAFETY: GDI regions made and freed here; once given to the window,
    // the system owns the region.
    unsafe {
        let region = CreateRectRgn(0, 0, size.width as i32, size.height as i32);
        if region.is_invalid() {
            log::warn!("page region: no region");
            return;
        }
        for hole in holes {
            let rect = CreateRectRgn(
                hole.x,
                hole.y,
                hole.x + hole.width as i32,
                hole.y + hole.height as i32,
            );
            if rect.is_invalid() {
                continue;
            }
            if CombineRgn(Some(region), Some(region), Some(rect), RGN_DIFF) == RGN_ERROR {
                log::warn!("page region: hole {hole:?} not cut");
            }
            let _ = DeleteObject(rect.into());
        }
        if SetWindowRgn(window, Some(region), true) == 0 {
            log::warn!("page region not set");
            let _ = DeleteObject(region.into());
        }
    }
}
