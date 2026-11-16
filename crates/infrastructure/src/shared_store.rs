//! Profile files that several windows of a profile write (a tab moved to
//! its own window runs in its own process).

use application::ports::{Repository, RepositoryError};
use domain::{Bookmarks, BrowsingHistory};

/// What another window may have saved since this one loaded it, and how to
/// take this window's value into it.
pub trait Shared: Sized {
    /// Takes `ours` into `self`, the value on disk; `false` when `ours`
    /// replaces it instead.
    fn absorb(&mut self, ours: &Self) -> bool;
}

impl Shared for BrowsingHistory {
    /// A cleared history is written as is.
    fn absorb(&mut self, ours: &Self) -> bool {
        if ours.is_empty() {
            return false;
        }
        self.merge(ours.visits());
        true
    }
}

impl Shared for Bookmarks {
    /// Removals are kept as dated tombstones: merging applies them.
    fn absorb(&mut self, ours: &Self) -> bool {
        self.merge(ours);
        true
    }
}

/// Saves the union of what is on disk and what is saved, so that windows
/// sharing the file don't erase each other's changes.
pub struct MergingRepository<R> {
    inner: R,
}

impl<R> MergingRepository<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<T: Shared, R: Repository<T>> Repository<T> for MergingRepository<R> {
    fn load(&self) -> Result<T, RepositoryError> {
        self.inner.load()
    }

    fn save(&self, value: &T) -> Result<(), RepositoryError> {
        if let Ok(mut on_disk) = self.inner.load()
            && on_disk.absorb(value)
        {
            return self.inner.save(&on_disk);
        }
        self.inner.save(value)
    }
}

#[cfg(test)]
mod tests {
    use domain::WebAddress;

    use super::*;
    use crate::JsonRepository;

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    #[test]
    fn two_windows_keep_each_others_visits_until_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let a = MergingRepository::new(JsonRepository::new(&path));
        let b = MergingRepository::new(JsonRepository::new(&path));
        let mut ha: BrowsingHistory = a.load().unwrap();
        let mut hb: BrowsingHistory = b.load().unwrap();
        ha.record(addr("https://a.com/"), 1);
        a.save(&ha).unwrap();
        hb.record(addr("https://b.com/"), 2);
        b.save(&hb).unwrap();
        let saved: BrowsingHistory = a.load().unwrap();
        assert_eq!(saved.len(), 2);

        a.save(&BrowsingHistory::default()).unwrap();
        let cleared: BrowsingHistory = a.load().unwrap();
        assert!(cleared.is_empty());
    }

    #[test]
    fn a_bookmark_removed_in_one_window_stays_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bookmarks.json");
        let a = MergingRepository::new(JsonRepository::new(&path));
        let b = MergingRepository::new(JsonRepository::new(&path));
        let mut ba = Bookmarks::default();
        ba.toggle(addr("https://a.com/"), "A".to_owned(), 1);
        a.save(&ba).unwrap();
        let mut bb: Bookmarks = b.load().unwrap();
        bb.toggle(addr("https://b.com/"), "B".to_owned(), 2);
        b.save(&bb).unwrap();
        ba.remove(&addr("https://a.com/"), 3);
        a.save(&ba).unwrap();
        let saved: Bookmarks = a.load().unwrap();
        assert!(!saved.contains(&addr("https://a.com/")));
        assert!(saved.contains(&addr("https://b.com/")));
    }
}
