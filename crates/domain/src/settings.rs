use crate::{SearchEngine, Timestamp, WebAddress};

/// User preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub home_page: WebAddress,
    pub search_engine: SearchEngine,
    /// When they were last changed (0: never). The most recent wins when
    /// devices are merged.
    pub updated_at: Timestamp,
    /// The built-in ad blocker is on.
    pub blocker: bool,
    /// Hosts where the ad blocker is off (each covers its subdomains).
    pub blocker_allowlist: Vec<String>,
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

    /// Whether the ad blocker applies to pages of `host`: it is on, and
    /// neither `host` nor one of its parents is allowlisted. Case and a
    /// leading `www.` are ignored.
    pub fn blocks_on(&self, host: &str) -> bool {
        self.blocker && !allowlisted(&self.blocker_allowlist, host)
    }

    /// Switches the ad blocker on (or off) for `host` and its subdomains:
    /// on removes every allowlist entry covering it, off adds the host.
    pub fn set_blocks_on(&mut self, host: &str, on: bool) {
        self.blocker_allowlist.retain(|a| !host_covers(a, host));
        if !on {
            let host = normalize_host(host);
            if !host.is_empty() {
                self.blocker_allowlist.push(host);
            }
        }
    }
}

/// `host` is one of `allowlist`'s hosts or one of their subdomains. Case
/// and a leading `www.` are ignored.
pub fn allowlisted(allowlist: &[String], host: &str) -> bool {
    allowlist.iter().any(|a| host_covers(a, host))
}

/// `host` is `allowed` or one of its subdomains.
fn host_covers(allowed: &str, host: &str) -> bool {
    let allowed = normalize_host(allowed);
    let host = normalize_host(host);
    !allowed.is_empty()
        && (host == allowed
            || host
                .strip_suffix(allowed.as_str())
                .is_some_and(|rest| rest.ends_with('.')))
}

/// Lowercase, without a leading `www.` nor a trailing dot.
fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match host.strip_prefix("www.") {
        Some(rest) => rest.to_string(),
        None => host,
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            home_page: WebAddress::parse("https://duckduckgo.com/").expect("valid default"),
            search_engine: SearchEngine::default(),
            updated_at: 0,
            blocker: true,
            blocker_allowlist: Vec::new(),
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

    fn allowing(hosts: &[&str]) -> Settings {
        Settings {
            blocker_allowlist: hosts.iter().map(|h| h.to_string()).collect(),
            ..Settings::default()
        }
    }

    #[test]
    fn the_blocker_is_on_by_default() {
        assert!(Settings::default().blocks_on("example.com"));
    }

    #[test]
    fn an_allowlisted_host_covers_its_subdomains() {
        let s = allowing(&["lemonde.fr"]);
        assert!(!s.blocks_on("lemonde.fr"));
        assert!(!s.blocks_on("news.lemonde.fr"));
        assert!(s.blocks_on("notlemonde.fr"));
        assert!(s.blocks_on("example.com"));
    }

    #[test]
    fn allowlist_ignores_case_and_www() {
        assert!(!allowing(&["WWW.LeMonde.fr"]).blocks_on("lemonde.fr"));
        assert!(!allowing(&["lemonde.fr"]).blocks_on("www.LEMONDE.fr"));
    }

    #[test]
    fn switching_a_site_off_and_on() {
        let mut s = allowing(&["a.com"]);
        s.set_blocks_on("www.LeMonde.fr", false);
        assert_eq!(s.blocker_allowlist, vec!["a.com", "lemonde.fr"]);
        assert!(!s.blocks_on("news.lemonde.fr"));
        // On again for a subdomain: the parent entry goes too.
        s.set_blocks_on("news.lemonde.fr", true);
        assert_eq!(s.blocker_allowlist, vec!["a.com"]);
        assert!(s.blocks_on("lemonde.fr"));
    }

    #[test]
    fn a_disabled_blocker_blocks_nowhere() {
        let s = Settings {
            blocker: false,
            ..Settings::default()
        };
        assert!(!s.blocks_on("example.com"));
    }

    #[test]
    fn merge_takes_the_newer_allowlist() {
        let mut mine = allowing(&["a.com"]);
        let theirs = Settings {
            updated_at: 3,
            ..allowing(&["b.com"])
        };
        assert!(mine.merge(&theirs));
        assert_eq!(mine.blocker_allowlist, vec!["b.com".to_string()]);
    }
}
