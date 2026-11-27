//! Tabs going to sleep, waking, crashing and reloading.

use domain::{TabId, Timestamp, WebAddress};

use super::Browser;
use crate::BrowserError;
use crate::ports::{Viewport, WebEngine};

impl<E: WebEngine> Browser<E> {
    /// See [`WebEngine::show_panes`]: the active tab has the keyboard.
    pub fn show_panes(&mut self, panes: &[(TabId, Viewport)]) {
        for &(id, _) in panes {
            if let Err(e) = self.wake(id) {
                log::warn!("waking tab {id}: {e}");
            }
        }
        if let Some(active) = self.session.active_id() {
            self.engine.show_panes(panes, active);
        }
    }

    /// Loads `address` in `tab` (an extension's `tabs.update`), waking it
    /// first if it sleeps.
    pub fn load_in_tab(&mut self, tab: TabId, address: &WebAddress) -> Result<(), BrowserError> {
        if self.session.tab(tab).is_some_and(|t| t.is_discarded()) {
            if let Some(t) = self.session.tab_mut(tab) {
                t.restore(address.clone(), "");
            }
            return self.wake(tab);
        }
        Ok(self.engine.navigate(tab, address)?)
    }

    /// Whether `tab`'s page crashed (it has none until reloaded).
    pub fn is_crashed(&self, tab: TabId) -> bool {
        self.crashed.contains(&tab)
    }

    /// Reloads `tab` (an extension's `tabs.reload`); asleep or crashed, it
    /// wakes.
    pub fn reload_tab(&mut self, tab: TabId) {
        self.crashed.remove(&tab);
        if self.session.tab(tab).is_some_and(|t| t.is_discarded()) {
            if let Err(e) = self.wake(tab) {
                log::warn!("waking tab {tab}: {e}");
            }
        } else {
            self.engine.reload(tab);
        }
    }

    /// Shows `tab`, waking it first if it sleeps.
    pub(super) fn show(&mut self, tab: Option<TabId>) {
        if let Some(id) = tab {
            // Seen now: its idle time counts again from the next look.
            self.last_seen.remove(&id);
            // Shown again by the user: a crashed page gets another go.
            self.crashed.remove(&id);
            if let Err(e) = self.wake(id) {
                log::warn!("waking tab {id}: {e}");
            }
        }
        self.engine.show(tab);
    }

    /// Loads the page of `id` again if it sleeps (a new view).
    pub(super) fn wake(&mut self, id: TabId) -> Result<(), BrowserError> {
        let Some(tab) = self
            .session
            .tab(id)
            .filter(|t| t.is_discarded() && !self.crashed.contains(&id))
        else {
            return Ok(());
        };
        let private = tab.is_private();
        let address = tab
            .address()
            .cloned()
            .unwrap_or_else(|| self.settings.home_page.clone());
        if private {
            self.engine.open_private_view(id, &address)?;
        } else {
            self.engine.open_view(id, &address)?;
        }
        if let Some(tab) = self.session.tab_mut(id) {
            tab.wake();
        }
        Ok(())
    }

