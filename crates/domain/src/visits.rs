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

/// Every page visited, oldest first, capped at [`BrowsingHistory::CAPACITY`]
/// (recording goes up to a tenth over it, then drops the oldest at once).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrowsingHistory {
    visits: Vec<Visit>,
}

impl BrowsingHistory {
    pub const CAPACITY: usize = 10_000;
    /// How far [`Self::record`] goes over [`Self::CAPACITY`] before trimming:
    /// the oldest visits go by the batch, not one shift of the whole
    /// history per page load.
    const SLACK: usize = Self::CAPACITY / 10;

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
        if self.visits.len() >= Self::CAPACITY + Self::SLACK {
            let excess = self.visits.len() + 1 - Self::CAPACITY;
            self.visits.drain(..excess);
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

    /// The latest `count` visits, oldest first.
    pub fn recent(&self, count: usize) -> &[Visit] {
        &self.visits[self.visits.len().saturating_sub(count)..]
    }

    /// Takes in another device's visits. A visit already known (same address
    /// at the same time) is not added again; a title fills in an empty one.
    /// Returns whether anything changed: visits too old to fit in a full
    /// history are not a change.
    pub fn merge(&mut self, others: &[Visit]) -> bool {
        let before = self.visits.len();
        let mut known: std::collections::HashMap<(WebAddress, Timestamp), usize> = self
            .visits
            .iter()
            .enumerate()
            .map(|(i, v)| ((v.address.clone(), v.at), i))
            .collect();
        // Titles filled in; new visits are a change if they are kept.
        let mut changed = false;
        for v in others {
            if !Self::is_recordable(&v.address) {
                continue;
            }
            match known.get(&(v.address.clone(), v.at)) {
                Some(&i) => {
                    if self.visits[i].title.is_empty() && !v.title.is_empty() {
                        self.visits[i].title = v.title.clone();
                        changed = true;
                    }
                }
                None => {
                    known.insert((v.address.clone(), v.at), self.visits.len());
                    self.visits.push(v.clone());
                }
            }
        }
        if self.visits.len() > before {
            // Which visits `from_visits` keeps (a stable sort: new visits
            // come after the known ones of the same time).
            let mut order: Vec<usize> = (0..self.visits.len()).collect();
            order.sort_by_key(|&i| self.visits[i].at);
            let excess = order.len().saturating_sub(Self::CAPACITY);
            if order[excess..].iter().any(|&i| i >= before) {
                *self = Self::from_visits(std::mem::take(&mut self.visits));
                changed = true;
            } else {
                // All older than what a full history keeps.
                self.visits.truncate(before);
            }
        }
        changed
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
    fn merge_adds_unknown_visits_in_time_order_once() {
        let mut desktop = BrowsingHistory::default();
        desktop.record(addr("https://a.com/"), 10);
        desktop.record(addr("https://c.com/"), 30);
        let phone = vec![
            Visit {
                address: addr("https://b.com/"),
                title: "B".into(),
                at: 20,
            },
            Visit {
                address: addr("https://a.com/"),
                title: "A".into(),
                at: 10,
            },
            Visit {
                address: addr("about:blank"),
                title: String::new(),
                at: 40,
            },
        ];
        assert!(desktop.merge(&phone));
        let order: Vec<u64> = desktop.visits().iter().map(|v| v.at).collect();
        assert_eq!(order, vec![10, 20, 30]);
        assert_eq!(desktop.visits()[0].title, "A", "title filled in");
        assert!(!desktop.merge(&phone), "nothing new the second time");
        assert_eq!(desktop.recent(2).len(), 2);
        assert_eq!(desktop.recent(2)[1].at, 30);
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
        assert_eq!(h.len(), BrowsingHistory::CAPACITY + 1, "trimmed by batch");
        for i in 0..BrowsingHistory::SLACK as u64 {
            h.record(addr(&format!("https://b.com/{i}")), 100_000 + i);
        }
        assert_eq!(h.len(), BrowsingHistory::CAPACITY);
        assert_eq!(h.visits().last().unwrap().at, 100_000 + 999);
    }

    #[test]
    fn a_full_history_ignores_visits_older_than_its_own() {
        let visits = (100..BrowsingHistory::CAPACITY as u64 + 100)
            .map(|i| Visit {
                address: addr(&format!("https://a.com/{i}")),
                title: String::new(),
                at: i,
            })
            .collect();
        let mut h = BrowsingHistory::from_visits(visits);
        let old = vec![Visit {
            address: addr("https://old.com/"),
            title: "Old".into(),
            at: 5,
        }];
        assert!(!h.merge(&old), "dropped at once: nothing to save");
        assert_eq!(h.visits()[0].at, 100);
        let new = vec![Visit {
            address: addr("https://new.com/"),
            title: "New".into(),
            at: 1_000_000,
        }];
        assert!(h.merge(&new));
        assert_eq!(h.len(), BrowsingHistory::CAPACITY);
        assert_eq!(h.visits()[0].at, 101);
    }
}
