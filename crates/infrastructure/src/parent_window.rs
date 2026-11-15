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
}

impl HasWindowHandle for ParentWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // Safety: `ParentWindow::new`'s caller guarantees the window is alive.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}
