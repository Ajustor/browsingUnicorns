use std::collections::HashMap;

use crate::{Timestamp, WebAddress};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub address: WebAddress,
    pub title: String,
    /// When it was (last) added or moved: the most recent of an add and a
    /// removal of the same address wins when devices are merged.
    pub added_at: Timestamp,
    /// The folder it is in, its names separated by `/` (empty: none).
    pub folder: String,
}

impl Bookmark {
    /// The title, else the host, else the address.
    pub fn label(&self) -> &str {
        let title = self.title.trim();
        if !title.is_empty() {
            return title;
        }
        self.address.host().unwrap_or(self.address.as_str())
    }
}

/// A bookmark that was removed, kept so that merging with a device that
/// still has it does not bring it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    pub address: WebAddress,
    pub at: Timestamp,
}

/// Saved pages, in the order they were added; one per address.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bookmarks {
    items: Vec<Bookmark>,
    removed: Vec<Removal>,
}

impl Bookmarks {
    /// Removals older than the newest ones beyond this count are forgotten.
    pub const MAX_REMOVALS: usize = 1_000;

    pub fn from_parts(items: Vec<Bookmark>, removed: Vec<Removal>) -> Self {
        let mut bookmarks = Self::default();
        for b in items {
            bookmarks.apply_add(b);
        }
        for r in removed {
            bookmarks.apply_removal(r);
        }
        bookmarks
    }

    pub fn items(&self) -> &[Bookmark] {
        &self.items
    }

    pub fn removals(&self) -> &[Removal] {
        &self.removed
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, address: &WebAddress) -> bool {
        self.items.iter().any(|b| &b.address == address)
    }

    /// Adds a bookmark; returns `false` when the address is already saved.
    pub fn add(&mut self, address: WebAddress, title: impl Into<String>, at: Timestamp) -> bool {
        if self.contains(&address) {
            return false;
        }
        self.removed.retain(|r| r.address != address);
        self.items.push(Bookmark {
            address,
            title: title.into().trim().to_owned(),
            added_at: at,
            folder: String::new(),
        });
        true
    }

    /// Adds a bookmark in `folder`; `false` when the address is already
    /// saved (it stays where it is).
    pub fn add_in(
        &mut self,
        address: WebAddress,
        title: impl Into<String>,
        folder: &str,
        at: Timestamp,
    ) -> bool {
        if !self.add(address, title, at) {
            return false;
        }
        if let Some(b) = self.items.last_mut() {
            b.folder = clean_folder(folder);
        }
        true
    }

    /// Moves the bookmark of `address` to `folder` (empty: out of any).
    /// The move wins over older changes on other devices.
    pub fn move_to(&mut self, address: &WebAddress, folder: &str, at: Timestamp) -> bool {
        let folder = clean_folder(folder);
        match self.items.iter_mut().find(|b| &b.address == address) {
            Some(b) if b.folder != folder => {
                b.folder = folder;
                b.added_at = b.added_at.max(at);
                true
            }
            _ => false,
        }
    }

    /// The folders that hold bookmarks, sorted.
    pub fn folders(&self) -> Vec<String> {
        let mut folders: Vec<String> = self
            .items
            .iter()
            .filter(|b| !b.folder.is_empty())
            .map(|b| b.folder.clone())
            .collect();
        folders.sort_by_key(|f| f.to_lowercase());
        folders.dedup();
        folders
    }

    pub fn remove(&mut self, address: &WebAddress, at: Timestamp) -> bool {
        let before = self.items.len();
        self.items.retain(|b| &b.address != address);
        if self.items.len() == before {
            return false;
        }
        self.record_removal(Removal {
            address: address.clone(),
            at,
        });
        true
    }

    /// Adds the page when it is not saved, removes it otherwise. Returns
    /// whether it is saved afterwards.
    pub fn toggle(&mut self, address: WebAddress, title: impl Into<String>, at: Timestamp) -> bool {
        if self.remove(&address, at) {
            false
        } else {
            self.add(address, title, at)
        }
    }

    /// Takes in another device's bookmarks: for each address, its most
    /// recent add or removal wins (an add wins a tie). Returns whether
    /// anything changed.
    pub fn merge(&mut self, other: &Bookmarks) -> bool {
        let before = self.clone();
        for b in &other.items {
            self.apply_add(b.clone());
        }
        for r in &other.removed {
            self.apply_removal(r.clone());
        }
        *self != before
    }

    fn apply_add(&mut self, bookmark: Bookmark) {
        let removed_after = self
            .removed
            .iter()
            .any(|r| r.address == bookmark.address && r.at > bookmark.added_at);
        if removed_after {
            return;
        }
        self.removed.retain(|r| r.address != bookmark.address);
        match self
            .items
            .iter_mut()
            .find(|b| b.address == bookmark.address)
        {
            Some(existing) if bookmark.added_at > existing.added_at => *existing = bookmark,
            Some(_) => {}
            None => self.items.push(bookmark),
        }
    }

    fn apply_removal(&mut self, removal: Removal) {
        let added_since = self
            .items
            .iter()
            .any(|b| b.address == removal.address && b.added_at >= removal.at);
        if added_since {
            return;
        }
        self.items.retain(|b| b.address != removal.address);
        self.record_removal(removal);
    }

