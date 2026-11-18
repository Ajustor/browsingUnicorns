//! Serialized forms of the domain types, shared by the profile files and
//! the sync records. Entries whose address no longer parses are dropped.

use domain::{
    Bookmark, Bookmarks, DeviceId, DeviceRecord, RemoteTab, Removal, SearchEngine, Settings, Visit,
    WebAddress,
};
use serde::{Deserialize, Serialize};

fn parse(url: &str) -> Option<WebAddress> {
    WebAddress::parse(url).ok()
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BookmarksDto {
    pub bookmarks: Vec<BookmarkDto>,
    #[serde(default)]
    pub removed: Vec<RemovalDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BookmarkDto {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub added_at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemovalDto {
    pub url: String,
    pub at: u64,
}

impl From<&Bookmarks> for BookmarksDto {
    fn from(b: &Bookmarks) -> Self {
        Self {
            bookmarks: b
                .items()
                .iter()
                .map(|b| BookmarkDto {
                    url: b.address.to_string(),
                    title: b.title.clone(),
                    added_at: b.added_at,
                    folder: b.folder.clone(),
                })
                .collect(),
            removed: b
                .removals()
                .iter()
                .map(|r| RemovalDto {
                    url: r.address.to_string(),
                    at: r.at,
                })
                .collect(),
        }
    }
}

impl From<BookmarksDto> for Bookmarks {
    fn from(dto: BookmarksDto) -> Self {
        Bookmarks::from_parts(
            dto.bookmarks
                .into_iter()
                .filter_map(|b| {
                    Some(Bookmark {
                        address: parse(&b.url)?,
                        title: b.title,
                        added_at: b.added_at,
                        folder: b.folder,
                    })
                })
                .collect(),
            dto.removed
                .into_iter()
                .filter_map(|r| {
                    Some(Removal {
                        address: parse(&r.url)?,
                        at: r.at,
                    })
                })
                .collect(),
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VisitDto {
    pub url: String,
    #[serde(default)]
    pub title: String,
    pub at: u64,
}

impl From<&Visit> for VisitDto {
    fn from(v: &Visit) -> Self {
        Self {
            url: v.address.to_string(),
            title: v.title.clone(),
            at: v.at,
        }
    }
}

impl VisitDto {
    pub fn into_visit(self) -> Option<Visit> {
        Some(Visit {
            address: parse(&self.url)?,
            title: self.title,
            at: self.at,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsDto {
    pub home_page: String,
    pub search_name: String,
    pub search_template: String,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default = "yes")]
    pub blocker: bool,
    #[serde(default)]
    pub blocker_allowlist: Vec<String>,
}

fn yes() -> bool {
    true
}

impl From<&Settings> for SettingsDto {
    fn from(s: &Settings) -> Self {
        Self {
            home_page: s.home_page.to_string(),
            search_name: s.search_engine.name.clone(),
            search_template: s.search_engine.template.clone(),
            updated_at: s.updated_at,
            blocker: s.blocker,
            blocker_allowlist: s.blocker_allowlist.clone(),
        }
    }
}

impl SettingsDto {
    pub fn into_settings(self) -> Option<Settings> {
        Some(Settings {
            home_page: parse(&self.home_page)?,
            search_engine: SearchEngine::new(self.search_name, self.search_template),
            updated_at: self.updated_at,
            blocker: self.blocker,
            blocker_allowlist: self.blocker_allowlist,
            // Not synced: the device keeps its own.
            device: Default::default(),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TabDto {
    pub url: String,
    #[serde(default)]
    pub title: String,
}

/// A device's sync record, before encryption.
#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceRecordDto {
    /// Format version, for later changes.
    pub v: u32,
    pub device: String,
    pub name: String,
    pub updated_at: u64,
    pub bookmarks: BookmarksDto,
    pub history: Vec<VisitDto>,
    pub tabs: Vec<TabDto>,
    pub settings: SettingsDto,
}

impl From<&DeviceRecord> for DeviceRecordDto {
    fn from(r: &DeviceRecord) -> Self {
        Self {
            v: 1,
            device: r.device.to_string(),
            name: r.name.clone(),
            updated_at: r.updated_at,
            bookmarks: (&r.bookmarks).into(),
            history: r.history.iter().map(VisitDto::from).collect(),
            tabs: r
                .tabs
                .iter()
                .map(|t| TabDto {
                    url: t.address.to_string(),
                    title: t.title.clone(),
                })
                .collect(),
            settings: (&r.settings).into(),
        }
    }
}

impl DeviceRecordDto {
    pub fn into_record(self) -> Option<DeviceRecord> {
        Some(DeviceRecord {
            device: DeviceId::new(self.device)?,
            name: self.name,
            updated_at: self.updated_at,
            bookmarks: self.bookmarks.into(),
            history: self
                .history
                .into_iter()
                .filter_map(VisitDto::into_visit)
                .collect(),
            tabs: self
                .tabs
                .into_iter()
                .filter_map(|t| {
                    Some(RemoteTab {
                        address: parse(&t.url)?,
                        title: t.title,
                    })
                })
                .collect(),
            settings: self.settings.into_settings()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_settings_record_keeps_the_blocker_on() {
        let dto: SettingsDto = serde_json::from_str(
            r#"{"home_page":"https://a.com/","search_name":"DDG","search_template":"https://d.com/?q={query}"}"#,
        )
        .unwrap();
        let settings = dto.into_settings().unwrap();
        assert!(settings.blocker);
        assert!(settings.blocker_allowlist.is_empty());
    }

    #[test]
    fn settings_round_trip_with_the_allowlist() {
        let settings = Settings {
            blocker: false,
            blocker_allowlist: vec!["lemonde.fr".into()],
            updated_at: 7,
            ..Settings::default()
        };
        let json = serde_json::to_string(&SettingsDto::from(&settings)).unwrap();
        let back: SettingsDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back.into_settings().unwrap(), settings);
    }
}
