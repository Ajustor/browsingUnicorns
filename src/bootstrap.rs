//! Composition root: the only place that knows every layer.

use std::sync::Arc;
use std::sync::mpsc;

use anyhow::Context as _;
use application::ports::{Repository, WebEngine as _};
use application::{Browser, ExtensionManager, ProfileManager, Target, UserScriptManager};
use domain::{Extensions, ProfileId, Profiles, Settings, UserScripts};
use infrastructure::blocker::{BlockerHandle, BlockerLists, BlockerPolicy};
use infrastructure::extension_hub::Hub;
use infrastructure::extensions::{ExtensionFolder, UpdateClock};
use infrastructure::page_scripts::Bridge;
use infrastructure::profiles::{ProfileDirs, ProfileLock};
use infrastructure::script_storage::{ScriptStorage, new_token};
use infrastructure::user_scripts::{UserScriptFolder, UserScriptRepository};
use infrastructure::{
    DrmReset, EngineFiles, JsonRepository, MemoryRepository, NoPackages, ParentWindow,
    PrivateDataDir, TomlSettingsRepository, WryEngine,
};
use updater::{UpdateMemory, Updater};

use infrastructure::sync::SyncConfigFile;

use crate::Args;
use crate::ui::{
    AddOns, BlockerRunner, BrowserApp, DefaultBrowser, ExitSlot, ExtensionBar, ExtensionsRunner,
    ProfilesRunner, SyncRunner, Updates, UserScriptsRunner,
};

/// What a window runs: a private one leaves the profile alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setup {
    /// Reopens the profile's last tabs.
    pub restore_session: bool,
    /// Runs the profile's extensions and user scripts.
    pub extensions: bool,
    pub sync: bool,
    /// Checks for updates once a day.
    pub updates: bool,
    /// Downloads the ad blocker's lists (and writes their cache).
    pub blocker_refresh: bool,
}

pub fn private_setup(private: bool) -> Setup {
    let normal = !private;
    Setup {
        restore_session: normal,
        extensions: normal,
        sync: normal,
        updates: normal,
        blocker_refresh: normal,
    }
}

/// A tab moved to its own window: the profile's window, still open, keeps
/// the session, the sync, the updates and the ad blocker's lists.
pub const TAB_WINDOW: Setup = Setup {
    restore_session: false,
    extensions: true,
    sync: false,
    updates: false,
    blocker_refresh: false,
};

fn profile_of(args: &Args) -> ProfileId {
    match args.profile.as_deref().map(|p| (p, ProfileId::parse(p))) {
        Some((_, Some(id))) => id,
        Some((p, None)) => {
            log::warn!("--profile {p:?}: not a profile id, the default profile opens");
            ProfileId::default_profile()
        }
        None => ProfileId::default_profile(),
    }
}

/// `browsingUnicorns <address>` while the profile's window is open (a link
/// opened by the OS): the address goes to that window, no new one opens.
/// True when it went.
pub fn hand_over(args: &Args) -> bool {
    let Some(input) = args.start_page.as_deref() else {
        return false;
    };
    if args.private || args.tab_window {
        return false;
    }
    let Some(dirs) = ProfileDirs::default_location() else {
        return false;
    };
    if !infrastructure::profiles::hand_over(&dirs.dir(&profile_of(args)), input) {
        return false;
    }
    infrastructure::default_browser::let_open_window_come_forward();
    true
}

