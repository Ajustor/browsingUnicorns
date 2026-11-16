use std::collections::HashMap;

use crate::{BrowsingHistory, SearchEngine, Timestamp, WebAddress};

/// What the address bar proposes while the user types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Suggestion {
    /// A search the user already made; `address` is its results page on
    /// the current search engine.
    Search { terms: String, address: WebAddress },
    /// A page of the history.
    Page { title: String, address: WebAddress },
}

impl Suggestion {
    pub fn address(&self) -> &WebAddress {
        match self {
            Self::Search { address, .. } | Self::Page { address, .. } => address,
        }
    }
}

/// At most this many past searches, the rest being pages.
const MAX_SEARCHES: usize = 4;

/// Up to `limit` suggestions for `input`: the past searches it matches,
/// then the history's pages. Searches are the ones made with the current
/// engine or a well-known one, found in the history by their results pages.
/// Both are ranked by what starts with `input`, then by how often they were
/// made or visited, then by how recently.
pub fn suggest(
    input: &str,
    history: &BrowsingHistory,
    engine: &SearchEngine,
    limit: usize,
) -> Vec<Suggestion> {
    let query = input.trim().to_lowercase();
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }
    let words: Vec<&str> = query.split_whitespace().collect();
    let matches = |text: &str| words.iter().all(|w| text.contains(w));
    let mut engines = SearchEngine::well_known();
    engines.retain(|e| e.template != engine.template);
    engines.insert(0, engine.clone());

    let mut searches: HashMap<String, Tally<String>> = HashMap::new();
    let mut pages: HashMap<&WebAddress, Tally<&str>> = HashMap::new();
    for visit in history.visits() {
        if let Some(terms) = engines.iter().find_map(|e| e.terms_in(&visit.address)) {
            let key = terms.to_lowercase();
            if key != query && matches(&key) {
                let starts = key.starts_with(&query);
                searches
                    .entry(key)
                    .or_insert_with(|| Tally::new(starts))
                    .add(visit.at, terms);
            }
            continue;
        }
        let tally = pages.entry(&visit.address).or_insert_with(|| {
            let address = visit.address.as_str().to_lowercase();
            let host = visit.address.host().unwrap_or_default();
            let starts = host.trim_start_matches("www.").starts_with(&query)
                || address
                    .split("://")
                    .nth(1)
                    .unwrap_or_default()
                    .starts_with(&query);
            Tally::new(starts)
        });
        if !visit.title.trim().is_empty() {
            tally.add(visit.at, visit.title.trim());
        } else {
            let title = tally.value;
            tally.add(visit.at, title);
        }
    }
    // A page matches when the words are in its last title or its address.
    let pages = pages.into_iter().filter(|(address, t)| {
        matches(&format!("{} {}", t.value, address.as_str()).to_lowercase())
    });

    let mut searches: Vec<Tally<String>> = searches.into_values().collect();
    searches.sort_by(Tally::rank);
    let mut pages: Vec<(&WebAddress, Tally<&str>)> = pages.collect();
    pages.sort_by(|a, b| Tally::rank(&a.1, &b.1));

    let searches = searches
        .into_iter()
        .filter_map(|t| {
            Some(Suggestion::Search {
                address: engine.results_for(&t.value)?,
                terms: t.value,
            })
        })
        .take(MAX_SEARCHES.min(limit));
    let mut out: Vec<Suggestion> = searches.collect();
    out.extend(
        pages
            .into_iter()
            .take(limit - out.len())
            .map(|(address, t)| Suggestion::Page {
                title: t.value.to_owned(),
                address: address.clone(),
            }),
    );
    out
}

/// How often and how recently a search or page came up; `value` is its
/// latest form (terms as typed, page title).
struct Tally<T> {
    starts_with_input: bool,
    count: usize,
    last: Timestamp,
    value: T,
}

impl<T: Default> Tally<T> {
    fn new(starts_with_input: bool) -> Self {
        Self {
            starts_with_input,
            count: 0,
            last: 0,
            value: T::default(),
        }
    }

    /// The visits come oldest first: the last value is the latest.
    fn add(&mut self, at: Timestamp, value: T) {
        self.count += 1;
        self.last = self.last.max(at);
        self.value = value;
    }

