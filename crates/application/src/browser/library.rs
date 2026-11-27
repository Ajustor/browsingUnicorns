//! Bookmarks and history.

use std::collections::HashSet;

use domain::{BrowsingHistory, Timestamp, Visit, WebAddress};

use super::{Browser, Stores};
use crate::BrowserError;
use crate::ports::WebEngine;

impl<E: WebEngine> Browser<E> {
    pub fn clear_history(&mut self) -> Result<(), BrowserError> {
        self.history.clear();
        self.unsynced.note();
        self.mark_dirty(Stores::HISTORY);
        self.save_dirty(Stores::HISTORY)
    }

    pub fn is_active_bookmarked(&self) -> bool {
        self.session
            .active()
            .and_then(|t| t.address())
            .is_some_and(|a| self.bookmarks.contains(a))
    }

    /// Bookmarks the active page, or removes its bookmark. Returns whether
    /// it is bookmarked afterwards.
    pub fn toggle_bookmark(&mut self) -> Result<bool, BrowserError> {
        let Some(tab) = self.session.active() else {
            return Ok(false);
        };
        let Some(address) = tab.address().cloned() else {
            return Ok(false);
        };
        let title = tab.title().to_owned();
        let saved = self.bookmarks.toggle(address, title, self.now());
        self.bookmarks_changed()?;
        Ok(saved)
    }

    pub fn remove_bookmark(&mut self, address: &WebAddress) -> Result<(), BrowserError> {
        if self.bookmarks.remove(address, self.now()) {
            self.bookmarks_changed()?;
        }
        Ok(())
    }

    /// Adds bookmarks of another browser under `root` (their folders kept
    /// inside it); those already saved stay where they are. Returns how
    /// many were added.
    pub fn import_bookmarks(
        &mut self,
        root: &str,
        items: impl IntoIterator<Item = (WebAddress, String, String)>,
    ) -> Result<usize, BrowserError> {
        let now = self.now();
        let mut added = 0;
        for (address, title, folder) in items {
            let folder = if folder.is_empty() {
                root.to_owned()
            } else {
                format!("{root}/{folder}")
            };
            if self.bookmarks.add_in(address, title, &folder, now) {
                added += 1;
            }
        }
        if added > 0 {
            self.bookmarks_changed()?;
        }
        Ok(added)
    }

    /// Adds another browser's history (its pages at their last visit).
    /// Returns how many visits were new.
    pub fn import_history(
        &mut self,
        visits: impl IntoIterator<Item = Visit>,
    ) -> Result<usize, BrowserError> {
        let known: HashSet<(WebAddress, Timestamp)> = self
            .history
            .visits()
            .iter()
            .map(|v| (v.address.clone(), v.at))
            .collect();
        let new: Vec<Visit> = visits
            .into_iter()
            .filter(|v| {
                BrowsingHistory::is_recordable(&v.address)
                    && !known.contains(&(v.address.clone(), v.at))
            })
            .collect();
        if self.history.merge(&new) {
            self.unsynced.note();
            self.mark_dirty(Stores::HISTORY);
            self.save_dirty(Stores::HISTORY)?;
        }
        Ok(new.len())
    }

    /// Moves a bookmark to `folder` (empty: out of any).
    pub fn move_bookmark(
        &mut self,
        address: &WebAddress,
        folder: &str,
    ) -> Result<(), BrowserError> {
        if self.bookmarks.move_to(address, folder, self.now()) {
            self.bookmarks_changed()?;
        }
        Ok(())
    }

    /// Saves the bookmarks changed, for the other devices too.
    fn bookmarks_changed(&mut self) -> Result<(), BrowserError> {
        self.unsynced.note();
        self.mark_dirty(Stores::BOOKMARKS);
        self.save_dirty(Stores::BOOKMARKS)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    #[test]
    fn bookmarks_are_toggled_on_the_active_page_and_saved() {
        let store = Store::default();
        let mut b = browser_with(&store);
        assert!(!b.toggle_bookmark().unwrap());
        let id = b.open_tab(None).unwrap();
        assert!(!b.toggle_bookmark().unwrap(), "nothing loaded yet");
        finish(&mut b, id, "https://a.com/");
        assert!(b.toggle_bookmark().unwrap());
        assert!(b.is_active_bookmarked());
        assert_eq!(store.bookmarks.lock().unwrap().as_ref().unwrap().len(), 1);
        b.remove_bookmark(&addr("https://a.com/")).unwrap();
        assert!(!b.is_active_bookmarked());
        assert!(store.bookmarks.lock().unwrap().as_ref().unwrap().is_empty());
    }

    #[test]
    fn imported_history_skips_known_visits() {
        let store = Store::default();
        let mut b = browser_with(&store);
        let visit = |url: &str, at| Visit {
            address: addr(url),
            title: "T".into(),
            at,
        };
        let pages = || {
            [
                visit("https://a.org/", 10),
                visit("https://b.org/", 20),
                visit("about:blank", 30),
            ]
        };
        assert_eq!(b.import_history(pages()).unwrap(), 2);
        assert_eq!(b.history().len(), 2);
        assert!(store.history.lock().unwrap().is_some());
        assert_eq!(b.import_history(pages()).unwrap(), 0);
        assert_eq!(b.history().len(), 2);
    }
}
