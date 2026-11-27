//! Opening, closing, ordering, grouping and splitting tabs.

use domain::{GroupColor, GroupId, TabId, WebAddress};

use super::Browser;
use crate::BrowserError;
use crate::ports::WebEngine;

/// How many closed tabs can be reopened.
const CLOSED_TABS: usize = 25;

impl<E: WebEngine> Browser<E> {
    /// Opens a tab for a link of `opener`'s page, next to it, and leaves
    /// `opener` shown. Without `opener` (or when it is closed), a tab after
    /// the last one, shown.
    pub fn open_link_tab(
        &mut self,
        opener: Option<TabId>,
        address: WebAddress,
    ) -> Result<TabId, BrowserError> {
        let Some(id) = opener.and_then(|o| self.session.open_tab_from(o)) else {
            return self.open_tab_in(Some(address), self.active_is_private());
        };
        let opened = if self.session.tab(id).is_some_and(|t| t.is_private()) {
            self.engine.open_private_view(id, &address)
        } else {
            self.engine.open_view(id, &address)
        };
        if let Err(e) = opened {
            self.session.close_tab(id);
            return Err(e.into());
        }
        Ok(id)
    }

    /// Moves tab `id` to index `to` of the tab strip.
    pub fn move_tab(&mut self, id: TabId, to: usize) {
        self.session.move_tab(id, to);
    }

    /// Pins tab `id` (first, small, out of its group) or unpins it.
    pub fn set_pinned(&mut self, id: TabId, pinned: bool) {
        self.session.set_pinned(id, pinned);
    }

    /// Puts tab `id` in a new group of its own.
    pub fn new_group(&mut self, id: TabId) -> Option<GroupId> {
        self.session.new_group(id)
    }

    /// Moves tab `id` to the end of `group`.
    pub fn add_to_group(&mut self, id: TabId, group: GroupId) {
        self.session.add_to_group(id, group);
    }

    /// Leaves tab `id` out of the saved session (an extension's window).
    pub fn keep_out_of_session(&mut self, id: TabId) {
        self.session.set_transient(id);
    }

    /// Takes tab `id` out of its group.
    pub fn remove_from_group(&mut self, id: TabId) {
        self.session.remove_from_group(id);
    }

    /// Shows tab `id` beside the active one (a split view), and activates
    /// it. Nothing when the engine shows one page at a time.
    pub fn split_with_active(&mut self, id: TabId) {
        if !self.engine.can_split() {
            return;
        }
        if let Some(active) = self.session.active_id()
            && self.session.split(active, id)
        {
            self.activate(id);
        }
    }

    /// Opens a new tab beside the active one, in a split view with it.
    pub fn split_with_new_tab(&mut self) -> Result<(), BrowserError> {
        let Some(active) = self.session.active_id() else {
            return Ok(());
        };
        if !self.engine.can_split() || self.session.split_of(active).is_some() {
            return Ok(());
        }
        let private = self.session.tab(active).is_some_and(|t| t.is_private());
        let id = self.open_tab_in(None, private)?;
        self.session.split(active, id);
        Ok(())
    }

    /// Shows the tabs of `id`'s split one at a time again.
    pub fn unsplit(&mut self, id: TabId) {
        self.session.unsplit(id);
    }

    /// The first tab's share of the width of `id`'s split, per thousand.
    pub fn set_split_ratio(&mut self, id: TabId, ratio: u16) {
        self.session.set_split_ratio(id, ratio);
    }

    /// Whether the engine can show two pages side by side.
    pub fn can_split(&self) -> bool {
        self.engine.can_split()
    }

    /// Takes all the tabs of `group` out of it.
    pub fn ungroup(&mut self, group: GroupId) {
        self.session.ungroup(group);
    }

    pub fn rename_group(&mut self, group: GroupId, name: &str) {
        self.session.rename_group(group, name);
    }

    pub fn set_group_color(&mut self, group: GroupId, color: GroupColor) {
        self.session.set_group_color(group, color);
    }

    /// Collapses or expands `group`; collapsed with the active tab, another
    /// one is shown.
    pub fn set_group_collapsed(&mut self, group: GroupId, collapsed: bool) {
        let before = self.session.active_id();
        self.session.set_group_collapsed(group, collapsed);
        if self.session.active_id() != before {
            self.show(self.session.active_id());
        }
    }

    /// Closes every tab of `group`.
    pub fn close_group(&mut self, group: GroupId) {
        for id in self.session.group_tabs(group) {
            self.close_tab(id);
        }
    }

