//! What the Android app implements in Kotlin: its WebViews and its HTTP
//! client. Rust calls them from the thread that called into Rust.

use std::sync::Arc;

use application::ports::{EngineError, SyncError, Viewport, WebEngine};
use domain::{TabId, WebAddress};
use infrastructure::sync::BlobTransport;
use infrastructure::sync::relay_api::{decode_group, device_url, group_url};

use crate::MobileError;

/// One Android WebView per tab, only the shown one visible.
#[uniffi::export(with_foreign)]
pub trait WebViewHost: Send + Sync {
    fn open_view(&self, tab: u64, url: String);
    fn close_view(&self, tab: u64);
    /// Shows `tab`'s view (hides the others); `None` hides them all.
    fn show(&self, tab: Option<u64>);
    fn navigate(&self, tab: u64, url: String);
    fn go_back(&self, tab: u64);
    fn go_forward(&self, tab: u64);
    fn reload(&self, tab: u64);
}

/// HTTP for the sync relay. Blocking: called from a background thread.
#[uniffi::export(with_foreign)]
pub trait SyncHttp: Send + Sync {
    fn put(&self, url: String, token: String, body: Vec<u8>) -> Result<(), MobileError>;
    /// The response body of a successful GET.
    fn get(&self, url: String, token: String) -> Result<String, MobileError>;
    fn delete(&self, url: String, token: String) -> Result<(), MobileError>;
}

/// [`WebEngine`] backed by the app's [`WebViewHost`]. Page events come back
/// through `MobileBrowser::on_*`.
pub struct HostEngine(pub Arc<dyn WebViewHost>);

impl WebEngine for HostEngine {
    fn open_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        self.0.open_view(tab.get(), address.to_string());
        Ok(())
    }
    fn close_view(&mut self, tab: TabId) {
        self.0.close_view(tab.get());
    }
    fn show(&mut self, tab: Option<TabId>) {
        self.0.show(tab.map(TabId::get));
    }
    fn navigate(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        self.0.navigate(tab.get(), address.to_string());
        Ok(())
    }
    fn go_back(&mut self, tab: TabId) {
        self.0.go_back(tab.get());
    }
    fn go_forward(&mut self, tab: TabId) {
        self.0.go_forward(tab.get());
    }
    fn reload(&mut self, tab: TabId) {
        self.0.reload(tab.get());
    }
    /// Compose lays the WebView out.
    fn set_viewport(&mut self, _viewport: Viewport) {}
}

/// [`BlobTransport`] over the app's [`SyncHttp`].
pub struct HostTransport {
    pub http: Arc<dyn SyncHttp>,
    pub base: String,
}

fn sync_error(e: MobileError) -> SyncError {
    SyncError(e.to_string())
}

impl BlobTransport for HostTransport {
    fn put(&self, group: &str, token: &str, device: &str, blob: &[u8]) -> Result<(), SyncError> {
        self.http
            .put(
                device_url(&self.base, group, device),
                token.to_owned(),
                blob.to_vec(),
            )
            .map_err(sync_error)
    }

    fn list(&self, group: &str, token: &str) -> Result<Vec<(String, Vec<u8>)>, SyncError> {
        let body = self
            .http
            .get(group_url(&self.base, group), token.to_owned())
            .map_err(sync_error)?;
        decode_group(&body)
    }

    fn delete(&self, group: &str, token: &str, device: &str) -> Result<(), SyncError> {
        self.http
            .delete(device_url(&self.base, group, device), token.to_owned())
            .map_err(sync_error)
    }
}
