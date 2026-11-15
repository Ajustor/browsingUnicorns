use crate::{SearchEngine, WebAddress};

/// User preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub home_page: WebAddress,
    pub search_engine: SearchEngine,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            home_page: WebAddress::parse("https://duckduckgo.com/").expect("valid default"),
            search_engine: SearchEngine::default(),
        }
    }
}
