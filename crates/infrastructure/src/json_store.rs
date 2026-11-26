use std::path::{Path, PathBuf};

use application::ports::{Repository, RepositoryError};
use domain::{
    Bookmarks, BrowsingHistory, Extension, ExtensionId, Extensions, GroupColor, SavedGroup,
    SavedSplit, SessionSnapshot, WebAddress,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::dto::{BookmarksDto, VisitDto};
use crate::files;

/// A domain value kept in a JSON file. A missing file loads as the default
/// value; entries whose address no longer parses are dropped.
pub struct JsonRepository {
    path: PathBuf,
}

impl JsonRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load_file<F: DeserializeOwned + Default>(&self) -> Result<F, RepositoryError> {
        match files::read(&self.path)? {
            Some(text) => serde_json::from_str(&text).map_err(|e| files::error(&self.path, e)),
            None => Ok(F::default()),
        }
    }

    fn save_file<F: Serialize>(&self, file: &F) -> Result<(), RepositoryError> {
        let text = serde_json::to_string(file).map_err(|e| files::error(&self.path, e))?;
        files::write(&self.path, &text)
    }
}

fn parse(url: &str) -> Option<WebAddress> {
    WebAddress::parse(url).ok()
}

#[derive(Default, Serialize, Deserialize)]
struct HistoryFile {
    visits: Vec<VisitDto>,
}

impl Repository<BrowsingHistory> for JsonRepository {
    fn load(&self) -> Result<BrowsingHistory, RepositoryError> {
        let file: HistoryFile = self.load_file()?;
        let visits = file
            .visits
            .into_iter()
            .filter_map(VisitDto::into_visit)
            .collect();
        Ok(BrowsingHistory::from_visits(visits))
    }

    fn save(&self, history: &BrowsingHistory) -> Result<(), RepositoryError> {
        self.save_file(&HistoryFile {
            visits: history.visits().iter().map(VisitDto::from).collect(),
        })
    }
}

impl Repository<Bookmarks> for JsonRepository {
    fn load(&self) -> Result<Bookmarks, RepositoryError> {
        Ok(self.load_file::<BookmarksDto>()?.into())
    }

    fn save(&self, bookmarks: &Bookmarks) -> Result<(), RepositoryError> {
        self.save_file(&BookmarksDto::from(bookmarks))
    }
}

#[derive(Default, Serialize, Deserialize)]
struct SessionFile {
    tabs: Vec<String>,
    /// Side by side with `tabs`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    titles: Vec<String>,
    active: Option<usize>,
    /// The first `pinned` tabs are pinned.
    #[serde(default, skip_serializing_if = "is_zero")]
    pinned: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    groups: Vec<GroupDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    splits: Vec<SplitDto>,
}

/// Two tabs side by side: `first` and the next one.
#[derive(Serialize, Deserialize)]
struct SplitDto {
    first: usize,
    /// The first tab's share of the width, per thousand.
    ratio: u16,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

#[derive(Serialize, Deserialize)]
struct GroupDto {
    #[serde(default)]
    name: String,
    color: String,
    #[serde(default)]
    collapsed: bool,
    /// Index of its first tab, and how many it has.
    first: usize,
    len: usize,
}

impl Repository<SessionSnapshot> for JsonRepository {
    fn load(&self) -> Result<SessionSnapshot, RepositoryError> {
        let file: SessionFile = self.load_file()?;
        let mut snapshot = SessionSnapshot::default();
        // Where each tab of the file lands, its invalid ones left out.
        let mut kept = Vec::with_capacity(file.tabs.len());
        for (i, url) in file.tabs.iter().enumerate() {
            kept.push(snapshot.tabs.len());
            if let Some(address) = parse(url) {
                if file.active == Some(i) {
                    snapshot.active = Some(snapshot.tabs.len());
                }
                if i < file.pinned {
                    snapshot.pinned += 1;
                }
                snapshot.tabs.push(address);
                snapshot
                    .titles
                    .push(file.titles.get(i).cloned().unwrap_or_default());
            }
        }
        kept.push(snapshot.tabs.len());
        for group in file.groups {
            let end = group.first.saturating_add(group.len).min(file.tabs.len());
            let Some((&start, &end)) = kept.get(group.first).zip(kept.get(end)) else {
                continue;
            };
            if start < end {
                snapshot.groups.push(SavedGroup {
                    name: group.name,
                    color: GroupColor::parse(&group.color),
                    collapsed: group.collapsed,
                    tabs: start..end,
                });
            }
        }
        for split in file.splits {
            // Both tabs kept, still side by side.
            let (Some(&first), Some(&next), Some(&after)) = (
                kept.get(split.first),
                kept.get(split.first + 1),
                kept.get(split.first + 2),
            ) else {
                continue;
            };
            if next == first + 1 && after == next + 1 {
                snapshot.splits.push(SavedSplit {
                    first,
                    ratio: split.ratio,
                });
            }
        }
        Ok(snapshot)
    }

