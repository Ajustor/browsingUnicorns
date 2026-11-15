//! What the Android app implements in Kotlin: its WebViews and its HTTP
//! client. Rust calls them from the thread that called into Rust.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use application::ports::{EngineError, ExtensionSupport, SyncError, Viewport, WebEngine};
use domain::{Extensions, RuleSet, TabId, UserScripts, WebAddress};
use infrastructure::page_scripts::{Bridge, Sources, page_scripts};
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
    /// The scripts to inject at document start into every page of every
    /// WebView from now on (they replace the previous ones). May be called
    /// from any thread.
    fn set_page_scripts(&self, scripts: Vec<String>);
}

/// HTTP for the sync relay. Blocking: called from a background thread.
#[uniffi::export(with_foreign)]
pub trait SyncHttp: Send + Sync {
    fn put(&self, url: String, token: String, body: Vec<u8>) -> Result<(), MobileError>;
    /// The response body of a successful GET.
    fn get(&self, url: String, token: String) -> Result<String, MobileError>;
    fn delete(&self, url: String, token: String) -> Result<(), MobileError>;
    /// The body of a plain GET (following redirects): extension packages.
    fn download(&self, url: String) -> Result<Vec<u8>, MobileError>;
}

/// [`WebEngine`] backed by the app's [`WebViewHost`]. Page events come back
/// through `MobileBrowser::on_*`. Extensions: their network rules, which
/// `MobileBrowser::intercept` applies to the WebViews' requests.
pub struct HostEngine {
    pub host: Arc<dyn WebViewHost>,
    pub rules: Arc<RwLock<RuleSet>>,
    pub extensions_dir: PathBuf,
    /// What extensions change at run time (rules, registered scripts).
    pub state_dir: PathBuf,
    pub user_scripts_dir: PathBuf,
    /// Written into the page scripts, checked by the script storage.
    pub page_token: String,
    pub extensions: Extensions,
    pub user_scripts: UserScripts,
}

impl HostEngine {
    fn send_page_scripts(&self) {
        let scripts = page_scripts(&Sources {
            extensions_dir: &self.extensions_dir,
            extensions: Some(&self.extensions),
            user_scripts_dir: &self.user_scripts_dir,
            user_scripts: &self.user_scripts,
            token: &self.page_token,
            bridge: Bridge::Android,
            origins: infrastructure::extension_server::Origins::Android,
            state_dir: Some(&self.state_dir),
        });
        self.host.set_page_scripts(scripts);
    }
}

impl WebEngine for HostEngine {
    fn open_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        self.host.open_view(tab.get(), address.to_string());
        Ok(())
    }
    fn close_view(&mut self, tab: TabId) {
        self.host.close_view(tab.get());
    }
    fn show(&mut self, tab: Option<TabId>) {
        self.host.show(tab.map(TabId::get));
    }
    fn navigate(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
        self.host.navigate(tab.get(), address.to_string());
        Ok(())
    }
    fn go_back(&mut self, tab: TabId) {
        self.host.go_back(tab.get());
    }
    fn go_forward(&mut self, tab: TabId) {
        self.host.go_forward(tab.get());
    }
    fn reload(&mut self, tab: TabId) {
        self.host.reload(tab.get());
    }
    /// Compose lays the WebView out.
    fn set_viewport(&mut self, _viewport: Viewport) {}

    fn extension_support(&self) -> ExtensionSupport {
        ExtensionSupport::Partial
    }

    fn set_extensions(&mut self, extensions: &Extensions) {
        let rules = infrastructure::extensions::rule_set(
            &self.extensions_dir,
            Some(&self.state_dir),
            extensions,
        );
        *self.rules.write().unwrap_or_else(|p| p.into_inner()) = rules;
        self.extensions = extensions.clone();
        self.send_page_scripts();
    }

    fn set_user_scripts(&mut self, scripts: &UserScripts) {
        self.user_scripts = scripts.clone();
        self.send_page_scripts();
    }
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
