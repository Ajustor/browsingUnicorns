use percent_encoding::percent_decode_str;
use url::Url;
use url::form_urlencoded::byte_serialize;

use crate::WebAddress;

/// A search engine, described by a URL template where `{query}` is replaced
/// by the encoded search terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchEngine {
    pub name: String,
    pub template: String,
}

impl SearchEngine {
    pub const PLACEHOLDER: &'static str = "{query}";

    pub fn new(name: impl Into<String>, template: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            template: template.into(),
        }
    }

    pub fn duckduckgo() -> Self {
        Self::new("DuckDuckGo", "https://duckduckgo.com/?q={query}")
    }

    /// The address of the results page for `terms`, or `None` when the
    /// template does not produce a valid address.
    pub fn results_for(&self, terms: &str) -> Option<WebAddress> {
        let encoded: String = byte_serialize(terms.trim().as_bytes()).collect();
        WebAddress::parse(&self.template.replace(Self::PLACEHOLDER, &encoded)).ok()
    }

    /// The common engines, to recognize the searches made with them.
    pub fn well_known() -> Vec<Self> {
        vec![
            Self::duckduckgo(),
            Self::new("Google", "https://www.google.com/search?q={query}"),
            Self::new("Bing", "https://www.bing.com/search?q={query}"),
            Self::new("Brave", "https://search.brave.com/search?q={query}"),
            Self::new("Ecosia", "https://www.ecosia.org/search?q={query}"),
            Self::new("Qwant", "https://www.qwant.com/?q={query}"),
            Self::new("Startpage", "https://www.startpage.com/do/search?q={query}"),
        ]
    }

    /// The terms searched when `address` is one of this engine's results
    /// pages (other parameters the engine added are ignored).
    pub fn terms_in(&self, address: &WebAddress) -> Option<String> {
        const MARK: &str = "browsingunicornsquery";
        let sample = Url::parse(&self.template.replace(Self::PLACEHOLDER, MARK)).ok()?;
        let page = Url::parse(address.as_str()).ok()?;
        let same_site = |a: &Url, b: &Url| {
            let host = |u: &Url| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_owned())
            };
            host(a) == host(b)
        };
        if !same_site(&sample, &page) {
            return None;
        }
        let terms = if let Some((name, _)) = sample.query_pairs().find(|(_, v)| v == MARK) {
            if sample.path() != page.path() {
                return None;
            }
            page.query_pairs().find(|(k, _)| *k == name)?.1.into_owned()
        } else {
            // In the path (`/search/{query}`).
            let (before, after) = sample.path().split_once(MARK)?;
            let middle = page.path().strip_prefix(before)?.strip_suffix(after)?;
            percent_decode_str(middle).decode_utf8().ok()?.into_owned()
        };
        let terms = terms.split_whitespace().collect::<Vec<_>>().join(" ");
        (!terms.is_empty()).then_some(terms)
    }
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::duckduckgo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_terms() {
        let page = SearchEngine::duckduckgo()
            .results_for(" rust & egui ")
            .unwrap();
        assert_eq!(page.as_str(), "https://duckduckgo.com/?q=rust+%26+egui");
    }

    fn page(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    #[test]
    fn finds_the_terms_of_a_results_page() {
        let ddg = SearchEngine::duckduckgo();
        assert_eq!(
            ddg.terms_in(&page("https://duckduckgo.com/?q=rust+%26+egui&ia=web"))
                .as_deref(),
            Some("rust & egui")
        );
        assert_eq!(
            ddg.terms_in(&page("https://www.duckduckgo.com/?ia=web&q=%20a%20%20b"))
                .as_deref(),
            Some("a b")
        );
        assert_eq!(ddg.terms_in(&page("https://duckduckgo.com/?q=")), None);
        assert_eq!(ddg.terms_in(&page("https://duckduckgo.com/about")), None);
        assert_eq!(ddg.terms_in(&page("https://example.com/?q=rust")), None);
        let google = SearchEngine::new("G", "https://www.google.com/search?q={query}");
        assert_eq!(google.terms_in(&page("https://www.google.com/?q=x")), None);
        let path = SearchEngine::new("P", "https://s.example/find/{query}");
        assert_eq!(
            path.terms_in(&page("https://s.example/find/rust%20book"))
                .as_deref(),
            Some("rust book")
        );
    }

    #[test]
    fn invalid_template_gives_none() {
        assert!(
            SearchEngine::new("x", "nope {query}")
                .results_for("a")
                .is_none()
        );
    }
}
