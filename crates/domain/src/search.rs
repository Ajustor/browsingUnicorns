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

    #[test]
    fn invalid_template_gives_none() {
        assert!(
            SearchEngine::new("x", "nope {query}")
                .results_for("a")
                .is_none()
        );
    }
}