    fn save(&self, snapshot: &SessionSnapshot) -> Result<(), RepositoryError> {
        self.save_file(&SessionFile {
            tabs: snapshot.tabs.iter().map(ToString::to_string).collect(),
            titles: snapshot.titles.clone(),
            active: snapshot.active,
            pinned: snapshot.pinned,
            groups: snapshot
                .groups
                .iter()
                .map(|g| GroupDto {
                    name: g.name.clone(),
                    color: g.color.as_str().to_owned(),
                    collapsed: g.collapsed,
                    first: g.tabs.start,
                    len: g.tabs.len(),
                })
                .collect(),
            splits: snapshot
                .splits
                .iter()
                .map(|s| SplitDto {
                    first: s.first,
                    ratio: s.ratio,
                })
                .collect(),
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
struct ExtensionsFile {
    extensions: Vec<ExtensionDto>,
}

#[derive(Serialize, Deserialize)]
struct ExtensionDto {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    enabled: bool,
    #[serde(default)]
    popup: Option<String>,
    #[serde(default)]
    options: Option<String>,
}

impl Repository<Extensions> for JsonRepository {
    fn load(&self) -> Result<Extensions, RepositoryError> {
        let file: ExtensionsFile = self.load_file()?;
        Ok(Extensions::from_items(
            file.extensions
                .into_iter()
                .filter_map(|e| {
                    Some(Extension {
                        id: ExtensionId::parse(&e.id)?,
                        name: e.name,
                        version: e.version,
                        description: e.description,
                        enabled: e.enabled,
                        popup: e.popup,
                        options: e.options,
                    })
                })
                .collect(),
        ))
    }

    fn save(&self, extensions: &Extensions) -> Result<(), RepositoryError> {
        self.save_file(&ExtensionsFile {
            extensions: extensions
                .items()
                .iter()
                .map(|e| ExtensionDto {
                    id: e.id.to_string(),
                    name: e.name.clone(),
                    version: e.version.clone(),
                    description: e.description.clone(),
                    enabled: e.enabled,
                    popup: e.popup.clone(),
                    options: e.options.clone(),
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    fn repo(dir: &tempfile::TempDir, name: &str) -> JsonRepository {
        JsonRepository::new(dir.path().join("profile").join(name))
    }

    #[test]
    fn missing_files_load_empty() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "x.json");
        assert!(Repository::<BrowsingHistory>::load(&r).unwrap().is_empty());
        assert!(Repository::<Bookmarks>::load(&r).unwrap().is_empty());
        assert!(Repository::<SessionSnapshot>::load(&r).unwrap().is_empty());
    }

    #[test]
    fn history_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "history.json");
        let mut history = BrowsingHistory::default();
        history.record(addr("https://a.com/"), 1);
        history.retitle(&addr("https://a.com/"), "A");
        history.record(addr("https://b.com/"), 2);
        r.save(&history).unwrap();
        assert_eq!(Repository::<BrowsingHistory>::load(&r).unwrap(), history);
    }

    #[test]
    fn bookmarks_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "bookmarks.json");
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(addr("https://a.com/"), "A", 1);
        bookmarks.add(addr("https://b.com/"), "B", 2);
        bookmarks.remove(&addr("https://b.com/"), 3);
        r.save(&bookmarks).unwrap();
        assert_eq!(Repository::<Bookmarks>::load(&r).unwrap(), bookmarks);
    }

    #[test]
    fn invalid_entries_are_dropped_and_the_active_index_follows() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "session.json");
        files::write(
            r.path(),
            r#"{"tabs":["javascript:x","https://a.com/","https://b.com/"],"active":2}"#,
        )
        .unwrap();
        let snapshot: SessionSnapshot = r.load().unwrap();
        assert_eq!(
            snapshot.tabs,
            vec![addr("https://a.com/"), addr("https://b.com/")]
        );
        assert_eq!(snapshot.active, Some(1));
    }

    #[test]
    fn pinned_tabs_and_groups_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "session.json");
        let snapshot = SessionSnapshot {
            tabs: ["a", "b", "c", "d"]
                .iter()
                .map(|p| addr(&format!("https://{p}.com/")))
                .collect(),
            titles: vec!["A".into(), String::new(), "C".into(), "D".into()],
            active: Some(3),
            pinned: 1,
            groups: vec![SavedGroup {
                name: "Work".into(),
                color: GroupColor::Green,
                collapsed: true,
                tabs: 1..3,
            }],
            splits: vec![SavedSplit {
                first: 1,
                ratio: 400,
            }],
        };
        r.save(&snapshot).unwrap();
        assert_eq!(Repository::<SessionSnapshot>::load(&r).unwrap(), snapshot);
    }