    /// Opens a tab on `address` (the home page when `None`) and shows it.
    pub fn open_tab(&mut self, address: Option<WebAddress>) -> Result<TabId, BrowserError> {
        self.open_tab_in(address, false)
    }

    /// [`Self::open_tab`], private or not: a private tab's pages stay out of
    /// the history, the saved session and sync.
    pub fn open_tab_in(
        &mut self,
        address: Option<WebAddress>,
        private: bool,
    ) -> Result<TabId, BrowserError> {
        let address = address.unwrap_or_else(|| self.settings.home_page.clone());
        let id = if private {
            self.session.open_private_tab()
        } else {
            self.session.open_tab()
        };
        let opened = if private {
            self.engine.open_private_view(id, &address)
        } else {
            self.engine.open_view(id, &address)
        };
        if let Err(e) = opened {
            self.session.close_tab(id);
            return Err(e.into());
        }
        self.engine.show(Some(id));
        Ok(id)
    }

    pub fn close_tab(&mut self, id: TabId) {
        // Not a private one: it would come back in the open.
        let index = self.session.tabs().iter().position(|t| t.id() == id);
        if let (Some(index), Some(tab)) = (index, self.session.tab(id))
            && !tab.is_private()
            && let Some(address) = tab.address()
        {
            if self.closed.len() == CLOSED_TABS {
                self.closed.remove(0);
            }
            self.closed.push((address.clone(), index));
        }
        self.typed_usernames.remove(&id);
        if self.session.close_tab(id) {
            self.engine.close_view(id);
            self.show(self.session.active_id());
        }
    }

    /// Whether a closed tab can be reopened.
    pub fn can_reopen_closed_tab(&self) -> bool {
        !self.closed.is_empty()
    }

    /// Reopens the last tab closed where it was, shown (Ctrl+Shift+T).
    pub fn reopen_closed_tab(&mut self) -> Result<Option<TabId>, BrowserError> {
        let Some((address, index)) = self.closed.pop() else {
            return Ok(None);
        };
        let id = self.open_tab(Some(address))?;
        self.session.move_tab(id, index);
        Ok(Some(id))
    }

