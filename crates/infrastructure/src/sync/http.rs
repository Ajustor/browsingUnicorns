use std::time::Duration;

use application::ports::SyncError;
use data_encoding::BASE64;
use serde::Deserialize;

use super::BlobTransport;

/// [`BlobTransport`] over HTTPS to the relay (`relay/sync` in this repo):
///
/// - `PUT    {base}/v1/groups/{group}/devices/{device}`  body: the blob
/// - `GET    {base}/v1/groups/{group}`  → `{"devices":[{"id","data"}]}` (base64)
/// - `DELETE {base}/v1/groups/{group}/devices/{device}`
///
/// with `Authorization: Bearer {token}`.
pub struct HttpTransport {
    base: String,
    agent: ureq::Agent,
}

#[derive(Deserialize)]
struct GroupDto {
    devices: Vec<BlobDto>,
}

#[derive(Deserialize)]
struct BlobDto {
    id: String,
    data: String,
}

const USER_AGENT: &str = concat!("browsingUnicorns-sync/", env!("CARGO_PKG_VERSION"));

impl HttpTransport {
    pub fn new(base_url: &str) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self {
            base: base_url.trim_end_matches('/').to_owned(),
            agent,
        }
    }

    fn url(&self, group: &str, device: Option<&str>) -> String {
        match device {
            Some(d) => format!("{}/v1/groups/{group}/devices/{d}", self.base),
            None => format!("{}/v1/groups/{group}", self.base),
        }
    }
}

fn check(
    mut resp: ureq::http::Response<ureq::Body>,
) -> Result<ureq::http::Response<ureq::Body>, SyncError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or(body);
    Err(SyncError(format!("relay: {status} {message}")))
}

fn network(e: ureq::Error) -> SyncError {
    SyncError(format!("relay unreachable: {e}"))
}

impl BlobTransport for HttpTransport {
    fn put(&self, group: &str, token: &str, device: &str, blob: &[u8]) -> Result<(), SyncError> {
        let resp = self
            .agent
            .put(&self.url(group, Some(device)))
            .header("Authorization", &format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .content_type("application/octet-stream")
            .send(blob)
            .map_err(network)?;
        check(resp).map(|_| ())
    }

    fn list(&self, group: &str, token: &str) -> Result<Vec<(String, Vec<u8>)>, SyncError> {
        let resp = self
            .agent
            .get(&self.url(group, None))
            .header("Authorization", &format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(network)?;
        let mut resp = check(resp)?;
        let text = resp
            .body_mut()
            .with_config()
            .limit(32 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| SyncError(format!("relay: {e}")))?;
        let group: GroupDto =
            serde_json::from_str(&text).map_err(|e| SyncError(format!("relay answer: {e}")))?;
        Ok(group
            .devices
            .into_iter()
            .filter_map(|b| Some((b.id, BASE64.decode(b.data.as_bytes()).ok()?)))
            .collect())
    }

    fn delete(&self, group: &str, token: &str, device: &str) -> Result<(), SyncError> {
        let resp = self
            .agent
            .delete(&self.url(group, Some(device)))
            .header("Authorization", &format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(network)?;
        check(resp).map(|_| ())
    }
}
