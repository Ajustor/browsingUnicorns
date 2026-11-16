//! The browser core for the Android app.
//!
//! The same domain and application layers as the desktop; the app implements
//! the engine ([`WebViewHost`], Android WebViews) and the HTTP client
//! ([`SyncHttp`]) in Kotlin. Exposed with UniFFI: Kotlin calls
//! [`MobileBrowser`], which is safe to use from any thread.

// MSVC prints "creating library ….lib" for the host build of the cdylib.
#![allow(linker_messages)]

mod extensions;
mod host;

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};

use application::sync::sync_round;
use application::{Browser, ExtensionManager, Target, UserScriptManager};
use domain::{RuleSet, Settings, TabId, WebAddress, allowlisted};
use infrastructure::blocker::{BlockerHandle, BlockerLists, BlockerPolicy};
use infrastructure::extension_hub::{BrowserHub, Hub};
use infrastructure::extension_server::{ExtensionServer, Origins};
use infrastructure::extensions::ExtensionFolder;
use infrastructure::page_scripts::Bridge;
use infrastructure::script_storage::{ScriptStorage, new_token};
use infrastructure::sync::{
    EncryptedSyncStore, SyncConfig, SyncConfigFile, SyncKey, new_device_id,
};
use infrastructure::user_scripts::{UserScriptFolder, UserScriptRepository};
use infrastructure::{JsonRepository, SystemClock, TomlSettingsRepository};

pub use extensions::{ExtensionInfo, ExtensionResource, Interception, UserScriptInfo};
pub use host::{BackgroundPage, PageTarget, SyncHttp, WebViewHost};
use host::{HostEngine, HostTransport};

uniffi::setup_scaffolding!();

/// The relay deployed from `relay/sync` (same as the desktop's).
pub const DEFAULT_RELAY: &str = "https://browsing-unicorns-sync.minecities.workers.dev";

#[derive(Debug, uniffi::Error)]
pub enum MobileError {
    Failed { reason: String },
}

impl fmt::Display for MobileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failed { reason } => f.write_str(reason),
        }
    }
}

impl std::error::Error for MobileError {}

impl MobileError {
    fn new(message: impl fmt::Display) -> Self {
        Self::Failed {
            reason: message.to_string(),
        }
    }
}

impl From<uniffi::UnexpectedUniFFICallbackError> for MobileError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::new(e.reason)
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct TabInfo {
    pub id: u64,
    pub title: String,
    pub url: Option<String>,
    pub loading: bool,
    /// Kept out of the history, the session and sync; its WebView has its
    /// own cookies and storage.
    pub private: bool,
}

