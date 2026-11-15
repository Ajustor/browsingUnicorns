use std::path::{Path, PathBuf};

use application::ports::{Repository, RepositoryError};
use domain::{Bookmarks, BrowsingHistory, SessionSnapshot, WebAddress};
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
    active: Option<usize>,
}

impl Repository<SessionSnapshot> for JsonRepository {
    fn load(&self) -> Result<SessionSnapshot, RepositoryError> {
        let file: SessionFile = self.load_file()?;
        let mut snapshot = SessionSnapshot::default();
        for (i, url) in file.tabs.iter().enumerate() {
            if let Some(address) = parse(url) {
                if file.active == Some(i) {
                    snapshot.active = Some(snapshot.tabs.len());
                }
                snapshot.tabs.push(address);
            }
        }
        Ok(snapshot)
    }

    fn save(&self, snapshot: &SessionSnapshot) -> Result<(), RepositoryError> {
        self.save_file(&SessionFile {
            tabs: snapshot.tabs.iter().map(ToString::to_string).collect(),
            active: snapshot.active,
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
    fn corrupt_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let r = repo(&dir, "history.json");
        files::write(r.path(), "{not json").unwrap();
        assert!(Repository::<BrowsingHistory>::load(&r).is_err());
    }
}
