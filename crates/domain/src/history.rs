use crate::WebAddress;

/// The pages a tab went through, mirrored from what the web engine reports.
///
/// The engine keeps its own history (form state, `pushState`...), so going
/// back and forward is left to it. This mirror only learns the outcome: an
/// address equal to the previous (next) entry is taken as a step back
/// (forward), anything else as a new page that drops the forward entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    entries: Vec<WebAddress>,
    cursor: usize,
}

impl History {
    pub fn current(&self) -> Option<&WebAddress> {
        self.entries.get(self.cursor)
    }

    pub fn can_go_back(&self) -> bool {
        !self.entries.is_empty() && self.cursor > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records that the engine now shows `address`.
    pub fn observe(&mut self, address: WebAddress) {
        if self.entries.is_empty() {
            self.entries.push(address);
            self.cursor = 0;
        } else if self.entries[self.cursor] == address {
            // Reload or same-page navigation.
        } else if self.can_go_back() && self.entries[self.cursor - 1] == address {
            self.cursor -= 1;
        } else if self.can_go_forward() && self.entries[self.cursor + 1] == address {
            self.cursor += 1;
        } else {
            self.entries.truncate(self.cursor + 1);
            self.entries.push(address);
            self.cursor = self.entries.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(path: &str) -> WebAddress {
        WebAddress::parse(&format!("https://example.com/{path}")).unwrap()
    }

    fn history(paths: &[&str]) -> History {
        let mut h = History::default();
        for p in paths {
            h.observe(addr(p));
        }
        h
    }

    #[test]
    fn empty_history_goes_nowhere() {
        let h = History::default();
        assert!(h.current().is_none());
        assert!(!h.can_go_back());
        assert!(!h.can_go_forward());
    }

    #[test]
    fn new_pages_are_appended() {
        let h = history(&["a", "b", "c"]);
        assert_eq!(h.len(), 3);
        assert_eq!(h.current(), Some(&addr("c")));
        assert!(h.can_go_back());
        assert!(!h.can_go_forward());
    }

    #[test]
    fn reload_is_not_a_new_entry() {
        let h = history(&["a", "a"]);
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn back_and_forward_move_the_cursor() {
        let mut h = history(&["a", "b", "c"]);
        h.observe(addr("b"));
        assert_eq!(h.current(), Some(&addr("b")));
        assert!(h.can_go_forward());
        h.observe(addr("c"));
        assert_eq!(h.current(), Some(&addr("c")));
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn new_page_after_back_drops_forward_entries() {
        let mut h = history(&["a", "b", "c"]);
        h.observe(addr("b"));
        h.observe(addr("d"));
        assert_eq!(h.len(), 3);
        assert_eq!(h.current(), Some(&addr("d")));
        assert!(!h.can_go_forward());
    }
}
