//! File helpers shared by the repositories.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use application::ports::RepositoryError;

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

/// Replaces the file in one step (temporary file then rename), so a crash
/// mid-write never leaves it truncated.
pub fn write(path: &Path, contents: &str) -> Result<(), RepositoryError> {
    write_bytes(path, contents.as_bytes())
}

/// [`write`] for bytes.
pub fn write_bytes(path: &Path, contents: &[u8]) -> Result<(), RepositoryError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| error(path, e))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents).map_err(|e| error(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| error(path, e))
}
