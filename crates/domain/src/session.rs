use std::ops::Range;

use crate::{GroupColor, GroupId, Tab, TabGroup, TabId, WebAddress};

/// What is needed to reopen a window's tabs: their pages, which one was
/// shown, which are pinned and how they are grouped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub tabs: Vec<WebAddress>,
    /// The tabs' titles, side by side with `tabs` (shorter in old files).
    pub titles: Vec<String>,
    /// Index into `tabs`.
    pub active: Option<usize>,
    /// The first `pinned` tabs are pinned.
    pub pinned: usize,
    pub groups: Vec<SavedGroup>,
    pub splits: Vec<SavedSplit>,
}

/// A split of a [`SessionSnapshot`]: its tabs are `first` and the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedSplit {
    pub first: usize,
    /// The first tab's share of the width, per thousand.
    pub ratio: u16,
}

/// Two tabs shown side by side: `first` on the left, `second` right after
/// it in the tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    pub first: TabId,
    pub second: TabId,
    /// The first tab's share of the width, per thousand.
    pub ratio: u16,
}

/// A split's ratio: (narrowest, default, widest), per thousand.
pub const SPLIT_RATIO: (u16, u16, u16) = (100, 500, 900);

/// A group of a [`SessionSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedGroup {
    pub name: String,
    pub color: GroupColor,
    pub collapsed: bool,
    /// Its tabs: indexes into the snapshot's tabs, side by side.
    pub tabs: Range<usize>,
}

