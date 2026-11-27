//! File helpers shared by the repositories.
//!
//! Everything written here is the user's own data (history, cookies,
//! passwords, sync key...): on Unix the files are created `0600`. On
//! Windows they inherit the ACL of the user's profile folder.

use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use application::ports::RepositoryError;

/// Read and write for the owner only.
#[cfg(unix)]
const FILE_MODE: u32 = 0o600;

pub fn error(path: &Path, e: impl std::fmt::Display) -> RepositoryError {
    RepositoryError(format!("{}: {e}", path.display()))
}

/// The file's text, or `None` when it does not exist.
pub fn read(path: &Path) -> Result<Option<String>, RepositoryError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(path, e)),
    }
}

/// Replaces the file in one step (temporary file, flushed to the disk, then
/// renamed), so a crash or a power loss never leaves it truncated. On Unix
/// the file is private to the user (`0600`).
pub fn write(path: &Path, contents: &str) -> Result<(), RepositoryError> {
    write_bytes(path, contents.as_bytes())
}

/// [`write`] for bytes.
pub fn write_bytes(path: &Path, contents: &[u8]) -> Result<(), RepositoryError> {
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    fs::create_dir_all(dir).map_err(|e| error(path, e))?;
    let (tmp, mut file) = create_temp(path).map_err(|e| error(path, e))?;
    let written = file
        .write_all(contents)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            fs::rename(&tmp, path)
        });
    if let Err(e) = written {
        let _ = fs::remove_file(&tmp);
        return Err(error(path, e));
    }
    sync_dir(dir);
    Ok(())
}

/// A new file next to `path`, with a name no other writer uses: this
/// process's id and a counter, and `create_new` in case a crashed process
/// with the same id left one behind.
fn create_temp(path: &Path) -> std::io::Result<(PathBuf, File)> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy());
    let mut tries = 0;
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = path.with_file_name(format!(".{name}.{}.{n}.tmp", std::process::id()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, FILE_MODE);
        match options.open(&tmp) {
            Ok(file) => return Ok((tmp, file)),
            Err(e) if e.kind() == ErrorKind::AlreadyExists && tries < 16 => tries += 1,
            Err(e) => return Err(e),
        }
    }
}

/// Makes the rename itself durable (Unix: the folder's entry is flushed).
/// Windows offers no folder handle through std; NTFS journals the rename.
fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Err(e) = File::open(dir).and_then(|d| d.sync_all()) {
        log::debug!("{}: {e}", dir.display());
    }
    #[cfg(not(unix))]
    let _ = dir;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_replace_and_leave_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b.json");
        write(&path, "one").unwrap();
        write(&path, "two").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some("two"));
        let names: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["b.json"]);
    }

    #[test]
    fn temp_names_are_unique() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.json");
        let (a, _fa) = create_temp(&path).unwrap();
        let (b, _fb) = create_temp(&path).unwrap();
        assert_ne!(a, b);
        assert_eq!(a.parent(), Some(dir.path()));
    }

    #[test]
    fn concurrent_writes_never_mix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.json");
        let payloads: Vec<String> = (0..8)
            .map(|i| char::from(b'a' + i).to_string().repeat(64 * 1024))
            .collect();
        std::thread::scope(|s| {
            for payload in &payloads {
                let path = &path;
                s.spawn(move || {
                    for _ in 0..20 {
                        // Windows may refuse a rename onto a file another
                        // rename holds for a moment: the write fails whole,
                        // it never mixes.
                        let _ = write(path, payload);
                    }
                });
            }
        });
        let last = read(&path).unwrap().unwrap();
        assert!(payloads.contains(&last), "the file mixes writers");
        let leftovers = fs::read_dir(dir.path()).unwrap().count();
        assert_eq!(leftovers, 1, "temporary files left behind");
    }

    #[cfg(unix)]
    #[test]
    fn files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.json");
        write(&path, "{}").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
