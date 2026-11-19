//! The ad blocker's scriptlets, injected at document start for one site:
//! each view keeps the hosts it has a script for, the script runs on that
//! host only and cannot break the page.

use std::collections::HashSet;

use domain::WebAddress;

/// Starts every scriptlet script: tells them from the other page scripts.
pub const MARKER: &str = "/* browsingUnicorns scriptlets */";

/// The host of `url` when it is a web page (http, https).
pub fn web_host(url: &str) -> Option<String> {
    let address = WebAddress::parse(url).ok()?;
    matches!(address.scheme(), "http" | "https")
        .then(|| address.host().map(str::to_owned))
        .flatten()
}

/// `script`, run on `host`'s documents only, its errors caught.
pub fn guarded(host: &str, script: &str) -> String {
    let host = serde_json::to_string(host).expect("a string");
    format!(
        "{MARKER}
if (location.hostname === {host}) {{ try {{
{script}
}} catch (e) {{}} }}
"
    )
}

/// The sites a view has a scriptlet script for, and what the web view
/// handed back for each (`H`: an id, a script object, nothing).
pub struct SiteScripts<H> {
    hosts: HashSet<String>,
    handles: Vec<H>,
}

impl<H> Default for SiteScripts<H> {
    fn default() -> Self {
        Self {
            hosts: HashSet::new(),
            handles: Vec::new(),
        }
    }
}

impl<H> SiteScripts<H> {
    /// The script to add for a navigation to `url`, when its host has
    /// scriptlets (`scriptlets(url)` not empty) and none was added yet.
    pub fn script_for(
        &mut self,
        url: &str,
        scriptlets: impl FnOnce(&str) -> String,
    ) -> Option<String> {
        let host = web_host(url)?;
        if self.hosts.contains(&host) {
            return None;
        }
        let script = scriptlets(url);
        if script.is_empty() {
            return None;
        }
        let guarded = guarded(&host, &script);
        self.hosts.insert(host);
        Some(guarded)
    }

    /// Keeps what the web view handed back for an added script.
    pub fn keep(&mut self, handle: H) {
        self.handles.push(handle);
    }

    /// Forgets `host` alone (its script was refused): the next navigation
    /// to it asks for a script again.
    pub fn forget(&mut self, host: &str) {
        self.hosts.remove(host);
    }

    /// Forgets every site; the handles, to remove their scripts.
    pub fn clear(&mut self) -> Vec<H> {
        self.hosts.clear();
        std::mem::take(&mut self.handles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_pages_have_a_host() {
        assert_eq!(
            web_host("https://www.youtube.com/watch?v=1").as_deref(),
            Some("www.youtube.com")
        );
        assert_eq!(
            web_host("http://127.0.0.1:8000/").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(web_host("file:///C:/page.html"), None);
        assert_eq!(web_host("about:blank"), None);
        assert_eq!(web_host("not a url"), None);
    }

    #[test]
    fn the_script_runs_on_its_host_only() {
        let s = guarded("www.youtube.com", "window.x = 1;");
        assert!(s.starts_with(MARKER));
        assert!(
            s.contains(r#"location.hostname === "www.youtube.com""#),
            "{s}"
        );
        assert!(s.contains("try {") && s.contains("catch (e)"), "{s}");
        assert!(s.contains("window.x = 1;"));
        // A host cannot end the string it is compared with.
        assert!(guarded(r#"a"b"#, "").contains(r#""a\"b""#));
    }

    #[test]
    fn a_site_gets_its_script_once() {
        let mut sites = SiteScripts::<u32>::default();
        let with = |_: &str| "window.x = 1;".to_string();
        let script = sites.script_for("https://site.test/a", with).unwrap();
        assert!(script.contains(r#""site.test""#));
        assert_eq!(sites.script_for("https://site.test/b", with), None);
        assert!(sites.script_for("https://other.test/", with).is_some());
    }

    #[test]
    fn a_site_without_scriptlets_is_asked_again() {
        let mut sites = SiteScripts::<u32>::default();
        assert_eq!(
            sites.script_for("https://site.test/", |_| String::new()),
            None
        );
        // The lists arrived, or the site left the allowlist.
        assert!(
            sites
                .script_for("https://site.test/", |_| "x();".into())
                .is_some()
        );
    }

    #[test]
    fn a_forgotten_site_is_asked_again() {
        let mut sites = SiteScripts::<u32>::default();
        let with = |_: &str| "x();".to_string();
        assert!(sites.script_for("https://site.test/", with).is_some());
        assert_eq!(sites.script_for("https://site.test/", with), None);
        sites.forget("site.test");
        assert!(sites.script_for("https://site.test/", with).is_some());
    }

    #[test]
    fn other_addresses_get_nothing() {
        let mut sites = SiteScripts::<u32>::default();
        assert_eq!(
            sites.script_for("file:///page.html", |_| "x();".into()),
            None
        );
    }

    #[test]
    fn clearing_forgets_the_sites_and_hands_the_handles_back() {
        let mut sites = SiteScripts::default();
        sites.script_for("https://site.test/", |_| "x();".into());
        sites.keep(7u32);
        assert_eq!(sites.clear(), vec![7]);
        assert!(sites.clear().is_empty());
        assert!(
            sites
                .script_for("https://site.test/", |_| "x();".into())
                .is_some()
        );
    }
}
