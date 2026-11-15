use crate::{SearchEngine, Timestamp, WebAddress};

/// User preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub home_page: WebAddress,
    pub search_engine: SearchEngine,
    /// When they were last changed (0: never). The most recent wins when
    /// devices are merged.
    pub updated_at: Timestamp,
}

impl Settings {
    /// Takes `other` when it is more recent. Returns whether it did.
    pub fn merge(&mut self, other: &Settings) -> bool {
        if other.updated_at > self.updated_at {
            *self = other.clone();
            true
        } else {
            false
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            home_page: WebAddress::parse("https://duckduckgo.com/").expect("valid default"),
            search_engine: SearchEngine::default(),
            updated_at: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_recent_settings_win() {
        let mut mine = Settings::default();
        let mut theirs = Settings {
            home_page: WebAddress::parse("https://example.com/").unwrap(),
            updated_at: 5,
            ..Settings::default()
        };
        assert!(mine.merge(&theirs));
        assert_eq!(mine.home_page.as_str(), "https://example.com/");
        theirs.updated_at = 1;
        assert!(!mine.merge(&theirs));
    }
}
