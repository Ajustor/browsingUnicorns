//! Finding an open tab by a few letters of its title or address, the way
//! an editor's "go to file" does.

use crate::{Tab, TabId};

/// A tab found by [`search_tabs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabMatch {
    pub id: TabId,
    /// Its address as shown (without `https://`), empty when it has none.
    pub address: String,
    /// The characters (indexes of chars) of its title that matched.
    pub title_hits: Vec<usize>,
    /// The characters of `address` that matched.
    pub address_hits: Vec<usize>,
    score: i64,
}

/// The tabs whose title or address holds each word of `query` (its letters
/// in order, not necessarily side by side), the best first: whole words,
/// starts of words and titles count more. An empty query gives every tab,
/// in order.
pub fn search_tabs(tabs: &[Tab], query: &str) -> Vec<TabMatch> {
    let words: Vec<Vec<char>> = query
        .split_whitespace()
        .map(|w| w.chars().flat_map(char::to_lowercase).collect())
        .collect();
    let mut found: Vec<(usize, TabMatch)> = tabs
        .iter()
        .enumerate()
        .filter_map(|(order, tab)| {
            let title: Vec<char> = tab.display_title().chars().collect();
            let address = shown_address(tab);
            let address_chars: Vec<char> = address.chars().collect();
            let mut found = TabMatch {
                id: tab.id(),
                address,
                title_hits: Vec::new(),
                address_hits: Vec::new(),
                score: 0,
            };
            for word in &words {
                let in_title = fuzzy(&title, word).map(|(s, hits)| (s * 2, hits));
                let in_address = fuzzy(&address_chars, word);
                match (in_title, in_address) {
                    (Some((t, hits)), Some((a, _))) if t >= a => {
                        found.score += t;
                        found.title_hits.extend(hits);
                    }
                    (Some((t, hits)), None) => {
                        found.score += t;
                        found.title_hits.extend(hits);
                    }
                    (_, Some((a, hits))) => {
                        found.score += a;
                        found.address_hits.extend(hits);
                    }
                    (None, None) => return None,
                }
            }
            found.title_hits.sort_unstable();
            found.title_hits.dedup();
            found.address_hits.sort_unstable();
            found.address_hits.dedup();
            Some((order, found))
        })
        .collect();
    found.sort_by(|(a_order, a), (b_order, b)| b.score.cmp(&a.score).then(a_order.cmp(b_order)));
    found.into_iter().map(|(_, m)| m).collect()
}

/// A tab's address without the scheme of the web (`https://`, `http://`).
fn shown_address(tab: &Tab) -> String {
    let Some(address) = tab.address() else {
        return String::new();
    };
    let full = address.to_string();
    ["https://", "http://"]
        .iter()
        .find_map(|scheme| full.strip_prefix(scheme))
        .map(str::to_owned)
        .unwrap_or(full)
}

/// Whether `needle` (lower case) is in `hay`, its letters in order: a score
/// and the indexes of the letters matched. A run of the letters side by
/// side scores most, then letters starting words.
fn fuzzy(hay: &[char], needle: &[char]) -> Option<(i64, Vec<usize>)> {
    if needle.is_empty() {
        return Some((0, Vec::new()));
    }
    let lower: Vec<char> = hay
        .iter()
        .map(|c| c.to_lowercase().next().unwrap_or(*c))
        .collect();
    let word_start = |i: usize| i == 0 || !hay[i - 1].is_alphanumeric();
    // Side by side: the earliest run, preferably at a word's start.
    let runs: Vec<usize> = (0..lower.len().saturating_sub(needle.len() - 1))
        .filter(|&i| lower[i..i + needle.len()] == *needle)
        .collect();
    if let Some(&at) = runs
        .iter()
        .find(|&&i| word_start(i))
        .or_else(|| runs.first())
    {
        let mut score = 100 + 10 * needle.len() as i64;
        if word_start(at) {
            score += 40;
        }
        if at == 0 {
            score += 20;
        }
        return Some((score, (at..at + needle.len()).collect()));
    }
    // Spread out: each letter as early as possible.
    let mut hits = Vec::with_capacity(needle.len());
    let mut from = 0;
    let mut score = 0;
    for &letter in needle {
        let i = from + lower[from..].iter().position(|&c| c == letter)?;
        score += 1;
        if word_start(i) {
            score += 8;
        }
        if hits.last() == Some(&(i.wrapping_sub(1))) {
            score += 5;
        }
        hits.push(i);
        from = i + 1;
    }
    Some((score, hits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WebAddress;

    fn tab(id: u64, title: &str, url: &str) -> Tab {
        let mut tab = Tab::new(TabId::new(id));
        tab.load_finished(WebAddress::parse(url).unwrap());
        tab.set_title(title);
        tab
    }

    fn ids(found: &[TabMatch]) -> Vec<u64> {
        found.iter().map(|m| m.id.get()).collect()
    }

    fn tabs() -> Vec<Tab> {
        vec![
            tab(1, "Inbox - Mail", "https://mail.example.com/inbox"),
            tab(2, "Rust documentation", "https://doc.rust-lang.org/std/"),
            tab(3, "Pull requests", "https://github.com/pulls"),
        ]
    }

    #[test]
    fn an_empty_query_lists_every_tab_in_order() {
        assert_eq!(ids(&search_tabs(&tabs(), "  ")), vec![1, 2, 3]);
    }

    #[test]
    fn letters_in_order_match_the_title_or_the_address() {
        let found = search_tabs(&tabs(), "rdoc");
        assert_eq!(ids(&found), vec![2]);
        assert_eq!(found[0].title_hits, vec![0, 5, 6, 7]);
        let found = search_tabs(&tabs(), "github");
        assert_eq!(ids(&found), vec![3]);
        assert_eq!(found[0].address, "github.com/pulls");
        assert_eq!(found[0].address_hits, (0..6).collect::<Vec<_>>());
        assert!(search_tabs(&tabs(), "zzz").is_empty());
    }

    #[test]
    fn every_word_must_match_and_case_does_not_count() {
        assert_eq!(ids(&search_tabs(&tabs(), "PULL git")), vec![3]);
        assert!(search_tabs(&tabs(), "pull lang").is_empty());
    }

    #[test]
    fn whole_words_rank_before_scattered_letters() {
        // "doc" whole in tab 2, scattered in none other; "in" starts tab 1's
        // title, and is inside tab 2's address ("rust-lang" has no "in").
        let found = search_tabs(&tabs(), "in");
        assert_eq!(found.first().map(|m| m.id.get()), Some(1));
        let found = search_tabs(&tabs(), "ul");
        // "Pull" holds "ul" side by side; "Rust documentation" only spread.
        assert_eq!(found.first().map(|m| m.id.get()), Some(3));
    }
}