/// What the browser screen shows.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BrowserState {
    pub tabs: Vec<TabInfo>,
    pub active: Option<u64>,
    pub active_url: Option<String>,
    pub active_secure: bool,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub bookmarked: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct PageLink {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct VisitInfo {
    pub url: String,
    pub title: String,
    pub at: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DeviceInfo {
    pub name: String,
    pub updated_at: u64,
    pub tabs: Vec<PageLink>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncInfo {
    pub enabled: bool,
    /// The pairing code (secret).
    pub code: Option<String>,
    /// What the pairing QR code holds.
    pub qr_text: Option<String>,
    pub device_name: Option<String>,
    pub last_sync_at: Option<u64>,
    pub last_error: Option<String>,
    pub devices: Vec<DeviceInfo>,
}

/// The ad blocker's state for a tab, for its shield.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BlockerInfo {
    /// The ad blocker is on.
    pub enabled: bool,
    /// The tab's site is allowlisted: the blocker is off there.
    pub site_allowed: bool,
    /// What it stopped on the tab's page.
    pub blocked: u32,
    /// When the lists were downloaded (unix seconds); `None` before they
    /// first arrive.
    pub lists_updated: Option<i64>,
}

struct Inner {
    browser: Browser<HostEngine>,
    sync: Option<SyncConfig>,
    last_sync_at: Option<u64>,
    last_error: Option<String>,
}

#[derive(uniffi::Object)]
pub struct MobileBrowser {
    inner: Mutex<Inner>,
    /// Locked before `inner` when both are.
    extensions: Mutex<ExtensionManager>,
    /// Locked before `inner` when both are.
    user_scripts: Mutex<UserScriptManager>,
    /// The browser side of the extension APIs (storage, messages, tabs).
    /// Locked before `inner`, after `extensions`.
    hub: Mutex<Hub>,
    /// Serves the extensions' files.
    server: Arc<ExtensionServer>,
    /// The extensions' network rules, read for every request.
    rules: Arc<RwLock<RuleSet>>,
    /// The built-in ad blocker, read for every request after the rules.
    blocker: BlockerHandle,
    blocker_lists: BlockerLists,
    /// The lists are being loaded or downloaded.
    blocker_busy: AtomicBool,
    /// What the ad blocker stopped on each tab's page.
    blocked: Mutex<HashMap<u64, u32>>,
    extensions_dir: PathBuf,
    sync_file: SyncConfigFile,
    http: Arc<dyn SyncHttp>,
    relay_url: String,
    device_name: String,
}

fn parse(url: &str) -> Result<WebAddress, MobileError> {
    WebAddress::parse(url).map_err(MobileError::new)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl MobileBrowser {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn extensions_lock(&self) -> MutexGuard<'_, ExtensionManager> {
        self.extensions.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn user_scripts_lock(&self) -> MutexGuard<'_, UserScriptManager> {
        self.user_scripts.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn hub_lock(&self) -> MutexGuard<'_, Hub> {
        self.hub.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Never held while taking another lock.
    fn blocked_lock(&self) -> MutexGuard<'_, HashMap<u64, u32>> {
        self.blocked.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Changes the settings, then hands the ad blocker its new policy.
    fn change_settings(&self, change: impl FnOnce(&mut Settings)) -> Result<(), MobileError> {
        let mut inner = self.lock();
        let mut settings = inner.browser.settings().clone();
        change(&mut settings);
        inner
            .browser
            .update_settings(settings)
            .map_err(MobileError::new)?;
        self.blocker
            .set_policy(BlockerPolicy::from_settings(inner.browser.settings()));
        Ok(())
    }

    /// `tabs.on*` events for the extensions, after the tabs changed.
    fn hub_tick(&self) {
        let extensions = self.extensions_lock().extensions().clone();
        let mut hub = self.hub_lock();
        let mut inner = self.lock();
        hub.tabs_changed(&mut BrowserHub {
            browser: &mut inner.browser,
            extensions: &extensions,
        });
    }

    fn store(&self, config: &SyncConfig) -> EncryptedSyncStore<HostTransport> {
        EncryptedSyncStore::new(
            config.key.clone(),
            HostTransport {
                http: self.http.clone(),
                base: config.relay_url.clone(),
            },
        )
    }

    fn join_group(&self, key: SyncKey) -> Result<(), MobileError> {
        let config = SyncConfig {
            key,
            device: new_device_id(),
            device_name: self.device_name.clone(),
            relay_url: self.relay_url.clone(),
        };
        self.sync_file.save(&config).map_err(MobileError::new)?;
        let mut inner = self.lock();
        inner.sync = Some(config);
        inner.last_sync_at = None;
        inner.last_error = None;
        Ok(())
    }
}

#[uniffi::export]
impl MobileBrowser {
    /// The browser on the profile kept in `data_dir` (the app's files dir).
    /// No tab is open: call [`Self::restore_session`] once the WebViews can
    /// be created. `relay_url` overrides the sync relay (tests).
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        device_name: String,
        relay_url: Option<String>,
        host: Arc<dyn WebViewHost>,
        http: Arc<dyn SyncHttp>,
    ) -> Arc<Self> {
        let dir = PathBuf::from(data_dir);
        let settings = TomlSettingsRepository::new(dir.join("settings.toml"));
        let profile = application::ports::Profile {
            settings: Box::new(TomlSettingsRepository::new(dir.join("settings.toml"))),
            history: Box::new(JsonRepository::new(dir.join("history.json"))),
            bookmarks: Box::new(JsonRepository::new(dir.join("bookmarks.json"))),
            session: Box::new(JsonRepository::new(dir.join("session.json"))),
            clock: Box::new(SystemClock),
        };
        let initial = application::ports::Repository::load(&settings).unwrap_or_default();
        let blocker = BlockerHandle::default();
        blocker.set_policy(BlockerPolicy::from_settings(&initial));
        let blocker_http = http.clone();
        let blocker_lists = BlockerLists::new(
            dir.join("blocker"),
            Arc::new(move |url: &str| {
                blocker_http
                    .download(url.to_owned())
                    .map_err(|e| e.to_string())
            }),
        );
        let rules = Arc::new(RwLock::new(RuleSet::default()));
        let extensions_dir = dir.join("extensions");
        let user_scripts_dir = dir.join("userscripts");
        let page_token = new_token();
        let state_dir = dir.join("extension-state");
        let server = ExtensionServer::new(
            dir.join("extensions"),
            state_dir.join("versions.json"),
            page_token.clone(),
            Origins::Android,
            Bridge::Android,
        );
        let engine = HostEngine {
            host,
            rules: rules.clone(),
            extensions_dir: extensions_dir.clone(),
            state_dir: state_dir.clone(),
            user_scripts_dir: user_scripts_dir.clone(),
            page_token: page_token.clone(),
            extensions: Default::default(),
            user_scripts: Default::default(),
            server: server.clone(),
        };
        let mut browser = Browser::new(engine, initial, profile);
        let _ = browser.load_profile();
        let download_http = http.clone();
        let packages = Arc::new(ExtensionFolder::new(
            extensions_dir.clone(),
            Arc::new(move |url: &str| {
                download_http
                    .download(url.to_owned())
                    .map_err(|e| e.to_string())
            }),
        ));
        let repository = Box::new(JsonRepository::new(dir.join("extensions.json")));
        let extensions = ExtensionManager::load(repository, packages.clone()).unwrap_or_else(|e| {
            eprintln!("extensions: {e}");
            ExtensionManager::load(
                Box::new(JsonRepository::new(dir.join("extensions.json.unreadable"))),
                packages,
            )
            .expect("an empty list loads")
        });
        application::ports::WebEngine::set_extensions(
            browser.engine_mut(),
            extensions.extensions(),
        );
        let download_scripts = http.clone();
        let script_packages = Arc::new(UserScriptFolder::new(
            user_scripts_dir.clone(),
            Arc::new(move |url: &str| {
                download_scripts
                    .download(url.to_owned())
                    .map_err(|e| e.to_string())
            }),
        ));
        let script_list = || {
            Box::new(UserScriptRepository::new(
                dir.join("userscripts.json"),
                user_scripts_dir.clone(),
            ))
        };
        let user_scripts = UserScriptManager::load(script_list(), script_packages.clone())
            .unwrap_or_else(|e| {
                eprintln!("user scripts: {e}");
                UserScriptManager::load(
                    Box::new(UserScriptRepository::new(
                        dir.join("userscripts.json.unreadable"),
                        user_scripts_dir.clone(),
                    )),
                    script_packages,
                )
                .expect("an empty list loads")
            });
        application::ports::WebEngine::set_user_scripts(
            browser.engine_mut(),
            user_scripts.scripts(),
        );
        let mut hub = Hub::new(
            ScriptStorage::new(dir.join("script-storage"), page_token),
            Bridge::Android,
            extensions_dir.clone(),
            state_dir,
        );
        hub.set_blocker(blocker.clone());
        let sync_file = SyncConfigFile::new(dir.join("sync.json"));
        let sync = sync_file.load();
        Arc::new(Self {
            inner: Mutex::new(Inner {
                browser,
                sync,
                last_sync_at: None,
                last_error: None,
            }),
            extensions: Mutex::new(extensions),
            user_scripts: Mutex::new(user_scripts),
            hub: Mutex::new(hub),
            server,
            rules,
            blocker,
            blocker_lists,
            blocker_busy: AtomicBool::new(false),
            blocked: Mutex::default(),
            extensions_dir,
            sync_file,
            http,
            relay_url: relay_url.unwrap_or_else(|| DEFAULT_RELAY.to_owned()),
            device_name,
        })
    }

    pub fn restore_session(&self) -> Result<(), MobileError> {
        self.lock()
            .browser
            .restore_session()
            .map(|_| ())
            .map_err(MobileError::new)
    }

    pub fn state(&self) -> BrowserState {
        let inner = self.lock();
        let b = &inner.browser;
        let session = b.session();
        let active = session.active();
        BrowserState {
            tabs: session
                .tabs()
                .iter()
                .map(|t| TabInfo {
                    id: t.id().get(),
                    title: t.display_title().to_owned(),
                    url: t.address().map(ToString::to_string),
                    loading: t.is_loading(),
                    private: t.is_private(),
                })
                .collect(),
            active: session.active_id().map(TabId::get),
            active_url: active.and_then(|t| t.address()).map(ToString::to_string),
            active_secure: active
                .and_then(|t| t.address())
                .is_some_and(WebAddress::is_secure),
            can_go_back: active.is_some_and(|t| t.history().can_go_back()),
            can_go_forward: active.is_some_and(|t| t.history().can_go_forward()),
            bookmarked: b.is_active_bookmarked(),
        }
    }

    // --- Tabs and navigation ---------------------------------------------

    /// Opens a tab on `url` (the home page when `None`), private or not.
    pub fn open_tab(&self, url: Option<String>, private: bool) -> Result<u64, MobileError> {
        let address = url.as_deref().map(parse).transpose()?;
        self.lock()
            .browser
            .open_tab_in(address, private)
            .map(TabId::get)
            .map_err(MobileError::new)
    }

    /// Closes every private tab.
    pub fn close_private_tabs(&self) {
        let closed = self.lock().browser.close_private_tabs();
        let mut blocked = self.blocked_lock();
        for id in closed {
            blocked.remove(&id.get());
        }
        drop(blocked);
        self.hub_tick();
    }

    pub fn close_tab(&self, id: u64) {
        self.lock().browser.close_tab(TabId::new(id));
        self.blocked_lock().remove(&id);
        self.hub_tick();
    }

    pub fn activate(&self, id: u64) {
        self.lock().browser.activate(TabId::new(id));
        self.hub_tick();
    }

    /// What was typed in the address bar: address, host name or search.
    pub fn submit(&self, input: String, new_tab: bool) -> Result<(), MobileError> {
        let target = if new_tab {
            Target::NewTab
        } else {
            Target::ActiveTab
        };
        self.lock()
            .browser
            .submit(&input, target)
            .map(|_| ())
            .map_err(MobileError::new)
    }

    pub fn open(&self, url: String, new_tab: bool) -> Result<(), MobileError> {
        let target = if new_tab {
            Target::NewTab
        } else {
            Target::ActiveTab
        };
        self.lock()
            .browser
            .load(parse(&url)?, target)
            .map_err(MobileError::new)
    }

    pub fn go_back(&self) {
        self.lock().browser.go_back();
    }

    pub fn go_forward(&self) {
        self.lock().browser.go_forward();
    }

    pub fn reload(&self) {
        self.lock().browser.reload();
    }

    pub fn go_home(&self) -> Result<(), MobileError> {
        self.lock().browser.go_home().map_err(MobileError::new)
    }

    // --- Events from the WebViews ----------------------------------------

    pub fn on_load_started(&self, tab: u64, url: String) {
        self.event(
            |tab_id, address| application::ports::EngineEvent::LoadStarted {
                tab: tab_id,
                address,
            },
            tab,
            &url,
        );
    }

    pub fn on_load_finished(&self, tab: u64, url: String) {
        self.event(
            |tab_id, address| application::ports::EngineEvent::LoadFinished {
                tab: tab_id,
                address,
            },
            tab,
            &url,
        );
    }

    pub fn on_title_changed(&self, tab: u64, title: String) {
        let _ = self
            .lock()
            .browser
            .handle(application::ports::EngineEvent::TitleChanged {
                tab: TabId::new(tab),
                title,
            });
        self.hub_tick();
    }

    /// A page asked for a new window.
    pub fn on_new_window(&self, url: String) -> Result<(), MobileError> {
        let address = parse(&url)?;
        self.lock()
            .browser
            .handle(application::ports::EngineEvent::NewTabRequested { address })
            .map_err(MobileError::new)
    }

    /// Saves history and open tabs (call when the app goes to background).
    pub fn persist(&self) -> Result<(), MobileError> {
        self.lock().browser.persist().map_err(MobileError::new)
    }

    // --- Bookmarks and history -------------------------------------------

    pub fn toggle_bookmark(&self) -> Result<bool, MobileError> {
        self.lock()
            .browser
            .toggle_bookmark()
            .map_err(MobileError::new)
    }

    pub fn bookmarks(&self) -> Vec<PageLink> {
        self.lock()
            .browser
            .bookmarks()
            .items()
            .iter()
            .map(|b| PageLink {
                url: b.address.to_string(),
                title: b.label().to_owned(),
            })
            .collect()
    }

    pub fn remove_bookmark(&self, url: String) -> Result<(), MobileError> {
        let address = parse(&url)?;
        self.lock()
            .browser
            .remove_bookmark(&address)
            .map_err(MobileError::new)
    }

    /// Latest visit of each page matching `query`, newest first.
    pub fn history(&self, query: String, limit: u32) -> Vec<VisitInfo> {
        self.lock()
            .browser
            .history()
            .search(&query, limit as usize)
            .into_iter()
            .map(|v| VisitInfo {
                url: v.address.to_string(),
                title: v.title.clone(),
                at: v.at,
            })
            .collect()
    }

    pub fn clear_history(&self) -> Result<(), MobileError> {
        self.lock()
            .browser
            .clear_history()
            .map_err(MobileError::new)
    }

    // --- Sync ----------------------------------------------------------

    pub fn sync_info(&self) -> SyncInfo {
        let inner = self.lock();
        let config = inner.sync.as_ref();
        SyncInfo {
            enabled: config.is_some(),
            code: config.map(|c| c.key.code()),
            qr_text: config.map(|c| c.key.qr_text()),
            device_name: config.map(|c| c.device_name.clone()),
            last_sync_at: inner.last_sync_at,
            last_error: inner.last_error.clone(),
            devices: inner
                .browser
                .remote_devices()
                .iter()
                .map(|d| DeviceInfo {
                    name: d.name.clone(),
                    updated_at: d.updated_at,
                    tabs: d
                        .tabs
                        .iter()
                        .map(|t| PageLink {
                            url: t.address.to_string(),
                            title: t.title.clone(),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// Starts a new sync group with a fresh key.
    pub fn sync_turn_on(&self) -> Result<(), MobileError> {
        self.join_group(SyncKey::generate())
    }

    /// Joins another device's group from its code (typed or scanned).
    pub fn sync_join(&self, code: String) -> Result<(), MobileError> {
        let key = SyncKey::from_code(&code).ok_or_else(|| {
            MobileError::new("This code is not valid: check it on the other device.")
        })?;
        self.join_group(key)
    }

    /// Leaves the group. Blocking (network): call it off the main thread.
    pub fn sync_turn_off(&self) -> Result<(), MobileError> {
        let config = self.lock().sync.take();
        if let Some(config) = config {
            use application::ports::SyncStore;
            if let Err(e) = self.store(&config).leave(&config.device) {
                // The key is forgotten anyway; the relay drops the record later.
                self.lock().last_error = Some(e.to_string());
            }
        }
        self.sync_file.delete().map_err(MobileError::new)
    }

    /// Whether something changed since the last sync.
    pub fn has_unsynced_changes(&self) -> bool {
        self.lock().browser.has_unsynced_changes()
    }

    /// One sync round. Blocking (network): call it off the main thread.
    /// Does nothing while sync is off.
    pub fn sync_now(&self) -> Result<(), MobileError> {
        let (config, record) = {
            let mut inner = self.lock();
            let Some(config) = inner.sync.clone() else {
                return Ok(());
            };
            let record = inner
                .browser
                .sync_record(config.device.clone(), &config.device_name);
            (config, record)
        };
        // The lock is not held during the network round.
        let result = sync_round(&self.store(&config), &record);
        let mut inner = self.lock();
        match result {
            Ok(records) => {
                inner.last_sync_at = Some(now());
                inner.last_error = None;
                let applied = inner
                    .browser
                    .apply_remote(&records)
                    .map_err(MobileError::new);
                // Another device's settings may have come along.
                self.blocker
                    .set_policy(BlockerPolicy::from_settings(inner.browser.settings()));
                applied
            }
            Err(e) => {
                inner.last_error = Some(e.to_string());
                Err(MobileError::new(e))
            }
        }
    }

    // --- Ad blocker ----------------------------------------------------

    /// Loads the cached lists when none is loaded, then downloads them when
    /// due (once a day). Blocking (network): call it off the main thread,
    /// at start and when the app comes back.
    pub fn update_blocker_lists(&self) {
        if self.blocker_busy.swap(true, Ordering::AcqRel) {
            return;
        }
        if self.blocker.get().is_none()
            && let Some(blocker) = self.blocker_lists.load_cached()
        {
            self.blocker.set(blocker);
        }
        if self.blocker_lists.due() {
            match self.blocker_lists.refresh() {
                Ok(blocker) => self.blocker.set(blocker),
                Err(e) => eprintln!("ad blocker lists: {e}"),
            }
        }
        self.blocker_busy.store(false, Ordering::Release);
    }

    /// The ad blocker's state for `tab`'s page.
    pub fn blocker_info(&self, tab: u64) -> BlockerInfo {
        let inner = self.lock();
        let settings = inner.browser.settings();
        let host = inner
            .browser
            .session()
            .tabs()
            .iter()
            .find(|t| t.id().get() == tab)
            .and_then(|t| t.address())
            .and_then(|a| a.host())
            .map(str::to_owned);
        BlockerInfo {
            enabled: settings.blocker,
            site_allowed: host
                .as_deref()
                .is_some_and(|h| allowlisted(&settings.blocker_allowlist, h)),
            blocked: self.blocked_lock().get(&tab).copied().unwrap_or(0),
            lists_updated: self.blocker.get().map(|b| {
                b.updated()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs() as i64)
            }),
        }
    }

    /// Turns the ad blocker on or off (reload the page to apply it).
    pub fn set_blocker(&self, enabled: bool) -> Result<(), MobileError> {
        self.change_settings(|s| s.blocker = enabled)
    }

    /// Turns the ad blocker on or off for `host` and its subdomains (reload
    /// the page to apply it).
    pub fn set_blocker_for_site(&self, host: String, enabled: bool) -> Result<(), MobileError> {
        self.change_settings(|s| s.set_blocks_on(&host, enabled))
    }
}

impl MobileBrowser {
    fn event(
        &self,
        make: impl FnOnce(TabId, WebAddress) -> application::ports::EngineEvent,
        tab: u64,
        url: &str,
    ) {
        // Pages the domain does not track (data:, blob:...) are ignored.
        if let Ok(address) = WebAddress::parse(url) {
            let _ = self.lock().browser.handle(make(TabId::new(tab), address));
        }
        self.hub_tick();
    }
}

#[cfg(test)]
mod tests;