    /// Puts to sleep the tabs not seen for `idle` seconds at `now`: their
    /// views close, their address and title stay, they load again once
    /// shown. The tabs shown (the active one, its split partner), those
    /// loading, without a page, of an extension, and those in `keep` (playing
    /// sound) stay awake. Returns the tabs put to sleep.
    pub fn discard_idle(&mut self, now: Timestamp, idle: u64, keep: &[TabId]) -> Vec<TabId> {
        let active = self.session.active_id();
        let partner = active.and_then(|a| self.session.split_partner(a));
        let open: Vec<TabId> = self.session.tabs().iter().map(domain::Tab::id).collect();
        self.last_seen.retain(|id, _| open.contains(id));
        let mut asleep = Vec::new();
        for tab in self.session.tabs() {
            let id = tab.id();
            let seen = self.last_seen.entry(id).or_insert(now);
            if Some(id) == active || Some(id) == partner {
                *seen = now;
                continue;
            }
            // An extension's own page (its window: a passkey prompt...).
            let extension_page = tab.address().is_some_and(|a| {
                a.as_str().starts_with("bu-ext:")
                    || a.host()
                        .is_some_and(|h| h.starts_with("bu-ext.") || h.ends_with(".bu-ext.invalid"))
            });
            if tab.is_discarded()
                || tab.is_loading()
                || tab.address().is_none()
                || extension_page
                || keep.contains(&id)
                || now.saturating_sub(*seen) < idle
            {
                continue;
            }
            asleep.push(id);
        }
        for &id in &asleep {
            self.engine.close_view(id);
            if let Some(tab) = self.session.tab_mut(id) {
                tab.discard();
            }
        }
        asleep
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    #[test]
    fn tabs_left_unseen_a_while_go_to_sleep() {
        let mut b = browser();
        let t1 = b.open_tab(None).unwrap();
        let t2 = b.open_tab(None).unwrap();
        let t3 = b.open_tab(None).unwrap();
        let t4 = b.open_tab(None).unwrap();
        for (t, url) in [
            (t1, "https://a.com/"),
            (t2, "https://b.com/"),
            (t3, "https://c.com/"),
            (t4, "https://d.com/"),
        ] {
            finish(&mut b, t, url);
        }
        b.activate(t1);
        b.split_with_active(t2);
        b.activate(t1);
        take_calls(&mut b);
        // The first look starts counting.
        assert!(b.discard_idle(1_000, 60, &[]).is_empty());
        // Not long enough.
        assert!(b.discard_idle(1_030, 60, &[]).is_empty());
        // The active tab and its split partner stay; t4 plays sound.
        assert_eq!(b.discard_idle(1_100, 60, &[t4]), vec![t3]);
        assert_eq!(take_calls(&mut b), vec![Call::Close(t3.get())]);
        assert!(b.session().tab(t3).unwrap().is_discarded());
        // Shown again: it wakes, and counts from then on.
        b.activate(t3);
        assert!(take_calls(&mut b).contains(&Call::Open(t3.get(), "https://c.com/".into())));
        b.activate(t1);
        assert!(b.discard_idle(1_120, 60, &[t4]).is_empty());
    }

    #[test]
    fn a_crashed_page_waits_to_be_reloaded() {
        let mut b = browser();
        let t1 = b.open_tab(None).unwrap();
        finish(&mut b, t1, "https://a.com/");
        take_calls(&mut b);
        b.handle(EngineEvent::PageCrashed {
            tab: t1,
            all: false,
        })
        .unwrap();
        assert!(b.is_crashed(t1));
        assert_eq!(take_calls(&mut b), vec![Call::Close(t1.get())]);
        // Not loaded again behind the user's back (it could crash again).
        let viewport = Viewport::default();
        b.show_panes(&[(t1, viewport)]);
        assert!(
            !take_calls(&mut b)
                .iter()
                .any(|c| matches!(c, Call::Open(..)))
        );
        // Reloaded: a new view.
        b.reload();
        assert!(!b.is_crashed(t1));
        assert!(take_calls(&mut b).contains(&Call::Open(t1.get(), "https://a.com/".into())));
    }

    #[test]
    fn the_engine_crashing_restarts_the_page_shown_only() {
        let mut b = browser();
        let t1 = b.open_tab(None).unwrap();
        let t2 = b.open_tab(None).unwrap();
        finish(&mut b, t1, "https://a.com/");
        finish(&mut b, t2, "https://b.com/");
        take_calls(&mut b);
        b.handle(EngineEvent::PageCrashed { tab: t1, all: true })
            .unwrap();
        let calls = take_calls(&mut b);
        assert!(calls.contains(&Call::Close(t1.get())));
        assert!(calls.contains(&Call::Close(t2.get())));
        assert!(calls.contains(&Call::Open(t2.get(), "https://b.com/".into())));
        assert!(!calls.iter().any(|c| matches!(c, Call::Open(1, _))));
        assert!(b.session().tab(t1).unwrap().is_discarded());
    }
}