    fn rank(a: &Self, b: &Self) -> std::cmp::Ordering {
        b.starts_with_input
            .cmp(&a.starts_with_input)
            .then(b.count.cmp(&a.count))
            .then(b.last.cmp(&a.last))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Visit;

    fn visit(url: &str, title: &str, at: Timestamp) -> Visit {
        Visit {
            address: WebAddress::parse(url).unwrap(),
            title: title.into(),
            at,
        }
    }

    fn history(visits: Vec<Visit>) -> BrowsingHistory {
        BrowsingHistory::from_visits(visits)
    }

    fn texts(s: &[Suggestion]) -> Vec<String> {
        s.iter()
            .map(|s| match s {
                Suggestion::Search { terms, .. } => format!("search:{terms}"),
                Suggestion::Page { address, .. } => format!("page:{address}"),
            })
            .collect()
    }

    fn ddg() -> SearchEngine {
        SearchEngine::duckduckgo()
    }

    #[test]
    fn past_searches_come_first_and_lead_to_the_current_engine() {
        let h = history(vec![
            visit(
                "https://duckduckgo.com/?q=rust+egui&ia=web",
                "rust egui at DuckDuckGo",
                1,
            ),
            visit(
                "https://www.google.com/search?q=rust+tabs",
                "rust tabs - Google",
                2,
            ),
            visit("https://www.rust-lang.org/", "Rust Programming Language", 3),
        ]);
        let s = suggest("rust", &h, &ddg(), 8);
        assert_eq!(
            texts(&s),
            vec![
                "search:rust tabs",
                "search:rust egui",
                "page:https://www.rust-lang.org/"
            ]
        );
        assert_eq!(
            s[0].address().as_str(),
            "https://duckduckgo.com/?q=rust+tabs"
        );
    }

    #[test]
    fn frequent_and_prefix_matches_rank_higher() {
        let h = history(vec![
            visit("https://duckduckgo.com/?q=learn+rust", "", 1),
            visit("https://duckduckgo.com/?q=rust+book", "", 2),
            visit("https://duckduckgo.com/?q=learn+rust", "", 3),
            visit("https://duckduckgo.com/?q=learn+rust", "", 4),
            visit("https://blog.example/rust", "Rust blog", 5),
            visit("https://rustup.rs/", "rustup", 6),
            visit("https://rustup.rs/", "rustup", 7),
            visit("https://blog.example/rust", "Rust blog", 8),
            visit("https://blog.example/rust", "Rust blog", 9),
        ]);
        let s = suggest("rust", &h, &ddg(), 8);
        assert_eq!(
            texts(&s),
            vec![
                "search:rust book",
                "search:learn rust",
                "page:https://rustup.rs/",
                "page:https://blog.example/rust",
            ]
        );
    }

    #[test]
    fn every_word_must_match_and_the_input_itself_is_left_out() {
        let h = history(vec![
            visit("https://duckduckgo.com/?q=rust+egui", "", 1),
            visit("https://duckduckgo.com/?q=rust", "", 2),
            visit("https://example.com/", "Egui demo", 3),
        ]);
        assert_eq!(
            texts(&suggest("Egui rust", &h, &ddg(), 8)),
            vec!["search:rust egui"]
        );
        assert_eq!(
            texts(&suggest("RUST", &h, &ddg(), 8)),
            vec!["search:rust egui"]
        );
        assert!(suggest("  ", &h, &ddg(), 8).is_empty());
    }

    #[test]
    fn the_limit_is_kept() {
        let visits = (0..20)
            .map(|i| visit(&format!("https://duckduckgo.com/?q=rust+{i}"), "", i))
            .chain((0..20).map(|i| visit(&format!("https://rust.example/{i}"), "", 100 + i)))
            .collect();
        let s = suggest("rust", &history(visits), &ddg(), 6);
        assert_eq!(s.len(), 6);
        let searches = s
            .iter()
            .filter(|s| matches!(s, Suggestion::Search { .. }))
            .count();
        assert_eq!(searches, MAX_SEARCHES);
    }
}
