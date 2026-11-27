//! What Chrome grants an extension's own pages: `fetch` to the sites in the
//! extension's host permissions without CORS. Here the browser makes those
//! requests itself (the page hands them over through its bridge) and gives
//! the page the answer.
//!
//! Redirects are followed here, one hop at a time, each one checked as the
//! first: an allowed site can't send the request elsewhere (the computer
//! itself, the local network, the cloud's metadata address...).

use std::net::IpAddr;
use std::sync::Arc;

use domain::UrlPattern;
use serde_json::{Map, Value};
use url::{Host, Url};

/// A request an extension page asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// Whether the request may reach the computer itself or the local
    /// network (whatever the name resolves to): set by [`fetch`], only when
    /// the extension's host permissions name the host.
    pub private_network: bool,
}

/// The answer, whatever its status (4xx and 5xx included, as `fetch` does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchResponse {
    /// The final address, after redirects.
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Makes a request, without following redirects (a 3xx is an answer).
/// Blocking: called off the UI thread.
pub type Fetcher = Arc<dyn Fn(FetchRequest) -> Result<FetchResponse, String> + Send + Sync>;

/// Largest answer handed to a page.
#[cfg(feature = "desktop")]
const MAX_BODY: u64 = 64 * 1024 * 1024;

/// Redirects followed before giving up.
const MAX_REDIRECTS: usize = 10;

/// The error of a request the extension may not make.
pub const NOT_PERMITTED: &str = "not permitted";

/// Headers the browser sets itself (a page can't, in `fetch` either).
const FORBIDDEN: &[&str] = &[
    "host",
    "content-length",
    "connection",
    "cookie",
    "origin",
    "referer",
    "keep-alive",
    "transfer-encoding",
    "upgrade",
];

/// Whether the extension whose manifest is `manifest` may reach `url`
/// without CORS: its host permissions (`host_permissions`, or the URL
/// patterns of `permissions` in manifest v2) cover it.
pub fn allowed(manifest: &Map<String, Value>, url: &str) -> bool {
    host_permissions(manifest)
        .filter_map(UrlPattern::parse_match)
        .any(|pattern| pattern.matches(url) == Some(true))
}

/// The match patterns of `manifest`.
fn host_permissions(manifest: &Map<String, Value>) -> impl Iterator<Item = &str> {
    [
        "host_permissions",
        "optional_host_permissions",
        "permissions",
    ]
    .into_iter()
    .filter_map(|key| manifest.get(key).and_then(Value::as_array))
    .flatten()
    .filter_map(Value::as_str)
}

/// Whether a host permission of `manifest` covers `url` and names its host
/// as is (no `*`, not `<all_urls>`): what it takes to reach the computer
/// itself or the local network.
fn names_host(manifest: &Map<String, Value>, url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    host_permissions(manifest).any(|p| {
        pattern_host(p).is_some_and(|h| h.eq_ignore_ascii_case(host))
            && UrlPattern::parse_match(p).is_some_and(|m| m.matches(url.as_str()) == Some(true))
    })
}

/// The host of match pattern `pattern`, when it names one (no wildcard).
fn pattern_host(pattern: &str) -> Option<&str> {
    let rest = pattern.split_once("://")?.1;
    let authority = rest.split('/').next()?;
    if authority.is_empty() || authority.contains('*') {
        return None;
    }
    Some(match authority.strip_prefix('[') {
        Some(_) => &authority[..=authority.find(']')?],
        None => authority.split(':').next()?,
    })
}

/// Whether `url` leads to the computer itself or the local network:
/// `localhost`, loopback, private, link-local addresses...
fn is_local(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost")
        }
        Some(Host::Ipv4(ip)) => is_local_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => is_local_ip(IpAddr::V6(ip)),
        None => false,
    }
}

/// Loopback, private (RFC 1918, unique local), link-local (the cloud's
/// metadata address 169.254.169.254), shared (CGNAT), unspecified.
pub fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || a == 0
                || (a == 100 && b & 0xc0 == 64)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_local_ip(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
        }
    }
}

/// Makes `request` for the extension whose manifest is `manifest` with
/// `fetcher`, following redirects: every address (the first, each
/// redirect's) must be covered by the extension's host permissions, and one
/// on the computer itself or the local network must be named by them.
pub fn fetch(
    fetcher: &Fetcher,
    mut request: FetchRequest,
    manifest: &Map<String, Value>,
) -> Result<FetchResponse, String> {
    for _ in 0..=MAX_REDIRECTS {
        let url = Url::parse(&request.url).map_err(|e| e.to_string())?;
        if !allowed(manifest, url.as_str()) {
            return Err(NOT_PERMITTED.into());
        }
        request.private_network = names_host(manifest, &url);
        if is_local(&url) && !request.private_network {
            return Err(NOT_PERMITTED.into());
        }
        let response = fetcher(request.clone())?;
        if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }
        let Some(location) = response
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("location"))
            .map(|(_, value)| value)
        else {
            return Ok(response);
        };
        let next = url.join(location).map_err(|e| e.to_string())?;
        // As `fetch` does: a 303, or a POST answered 301/302, becomes a GET
        // without a body.
        if (response.status == 303 && request.method != "GET" && request.method != "HEAD")
            || (matches!(response.status, 301 | 302) && request.method == "POST")
        {
            request.method = "GET".into();
            request.body = None;
            request
                .headers
                .retain(|(name, _)| !name.to_ascii_lowercase().starts_with("content-"));
        }
        // Credentials stay with the site they were meant for.
        if next.origin() != url.origin() {
            request
                .headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
        }
        request.url = next.into();
    }
    Err("too many redirects".into())
}

