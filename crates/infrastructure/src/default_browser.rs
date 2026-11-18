//! The system's default browser: whether it is this one, and making it so.
//!
//! - Windows: the browser registers itself for the current user (a
//!   "StartMenuInternet" client with its capabilities, and a ProgID for
//!   http, https and .html), then opens its page in the Settings app, where
//!   the user chooses: Windows lets no program pick itself.
//! - Linux: `xdg-settings`, for the installed `browsingUnicorns.desktop`.
//! - macOS: not offered. The system hands links to the default browser as
//!   Apple Events, which the window toolkit doesn't pass on: the browser
//!   would open without the page.

/// The name the browser registers under.
#[cfg(any(windows, test))]
const APP: &str = "browsingUnicorns";

/// Whether this system lets the browser become the default one.
pub fn supported() -> bool {
    cfg!(any(windows, all(unix, not(target_os = "macos"))))
}

/// Whether it is the default browser (`None`: unknown, unsupported).
pub fn is_default() -> Option<bool> {
    imp::is_default()
}

/// Makes it the default browser, or opens where the user does it.
pub fn make_default() -> Result<(), String> {
    imp::make_default()
}

/// Keeps the registration pointing at this program (moved, updated), where
/// it registered before. Cheap: call at startup.
pub fn refresh_registration() {
    #[cfg(windows)]
    imp::refresh();
}

/// Lets the window a link was handed to come to the front: this launch, by
/// the user, may give it the foreground (Windows only lets it then).
pub fn let_open_window_come_forward() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
}

/// The registry values the browser writes, for `exe` (under HKCU).
#[cfg(any(windows, test))]
fn registry_values(exe: &str) -> Vec<(String, &'static str, String)> {
    let prog_id = format!("Software\\Classes\\{APP}HTML");
    let client = format!("Software\\Clients\\StartMenuInternet\\{APP}");
    let caps = format!("{client}\\Capabilities");
    let open = format!("\"{exe}\" \"%1\"");
    let icon = format!("{exe},0");
    vec![
        (prog_id.clone(), "", format!("{APP} HTML Document")),
        (prog_id.clone(), "URL Protocol", String::new()),
        (format!("{prog_id}\\DefaultIcon"), "", icon.clone()),
        (format!("{prog_id}\\shell\\open\\command"), "", open),
        (client.clone(), "", APP.to_owned()),
        (format!("{client}\\DefaultIcon"), "", icon.clone()),
        (
            format!("{client}\\shell\\open\\command"),
            "",
            format!("\"{exe}\""),
        ),
        (caps.clone(), "ApplicationName", APP.to_owned()),
        (
            caps.clone(),
            "ApplicationDescription",
            "Light and fast web browser".to_owned(),
        ),
        (caps.clone(), "ApplicationIcon", icon),
        (
            format!("{caps}\\StartMenu"),
            "StartMenuInternet",
            APP.to_owned(),
        ),
        (
            format!("{caps}\\URLAssociations"),
            "http",
            format!("{APP}HTML"),
        ),
        (
            format!("{caps}\\URLAssociations"),
            "https",
            format!("{APP}HTML"),
        ),
        (
            format!("{caps}\\FileAssociations"),
            ".htm",
            format!("{APP}HTML"),
        ),
        (
            format!("{caps}\\FileAssociations"),
            ".html",
            format!("{APP}HTML"),
        ),
        ("Software\\RegisteredApplications".to_owned(), APP, caps),
    ]
}

