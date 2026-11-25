use std::path::{Path, PathBuf};

use application::ports::{Repository, RepositoryError};
use domain::{DeviceSettings, SearchEngine, Settings, WebAddress};
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
    /// When the settings were last changed (unix seconds), for sync.
    updated_at: u64,
    /// The built-in ad blocker is on.
    blocker: bool,
    /// Hosts where the ad blocker is off.
    blocker_allowlist: Vec<String>,
    /// Where downloads go (empty: the system's Downloads folder).
    download_dir: String,
    /// Ask where to save each download.
    ask_where_to_save: bool,
    /// Offer to save the passwords typed in pages.
    save_passwords: bool,
    /// Share the sites' cookies with the other devices of the sync group.
    sync_cookies: bool,
    /// Zoom of the sites zoomed in or out.
    site_zoom: Vec<SiteZoomFile>,
    /// Too many tabs: they shrink to fit the strip instead of scrolling.
    shrink_tabs: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct SiteZoomFile {
    host: String,
    percent: u32,
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
            updated_at: s.updated_at,
            blocker: s.blocker,
            blocker_allowlist: s.blocker_allowlist.clone(),
            download_dir: s
                .device
                .download_dir
                .as_ref()
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
            ask_where_to_save: s.device.ask_where_to_save,
            save_passwords: s.device.save_passwords,
            sync_cookies: s.device.sync_cookies,
            site_zoom: s
                .device
                .site_zoom
                .iter()
                .map(|(host, percent)| SiteZoomFile {
                    host: host.clone(),
                    percent: *percent,
                })
                .collect(),
            shrink_tabs: s.device.shrink_tabs,
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
            updated_at: self.updated_at,
            blocker: self.blocker,
            blocker_allowlist: self.blocker_allowlist,
            device: DeviceSettings {
                download_dir: (!self.download_dir.is_empty())
                    .then(|| PathBuf::from(self.download_dir)),
                ask_where_to_save: self.ask_where_to_save,
                save_passwords: self.save_passwords,
                sync_cookies: self.sync_cookies,
                site_zoom: self
                    .site_zoom
                    .into_iter()
                    .map(|z| (z.host, z.percent))
                    .collect(),
                shrink_tabs: self.shrink_tabs,
            },
        })
    }
}

impl TomlSettingsRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `<config dir>/browsing-unicorns/settings.toml`; in `BU_PROFILE_DIR`
    /// when it is set (tests leave the user's settings alone).
    pub fn default_location() -> Option<Self> {
        if let Some(dir) = std::env::var_os("BU_PROFILE_DIR").filter(|d| !d.is_empty()) {
            return Some(Self::new(PathBuf::from(dir).join("settings.toml")));
        }
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
            updated_at: 42,
            blocker: false,
            blocker_allowlist: vec!["lemonde.fr".into()],
            device: DeviceSettings {
                download_dir: Some(PathBuf::from("D:/Download")),
                ask_where_to_save: true,
                save_passwords: true,
                sync_cookies: false,
                site_zoom: vec![("example.com".into(), 125), ("b.org".into(), 80)],
                shrink_tabs: true,
            },
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
        assert!(loaded.blocker);
        assert!(loaded.blocker_allowlist.is_empty());
    }

    #[test]
    fn invalid_home_page_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo(&dir);
        files::write(repo.path(), "home_page = \"javascript:void(0)\"\n").unwrap();
        assert!(repo.load().is_err());
    }
}
