use crate::WebAddress;

/// Seconds since the Unix epoch. The domain never reads the clock itself.
pub type Timestamp = u64;

/// A page the user went to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Visit {
    pub address: WebAddress,
    pub title: String,
    pub at: Timestamp,
}

/// Every page visited, oldest first, capped at [`BrowsingHistory::CAPACITY`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowsingHistory {
    visits: Vec<Visit>,
}

impl BrowsingHistory {
    pub const CAPACITY: usize = 10_000;

    pub fn from_visits(mut visits: Vec<Visit>) -> Self {
        visits.sort_by_key(|v| v.at);
        let excess = visits.len().saturating_sub(Self::CAPACITY);
        visits.drain(..excess);
        Self { visits }
    }

    pub fn visits(&self) -> &[Visit] {
        &self.visits
    }

    pub fn len(&self) -> usize {
        self.visits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.visits.is_empty()
    }

    /// Whether pages at `address` belong in the history (not `about:` or
    /// `file:` pages).
    pub fn is_recordable(address: &WebAddress) -> bool {
        matches!(address.scheme(), "http" | "https")
    }

    /// Records a visit. Returns `false` when nothing changed: the address is
    /// not recordable or is the latest visit already (reload, redirect echo).
    pub fn record(&mut self, address: WebAddress, at: Timestamp) -> bool {
        if !Self::is_recordable(&address)
            || self.visits.last().is_some_and(|v| v.address == address)
        {
            return false;
        }
        if self.visits.len() == Self::CAPACITY {
            self.visits.remove(0);
        }
        self.visits.push(Visit {
            address,
            title: String::new(),
            at,
        });
        true
    }

    /// Gives the latest visit of `address` its page title. Returns whether
    /// it changed.
    pub fn retitle(&mut self, address: &WebAddress, title: &str) -> bool {
        let title = title.trim();
        match self.visits.iter_mut().rev().find(|v| &v.address == address) {
            Some(v) if v.title != title => {
                v.title = title.to_owned();
                true
            }
            _ => false,
        }
    }

    /// The latest visit of each address whose title or address contains
    /// every word of `query` (case-insensitive), most recent first.
    pub fn search(&self, query: &str, limit: usize) -> Vec<&Visit> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let mut seen = std::collections::HashSet::new();
        self.visits
            .iter()
            .rev()
            .filter(|v| seen.insert(&v.address))
            .filter(|v| {
                let haystack = format!("{} {}", v.title, v.address).to_lowercase();
                words.iter().all(|w| haystack.contains(w.as_str()))
            })
            .take(limit)
            .collect()
    }

    pub fn clear(&mut self) {
        self.visits.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    #[test]
    fn records_web_pages_only_and_skips_repeats() {
        let mut h = BrowsingHistory::default();
        assert!(h.record(addr("https://a.com/"), 1));
        assert!(!h.record(addr("https://a.com/"), 2));
        assert!(!h.record(addr("about:blank"), 3));
        assert!(h.record(addr("https://b.com/"), 4));
        assert!(h.record(addr("https://a.com/"), 5));
        assert_eq!(h.len(), 3);
    }

    #[test]
    fn retitle_updates_the_latest_visit() {
        let mut h = BrowsingHistory::default();
        h.record(addr("https://a.com/"), 1);
        h.record(addr("https://b.com/"), 2);
        h.record(addr("https://a.com/"), 3);
        assert!(h.retitle(&addr("https://a.com/"), " A "));
        assert!(!h.retitle(&addr("https://a.com/"), "A"));
        assert_eq!(h.visits()[0].title, "");
        assert_eq!(h.visits()[2].title, "A");
    }

    #[test]
    fn search_matches_all_words_once_per_address_newest_first() {
        let mut h = BrowsingHistory::default();
        h.record(addr("https://docs.rs/egui"), 1);
        h.retitle(&addr("https://docs.rs/egui"), "egui docs");
        h.record(addr("https://rust-lang.org/"), 2);
        h.retitle(&addr("https://rust-lang.org/"), "Rust");
        h.record(addr("https://docs.rs/egui"), 3);
        h.retitle(&addr("https://docs.rs/egui"), "egui docs");

        let all = h.search("", 10);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].at, 3);
        let hits = h.search("DOCS egui", 10);
        assert_eq!(hits.len(), 1);
        assert!(h.search("egui python", 10).is_empty());
        assert_eq!(h.search("", 1).len(), 1);
    }

    #[test]
    fn capacity_drops_the_oldest() {
        let visits = (0..BrowsingHistory::CAPACITY as u64 + 5)
            .map(|i| Visit {
                address: addr(&format!("https://a.com/{i}")),
                title: String::new(),
                at: i,
            })
            .collect();
        let mut h = BrowsingHistory::from_visits(visits);
        assert_eq!(h.len(), BrowsingHistory::CAPACITY);
        assert_eq!(h.visits()[0].at, 5);
        h.record(addr("https://new.com/"), 99_999);
        assert_eq!(h.len(), BrowsingHistory::CAPACITY);
        assert_eq!(h.visits()[0].at, 6);
    }
}
