//! The browser's shortcuts on Windows while a page has the keyboard focus:
//! WebView2 keeps the keys of the page it shows, so the window never sees
//! them. Its `AcceleratorKeyPressed` event comes before the page: the
//! browser's shortcuts are taken there, every other key goes on to the page.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN, COREWEBVIEW2_PHYSICAL_KEY_STATUS,
};
use webview2_com::{AcceleratorKeyPressedEventHandler, FocusChangedEventHandler};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_F1, VK_F24, VK_HOME, VK_LEFT, VK_RIGHT, VK_SHIFT, VK_TAB,
};
use wry::{WebView, WebViewExtWindows};

use crate::wry_engine::KeyPress;

/// Calls `got_focus` when `view` takes the keyboard (a click in the page).
pub fn on_focus(view: &WebView, got_focus: impl Fn() + 'static) -> windows_core::Result<()> {
    let handler = FocusChangedEventHandler::create(Box::new(move |_, _| {
        got_focus();
        Ok(())
    }));
    let mut token = 0;
    unsafe { view.controller().add_GotFocus(&handler, &mut token) }
}

/// The name of virtual key `vk`, as the shortcuts name it.
fn key_name(vk: u32) -> Option<String> {
    Some(match vk {
        // Letters and digits are their ASCII code.
        0x30..=0x39 | 0x41..=0x5A => char::from(vk as u8).to_string(),
        v if v == u32::from(VK_TAB.0) => "Tab".into(),
        v if v == u32::from(VK_LEFT.0) => "Left".into(),
        v if v == u32::from(VK_RIGHT.0) => "Right".into(),
        v if v == u32::from(VK_HOME.0) => "Home".into(),
        v if (u32::from(VK_F1.0)..=u32::from(VK_F24.0)).contains(&v) => {
            format!("F{}", v - u32::from(VK_F1.0) + 1)
        }
        _ => return None,
    })
}

fn is_down(vk: u16) -> bool {
    // The high bit: the key is down.
    unsafe { GetKeyState(i32::from(vk)) < 0 }
}

/// Takes the key presses of `view` found in `shortcuts`, into `pressed`;
/// `wake` tells the UI.
pub fn attach(
    view: &WebView,
    shortcuts: Rc<RefCell<HashSet<KeyPress>>>,
    pressed: Rc<RefCell<Vec<KeyPress>>>,
    wake: impl Fn() + 'static,
) -> windows_core::Result<()> {
    let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND::default();
        let mut vk = 0u32;
        let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
        unsafe {
            args.KeyEventKind(&mut kind)?;
            args.VirtualKey(&mut vk)?;
            args.PhysicalKeyStatus(&mut status)?;
        }
        if kind != COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
            && kind != COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN
        {
            return Ok(());
        }
        let Some(key) = key_name(vk) else {
            return Ok(());
        };
        let press = KeyPress {
            key,
            ctrl: is_down(VK_CONTROL.0),
            shift: is_down(VK_SHIFT.0),
            alt: status.IsMenuKeyDown.as_bool(),
        };
        if !shortcuts.borrow().contains(&press) {
            return Ok(());
        }
        unsafe { args.SetHandled(true)? };
        // Held down: one press (Ctrl+Tab excepted, it walks the tabs).
        if !status.WasKeyDown.as_bool() || press.key == "Tab" {
            pressed.borrow_mut().push(press);
            wake();
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe {
        view.controller()
            .add_AcceleratorKeyPressed(&handler, &mut token)
    }
}