pub fn build(
    cc: &eframe::CreationContext<'_>,
    exit_slot: ExitSlot,
    args: Args,
) -> anyhow::Result<BrowserApp> {
    crate::ui::theme::install(&cc.egui_ctx);

    let dirs = ProfileDirs::default_location()
        .context("no data directory to keep history and bookmarks in")?;
    let current = profile_of(&args);
    let profile_dir = dirs.dir(&current);
    // What crashed private windows left behind.
    infrastructure::clean_leftovers();
    let setup = if args.tab_window && !args.private {
        TAB_WINDOW
    } else {
        private_setup(args.private)
    };
    // A private window writes nothing under the profile: what the engine
    // and the add-ons keep goes to a temporary folder, deleted at exit.
    let private_data = if args.private {
        Some(PrivateDataDir::create().context("no temporary folder for the private window")?)
    } else {
        None
    };
    let files_dir = private_data
        .as_ref()
        .map_or_else(|| profile_dir.clone(), |d| d.path().to_path_buf());
    // The normal window of the profile keeps its lock.
    let lock = if args.private || args.tab_window {
        None
    } else {
        let lock = ProfileLock::acquire(&profile_dir);
        if lock.is_none() {
            log::warn!("profile {current} is already open in another window");
        }
        lock
    };
    let holds_profile = lock.is_some();
    // The protected videos' data (WebView2's DRM files): reset by the
    // profile's window, before its engine opens them.
    let drm = (cfg!(windows) && holds_profile).then(|| DrmReset::new(&files_dir));
    match drm.as_ref().and_then(DrmReset::apply_if_requested) {
        Some(Ok(())) => log::info!("DRM data reset"),
        Some(Err(e)) => log::warn!("DRM data not reset, tried again next start: {e}"),
        None => {}
    }
    let profile_list: Box<dyn Repository<Profiles>> = if args.private {
        let list = dirs.list().load().context("could not read the profiles")?;
        Box::new(MemoryRepository::new(list))
    } else {
        Box::new(dirs.list())
    };
    let profiles = ProfileManager::load(profile_list, Box::new(dirs.clone()), current.clone())
        .context("could not read the profiles")?;
    let settings_repo: TomlSettingsRepository = dirs
        .settings(&current)
        .context("no config directory to keep the settings in")?;
    let settings = settings_repo.load().unwrap_or_else(|e| {
        log::warn!("settings ignored: {e}");
        Settings::default()
    });

    // Safety: the web views live in the app, which eframe drops before the
    // window.
    let parent = unsafe { ParentWindow::new(cc) }.context("no native window to embed pages in")?;
    parent.use_app_icon();
    let (sender, receiver) = mpsc::channel();
    let ctx = cc.egui_ctx.clone();
    let page_token = new_token();
    // The ad blocker: on every tab, its lists arriving off the UI thread.
    let blocker = BlockerHandle::default();
    blocker.set_policy(BlockerPolicy::from_settings(&settings));
    let engine = WryEngine::new(
        parent,
        EngineFiles {
            data_dir: files_dir.join("webview"),
            extensions_dir: files_dir.join("extensions"),
            user_scripts_dir: files_dir.join("userscripts"),
            state_dir: files_dir.join("extension-state"),
            page_token: page_token.clone(),
            private: args.private,
        },
        sender,
        Arc::new(move || ctx.request_repaint()),
        blocker.clone(),
    );

    let profile = if args.private {
        infrastructure::private_profile(&profile_dir, settings.clone())
    } else if args.tab_window {
        infrastructure::tab_window_profile(&profile_dir, settings_repo)
    } else {
        infrastructure::json_profile(&profile_dir, settings_repo)
    };
    let mut browser = Browser::new(engine, settings, profile);
    for e in browser.load_profile() {
        log::warn!("profile: {e}");
    }
    // Handed to the engine before the first tab: it installs them with it.
    // None in a private window: an empty list in memory, and no package
    // folder (loading prunes it).
    let extensions = if setup.extensions {
        ExtensionManager::load(
            Box::new(JsonRepository::new(profile_dir.join("extensions.json"))),
            Arc::new(ExtensionFolder::new(
                profile_dir.join("extensions"),
                infrastructure::extensions::ureq_download(),
            )),
        )
    } else {
        ExtensionManager::load(
            Box::new(MemoryRepository::<Extensions>::default()),
            Arc::new(NoPackages),
        )
    }
    .context("could not read the extensions")?;
    browser.engine_mut().set_extensions(extensions.extensions());
    let user_scripts = if setup.extensions {
        UserScriptManager::load(
            Box::new(UserScriptRepository::new(
                profile_dir.join("userscripts.json"),
                profile_dir.join("userscripts"),
            )),
            Arc::new(UserScriptFolder::new(
                profile_dir.join("userscripts"),
                infrastructure::extensions::ureq_download(),
            )),
        )
    } else {
        UserScriptManager::load(
            Box::new(MemoryRepository::<UserScripts>::default()),
            Arc::new(NoPackages),
        )
    }
    .context("could not read the user scripts")?;
    browser
        .engine_mut()
        .set_user_scripts(user_scripts.scripts());
    let script_storage = ScriptStorage::new(files_dir.join("script-storage"), page_token);
    // Before the first tab: its downloads, passwords and zoom follow them.
    let device = browser.settings().device.clone();
    browser.engine_mut().apply_device_settings(&device);
    if setup.restore_session {
        browser
            .restore_session()
            .context("could not open the first tab")?;
    }
    // `browsingUnicorns <address>`: from a terminal, or the OS opening a link.
    if let Some(input) = args.start_page
        && let Err(e) = browser.submit(&input, Target::NewTab)
    {
        log::warn!("start page {input:?}: {e}");
    }
    // A private window starts on the home page, unless given an address.
    if browser.session().is_empty() {
        browser
            .open_tab(None)
            .context("could not open the first tab")?;
    }

    let ctx = cc.egui_ctx.clone();
    let updates = Updates::new(
        Updater::new(&crate::APP, move || ctx.request_repaint()),
        setup.updates.then(|| UpdateMemory::new(&profile_dir)),
    );
    let ctx = cc.egui_ctx.clone();
    // Off in a private window: its folder has no sync configuration.
    let sync_dir = if setup.sync { &profile_dir } else { &files_dir };
    let sync = SyncRunner::new(SyncConfigFile::new(sync_dir.join("sync.json")), move || {
        ctx.request_repaint()
    });
    let ctx = cc.egui_ctx.clone();
    // Kept up to date from their stores (not from a private window).
    let update_clock = setup
        .extensions
        .then(|| UpdateClock::new(profile_dir.join("extensions").join(".last-update-check")));
    let extensions = ExtensionsRunner::new(extensions, update_clock, move || ctx.request_repaint());
    let ctx = cc.egui_ctx.clone();
    let user_scripts = UserScriptsRunner::new(user_scripts, move || ctx.request_repaint());
    let ctx = cc.egui_ctx.clone();
    let blocker = BlockerRunner::start(
        blocker,
        infrastructure::blocker_dir().map(|dir| {
            let lists = BlockerLists::new(dir, infrastructure::extensions::ureq_download());
            // WebKit blocks the requests with its own content rules: the
            // engine only hides ad slots and runs scriptlets there.
            if cfg!(any(target_os = "macos", target_os = "linux")) {
                lists.cosmetic_only()
            } else {
                lists
            }
        }),
        setup.blocker_refresh,
        move || ctx.request_repaint(),
    );
    // The profile's window takes the links later launches hand over.
    let default_browser = if holds_profile {
        let ctx = cc.egui_ctx.clone();
        DefaultBrowser::for_profile_window(&profile_dir, move || ctx.request_repaint())
    } else {
        DefaultBrowser::inert()
    };
    let mut hub = Hub::new(
        script_storage,
        Bridge::Ipc,
        files_dir.join("extensions"),
        files_dir.join("extension-state"),
    );
    let ctx = cc.egui_ctx.clone();
    hub.set_native_waker(move || ctx.request_repaint());
    hub.set_fetcher(infrastructure::extension_fetch::ureq_fetcher());
    hub.set_blocker(blocker.handle.clone());
    Ok(BrowserApp::new(
        browser,
        receiver,
        updates,
        sync,
        AddOns {
            extensions,
            bar: ExtensionBar::new(files_dir.join("extensions")),
            user_scripts,
            hub,
            blocker,
        },
        ProfilesRunner::new(profiles, lock),
        exit_slot,
        private_data,
        default_browser,
        drm,
        // The sites' icons, for the tabs asleep; none kept for a private one.
        (!args.private).then(|| profile_dir.join("favicons")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_window_runs_nothing_of_the_profile() {
        let normal = private_setup(false);
        assert!(normal.restore_session && normal.extensions && normal.sync);
        assert!(normal.updates && normal.blocker_refresh);
        assert_eq!(
            private_setup(true),
            Setup {
                restore_session: false,
                extensions: false,
                sync: false,
                updates: false,
                blocker_refresh: false,
            }
        );
    }
}
