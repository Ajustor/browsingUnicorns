//! The web view's data of a private window (cookies, cache, site storage):
//! a temporary folder, deleted when the window closes, or at the next
//! start of the browser if it crashed.

use std::hash::{BuildHasher, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PREFIX: &str = "browsing-unicorns-private-";

/// `<temp>/browsing-unicorns-private-<pid>-<random>/`, removed on drop.
#[derive(Debug)]
pub struct PrivateDataDir {
    path: PathBuf,
}

impl PrivateDataDir {
    /// Makes a new folder in the system's temporary folder.
    pub fn create() -> io::Result<Self> {
        Self::create_in(&std::env::temp_dir())
    }

    fn create_in(parent: &Path) -> io::Result<Self> {
        let path = parent.join(format!("{PREFIX}{}-{:016x}", std::process::id(), random()));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for PrivateDataDir {
    fn drop(&mut self) {
        // The web view's processes may still hold files for a moment after
        // the window closed.
        let mut tries = 0;
        loop {
            match std::fs::remove_dir_all(&self.path) {
                Ok(()) => return,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return,
                Err(e) if tries >= 20 => {
                    log::warn!("private data {}: {e}", self.path.display());
                    return;
                }
                Err(_) => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        }
    }
}

/// Unpredictable enough for a folder name: std's hasher keys are random.
fn random() -> u64 {
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    hasher.write_u128(nanos);
    hasher.finish()
}

/// Removes the private windows' folders left behind by processes that no
/// longer run (a crash).
pub fn clean_leftovers() {
    clean_leftovers_in(&std::env::temp_dir());
}

fn clean_leftovers_in(parent: &Path) {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(leftover_pid) else {
            continue;
        };
        if pid == std::process::id() || is_running(pid) {
            continue;
        }
        if let Err(e) = std::fs::remove_dir_all(entry.path()) {
            log::warn!("private data {}: {e}", entry.path().display());
        }
    }
}

/// The pid in `browsing-unicorns-private-<pid>-<random>`.
fn leftover_pid(name: &str) -> Option<u32> {
    let (pid, _) = name.strip_prefix(PREFIX)?.split_once('-')?;
    pid.parse().ok()
}

/// Whether process `pid` runs.
#[cfg(windows)]
fn is_running(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: plain Win32 calls on a handle closed before returning.
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let mut code = 0u32;
                let alive =
                    GetExitCodeProcess(handle, &mut code).is_err() || code == STILL_ACTIVE.0 as u32;
                let _ = CloseHandle(handle);
                alive
            }
            Err(e) => e.code() == ERROR_ACCESS_DENIED.to_hresult(),
        }
    }
}

/// Whether process `pid` runs.
#[cfg(unix)]
fn is_running(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 only checks that the process exists.
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_goes_away_on_drop() {
        let temp = tempfile::tempdir().unwrap();
        let data = PrivateDataDir::create_in(temp.path()).unwrap();
        let path = data.path().to_path_buf();
        assert!(path.is_dir());
        std::fs::write(path.join("cookies"), "secret").unwrap();
        drop(data);
        assert!(!path.exists());
    }

    #[test]
    fn leftovers_of_dead_processes_are_removed() {
        let temp = tempfile::tempdir().unwrap();
        // No system gives out such a pid.
        let dead = temp.path().join(format!("{PREFIX}2147483646-0123"));
        std::fs::create_dir_all(dead.join("EBWebView")).unwrap();
        let ours = PrivateDataDir::create_in(temp.path()).unwrap();
        let other = temp.path().join("browsing-unicorns-other");
        std::fs::create_dir_all(&other).unwrap();
        clean_leftovers_in(temp.path());
        assert!(!dead.exists());
        assert!(ours.path().is_dir());
        assert!(other.is_dir());
    }

    #[test]
    fn the_pid_is_read_from_the_name() {
        assert_eq!(leftover_pid("browsing-unicorns-private-42-abc"), Some(42));
        assert_eq!(leftover_pid("browsing-unicorns-private-x-abc"), None);
        assert_eq!(leftover_pid("browsing-unicorns"), None);
        assert!(is_running(std::process::id()));
    }
}
