//! Composition root: the only place that knows every layer.

use std::sync::Arc;
use std::sync::mpsc;

use anyhow::Context as _;
use application::ports::{Repository, WebEngine as _};
use application::{Browser, ExtensionManager, ProfileManager, Target, UserScriptManager};
use domain::{ProfileId, Settings};
use infrastructure::extension_hub::Hub;
use infrastructure::extensions::ExtensionFolder;
use infrastructure::page_scripts::Bridge;
use infrastructure::profiles::{ProfileDirs, ProfileLock};
use infrastructure::script_storage::{ScriptStorage, new_token};
use infrastructure::user_scripts::{UserScriptFolder, UserScriptRepository};
use infrastructure::{
    EngineFiles, JsonRepository, ParentWindow, TomlSettingsRepository, WryEngine,
};
use updater::{UpdateMemory, Updater};

use infrastructure::sync::SyncConfigFile;

use crate::Args;
use crate::ui::{
    AddOns, BrowserApp, ExitSlot, ExtensionBar, ExtensionsRunner, ProfilesRunner, SyncRunner,
    Updates, UserScriptsRunner,
};

pub fn build(
    cc: &eframe::CreationContext<'_>,
    exit_slot: ExitSlot,
    args: Args,
) -> anyhow::Result<BrowserApp> {
    crate::ui::theme::install(&cc.egui_ctx);

    let dirs = ProfileDirs::default_location()
        .context("no data directory to keep history and bookmarks in")?;
    let current = match args.profile.as_deref().map(|p| (p, ProfileId::parse(p))) {
        Some((_, Some(id))) => id,
        Some((p, None)) => {
            log::warn!("--profile {p:?}: not a profile id, the default profile opens");
            ProfileId::default_profile()
        }
        None => ProfileId::default_profile(),
    };
    let profile_dir = dirs.dir(&current);
    let lock = ProfileLock::acquire(&profile_dir);
    if lock.is_none() {
        log::warn!("profile {current} is already open in another window");
    }
    let profiles = ProfileManager::load(
        Box::new(dirs.list()),
        Box::new(dirs.clone()),
        current.clone(),
    )
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
    let (sender, receiver) = mpsc::channel();
    let ctx = cc.egui_ctx.clone();
    let page_token = new_token();
    let engine = WryEngine::new(
        parent,
        EngineFiles {
            data_dir: profile_dir.join("webview"),
            extensions_dir: profile_dir.join("extensions"),
            user_scripts_dir: profile_dir.join("userscripts"),
            state_dir: profile_dir.join("extension-state"),
            page_token: page_token.clone(),
        },
        sender,
        Arc::new(move || ctx.request_repaint()),
    );

    let mut browser = Browser::new(
        engine,
        settings,
        infrastructure::json_profile(&profile_dir, settings_repo),
    );
    for e in browser.load_profile() {
        log::warn!("profile: {e}");
    }
    // Handed to the engine before the first tab: it installs them with it.
    let extensions = ExtensionManager::load(
        Box::new(JsonRepository::new(profile_dir.join("extensions.json"))),
        Arc::new(ExtensionFolder::new(
            profile_dir.join("extensions"),
            infrastructure::extensions::ureq_download(),
        )),
    )
    .context("could not read the extensions")?;
    browser.engine_mut().set_extensions(extensions.extensions());
    let user_scripts = UserScriptManager::load(
        Box::new(UserScriptRepository::new(
            profile_dir.join("userscripts.json"),
            profile_dir.join("userscripts"),
        )),
        Arc::new(UserScriptFolder::new(
            profile_dir.join("userscripts"),
            infrastructure::extensions::ureq_download(),
        )),
    )
    .context("could not read the user scripts")?;
    browser
        .engine_mut()
        .set_user_scripts(user_scripts.scripts());
    let script_storage = ScriptStorage::new(profile_dir.join("script-storage"), page_token);
    browser
        .restore_session()
        .context("could not open the first tab")?;
    // `browsingUnicorns <address>`: from a terminal, or the OS opening a link.
    if let Some(input) = args.start_page
        && let Err(e) = browser.submit(&input, Target::NewTab)
    {
        log::warn!("start page {input:?}: {e}");
    }

    let ctx = cc.egui_ctx.clone();
    let updates = Updates::new(
        Updater::new(&crate::APP, move || ctx.request_repaint()),
        UpdateMemory::new(&profile_dir),
    );
    let ctx = cc.egui_ctx.clone();
    let sync = SyncRunner::new(
        SyncConfigFile::new(profile_dir.join("sync.json")),
        move || ctx.request_repaint(),
    );
    let ctx = cc.egui_ctx.clone();
    let extensions = ExtensionsRunner::new(extensions, move || ctx.request_repaint());
    let ctx = cc.egui_ctx.clone();
    let user_scripts = UserScriptsRunner::new(user_scripts, move || ctx.request_repaint());
    let mut hub = Hub::new(
        script_storage,
        Bridge::Ipc,
        profile_dir.join("extensions"),
        profile_dir.join("extension-state"),
    );
    let ctx = cc.egui_ctx.clone();
    hub.set_native_waker(move || ctx.request_repaint());
    hub.set_fetcher(infrastructure::extension_fetch::ureq_fetcher());
    Ok(BrowserApp::new(
        browser,
        receiver,
        updates,
        sync,
        AddOns {
            extensions,
            bar: ExtensionBar::new(profile_dir.join("extensions")),
            user_scripts,
            hub,
        },
        ProfilesRunner::new(profiles, lock),
        exit_slot,
    ))
}