/// The page's headers the request keeps.
pub fn sendable_headers(headers: Vec<(String, String)>) -> Vec<(String, String)> {
    headers
        .into_iter()
        .filter(|(name, _)| {
            let name = name.to_ascii_lowercase();
            !FORBIDDEN.contains(&name.as_str())
                && !name.starts_with("sec-")
                && !name.starts_with("proxy-")
        })
        .collect()
}

/// The system's name resolution, the addresses on the computer itself or
/// the local network left out: a public name can't lead there (DNS
/// rebinding). Through a proxy, which resolves names itself, as is.
#[cfg(feature = "desktop")]
#[derive(Debug, Default)]
struct PublicOnly(ureq::unversioned::resolver::DefaultResolver);

#[cfg(feature = "desktop")]
impl ureq::unversioned::resolver::Resolver for PublicOnly {
    fn resolve(
        &self,
        uri: &ureq::http::Uri,
        config: &ureq::config::Config,
        timeout: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let all = self.0.resolve(uri, config, timeout)?;
        if config.proxy().is_some() {
            return Ok(all);
        }
        let mut public = self.empty();
        for addr in all.iter().filter(|a| !is_local_ip(a.ip())) {
            public.push(*addr);
        }
        if public.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        Ok(public)
    }
}

/// [`Fetcher`] with ureq (desktop): the system's certificates, redirects
/// not followed ([`fetch`] does), any status returned as is.
#[cfg(feature = "desktop")]
pub fn ureq_fetcher() -> Fetcher {
    use std::time::Duration;

    use ureq::unversioned::resolver::DefaultResolver;
    use ureq::unversioned::transport::DefaultConnector;

    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let config = ureq::Agent::config_builder()
        .tls_config(tls)
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(120)))
        .build();
    // The local network for the hosts the extension names, else public
    // addresses only.
    let local = ureq::Agent::with_parts(
        config.clone(),
        DefaultConnector::default(),
        DefaultResolver::default(),
    );
    let public =
        ureq::Agent::with_parts(config, DefaultConnector::default(), PublicOnly::default());
    Arc::new(move |request: FetchRequest| {
        let agent = if request.private_network {
            &local
        } else {
            &public
        };
        let mut builder = ureq::http::Request::builder()
            .method(request.method.as_str())
            .uri(request.url.as_str());
        for (name, value) in sendable_headers(request.headers) {
            builder = builder.header(name, value);
        }
        let http = builder
            .body(request.body.unwrap_or_default())
            .map_err(|e| e.to_string())?;
        let mut response = agent.run(http).map_err(|e| e.to_string())?;
        let status = response.status();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect();
        let url = response
            .extensions()
            .get::<ureq::http::Uri>()
            .map_or(request.url.clone(), ToString::to_string);
        let body = response
            .body_mut()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        Ok(FetchResponse {
            url,
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or("").to_owned(),
            headers,
            body,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(json: &str) -> Map<String, Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn host_permissions_decide() {
        let ext = manifest(r#"{"host_permissions": ["https://*.bitwarden.com/*"]}"#);
        assert!(allowed(&ext, "https://api.bitwarden.com/accounts/prelogin"));
        assert!(!allowed(&ext, "https://evil.example/"));
        assert!(!allowed(&ext, "http://api.bitwarden.com/"));

        let all = manifest(r#"{"host_permissions": ["https://*/*", "http://*/*"]}"#);
        assert!(allowed(&all, "https://anything.test/x"));

        // Manifest v2: URL patterns among the permissions.
        let v2 = manifest(r#"{"permissions": ["storage", "<all_urls>"]}"#);
        assert!(allowed(&v2, "https://a.test/"));

        let none = manifest(r#"{"permissions": ["storage"]}"#);
        assert!(!allowed(&none, "https://a.test/"));
        assert!(!allowed(&Map::new(), "https://a.test/"));
    }

    #[test]
    fn the_browser_sets_some_headers_itself() {
        let kept = sendable_headers(vec![
            ("Content-Type".into(), "application/json".into()),
            ("Device-Type".into(), "2".into()),
            ("Cookie".into(), "x".into()),
            ("Origin".into(), "https://bu-ext.x".into()),
            ("Sec-Fetch-Mode".into(), "cors".into()),
        ]);
        assert_eq!(
            kept,
            [
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Device-Type".to_owned(), "2".to_owned()),
            ]
        );
    }

    fn get(url: &str) -> FetchRequest {
        FetchRequest {
            url: url.into(),
            method: "GET".into(),
            headers: vec![("Authorization".into(), "Bearer x".into())],
            body: None,
            private_network: false,
        }
    }

    /// Answers by address: `redirects` send elsewhere, anything else is a
    /// 200. Keeps the requests made.
    fn site(
        redirects: &[(&str, u16, &str)],
    ) -> (Fetcher, Arc<std::sync::Mutex<Vec<FetchRequest>>>) {
        let redirects: Vec<(String, u16, String)> = redirects
            .iter()
            .map(|(from, status, to)| ((*from).to_owned(), *status, (*to).to_owned()))
            .collect();
        let made = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = made.clone();
        let fetcher: Fetcher = Arc::new(move |r: FetchRequest| {
            log.lock().unwrap().push(r.clone());
            let (status, headers) = match redirects.iter().find(|(from, ..)| *from == r.url) {
                Some((_, status, to)) => (*status, vec![("Location".to_owned(), to.clone())]),
                None => (200, Vec::new()),
            };
            Ok(FetchResponse {
                url: r.url,
                status,
                status_text: String::new(),
                headers,
                body: Vec::new(),
            })
        });
        (fetcher, made)
    }

    #[test]
    fn every_redirect_is_checked() {
        let ext = manifest(r#"{"host_permissions": ["https://api.test/*"]}"#);
        let (fetcher, made) = site(&[
            ("https://api.test/a", 302, "/b"),
            ("https://api.test/b", 301, "https://elsewhere.test/"),
        ]);
        assert_eq!(
            fetch(&fetcher, get("https://api.test/a"), &ext),
            Err(NOT_PERMITTED.into())
        );
        let urls: Vec<String> = made.lock().unwrap().iter().map(|r| r.url.clone()).collect();
        assert_eq!(urls, ["https://api.test/a", "https://api.test/b"]);

        // Within the permissions: followed to the end.
        let (fetcher, _) = site(&[("https://api.test/a", 307, "https://api.test/c")]);
        let answer = fetch(&fetcher, get("https://api.test/a"), &ext).unwrap();
        assert_eq!(
            (answer.url.as_str(), answer.status),
            ("https://api.test/c", 200)
        );
    }

    #[test]
    fn the_local_network_takes_a_permission_naming_it() {
        let all = manifest(r#"{"host_permissions": ["<all_urls>"]}"#);
        for target in [
            "http://127.0.0.1/",
            "http://localhost:8080/admin",
            "http://169.254.169.254/latest/meta-data/",
            "http://192.168.1.1/",
            "http://10.0.0.1/",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://0x7f000001/",
        ] {
            let (fetcher, made) = site(&[("https://ok.test/", 302, target)]);
            assert_eq!(
                fetch(&fetcher, get("https://ok.test/"), &all),
                Err(NOT_PERMITTED.into()),
                "{target}"
            );
            assert_eq!(made.lock().unwrap().len(), 1, "{target}");
            assert_eq!(
                fetch(&fetcher, get(target), &all),
                Err(NOT_PERMITTED.into()),
                "{target}"
            );
        }
        // Public addresses: resolved without the local network.
        let (fetcher, made) = site(&[]);
        fetch(&fetcher, get("https://ok.test/"), &all).unwrap();
        assert!(!made.lock().unwrap()[0].private_network);

        // Named: allowed, and may resolve to the local network.
        let named =
            manifest(r#"{"host_permissions": ["http://localhost/*", "http://192.168.1.1/*"]}"#);
        for target in ["http://localhost:8080/admin", "http://192.168.1.1/"] {
            let (fetcher, made) = site(&[]);
            fetch(&fetcher, get(target), &named).unwrap();
            assert!(made.lock().unwrap()[0].private_network, "{target}");
        }
        let (fetcher, _) = site(&[]);
        assert!(fetch(&fetcher, get("http://10.0.0.1/"), &named).is_err());
    }

    #[test]
    fn redirects_are_followed_as_fetch_does() {
        let ext = manifest(r#"{"host_permissions": ["https://*.test/*"]}"#);
        let (fetcher, made) = site(&[
            ("https://a.test/form", 303, "https://a.test/done"),
            ("https://a.test/done", 302, "https://b.test/"),
        ]);
        let mut post = get("https://a.test/form");
        post.method = "POST".into();
        post.body = Some(b"x".to_vec());
        post.headers
            .push(("Content-Type".into(), "text/plain".into()));
        fetch(&fetcher, post, &ext).unwrap();
        let made = made.lock().unwrap();
        assert_eq!(made[0].method, "POST");
        assert_eq!((made[1].method.as_str(), &made[1].body), ("GET", &None));
        assert_eq!(made[1].headers.len(), 1, "no Content-Type");
        // Another site: the credentials stay behind.
        assert!(made[2].headers.is_empty());

        // A loop ends.
        let (fetcher, _) = site(&[("https://a.test/loop", 302, "/loop")]);
        assert!(fetch(&fetcher, get("https://a.test/loop"), &ext).is_err());
    }

    #[test]
    fn local_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(is_local_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700::1111", "100.128.0.1"] {
            assert!(!is_local_ip(ip.parse().unwrap()), "{ip}");
        }
    }
}
