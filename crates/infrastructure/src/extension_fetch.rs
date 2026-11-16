//! What Chrome grants an extension's own pages: `fetch` to the sites in the
//! extension's host permissions without CORS. Here the browser makes those
//! requests itself (the page hands them over through its bridge) and gives
//! the page the answer.

use std::path::Path;
use std::sync::Arc;

use domain::UrlPattern;
use serde_json::Value;

/// A request an extension page asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
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

/// Makes a request. Blocking: called off the UI thread.
pub type Fetcher = Arc<dyn Fn(FetchRequest) -> Result<FetchResponse, String> + Send + Sync>;

/// Largest answer handed to a page.
const MAX_BODY: u64 = 64 * 1024 * 1024;

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

/// Whether the extension in `dir` may reach `url` without CORS: its host
/// permissions (`host_permissions`, or the URL patterns of `permissions` in
/// manifest v2) cover it.
pub fn allowed(dir: &Path, url: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("manifest.json")) else {
        return false;
    };
    let Ok(manifest) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else {
        return false;
    };
    [
        "host_permissions",
        "optional_host_permissions",
        "permissions",
    ]
    .iter()
    .filter_map(|key| manifest.get(*key).and_then(Value::as_array))
    .flatten()
    .filter_map(Value::as_str)
    .filter_map(UrlPattern::parse_match)
    .any(|pattern| pattern.matches(url) == Some(true))
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

/// [`Fetcher`] with ureq (desktop): the system's certificates, redirects
/// followed, any status returned as is.
#[cfg(feature = "desktop")]
pub fn ureq_fetcher() -> Fetcher {
    use std::time::Duration;

    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .new_agent();
    Arc::new(move |request: FetchRequest| {
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

    fn extension(manifest: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
        dir
    }

    #[test]
    fn host_permissions_decide() {
        let ext = extension(r#"{"host_permissions": ["https://*.bitwarden.com/*"]}"#);
        assert!(allowed(
            ext.path(),
            "https://api.bitwarden.com/accounts/prelogin"
        ));
        assert!(!allowed(ext.path(), "https://evil.example/"));
        assert!(!allowed(ext.path(), "http://api.bitwarden.com/"));

        let all = extension(r#"{"host_permissions": ["https://*/*", "http://*/*"]}"#);
        assert!(allowed(all.path(), "https://anything.test/x"));

        // Manifest v2: URL patterns among the permissions.
        let v2 = extension(r#"{"permissions": ["storage", "<all_urls>"]}"#);
        assert!(allowed(v2.path(), "https://a.test/"));

        let none = extension(r#"{"permissions": ["storage"]}"#);
        assert!(!allowed(none.path(), "https://a.test/"));
        assert!(!allowed(Path::new("/no/such/extension"), "https://a.test/"));
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
}
