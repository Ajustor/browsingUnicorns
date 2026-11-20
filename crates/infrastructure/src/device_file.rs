//! What belongs to this computer whatever the profile: `device.toml`, next
//! to the list of profiles. Read before the window opens.

use std::path::{Path, PathBuf};

use application::ports::RepositoryError;
use domain::LanguageChoice;
use serde::{Deserialize, Serialize};

use crate::files;

/// The computer's own settings file. A missing file or key falls back to
/// the default.
pub struct DeviceFile {
    path: PathBuf,
}

/// The file format.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct DeviceToml {
    /// The interface's language.
    language: LanguageFile,
}

/// `auto`, `en` or `fr`; anything else follows the system.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LanguageFile {
    En,
    Fr,
    #[default]
    #[serde(other)]
    Auto,
}

impl DeviceFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `device.toml` in [`crate::profile_dir`].
    pub fn default_location() -> Option<Self> {
        crate::profile_dir().map(|d| Self::new(d.join("device.toml")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The interface's language (`Auto` without a file).
    pub fn language(&self) -> Result<LanguageChoice, RepositoryError> {
        Ok(match self.load()?.language {
            LanguageFile::Auto => LanguageChoice::Auto,
            LanguageFile::En => LanguageChoice::En,
            LanguageFile::Fr => LanguageChoice::Fr,
        })
    }

    /// Keeps `choice` as the interface's language.
    pub fn set_language(&self, choice: LanguageChoice) -> Result<(), RepositoryError> {
        // An unreadable file is replaced rather than blocking the choice.
        let mut file = self.load().unwrap_or_default();
        file.language = match choice {
            LanguageChoice::Auto => LanguageFile::Auto,
            LanguageChoice::En => LanguageFile::En,
            LanguageChoice::Fr => LanguageFile::Fr,
        };
        let text = toml::to_string_pretty(&file).map_err(|e| files::error(&self.path, e))?;
        files::write(&self.path, &text)
    }

    fn load(&self) -> Result<DeviceToml, RepositoryError> {
        match files::read(&self.path)? {
            Some(text) => toml::from_str(&text).map_err(|e| files::error(&self.path, e)),
            None => Ok(DeviceToml::default()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(dir: &tempfile::TempDir) -> DeviceFile {
        DeviceFile::new(dir.path().join("nested").join("device.toml"))
    }

    #[test]
    fn no_file_follows_the_system() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(file(&dir).language().unwrap(), LanguageChoice::Auto);
    }

    #[test]
    fn the_language_is_saved_by_code() {
        let dir = tempfile::tempdir().unwrap();
        let file = file(&dir);
        for choice in [LanguageChoice::Fr, LanguageChoice::En, LanguageChoice::Auto] {
            file.set_language(choice).unwrap();
            assert_eq!(file.language().unwrap(), choice);
        }
        file.set_language(LanguageChoice::Fr).unwrap();
        let text = std::fs::read_to_string(file.path()).unwrap();
        assert_eq!(text.trim(), "language = \"fr\"");
    }

    #[test]
    fn unknown_values_follow_the_system() {
        let dir = tempfile::tempdir().unwrap();
        let file = file(&dir);
        files::write(file.path(), "other = 1\n").unwrap();
        assert_eq!(file.language().unwrap(), LanguageChoice::Auto);
        files::write(file.path(), "language = \"klingon\"\n").unwrap();
        assert_eq!(file.language().unwrap(), LanguageChoice::Auto);
    }

    #[test]
    fn a_broken_file_is_an_error_but_can_be_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let file = file(&dir);
        files::write(file.path(), "language = [").unwrap();
        assert!(file.language().is_err());
        file.set_language(LanguageChoice::En).unwrap();
        assert_eq!(file.language().unwrap(), LanguageChoice::En);
    }
}
