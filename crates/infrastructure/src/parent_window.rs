use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};

/// The window the web views are embedded in, kept past the borrow the UI
/// toolkit hands out at start-up.
#[derive(Debug, Clone, Copy)]
pub struct ParentWindow(RawWindowHandle);

impl ParentWindow {
    /// Copies the handle of `window`.
    ///
    /// # Safety
    ///
    /// The window must outlive every web view created in it.
    pub unsafe fn new(window: &impl HasWindowHandle) -> Result<Self, HandleError> {
        Ok(Self(window.window_handle()?.as_raw()))
    }

    /// Gives the window the executable's icon (`assets/icon.ico`, put in by
    /// build.rs) at the sizes Windows asks for. winit sets the small icon
    /// alone; the taskbar and Alt+Tab show the big one, left empty.
    #[cfg(windows)]
    pub fn use_app_icon(&self) {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTCOLOR, LoadImageW,
            SM_CXICON, SM_CXSMICON, SM_CYICON, SM_CYSMICON, SendMessageW, WM_SETICON,
        };
        use windows::core::PCWSTR;

        let RawWindowHandle::Win32(handle) = self.0 else {
            return;
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        // build.rs's icon: winresource gives it the id 1.
        const APP_ICON: PCWSTR = PCWSTR(std::ptr::without_provenance(1));
        // Safety: a live window (`new`'s contract); the icons are the
        // module's, kept for the process's life, never destroyed.
        unsafe {
            let Ok(module) = GetModuleHandleW(None) else {
                return;
            };
            for (kind, w, h) in [
                (ICON_BIG, SM_CXICON, SM_CYICON),
                (ICON_SMALL, SM_CXSMICON, SM_CYSMICON),
            ] {
                let size = (GetSystemMetrics(w), GetSystemMetrics(h));
                match LoadImageW(
                    Some(module.into()),
                    APP_ICON,
                    IMAGE_ICON,
                    size.0,
                    size.1,
                    LR_DEFAULTCOLOR,
                ) {
                    Ok(icon) => {
                        SendMessageW(
                            hwnd,
                            WM_SETICON,
                            Some(WPARAM(kind as usize)),
                            Some(LPARAM(icon.0 as isize)),
                        );
                    }
                    Err(e) => log::warn!("window icon: {e}"),
                }
            }
        }
    }

    /// Windows only: elsewhere the toolkit's icon stays.
    #[cfg(not(windows))]
    pub fn use_app_icon(&self) {}
}

impl HasWindowHandle for ParentWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // Safety: `ParentWindow::new`'s caller guarantees the window is alive.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}