    fn record_removal(&mut self, removal: Removal) {
        match self
            .removed
            .iter_mut()
            .find(|r| r.address == removal.address)
        {
            Some(existing) => existing.at = existing.at.max(removal.at),
            None => self.removed.push(removal),
        }
        if self.removed.len() > Self::MAX_REMOVALS {
            self.removed.sort_by_key(|r| std::cmp::Reverse(r.at));
            self.removed.truncate(Self::MAX_REMOVALS);
        }
    }

    /// Titles by address, for tests and lookups.
    pub fn titles(&self) -> HashMap<&WebAddress, &str> {
        self.items
            .iter()
            .map(|b| (&b.address, b.title.as_str()))
            .collect()
    }
}

/// `folder` without blanks around its names, nor empty names.
fn clean_folder(folder: &str) -> String {
    folder
        .split('/')
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_go_in_folders_and_moves_win_merges() {
        let mut mine = Bookmarks::default();
        assert!(mine.add_in(a(), "A", " Work / Projects ", 1));
        assert!(mine.add(addr("https://b.com/"), "B", 1));
        assert_eq!(mine.folders(), ["Work/Projects"]);
        let theirs = mine.clone();
        assert!(mine.move_to(&a(), "", 5));
        assert!(mine.folders().is_empty());
        // The other device still has it in the folder, from before.
        assert!(!mine.merge(&theirs));
        assert_eq!(mine.items()[0].folder, "");
        let mut theirs = theirs;
        assert!(theirs.merge(&mine));
        assert_eq!(theirs.items()[0].folder, "");
    }

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    fn a() -> WebAddress {
        addr("https://a.com/")
    }

    #[test]
    fn one_bookmark_per_address() {
        let mut b = Bookmarks::default();
        assert!(b.add(a(), "A", 1));
        assert!(!b.add(a(), "again", 2));
        assert_eq!(b.len(), 1);
        let loaded =
            Bookmarks::from_parts(vec![b.items()[0].clone(), b.items()[0].clone()], vec![]);
        assert_eq!(loaded.len(), 1);
    }

    #[test]
    fn toggle_adds_then_removes_and_remembers_the_removal() {
        let mut b = Bookmarks::default();
        assert!(b.toggle(a(), "A", 1));
        assert!(b.contains(&a()));
        assert!(!b.toggle(a(), "A", 2));
        assert!(b.is_empty());
        assert_eq!(
            b.removals(),
            &[Removal {
                address: a(),
                at: 2
            }]
        );
        assert!(!b.remove(&a(), 3));
        // Adding again forgets the removal.
        b.add(a(), "A", 4);
        assert!(b.removals().is_empty());
    }

    #[test]
    fn label_falls_back_to_host() {
        let mut b = Bookmarks::default();
        b.add(addr("https://a.com/x"), "  ", 1);
        b.add(addr("file:///C:/x.html"), "", 1);
        assert_eq!(b.items()[0].label(), "a.com");
        assert_eq!(b.items()[1].label(), "file:///C:/x.html");
    }

    #[test]
    fn merge_takes_the_other_devices_additions() {
        let mut desktop = Bookmarks::default();
        desktop.add(a(), "A", 1);
        let mut phone = Bookmarks::default();
        phone.add(addr("https://b.com/"), "B", 2);
        assert!(desktop.merge(&phone));
        assert_eq!(desktop.len(), 2);
        assert!(!desktop.merge(&phone), "merging twice changes nothing");
    }

    #[test]
    fn a_later_removal_wins_over_an_earlier_add() {
        let mut desktop = Bookmarks::default();
        desktop.add(a(), "A", 1);
        let mut phone = desktop.clone();
        phone.remove(&a(), 5);
        assert!(desktop.merge(&phone));
        assert!(!desktop.contains(&a()));
        // And the other way round: the desktop's old copy does not revive it.
        assert!(!phone.merge(&Bookmarks::from_parts(
            vec![Bookmark {
                address: a(),
                title: "A".into(),
                added_at: 1,
                folder: String::new(),
            }],
            vec![]
        )));
        assert!(!phone.contains(&a()));
    }

    #[test]
    fn a_later_add_wins_over_an_earlier_removal() {
        let mut desktop = Bookmarks::default();
        desktop.add(a(), "A", 1);
        desktop.remove(&a(), 2);
        let mut phone = Bookmarks::default();
        phone.add(a(), "A again", 3);
        assert!(desktop.merge(&phone));
        assert_eq!(desktop.titles()[&a()], "A again");
        assert!(desktop.removals().is_empty());
    }

    #[test]
    fn merge_is_order_independent() {
        let mut x = Bookmarks::default();
        x.add(a(), "A", 1);
        x.add(addr("https://c.com/"), "C", 2);
        x.remove(&addr("https://c.com/"), 6);
        let mut y = Bookmarks::default();
        y.add(addr("https://c.com/"), "C", 3);
        y.add(a(), "A newer", 4);
        let mut xy = x.clone();
        xy.merge(&y);
        let mut yx = y.clone();
        yx.merge(&x);
        assert_eq!(xy.titles(), yx.titles());
        assert!(!xy.contains(&addr("https://c.com/")));
        assert_eq!(xy.titles()[&a()], "A newer");
    }
}
