//! Resetting the protected videos' data (DRM): what the web view keeps for
//! Widevine and PlayReady, outside of the sites' data. Broken, a streaming
//! site's player waits forever (Netflix stuck at "Authorizing") and clearing
//! the site's data does not help. The web view keeps these files open, so
//! the reset is asked for, then done at the next start, before the engine.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

/// The request, in the profile's files.
const MARKER: &str = "reset-drm";
/// Widevine's and PlayReady's data, in WebView2's profile folder.
const DATA: [&str; 3] = [
    "CdmStorage.db",
    "CdmStorage.db-journal",
    "MediaFoundationCdmStore",
];
/// The web view of the closing app may hold the files a moment: tries
/// this many times, this far apart (only when a reset was asked).
const TRIES: u32 = 20;
const WAIT: Duration = Duration::from_millis(250);

/// The DRM data of the profile whose files are in `files_dir` (its web view
/// data in `webview/`).
#[derive(Debug, Clone)]
pub struct DrmReset {
    files_dir: PathBuf,
}

impl DrmReset {
    pub fn new(files_dir: impl Into<PathBuf>) -> Self {
        Self {
            files_dir: files_dir.into(),
        }
    }

    fn marker(&self) -> PathBuf {
        self.files_dir.join(MARKER)
    }

    fn data_dir(&self) -> PathBuf {
        self.files_dir
            .join("webview")
            .join("EBWebView")
            .join("Default")
    }

    /// Asks for the reset at the next start.
    pub fn request(&self) -> io::Result<()> {
        fs::write(self.marker(), b"")
    }

    /// At start, before the engine: resets when it was asked. `None` when
    /// it was not; an error keeps the request for the next start.
    pub fn apply_if_requested(&self) -> Option<io::Result<()>> {
        self.apply_with(TRIES, WAIT)
    }

    fn apply_with(&self, tries: u32, wait: Duration) -> Option<io::Result<()>> {
        if !self.marker().exists() {
            return None;
        }
        let mut result = self.remove_data();
        for _ in 1..tries {
            if result.is_ok() {
                break;
            }
            thread::sleep(wait);
            result = self.remove_data();
        }
        Some(result.and_then(|()| fs::remove_file(self.marker())))
    }

    fn remove_data(&self) -> io::Result<()> {
        let dir = self.data_dir();
        DATA.iter().try_for_each(|name| remove(&dir.join(name)))
    }
}

/// Removes a file or a folder; done when it is not there.
fn remove(path: &Path) -> io::Result<()> {
    let result = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    match result {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> (tempfile::TempDir, DrmReset) {
        let dir = tempfile::tempdir().unwrap();
        let reset = DrmReset::new(dir.path());
        let data = reset.data_dir();
        fs::create_dir_all(data.join("MediaFoundationCdmStore").join("x")).unwrap();
        fs::write(data.join("CdmStorage.db"), b"db").unwrap();
        fs::write(data.join("CdmStorage.db-journal"), b"").unwrap();
        fs::write(data.join("Cookies"), b"keep").unwrap();
        (dir, reset)
    }

    #[test]
    fn nothing_happens_unless_asked() {
        let (_dir, reset) = profile();
        assert!(reset.apply_with(1, Duration::ZERO).is_none());
        assert!(reset.data_dir().join("CdmStorage.db").exists());
    }

    #[test]
    fn an_asked_reset_removes_the_drm_data_only_and_once() {
        let (_dir, reset) = profile();
        reset.request().unwrap();
        assert!(reset.apply_with(1, Duration::ZERO).unwrap().is_ok());
        let data = reset.data_dir();
        for name in DATA {
            assert!(!data.join(name).exists(), "{name}");
        }
        assert!(data.join("Cookies").exists());
        assert!(!reset.marker().exists());
        assert!(reset.apply_with(1, Duration::ZERO).is_none());
    }

    #[test]
    fn a_profile_without_drm_data_is_reset_too() {
        let dir = tempfile::tempdir().unwrap();
        let reset = DrmReset::new(dir.path());
        reset.request().unwrap();
        assert!(reset.apply_with(1, Duration::ZERO).unwrap().is_ok());
        assert!(!reset.marker().exists());
    }
}
