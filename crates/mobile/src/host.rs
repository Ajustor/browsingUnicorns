//! What the Android app implements in Kotlin: its WebViews and its HTTP
//! client. Rust calls them from the thread that called into Rust.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use application::ports::{
    EngineError, ExtensionSupport, PageSource, SyncError, Viewport, WebEngine,
};
use domain::{Extensions, RuleSet, TabId, UserScripts, WebAddress};
use infrastructure::extension_server::ExtensionServer;
use infrastructure::page_scripts::{Bridge, Sources, page_scripts};
use infrastructure::sync::BlobTransport;
use infrastructure::sync::relay_api::{decode_group, device_url, group_url};

use crate::MobileError;

/// Which WebView a page script message is for.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum PageTarget {
    Tab {
        tab: u64,
    },
    /// The hidden WebView of an extension's background page.
    Background {
        extension: String,
    },
    /// The extension popup.
    Panel,
}

impl From<&PageSource> for PageTarget {
    fn from(source: &PageSource) -> Self {
        match source {
            PageSource::Tab(tab) => Self::Tab { tab: tab.get() },
            PageSource::Background(id) => Self::Background {
                extension: id.to_string(),
            },
            PageSource::Panel => Self::Panel,
        }
    }
}

impl PageTarget {
    pub fn source(&self) -> Option<PageSource> {
        Some(match self {
            Self::Tab { tab } => PageSource::Tab(TabId::new(*tab)),
            Self::Background { extension } => {
                PageSource::Background(domain::ExtensionId::parse(extension)?)
            }
            Self::Panel => PageSource::Panel,
        })
    }
}

/// An extension's background page, run in a hidden WebView.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct BackgroundPage {
    pub extension: String,
    pub url: String,
}

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
    /// Runs `script` in `target`'s page. May be called from any thread.
    fn post_to_page(&self, target: PageTarget, script: String);
    /// The background pages to keep running (the others are closed). May
    /// be called from any thread.
    fn set_backgrounds(&self, pages: Vec<BackgroundPage>);
    /// Shows an extension popup (`None`: closes it).
    fn show_panel(&self, url: Option<String>);
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
    /// Serves the extensions' files (`https://<id>.bu-ext.invalid/`).
    pub server: Arc<ExtensionServer>,
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
            blocker: false,
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
        self.server.set_extensions(extensions);
        self.send_page_scripts();
        let pages = extensions
            .items()
            .iter()
            .filter(|e| e.enabled)
            .filter_map(|e| {
                Some(BackgroundPage {
                    extension: e.id.to_string(),
                    url: self.server.background_url(&e.id)?,
                })
            })
            .collect();
        self.host.set_backgrounds(pages);
    }

    fn reply_to_page(&mut self, to: &PageSource, message: &str) {
        self.host.post_to_page(
            to.into(),
            format!("window.__buReply && window.__buReply({message})"),
        );
    }

    fn run_in_tab(&mut self, tab: TabId, script: &str) {
        self.host
            .post_to_page(PageTarget::Tab { tab: tab.get() }, script.to_owned());
    }

    fn show_panel_page(&mut self, url: Option<&str>, _area: Viewport) {
        self.host.show_panel(url.map(str::to_owned));
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