#[cfg(windows)]
mod imp {
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegGetValueW, RegSetKeyValueW,
    };
    use windows::Win32::UI::Shell::{
        SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify, ShellExecuteW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows_core::{HSTRING, PCWSTR};

    use super::{APP, registry_values};

    fn read(key: &str, name: &str) -> Option<String> {
        let (key, name) = (HSTRING::from(key), HSTRING::from(name));
        let mut buffer = [0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                &key,
                &name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status.is_err() {
            return None;
        }
        let len = (size as usize / 2).saturating_sub(1).min(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }

    fn write(key: &str, name: &str, value: &str) -> Result<(), String> {
        let wide: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let (hkey, hname) = (HSTRING::from(key), HSTRING::from(name));
        let name = if name.is_empty() {
            PCWSTR::null()
        } else {
            PCWSTR(hname.as_ptr())
        };
        unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &hkey,
                name,
                REG_SZ.0,
                Some(wide.as_ptr().cast()),
                (wide.len() * 2) as u32,
            )
        }
        .ok()
        .map_err(|e| format!("registry {key}: {e}"))
    }

    fn exe() -> Result<String, String> {
        std::env::current_exe()
            .map(|p| p.display().to_string())
            .map_err(|e| e.to_string())
    }

    fn register() -> Result<(), String> {
        for (key, name, value) in registry_values(&exe()?) {
            write(&key, name, &value)?;
        }
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
        Ok(())
    }

    pub fn refresh() {
        let key = format!("Software\\Clients\\StartMenuInternet\\{APP}\\shell\\open\\command");
        let Some(current) = read(&key, "") else {
            return;
        };
        if let Ok(exe) = exe()
            && current != format!("\"{exe}\"")
            && let Err(e) = register()
        {
            log::warn!("default browser registration: {e}");
        }
    }

    pub fn is_default() -> Option<bool> {
        // Recent Windows 11 keeps the choice in `UserChoiceLatest`, which
        // wins over the older `UserChoice`.
        let https = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https";
        let choice = read(&format!(r"{https}\UserChoiceLatest\ProgId"), "ProgId")
            .or_else(|| read(&format!(r"{https}\UserChoice"), "ProgId"));
        Some(choice.is_some_and(|id| id == format!("{APP}HTML")))
    }

    pub fn make_default() -> Result<(), String> {
        register()?;
        // Windows 11 opens the browser's own page; 10, the default apps.
        // Through the shell: `explorer <uri>` drops a URI with a query and
        // opens the Documents folder instead.
        let uri = HSTRING::from(format!("ms-settings:defaultapps?registeredAppUser={APP}"));
        let result = unsafe {
            ShellExecuteW(
                None,
                &HSTRING::from("open"),
                &uri,
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // Above 32: done.
        if result.0 as isize > 32 {
            Ok(())
        } else {
            Err(format!(
                "could not open the Settings (error {})",
                result.0 as isize
            ))
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use std::process::Command;

    const DESKTOP_FILE: &str = "browsingUnicorns.desktop";

    pub fn is_default() -> Option<bool> {
        let out = Command::new("xdg-settings")
            .args(["get", "default-web-browser"])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim() == DESKTOP_FILE)
    }

    pub fn make_default() -> Result<(), String> {
        let out = Command::new("xdg-settings")
            .args(["set", "default-web-browser", DESKTOP_FILE])
            .output()
            .map_err(|e| format!("xdg-settings: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "xdg-settings: {} (is browsingUnicorns installed with its .desktop file?)",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn is_default() -> Option<bool> {
        None
    }

    pub fn make_default() -> Result<(), String> {
        Err("not available on macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registration_names_the_program_and_its_capabilities() {
        let values = registry_values("C:\\Apps\\browsingUnicorns.exe");
        let get = |key: &str, name: &str| {
            values
                .iter()
                .find(|(k, n, _)| k == key && *n == name)
                .map(|(_, _, v)| v.as_str())
        };
        assert_eq!(
            get(
                "Software\\Classes\\browsingUnicornsHTML\\shell\\open\\command",
                ""
            ),
            Some("\"C:\\Apps\\browsingUnicorns.exe\" \"%1\"")
        );
        let caps = "Software\\Clients\\StartMenuInternet\\browsingUnicorns\\Capabilities";
        assert_eq!(
            get(&format!("{caps}\\URLAssociations"), "https"),
            Some("browsingUnicornsHTML")
        );
        assert_eq!(
            get("Software\\RegisteredApplications", "browsingUnicorns"),
            Some(caps)
        );
    }
}
