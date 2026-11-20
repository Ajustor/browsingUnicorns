use std::time::Duration;

use application::ports::SyncError;

use super::BlobTransport;
use super::relay_api::{decode_group, device_url, error_message, group_url};

/// [`BlobTransport`] over HTTPS to the relay (see [`super::relay_api`]).
pub struct HttpTransport {
    base: String,
    agent: ureq::Agent,
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
            base: base_url.to_owned(),
            agent,
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
    Err(error_message(status.as_u16(), &body))
}

fn network(e: ureq::Error) -> SyncError {
    SyncError(i18n::errors::infrastructure::relay_unreachable(&e))
}

impl BlobTransport for HttpTransport {
    fn put(&self, group: &str, token: &str, device: &str, blob: &[u8]) -> Result<(), SyncError> {
        let resp = self
            .agent
            .put(&device_url(&self.base, group, device))
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
            .get(&group_url(&self.base, group))
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
            .map_err(|e| SyncError(i18n::errors::infrastructure::relay_failed(&e)))?;
        decode_group(&text)
    }

    fn delete(&self, group: &str, token: &str, device: &str) -> Result<(), SyncError> {
        let resp = self
            .agent
            .delete(&device_url(&self.base, group, device))
            .header("Authorization", &format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .call()
            .map_err(network)?;
        check(resp).map(|_| ())
    }
}