    #[test]
    fn groups_follow_the_invalid_tabs_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "session.json");
        files::write(
            r.path(),
            r#"{"tabs":["https://a.com/","javascript:x","https://b.com/","https://c.com/"],
                "active":null,"pinned":2,
                "groups":[{"name":"G","color":"blue","first":1,"len":2},
                          {"color":"red","first":1,"len":1}]}"#,
        )
        .unwrap();
        let snapshot: SessionSnapshot = r.load().unwrap();
        assert_eq!(snapshot.tabs.len(), 3);
        assert_eq!(snapshot.pinned, 1);
        assert_eq!(snapshot.groups.len(), 1, "a group left without tabs");
        assert_eq!(snapshot.groups[0].tabs, 1..2);
        assert_eq!(snapshot.groups[0].color, GroupColor::Blue);
    }

    #[test]
    fn a_split_with_a_tab_left_out_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "session.json");
        files::write(
            r.path(),
            r#"{"tabs":["https://a.com/","javascript:x","https://b.com/","https://c.com/"],
                "active":null,
                "splits":[{"first":0,"ratio":500},{"first":2,"ratio":300}]}"#,
        )
        .unwrap();
        let snapshot: SessionSnapshot = r.load().unwrap();
        assert_eq!(
            snapshot.splits,
            vec![SavedSplit {
                first: 1,
                ratio: 300
            }]
        );
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "history.json");
        files::write(r.path(), "{not json").unwrap();
        assert!(Repository::<BrowsingHistory>::load(&r).is_err());
    }

    #[test]
    fn extensions_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let r = JsonRepository::new(dir.path().join("extensions.json"));
        assert!(Repository::<Extensions>::load(&r).unwrap().is_empty());
        let extensions = Extensions::from_items(vec![Extension {
            id: ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap(),
            name: "uBO Lite".into(),
            version: "2025.1".into(),
            description: "Blocks ads".into(),
            enabled: false,
            popup: Some("popup.html".into()),
            options: None,
        }]);
        r.save(&extensions).unwrap();
        assert_eq!(Repository::<Extensions>::load(&r).unwrap(), extensions);
    }
}