    pub fn close_active_tab(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.close_tab(id);
        }
    }

    /// Closes every private tab; returns their ids.
    pub fn close_private_tabs(&mut self) -> Vec<TabId> {
        let ids: Vec<TabId> = self
            .session
            .tabs()
            .iter()
            .filter(|t| t.is_private())
            .map(|t| t.id())
            .collect();
        for &id in &ids {
            self.close_tab(id);
        }
        ids
    }

    /// Whether the active tab is private: the tabs it opens are too.
    pub(super) fn active_is_private(&self) -> bool {
        self.session.active().is_some_and(|t| t.is_private())
    }

    pub fn activate(&mut self, id: TabId) {
        if self.session.activate(id) {
            self.show(Some(id));
        }
    }

    /// Activates the tab `offset` places away from the active one.
    pub fn cycle_tabs(&mut self, offset: isize) {
        if let Some(id) = self.session.neighbour(offset) {
            self.activate(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    #[test]
    fn open_tab_loads_the_home_page_and_shows_it() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        assert_eq!(
            take_calls(&mut b),
            vec![
                Call::Open(id.get(), "https://duckduckgo.com/".into()),
                Call::Show(Some(id.get())),
            ]
        );
        assert_eq!(b.session().active_id(), Some(id));
    }

    #[test]
    fn a_failed_open_leaves_no_tab() {
        let mut b = browser();
        b.engine_mut().fail_open = true;
        assert!(b.open_tab(None).is_err());
        assert!(b.session().is_empty());
    }

    #[test]
    fn closing_shows_the_next_active_tab() {
        let mut b = browser();
        let first = b.open_tab(None).unwrap();
        let second = b.open_tab(None).unwrap();
        take_calls(&mut b);
        b.close_tab(second);
        assert_eq!(
            take_calls(&mut b),
            vec![Call::Close(second.get()), Call::Show(Some(first.get()))]
        );
        b.close_active_tab();
        assert!(b.session().is_empty());
    }

    #[test]
    fn links_open_in_a_tab_next_to_their_page_which_stays_shown() {
        let mut b = browser();
        let page = b.open_tab(None).unwrap();
        let last = b.open_tab(None).unwrap();
        b.activate(page);
        take_calls(&mut b);
        b.handle(EngineEvent::NewTabRequested {
            opener: Some(page),
            address: addr("https://link.com/"),
        })
        .unwrap();
        let tabs: Vec<TabId> = b.session().tabs().iter().map(|t| t.id()).collect();
        assert_eq!(tabs.len(), 3);
        let link = tabs[1];
        assert_eq!(tabs, vec![page, link, last]);
        assert_eq!(b.session().active_id(), Some(page));
        assert_eq!(
            take_calls(&mut b),
            vec![Call::Open(link.get(), "https://link.com/".into())],
            "opened but not shown"
        );
    }

    #[test]
    fn closed_tabs_reopen_where_they_were_last_first() {
        let mut b = browser();
        let a = b.open_tab(None).unwrap();
        finish(&mut b, a, "https://a.com/");
        let m = b.open_tab(None).unwrap();
        finish(&mut b, m, "https://m.com/");
        let z = b.open_tab(None).unwrap();
        finish(&mut b, z, "https://z.com/");
        b.close_tab(m);
        b.close_tab(a);
        let private = b.open_tab_in(Some(addr("https://p.com/")), true).unwrap();
        finish(&mut b, private, "https://p.com/");
        b.close_tab(private);
        let addresses = |b: &Browser<_>| -> Vec<String> {
            b.session()
                .tabs()
                .iter()
                .map(|t| t.address().unwrap().to_string())
                .collect()
        };
        // The private one is not kept.
        let first = b.reopen_closed_tab().unwrap().unwrap();
        assert_eq!(b.session().active_id(), Some(first));
        finish(&mut b, first, "https://a.com/");
        assert_eq!(addresses(&b), ["https://a.com/", "https://z.com/"]);
        let second = b.reopen_closed_tab().unwrap().unwrap();
        finish(&mut b, second, "https://m.com/");
        assert_eq!(
            addresses(&b),
            ["https://a.com/", "https://m.com/", "https://z.com/"]
        );
        assert!(!b.can_reopen_closed_tab());
        assert_eq!(b.reopen_closed_tab().unwrap(), None);
    }

    #[test]
    fn new_window_requests_without_opener_open_a_shown_tab() {
        let mut b = browser();
        b.open_tab(None).unwrap();
        let id = b.open_link_tab(None, addr("https://popup.com/")).unwrap();
        assert_eq!(b.session().len(), 2);
        assert_eq!(b.session().active_id(), Some(id));
    }

    #[test]
    fn tabs_are_moved_and_saved_in_their_new_order() {
        let store = Store::default();
        let mut b = browser_with(&store);
        let a = b.open_tab(None).unwrap();
        finish(&mut b, a, "https://a.com/");
        let c = b.open_tab(None).unwrap();
        finish(&mut b, c, "https://c.com/");
        b.move_tab(c, 0);
        b.persist().unwrap();
        let saved = store.session.lock().unwrap().clone().unwrap();
        assert_eq!(
            saved.tabs,
            vec![addr("https://c.com/"), addr("https://a.com/")]
        );
    }

    #[test]
    fn clicking_the_other_pane_of_a_split_activates_its_tab() {
        let mut b = browser();
        let t1 = b.open_tab(None).unwrap();
        let t2 = b.open_tab(None).unwrap();
        let t3 = b.open_tab(None).unwrap();
        b.activate(t1);
        b.split_with_active(t2);
        assert_eq!(b.session().active_id(), Some(t2));
        assert_eq!(b.session().split_partner(t1), Some(t2));

        b.handle(EngineEvent::PageFocused { tab: t1 }).unwrap();
        assert_eq!(b.session().active_id(), Some(t1));
        // Not in the split: focusing it changes nothing.
        b.handle(EngineEvent::PageFocused { tab: t3 }).unwrap();
        assert_eq!(b.session().active_id(), Some(t1));
    }

    #[test]
    fn a_new_tab_opens_beside_the_active_one() {
        let mut b = browser();
        let t1 = b.open_tab(None).unwrap();
        b.open_tab(None).unwrap();
        b.activate(t1);
        b.split_with_new_tab().unwrap();
        let new = b.session().active_id().unwrap();
        assert_ne!(new, t1);
        assert_eq!(b.session().split_partner(t1), Some(new));
        assert_eq!(b.session().tabs()[1].id(), new);
        b.unsplit(new);
        assert!(b.session().split_of(t1).is_none());
    }
}
