use std::path::{Path, PathBuf};

use application::ports::{Repository, RepositoryError};
use domain::{SearchEngine, Settings, WebAddress};
use serde::{Deserialize, Serialize};

use crate::files;

/// Settings saved as TOML. A missing file or key falls back to the default.
pub struct TomlSettingsRepository {
    path: PathBuf,
}

/// The file format, kept apart from the domain type so either can change.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
struct SettingsFile {
    home_page: String,
    search_engine: SearchEngineFile,
}

#[derive(Debug, Serialize, Deserialize)]
struct SearchEngineFile {
    name: String,
    template: String,
}

impl Default for SettingsFile {
    fn default() -> Self {
        Self::from(&Settings::default())
    }
}

impl From<&Settings> for SettingsFile {
    fn from(s: &Settings) -> Self {
        Self {
            home_page: s.home_page.to_string(),
            search_engine: SearchEngineFile {
                name: s.search_engine.name.clone(),
                template: s.search_engine.template.clone(),
            },
        }
    }
}

impl SettingsFile {
    fn into_settings(self) -> Result<Settings, RepositoryError> {
        let home_page = WebAddress::parse(&self.home_page)
            .map_err(|e| RepositoryError(format!("home_page: {e}")))?;
        Ok(Settings {
            home_page,
            search_engine: SearchEngine::new(self.search_engine.name, self.search_engine.template),
        })
    }
}

impl TomlSettingsRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `<config dir>/browsing-unicorns/settings.toml`.
    pub fn default_location() -> Option<Self> {
        dirs::config_dir().map(|d| Self::new(d.join("browsing-unicorns").join("settings.toml")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Repository<Settings> for TomlSettingsRepository {
    fn load(&self) -> Result<Settings, RepositoryError> {
        let Some(text) = files::read(&self.path)? else {
            return Ok(Settings::default());
        };
        toml::from_str::<SettingsFile>(&text)
            .map_err(|e| files::error(&self.path, e))?
            .into_settings()
    }

    fn save(&self, settings: &Settings) -> Result<(), RepositoryError> {
        let text = toml::to_string_pretty(&SettingsFile::from(settings))
            .map_err(|e| files::error(&self.path, e))?;
        files::write(&self.path, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(dir: &tempfile::TempDir) -> TomlSettingsRepository {
        TomlSettingsRepository::new(dir.path().join("nested").join("settings.toml"))
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(repo(&dir).load().unwrap(), Settings::default());
    }

    #[test]
    fn saved_settings_are_loaded_back() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            home_page: WebAddress::parse("https://example.com/").unwrap(),
            search_engine: SearchEngine::new("Qwant", "https://www.qwant.com/?q={query}"),
        };
        repo(&dir).save(&settings).unwrap();
        assert_eq!(repo(&dir).load().unwrap(), settings);
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(&dir);
        files::write(repo.path(), "home_page = \"https://example.com/\"\n").unwrap();
        let loaded = repo.load().unwrap();
        assert_eq!(loaded.home_page.as_str(), "https://example.com/");
        assert_eq!(loaded.search_engine, SearchEngine::default());
    }

    #[test]
    fn invalid_home_page_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(&dir);
        files::write(repo.path(), "home_page = \"javascript:void(0)\"\n").unwrap();
        assert!(repo.load().is_err());
    }
}
