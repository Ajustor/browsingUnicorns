use crate::{Tab, TabId};

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

    /// Opens an empty tab right after the active one and shows it.
    pub fn open_tab(&mut self) -> TabId {
        self.next_id += 1;
        let id = TabId::new(self.next_id);
        let at = self
            .active
            .and_then(|a| self.position(a))
            .map_or(self.tabs.len(), |i| i + 1);
        self.tabs.insert(at, Tab::new(id));
        self.active = Some(id);
        id
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
    fn opening_activates_and_inserts_after_active() {
        let mut s = Session::new();
        let a = s.open_tab();
        let b = s.open_tab();
        s.activate(a);
        let c = s.open_tab();
        assert_eq!(ids(&s), vec![a.get(), c.get(), b.get()]);
        assert_eq!(s.active_id(), Some(c));
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