impl SessionSnapshot {
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

/// The open tabs of a window and which one is shown.
///
/// The pinned tabs come first; the tabs of a group are side by side, and a
/// group has at least one tab. A split's two tabs follow each other, in the
/// same group and pinned alike; a tab is in one split at most.
#[derive(Debug, Clone, Default)]
pub struct Session {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: u64,
    groups: Vec<TabGroup>,
    next_group: u64,
    splits: Vec<Split>,
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
    ///
    /// It joins `opener`'s group; a pinned `opener`'s links go after the
    /// pinned tabs.
    pub fn open_tab_from(&mut self, opener: TabId) -> Option<TabId> {
        let index = self.position(opener)?;
        let private = self.tabs[index].is_private();
        let start = if self.tabs[index].pinned {
            self.pinned_count()
        } else if self.splits.iter().any(|s| s.first == opener) {
            // After its split, kept whole.
            index + 2
        } else {
            index + 1
        };
        let at = start
            + self.tabs[start..]
                .iter()
                .take_while(|t| t.opener() == Some(opener))
                .count();
        let group = self.tabs[index].group;
        let id = self.insert(at, private, Some(opener));
        self.tabs[at].group = group;
        self.settle(at);
        self.prune_splits();
        Some(id)
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
    ///
    /// A pinned tab stays among the pinned ones, another one after them.
    /// Dropped between two tabs of a group, it joins it; away from its
    /// group's other tabs, it leaves it.
    pub fn move_tab(&mut self, id: TabId, to: usize) -> bool {
        let Some(from) = self.position(id) else {
            return false;
        };
        let pinned = self.pinned_count();
        let to = if self.tabs[from].pinned {
            to.min(pinned - 1)
        } else {
            to.clamp(pinned, self.tabs.len() - 1)
        };
        if from == to {
            return false;
        }
        // Placed by hand: it no longer belongs to its opener's series.
        self.tabs[from].opener = None;
        self.shift(from, to);
        self.settle(to);
        self.prune_groups();
        self.prune_splits();
        true
    }

    /// Moves the tab at `from` to `to`, nothing else.
    fn shift(&mut self, from: usize, to: usize) {
        let tab = self.tabs.remove(from);
        self.tabs.insert(to, tab);
    }

    /// The group the tab at `index` belongs to once there: the one around
    /// it, else its own if it is still next to (or without) its other tabs.
    fn settle(&mut self, index: usize) {
        let group_at = |i: Option<usize>| i.and_then(|i| self.tabs.get(i)).and_then(Tab::group);
        let left = group_at(index.checked_sub(1));
        let right = group_at(Some(index + 1));
        let own = self.tabs[index].group;
        let alone = |g: GroupId| {
            self.tabs
                .iter()
                .enumerate()
                .all(|(i, t)| i == index || t.group != Some(g))
        };
        let group = if left.is_some() && left == right {
            left
        } else {
            own.filter(|&g| left == Some(g) || right == Some(g) || alone(g))
        };
        self.tabs[index].group = group;
    }

    /// Forgets the groups left without tabs.
    fn prune_groups(&mut self) {
        let tabs = &self.tabs;
        self.groups
            .retain(|g| tabs.iter().any(|t| t.group == Some(g.id())));
    }

    /// How many tabs are pinned (the first ones).
    pub fn pinned_count(&self) -> usize {
        self.tabs.iter().take_while(|t| t.pinned).count()
    }

    /// Pins `id` (after the pinned tabs, out of its group) or unpins it
    /// (first after them). Returns whether it changed.
    pub fn set_pinned(&mut self, id: TabId, pinned: bool) -> bool {
        let Some(from) = self.position(id) else {
            return false;
        };
        if self.tabs[from].pinned == pinned {
            return false;
        }
        let count = self.pinned_count();
        let tab = &mut self.tabs[from];
        tab.pinned = pinned;
        tab.group = None;
        tab.opener = None;
        if pinned {
            self.shift(from, count);
        } else {
            self.shift(from, count - 1);
        }
        self.prune_groups();
        self.prune_splits();
        true
    }

    pub fn groups(&self) -> &[TabGroup] {
        &self.groups
    }

    pub fn group(&self, id: GroupId) -> Option<&TabGroup> {
        self.groups.iter().find(|g| g.id() == id)
    }

    fn group_mut(&mut self, id: GroupId) -> Option<&mut TabGroup> {
        self.groups.iter_mut().find(|g| g.id() == id)
    }

    /// The tabs of a group, in order.
    pub fn group_tabs(&self, id: GroupId) -> Vec<TabId> {
        self.tabs
            .iter()
            .filter(|t| t.group == Some(id))
            .map(Tab::id)
            .collect()
    }

    /// Puts `id` in a new group of its own, where it is (unpinned, and out
    /// of its group first). `None` when `id` is not open.
    pub fn new_group(&mut self, id: TabId) -> Option<GroupId> {
        self.position(id)?;
        self.set_pinned(id, false);
        self.remove_from_group(id);
        self.next_group += 1;
        let group = GroupId::new(self.next_group);
        let color = GroupColor::next(self.groups.iter().map(|g| g.color));
        self.groups.push(TabGroup::new(group, color));
        let index = self.position(id)?;
        self.tabs[index].group = Some(group);
        self.prune_splits();
        Some(group)
    }

    /// Moves `id` to the end of `group`. Returns whether it changed.
    pub fn add_to_group(&mut self, id: TabId, group: GroupId) -> bool {
        let Some(index) = self.position(id) else {
            return false;
        };
        if self.group(group).is_none() || self.tabs[index].group == Some(group) {
            return false;
        }
        self.set_pinned(id, false);
        self.remove_from_group(id);
        let Some(from) = self.position(id) else {
            return false;
        };
        let Some(last) = self.tabs.iter().rposition(|t| t.group == Some(group)) else {
            return false;
        };
        let to = if from > last { last + 1 } else { last };
        self.tabs[from].group = Some(group);
        self.tabs[from].opener = None;
        self.shift(from, to);
        self.prune_splits();
        true
    }

    /// Takes `id` out of its group, right after it. Returns whether it was
    /// in one.
    pub fn remove_from_group(&mut self, id: TabId) -> bool {
        let Some(from) = self.position(id) else {
            return false;
        };
        let Some(group) = self.tabs[from].group else {
            return false;
        };
        let last = self
            .tabs
            .iter()
            .rposition(|t| t.group == Some(group))
            .unwrap_or(from);
        self.tabs[from].group = None;
        self.tabs[from].opener = None;
        self.shift(from, last);
        self.prune_groups();
        self.prune_splits();
        true
    }

    /// Takes all the tabs of `group` out of it, where they are.
    pub fn ungroup(&mut self, group: GroupId) -> bool {
        let known = self.group(group).is_some();
        for tab in self.tabs.iter_mut().filter(|t| t.group == Some(group)) {
            tab.group = None;
        }
        self.prune_groups();
        known
    }

    pub fn rename_group(&mut self, group: GroupId, name: &str) -> bool {
        self.group_mut(group)
            .map(|g| g.name = name.trim().to_owned())
            .is_some()
    }

    pub fn set_group_color(&mut self, group: GroupId, color: GroupColor) -> bool {
        self.group_mut(group).map(|g| g.color = color).is_some()
    }

    /// Collapses `group` (only its label shows) or expands it. Collapsed
    /// while it has the active tab, the nearest tab out of it becomes active
    /// (it stays open when it has all the tabs).
    pub fn set_group_collapsed(&mut self, group: GroupId, collapsed: bool) -> bool {
        if self.group(group).is_none() {
            return false;
        }
        if collapsed && self.active().and_then(Tab::group) == Some(group) {
            let index = self.active.and_then(|a| self.position(a)).unwrap_or(0);
            let outside = self.tabs[index..]
                .iter()
                .chain(self.tabs[..index].iter().rev())
                .find(|t| t.group != Some(group))
                .map(Tab::id);
            match outside {
                Some(id) => self.active = Some(id),
                None => return false,
            }
        }
        if let Some(g) = self.group_mut(group) {
            g.collapsed = collapsed;
        }
        true
    }

    /// Whether `id` is out of sight, in a collapsed group.
    pub fn is_hidden(&self, id: TabId) -> bool {
        self.tab(id)
            .and_then(Tab::group)
            .and_then(|g| self.group(g))
            .is_some_and(|g| g.collapsed)
    }

    /// Expands the group of `id`, if collapsed: it is being shown.
    fn reveal(&mut self, id: TabId) {
        if let Some(group) = self.tab(id).and_then(Tab::group)
            && let Some(g) = self.group_mut(group)
        {
            g.collapsed = false;
        }
    }

    /// Pins and groups the tabs `ids` (just restored, in the order of
    /// `snapshot.tabs`) as they were.
    pub fn restore_layout(&mut self, ids: &[TabId], snapshot: &SessionSnapshot) {
        for &id in ids.iter().take(snapshot.pinned) {
            self.set_pinned(id, true);
        }
        for saved in &snapshot.groups {
            let members: Vec<TabId> = saved
                .tabs
                .clone()
                .filter_map(|i| ids.get(i).copied())
                .filter(|&id| self.tab(id).is_some_and(|t| !t.pinned))
                .collect();
            let Some((&first, rest)) = members.split_first() else {
                continue;
            };
            let Some(group) = self.new_group(first) else {
                continue;
            };
            for &id in rest {
                self.add_to_group(id, group);
            }
            if let Some(g) = self.group_mut(group) {
                g.name = saved.name.clone();
                g.color = saved.color;
                g.collapsed = saved.collapsed;
            }
        }
        for saved in &snapshot.splits {
            if let (Some(&first), Some(&second)) = (ids.get(saved.first), ids.get(saved.first + 1))
                && self.split(first, second)
            {
                self.set_split_ratio(first, saved.ratio);
            }
        }
    }

    /// Closes `id`. When it was the active tab, its right neighbour (else its
    /// left one) becomes active. Returns whether a tab was closed.
    pub fn close_tab(&mut self, id: TabId) -> bool {
        let Some(index) = self.position(id) else {
            return false;
        };
        self.tabs.remove(index);
        self.prune_groups();
        self.prune_splits();
        if self.active == Some(id) {
            self.active = self
                .tabs
                .get(index)
                .or_else(|| self.tabs.last())
                .map(Tab::id);
            if let Some(active) = self.active {
                self.reveal(active);
            }
        }
        true
    }

    /// Shows `id`, expanding its group if collapsed.
    pub fn activate(&mut self, id: TabId) -> bool {
        let known = self.position(id).is_some();
        if known {
            self.active = Some(id);
            self.reveal(id);
        }
        known
    }

    /// The tabs that show a page, in order. Tabs that have not loaded
    /// anything yet, and private ones, are left out.
    pub fn snapshot(&self) -> SessionSnapshot {
        let mut snapshot = SessionSnapshot::default();
        let mut last_group = None;
        // Where each saved tab is in the snapshot.
        let mut saved = Vec::new();
        for tab in self.tabs.iter().filter(|t| !t.is_private()) {
            let Some(address) = tab.address() else {
                continue;
            };
            let index = snapshot.tabs.len();
            saved.push((tab.id(), index));
            if Some(tab.id()) == self.active {
                snapshot.active = Some(index);
            }
            if tab.pinned {
                snapshot.pinned += 1;
            }
            match tab.group.and_then(|g| self.group(g)) {
                Some(group) if last_group == Some(group.id()) => {
                    if let Some(saved) = snapshot.groups.last_mut() {
                        saved.tabs.end = index + 1;
                    }
                }
                Some(group) => {
                    snapshot.groups.push(SavedGroup {
                        name: group.name.clone(),
                        color: group.color,
                        collapsed: group.collapsed,
                        tabs: index..index + 1,
                    });
                    last_group = Some(group.id());
                }
                None => last_group = None,
            }
            snapshot.tabs.push(address.clone());
            snapshot.titles.push(tab.title().to_owned());
        }
        let at = |id: TabId| saved.iter().find(|(t, _)| *t == id).map(|(_, i)| *i);
        for split in &self.splits {
            // Both saved, still side by side.
            if let (Some(first), Some(second)) = (at(split.first), at(split.second))
                && second == first + 1
            {
                snapshot.splits.push(SavedSplit {
                    first,
                    ratio: split.ratio,
                });
            }
        }
        snapshot
    }

    /// The split `id` is in.
    pub fn split_of(&self, id: TabId) -> Option<&Split> {
        self.splits.iter().find(|s| s.first == id || s.second == id)
    }

    /// The tab shown beside `id`, in its split.
    pub fn split_partner(&self, id: TabId) -> Option<TabId> {
        self.split_of(id)
            .map(|s| if s.first == id { s.second } else { s.first })
    }

    /// Shows `second` beside `first`: it moves right after it (into its
    /// group, pinned alike). Returns whether they are split now; neither may
    /// be in a split already.
    pub fn split(&mut self, first: TabId, second: TabId) -> bool {
        if first == second
            || self.split_of(first).is_some()
            || self.split_of(second).is_some()
            || self.position(first).is_none()
            || self.position(second).is_none()
        {
            return false;
        }
        let (Some(a), Some(b)) = (self.position(first), self.position(second)) else {
            return false;
        };
        let (pinned, group) = (self.tabs[a].pinned, self.tabs[a].group);
        self.tabs[b].pinned = pinned;
        self.tabs[b].group = group;
        self.tabs[b].opener = None;
        let to = if b > a { a + 1 } else { a };
        self.shift(b, to);
        self.prune_groups();
        self.splits.push(Split {
            first,
            second,
            ratio: SPLIT_RATIO.1,
        });
        self.prune_splits();
        self.split_of(first).is_some()
    }

    /// Ends the split `id` is in. Returns whether it was in one.
    pub fn unsplit(&mut self, id: TabId) -> bool {
        let before = self.splits.len();
        self.splits.retain(|s| s.first != id && s.second != id);
        self.splits.len() != before
    }

    /// Sets the share of the width of the split `id` is in (its first tab's,
    /// per thousand, kept within [`SPLIT_RATIO`]).
    pub fn set_split_ratio(&mut self, id: TabId, ratio: u16) -> bool {
        let (min, _, max) = SPLIT_RATIO;
        match self
            .splits
            .iter_mut()
            .find(|s| s.first == id || s.second == id)
        {
            Some(split) => {
                split.ratio = ratio.clamp(min, max);
                true
            }
            None => false,
        }
    }

    /// Forgets the splits whose tabs parted: closed, no longer side by side,
    /// in different groups or pinned differently.
    fn prune_splits(&mut self) {
        let tabs = &self.tabs;
        let index = |id: TabId| tabs.iter().position(|t| t.id() == id);
        self.splits
            .retain(|s| match (index(s.first), index(s.second)) {
                (Some(a), Some(b)) => {
                    b == a + 1 && tabs[a].group == tabs[b].group && tabs[a].pinned == tabs[b].pinned
                }
                _ => false,
            });
    }

    /// The tab `offset` places away from the active one, wrapping around,
    /// among those in sight (not in a collapsed group).
    pub fn neighbour(&self, offset: isize) -> Option<TabId> {
        let shown: Vec<TabId> = self
            .tabs
            .iter()
            .map(Tab::id)
            .filter(|&id| Some(id) == self.active || !self.is_hidden(id))
            .collect();
        let index = self
            .active
            .and_then(|a| shown.iter().position(|&id| id == a))?;
        let len = shown.len() as isize;
        let target = (index as isize + offset).rem_euclid(len) as usize;
        Some(shown[target])
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

    fn open(s: &mut Session, n: usize) -> Vec<TabId> {
        (0..n).map(|_| s.open_tab()).collect()
    }

    fn group_of(s: &Session, id: TabId) -> Option<GroupId> {
        s.tab(id).unwrap().group()
    }

    #[test]
    fn pinned_tabs_come_first_and_stay_there() {
        let mut s = Session::new();
        let t = open(&mut s, 4);
        assert!(s.set_pinned(t[2], true));
        assert!(s.set_pinned(t[3], true));
        assert_eq!(ids(&s), [t[2], t[3], t[0], t[1]].map(TabId::get));
        assert_eq!(s.pinned_count(), 2);
        assert!(!s.set_pinned(t[3], true));
        // A pinned tab moves among the pinned ones, another after them.
        s.move_tab(t[2], 3);
        assert_eq!(ids(&s), [t[3], t[2], t[0], t[1]].map(TabId::get));
        s.move_tab(t[1], 0);
        assert_eq!(ids(&s), [t[3], t[2], t[1], t[0]].map(TabId::get));
        // Unpinned: first after the pinned tabs.
        assert!(s.set_pinned(t[3], false));
        assert_eq!(ids(&s), [t[2], t[3], t[1], t[0]].map(TabId::get));
    }

    #[test]
    fn a_pinned_tabs_links_open_after_the_pinned_tabs() {
        let mut s = Session::new();
        let t = open(&mut s, 3);
        s.set_pinned(t[2], true);
        let link = s.open_tab_from(t[2]).unwrap();
        assert_eq!(ids(&s), [t[2], link, t[0], t[1]].map(TabId::get));
        assert!(!s.tab(link).unwrap().is_pinned());
    }

    #[test]
    fn grouped_tabs_stay_side_by_side() {
        let mut s = Session::new();
        let t = open(&mut s, 5);
        let g = s.new_group(t[1]).unwrap();
        assert!(s.add_to_group(t[4], g));
        assert_eq!(ids(&s), [t[0], t[1], t[4], t[2], t[3]].map(TabId::get));
        assert_eq!(s.group_tabs(g), vec![t[1], t[4]]);
        // Out of it: right after it.
        assert!(s.remove_from_group(t[1]));
        assert_eq!(ids(&s), [t[0], t[4], t[1], t[2], t[3]].map(TabId::get));
        assert_eq!(group_of(&s, t[1]), None);
        // Its last tab gone, the group is too.
        assert!(s.remove_from_group(t[4]));
        assert!(s.group(g).is_none());
        assert!(s.groups().is_empty());
    }

    #[test]
    fn a_tab_dropped_inside_a_group_joins_it_and_leaves_it_dropped_away() {
        let mut s = Session::new();
        let t = open(&mut s, 5);
        let g = s.new_group(t[1]).unwrap();
        s.add_to_group(t[2], g);
        // Between t1 and t2: in.
        s.move_tab(t[4], 2);
        assert_eq!(group_of(&s, t[4]), Some(g));
        // At its group's edge: still in.
        s.move_tab(t[4], 3);
        assert_eq!(group_of(&s, t[4]), Some(g));
        // Far away: out.
        s.move_tab(t[4], 4);
        assert_eq!(group_of(&s, t[4]), None);
        // A group's only tab takes its group along.
        let h = s.new_group(t[0]).unwrap();
        s.move_tab(t[0], 4);
        assert_eq!(group_of(&s, t[0]), Some(h));
    }

    #[test]
    fn links_join_their_openers_group() {
        let mut s = Session::new();
        let t = open(&mut s, 2);
        let g = s.new_group(t[0]).unwrap();
        let link = s.open_tab_from(t[0]).unwrap();
        assert_eq!(group_of(&s, link), Some(g));
        let other = s.open_tab();
        assert_eq!(group_of(&s, other), None);
    }

    #[test]
    fn pinning_takes_a_tab_out_of_its_group_and_closing_its_tabs_ends_it() {
        let mut s = Session::new();
        let t = open(&mut s, 3);
        let g = s.new_group(t[1]).unwrap();
        s.add_to_group(t[2], g);
        s.set_pinned(t[2], true);
        assert_eq!(group_of(&s, t[2]), None);
        s.close_tab(t[1]);
        assert!(s.groups().is_empty());
    }

    #[test]
    fn collapsing_the_active_tabs_group_shows_a_tab_out_of_it() {
        let mut s = Session::new();
        let t = open(&mut s, 3);
        let g = s.new_group(t[0]).unwrap();
        s.add_to_group(t[1], g);
        s.activate(t[1]);
        assert!(s.set_group_collapsed(g, true));
        assert_eq!(s.active_id(), Some(t[2]));
        assert!(s.is_hidden(t[0]));
        // Ctrl+Tab skips the hidden tabs.
        assert_eq!(s.neighbour(1), Some(t[2]));
        // Shown again, its group opens.
        s.activate(t[0]);
        assert!(!s.group(g).unwrap().collapsed);
        // A group of every tab stays open.
        let mut all = Session::new();
        let a = all.open_tab();
        let ga = all.new_group(a).unwrap();
        assert!(!all.set_group_collapsed(ga, true));
    }

    #[test]
    fn a_new_group_gets_a_colour_no_other_has() {
        let mut s = Session::new();
        let t = open(&mut s, 2);
        let a = s.new_group(t[0]).unwrap();
        let b = s.new_group(t[1]).unwrap();
        assert_ne!(s.group(a).unwrap().color, s.group(b).unwrap().color);
        assert!(s.rename_group(b, "  Work "));
        assert_eq!(s.group(b).unwrap().name, "Work");
        assert!(s.ungroup(a));
        assert!(s.group(a).is_none());
    }

    #[test]
    fn pins_and_groups_are_saved_and_restored() {
        let mut s = Session::new();
        let t = open(&mut s, 5);
        let page = |p: &str| WebAddress::parse(&format!("https://x.com/{p}")).unwrap();
        for (i, &id) in t.iter().enumerate() {
            s.tab_mut(id).unwrap().load_started(page(&i.to_string()));
        }
        s.set_pinned(t[0], true);
        let g = s.new_group(t[2]).unwrap();
        s.add_to_group(t[3], g);
        s.rename_group(g, "Work");
        s.set_group_color(g, GroupColor::Orange);
        s.set_group_collapsed(g, true);
        let snap = s.snapshot();
        assert_eq!(snap.pinned, 1);
        assert_eq!(snap.groups.len(), 1);
        assert_eq!(snap.groups[0].tabs, 2..4);
        assert!(snap.groups[0].collapsed);

        let mut restored = Session::new();
        let ids: Vec<TabId> = snap
            .tabs
            .iter()
            .map(|address| {
                let id = restored.open_tab();
                restored.tab_mut(id).unwrap().load_started(address.clone());
                id
            })
            .collect();
        restored.restore_layout(&ids, &snap);
        assert_eq!(restored.snapshot(), snap);
        assert_eq!(restored.groups()[0].name, "Work");
    }

    fn order(s: &Session) -> Vec<TabId> {
        s.tabs().iter().map(Tab::id).collect()
    }

    #[test]
    fn a_split_puts_its_second_tab_right_after_the_first() {
        let mut s = Session::new();
        let t = open(&mut s, 4);
        assert!(s.split(t[0], t[3]));
        assert_eq!(order(&s), vec![t[0], t[3], t[1], t[2]]);
        let split = s.split_of(t[3]).unwrap();
        assert_eq!((split.first, split.second, split.ratio), (t[0], t[3], 500));
        assert_eq!(s.split_of(t[0]), s.split_of(t[3]));
        assert_eq!(s.split_partner(t[0]), Some(t[3]));
        assert!(s.split_of(t[1]).is_none());
        // A tab is in one split at most; not with itself.
        assert!(!s.split(t[1], t[0]));
        assert!(!s.split(t[1], t[1]));
        assert!(s.unsplit(t[3]));
        assert!(s.split_of(t[0]).is_none());
    }

    #[test]
    fn a_split_ends_when_its_tabs_part() {
        let mut s = Session::new();
        let t = open(&mut s, 4);
        // Closed.
        s.split(t[0], t[1]);
        s.close_tab(t[1]);
        assert!(s.split_of(t[0]).is_none());
        // Moved away.
        s.split(t[2], t[3]);
        s.move_tab(t[3], 0);
        assert!(s.split_of(t[2]).is_none());
        // Pinned.
        s.split(t[0], t[2]);
        s.set_pinned(t[0], true);
        assert!(s.split_of(t[2]).is_none());
        // Grouped alone.
        s.split(t[2], t[3]);
        s.new_group(t[3]);
        assert!(s.split_of(t[2]).is_none());
    }

    #[test]
    fn a_splits_links_open_after_it() {
        let mut s = Session::new();
        let t = open(&mut s, 2);
        s.split(t[0], t[1]);
        let link = s.open_tab_from(t[0]).unwrap();
        assert_eq!(order(&s), vec![t[0], t[1], link]);
        assert!(s.split_of(t[0]).is_some());
    }

    #[test]
    fn a_splits_ratio_stays_in_bounds() {
        let mut s = Session::new();
        let t = open(&mut s, 2);
        s.split(t[0], t[1]);
        s.set_split_ratio(t[1], 2000);
        assert_eq!(s.split_of(t[0]).unwrap().ratio, 900);
        s.set_split_ratio(t[0], 10);
        assert_eq!(s.split_of(t[0]).unwrap().ratio, 100);
    }

    #[test]
    fn splits_are_saved_and_restored() {
        let mut s = Session::new();
        let t = open(&mut s, 4);
        let page = |p: &str| WebAddress::parse(&format!("https://x.com/{p}")).unwrap();
        for (i, &id) in t.iter().enumerate() {
            s.tab_mut(id).unwrap().load_started(page(&i.to_string()));
        }
        s.split(t[2], t[3]);
        s.set_split_ratio(t[2], 300);
        let snap = s.snapshot();
        assert_eq!(
            snap.splits,
            vec![SavedSplit {
                first: 2,
                ratio: 300
            }]
        );

        let mut restored = Session::new();
        let ids: Vec<TabId> = snap
            .tabs
            .iter()
            .map(|address| {
                let id = restored.open_tab();
                restored.tab_mut(id).unwrap().load_started(address.clone());
                id
            })
            .collect();
        restored.restore_layout(&ids, &snap);
        assert_eq!(restored.snapshot(), snap);
    }
}
