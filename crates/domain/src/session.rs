use crate::{Tab, TabId, WebAddress};

/// What is needed to reopen a window's tabs: their pages and which one was
/// shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub tabs: Vec<WebAddress>,
    /// Index into `tabs`.
    pub active: Option<usize>,
}

impl SessionSnapshot {
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

/// The open tabs of a window and which one is shown.
#[derive(Debug, Clone, Default)]
pub struct Session {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: u64,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.active
    }

    pub fn active(&self) -> Option<&Tab> {
        self.active.and_then(|id| self.tab(id))
    }

    pub fn tab(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id() == id)
    }

    pub fn tab_mut(&mut self, id: TabId) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id() == id)
    }

    fn position(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|t| t.id() == id)
    }

    /// Opens an empty tab after the last one and shows it.
    pub fn open_tab(&mut self) -> TabId {
        let id = self.insert(self.tabs.len(), false, None);
        self.active = Some(id);
        id
    }

    /// [`Self::open_tab`] for a private tab.
    pub fn open_private_tab(&mut self) -> TabId {
        let id = self.insert(self.tabs.len(), true, None);
        self.active = Some(id);
        id
    }

    /// Opens a tab for a link of `opener`'s page, without showing it: right
    /// after `opener` and the tabs it already opened, so a series of links
    /// keeps its order. Private when `opener` is. `None` when `opener` is
    /// not open.
    pub fn open_tab_from(&mut self, opener: TabId) -> Option<TabId> {
        let index = self.position(opener)?;
        let private = self.tabs[index].is_private();
        let at = index
            + 1
            + self.tabs[index + 1..]
                .iter()
                .take_while(|t| t.opener() == Some(opener))
                .count();
        Some(self.insert(at, private, Some(opener)))
    }

    fn insert(&mut self, at: usize, private: bool, opener: Option<TabId>) -> TabId {
        self.next_id += 1;
        let id = TabId::new(self.next_id);
        let mut tab = if private {
            Tab::new_private(id)
        } else {
            Tab::new(id)
        };
        tab.opener = opener;
        self.tabs.insert(at, tab);
        id
    }

    /// Moves `id` to index `to` (clamped to the last place), the other tabs
    /// shifting around it. Returns whether the order changed.
    pub fn move_tab(&mut self, id: TabId, to: usize) -> bool {
        let Some(from) = self.position(id) else {
            return false;
        };
        let to = to.min(self.tabs.len() - 1);
        if from == to {
            return false;
        }
        let mut tab = self.tabs.remove(from);
        // Placed by hand: it no longer belongs to its opener's series.
        tab.opener = None;
        self.tabs.insert(to, tab);
        true
    }

    /// Closes `id`. When it was the active tab, its right neighbour (else its
    /// left one) becomes active. Returns whether a tab was closed.
    pub fn close_tab(&mut self, id: TabId) -> bool {
        let Some(index) = self.position(id) else {
            return false;
        };
        self.tabs.remove(index);
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.last())
                .map(Tab::id);
        }
        true
    }

    pub fn activate(&mut self, id: TabId) -> bool {
        let known = self.position(id).is_some();
        if known {
            self.active = Some(id);
        }
        known
    }

    /// The tabs that show a page, in order. Tabs that have not loaded
    /// anything yet, and private ones, are left out.
    pub fn snapshot(&self) -> SessionSnapshot {
        let mut snapshot = SessionSnapshot::default();
        for tab in self.tabs.iter().filter(|t| !t.is_private()) {
            if let Some(address) = tab.address() {
                if Some(tab.id()) == self.active {
                    snapshot.active = Some(snapshot.tabs.len());
                }
                snapshot.tabs.push(address.clone());
            }
        }
        snapshot
    }

    /// The tab `offset` places away from the active one, wrapping around.
    pub fn neighbour(&self, offset: isize) -> Option<TabId> {
        let index = self.active.and_then(|a| self.position(a))?;
        let len = self.tabs.len() as isize;
        let target = (index as isize + offset).rem_euclid(len) as usize;
        Some(self.tabs[target].id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(s: &Session) -> Vec<u64> {
        s.tabs().iter().map(|t| t.id().get()).collect()
    }

    #[test]
    fn a_new_tab_goes_last_and_shows() {
        let mut s = Session::new();
        let a = s.open_tab();
        let b = s.open_tab();
        s.activate(a);
        let c = s.open_tab();
        assert_eq!(ids(&s), vec![a.get(), b.get(), c.get()]);
        assert_eq!(s.active_id(), Some(c));
    }

    #[test]
    fn links_open_after_their_opener_in_order_without_showing() {
        let mut s = Session::new();
        let a = s.open_tab();
        let b = s.open_tab();
        s.activate(a);
        let l1 = s.open_tab_from(a).unwrap();
        let l2 = s.open_tab_from(a).unwrap();
        assert_eq!(ids(&s), vec![a.get(), l1.get(), l2.get(), b.get()]);
        assert_eq!(s.active_id(), Some(a), "the opener stays shown");
        assert_eq!(s.tab(l1).unwrap().opener(), Some(a));
        assert!(s.open_tab_from(TabId::new(99)).is_none());
    }

    #[test]
    fn a_private_tab_opens_private_links() {
        let mut s = Session::new();
        let p = s.open_private_tab();
        let link = s.open_tab_from(p).unwrap();
        assert!(s.tab(link).unwrap().is_private());
    }

    #[test]
    fn tabs_can_be_moved() {
        let mut s = Session::new();
        let a = s.open_tab();
        let b = s.open_tab();
        let c = s.open_tab();
        assert!(s.move_tab(a, 2));
        assert_eq!(ids(&s), vec![b.get(), c.get(), a.get()]);
        assert!(s.move_tab(a, 0));
        assert_eq!(ids(&s), vec![a.get(), b.get(), c.get()]);
        assert!(s.move_tab(b, 99), "clamped to the end");
        assert_eq!(ids(&s), vec![a.get(), c.get(), b.get()]);
        assert!(!s.move_tab(b, 2));
        assert!(!s.move_tab(TabId::new(99), 0));
        assert_eq!(
            s.active_id(),
            Some(c),
            "moving does not change the active tab"
        );
    }

    #[test]
    fn a_moved_link_leaves_its_openers_series() {
        let mut s = Session::new();
        let a = s.open_tab();
        let l1 = s.open_tab_from(a).unwrap();
        s.move_tab(l1, 0);
        let l2 = s.open_tab_from(a).unwrap();
        assert_eq!(ids(&s), vec![l1.get(), a.get(), l2.get()]);
    }

    #[test]
    fn closing_the_active_tab_activates_its_right_then_left_neighbour() {
        let mut s = Session::new();
        let a = s.open_tab();
        let b = s.open_tab();
        let c = s.open_tab();
        s.activate(b);
        assert!(s.close_tab(b));
        assert_eq!(s.active_id(), Some(c));
        assert!(s.close_tab(c));
        assert_eq!(s.active_id(), Some(a));
        assert!(s.close_tab(a));
        assert_eq!(s.active_id(), None);
        assert!(s.is_empty());
    }

    #[test]
    fn closing_another_tab_keeps_the_active_one() {
        let mut s = Session::new();
        let a = s.open_tab();
        let b = s.open_tab();
        assert!(s.close_tab(a));
        assert_eq!(s.active_id(), Some(b));
        assert!(!s.close_tab(a));
    }

    #[test]
    fn ids_are_never_reused() {
        let mut s = Session::new();
        let a = s.open_tab();
        s.close_tab(a);
        assert_ne!(s.open_tab(), a);
    }

    #[test]
    fn snapshot_keeps_loaded_tabs_and_the_active_index() {
        let mut s = Session::new();
        let a = s.open_tab();
        let _blank = s.open_tab();
        let c = s.open_tab();
        let page = |p: &str| WebAddress::parse(&format!("https://x.com/{p}")).unwrap();
        s.tab_mut(a).unwrap().load_started(page("a"));
        s.tab_mut(c).unwrap().load_started(page("c"));
        let snap = s.snapshot();
        assert_eq!(snap.tabs, vec![page("a"), page("c")]);
        assert_eq!(snap.active, Some(1));
        assert!(Session::new().snapshot().is_empty());
    }

    #[test]
    fn private_tabs_are_not_in_the_snapshot() {
        let mut s = Session::new();
        let a = s.open_tab();
        let p = s.open_private_tab();
        assert!(s.tab(p).unwrap().is_private());
        assert!(!s.tab(a).unwrap().is_private());
        let page = |p: &str| WebAddress::parse(&format!("https://x.com/{p}")).unwrap();
        s.tab_mut(a).unwrap().load_started(page("a"));
        s.tab_mut(p).unwrap().load_started(page("secret"));
        let snap = s.snapshot();
        assert_eq!(snap.tabs, vec![page("a")]);
        assert_eq!(snap.active, None, "the active tab is private");
    }

    #[test]
    fn neighbour_wraps_around() {
        let mut s = Session::new();
        let a = s.open_tab();
        let b = s.open_tab();
        assert_eq!(s.neighbour(1), Some(a));
        assert_eq!(s.neighbour(-1), Some(a));
        s.activate(a);
        assert_eq!(s.neighbour(1), Some(b));
        assert!(!s.activate(TabId::new(99)));
    }
}
