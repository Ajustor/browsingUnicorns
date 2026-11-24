use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use application::Browser;
use application::ports::{EngineEvent, PageSource, Viewport, WebEngine as _};
use domain::TabId;
use egui::{CentralPanel, Frame, TopBottomPanel, ViewportCommand};
use infrastructure::{PrivateDataDir, WryEngine};
use updater::ExitAction;

use super::blocker::BlockerRunner;
use super::command::Command;
use super::extension_bar::{DEFAULT_POPUP, ExtensionBar};
use super::extensions::{ExtensionsRunner, ExtensionsView};
use super::library::{Library, LibraryData, LibraryView};
use super::omnibox::Omnibox;
use super::page_holes::{floating_layers, page_holes, page_viewport};
use super::profiles::{ProfileCommand, ProfilesRunner};
use super::shield::{Shield, ShieldView};
use super::site_info::SiteInfo;
use super::sync::SyncRunner;
use super::sync_panel::SyncView;
use super::toolbar::ShieldButton;
use super::updates::{Notice, Updates};
use super::user_scripts::UserScriptsRunner;
use super::{APP_NAME, bookmarks_bar, tab_strip, toolbar};
use infrastructure::extension_hub::{BrowserHub, Hub};

/// What `main` runs once the window is gone (apply an update).
pub type ExitSlot = Arc<Mutex<Option<ExitAction>>>;

pub struct BrowserApp {
    browser: Browser<WryEngine>,
    /// The tab whose page shows an element in full screen (a video).
    page_fullscreen: Option<TabId>,
    /// The tabs' page icons.
    favicons: super::favicons::Favicons,
    /// The window was put in full screen for it: whether it was maximized
    /// before, to be again after.
    window_fullscreen: Option<bool>,
    events: Receiver<EngineEvent>,
    updates: Updates,
    sync: SyncRunner,
    extensions: ExtensionsRunner,
    user_scripts: UserScriptsRunner,
    hub: Hub,
    bar: ExtensionBar,
    blocker: BlockerRunner,
    profiles: ProfilesRunner,
    exit_slot: ExitSlot,
    omnibox: Omnibox,
    library: Library,
    notice: Option<Notice>,
    downloads: super::downloads::Downloads,
    default_browser: super::default_browser::DefaultBrowser,
    /// The padlock's bar, when open.
    site_info: Option<SiteInfo>,
    /// The ad blocker's bar, when open.
    shield: Option<Shield>,
    window_title: String,
    /// The language chosen on this computer (the Settings view's).
    language: domain::LanguageChoice,
    last_persist: Instant,
    close_requested: bool,
    /// The DRM data this window can reset (Windows, the profile's window).
    drm: Option<infrastructure::DrmReset>,
    /// Started again once closed (a DRM reset).
    relaunch: bool,
    /// A private window's web view data. Last: dropped (deleted) after the
    /// web views.
    private_data: Option<PrivateDataDir>,
}

/// Extensions, user scripts and the storage of their page scripts.
pub struct AddOns {
    pub extensions: ExtensionsRunner,
    /// The extensions' toolbar buttons and popups.
    pub bar: ExtensionBar,
    pub user_scripts: UserScriptsRunner,
    /// The browser side of the extension APIs (storage, messages, tabs...).
    pub hub: Hub,
    /// The built-in ad blocker's lists and policy.
    pub blocker: BlockerRunner,
}

/// History and session are written at most this often (and on exit).
const PERSIST_EVERY: Duration = Duration::from_secs(5);

impl BrowserApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        browser: Browser<WryEngine>,
        events: Receiver<EngineEvent>,
        mut updates: Updates,
        sync: SyncRunner,
        add_ons: AddOns,
        profiles: ProfilesRunner,
        exit_slot: ExitSlot,
        private_data: Option<PrivateDataDir>,
        default_browser: super::default_browser::DefaultBrowser,
        drm: Option<infrastructure::DrmReset>,
    ) -> Self {
        let AddOns {
            extensions,
            bar,
            user_scripts,
            hub,
            blocker,
        } = add_ons;
        updates.check_if_due();
        let mut browser = browser;
        browser
            .engine_mut()
            .set_shortcuts(super::shortcuts::page_keys());
        Self {
            browser,
            events,
            updates,
            sync,
            extensions,
            user_scripts,
            hub,
            bar,
            blocker,
            profiles,
            exit_slot,
            omnibox: Omnibox::default(),
            page_fullscreen: None,
            favicons: Default::default(),
            window_fullscreen: None,
            library: Library::default(),
            notice: None,
            downloads: Default::default(),
            default_browser,
            site_info: None,
            shield: None,
            window_title: String::new(),
            language: super::device_language(),
            last_persist: Instant::now(),
            close_requested: false,
            drm,
            relaunch: false,
            private_data,
        }
    }

    /// A private window: nothing written to the profile.
    fn is_private(&self) -> bool {
        self.private_data.is_some()
    }

    fn report(&mut self, error: impl std::fmt::Display) {
        log::error!("{error}");
        self.notice = Some(Notice {
            error: true,
            text: error.to_string(),
        });
    }

    fn run(&mut self, command: Command) {
        if self.is_private() && writes_profile(&command) {
            self.notice = Some(Notice {
                error: false,
                text: i18n::errors::infrastructure::not_in_private_window().into(),
            });
            return;
        }
        let result = match command {
            Command::NewTab => self.browser.open_tab(None).map(|_| {
                self.browser.engine().focus_window();
                self.omnibox.request_focus();
            }),
            Command::ReopenClosedTab => self.browser.reopen_closed_tab().map(drop),
            Command::UpdateSettings(settings) => {
                // Only this device's own: nothing for the other devices.
                let mut shared = (*settings).clone();
                shared.device = self.browser.settings().device.clone();
                let r = if shared == *self.browser.settings() {
                    self.browser.update_device_settings(settings.device)
                } else {
                    self.browser.update_settings(*settings)
                };
                let device = self.browser.settings().device.clone();
                self.browser.engine_mut().apply_device_settings(&device);
                r
            }
            Command::AnswerCredential(answer) => self.browser.answer_credential(answer),
            Command::RemoveCredential(origin, username) => {
                self.browser.remove_credential(&origin, &username)
            }
            Command::ClearBrowsingData(what) => self.clear_browsing_data(what),
            Command::ResetDrm => {
                let Some(drm) = &self.drm else { return };
                match drm.request() {
                    Ok(()) => {
                        self.relaunch = true;
                        self.close_requested = true;
                    }
                    Err(e) => self.report(i18n::notices::drm_reset_failed(&e.to_string())),
                }
                return;
            }
            Command::Import(from) => {
                let r = self.import(from);
                return self.report_err(r);
            }
            Command::MoveBookmark(address, folder) => self.browser.move_bookmark(&address, &folder),
            Command::Home => self.browser.go_home(),
            Command::Submit(input) => self.browser.submit_omnibox(&input).map(|_| ()),
            Command::Open(address, target) => self.browser.load(address, target),
            Command::ToggleBookmark => self.browser.toggle_bookmark().map(|_| ()),
            Command::RemoveBookmark(address) => self.browser.remove_bookmark(&address),
            Command::ClearHistory => self.browser.clear_history(),
            Command::SyncTurnOn => {
                let r = self.sync.turn_on();
                return self.report_err(r);
            }
            Command::SyncJoin(code) => {
                let r = self.sync.join(&code);
                return self.report_err(r);
            }
            Command::SyncTurnOff => {
                let r = self.sync.turn_off();
                return self.report_err(r);
            }
            Command::SyncNow => {
                self.sync.sync_now();
                Ok(())
            }
            Command::InstallExtension(input) => {
                self.extensions.install(&input);
                Ok(())
            }
            Command::SetExtensionEnabled(id, enabled) => {
                let r = self
                    .extensions
                    .set_enabled(&id, enabled, self.browser.engine_mut());
                return self.report_err(r);
            }
            Command::RemoveExtension(id) => {
                let r = self.extensions.remove(&id, self.browser.engine_mut());
                self.hub.forget(&format!("ext:{id}"));
                return self.report_err(r);
            }
            Command::ToggleExtensionPopup { id, anchor } => {
                // Served by the browser: only where it runs the extensions.
                let url = self
                    .extensions
                    .extensions()
                    .get(&id)
                    .and_then(|e| e.popup.as_deref())
                    .and_then(|page| self.browser.engine().extension_url(&id, page));
                if let Some(url) = url {
                    let tab = self.browser.session().active_id();
                    self.bar.toggle(id, url, anchor, tab);
                }
                Ok(())
            }
            Command::Profile(command) => {
                let rename = matches!(command, ProfileCommand::Rename(..));
                let r = self.profiles.run(command);
                if rename {
                    self.library.profiles.reset_name();
                }
                return self.report_err(r);
            }
            Command::InstallUserScript(url) => {
                self.user_scripts.install(&url);
                Ok(())
            }
            Command::SetUserScriptEnabled(id, enabled) => {
                let r = self
                    .user_scripts
                    .set_enabled(&id, enabled, self.browser.engine_mut());
                return self.report_err(r);
            }
            Command::RemoveUserScript(id) => {
                let r = self.user_scripts.remove(&id, self.browser.engine_mut());
                self.hub.forget(&format!("us:{id}"));
                return self.report_err(r);
            }
            Command::SetBlockerForSite(host, on) => {
                let mut settings = self.browser.settings().clone();
                settings.set_blocks_on(&host, on);
                self.change_blocker(settings)
            }
            Command::SetBlocker(on) => {
                let mut settings = self.browser.settings().clone();
                settings.blocker = on;
                self.change_blocker(settings)
            }
            Command::DetachTab(id, geometry) => {
                let r = self.detach_tab(id, geometry);
                return self.report_err(r);
            }
            Command::DefaultBrowser(command) => {
                let r = self
                    .default_browser
                    .run(command)
                    .map_err(|e| i18n::notices::default_browser_failed(&e));
                return self.report_err(r);
            }
            Command::OpenPrivateWindow => {
                let r = infrastructure::profiles::open_private_window(&self.profiles.current().id)
                    .map_err(|e| i18n::notices::private_window_failed(&e));
                return self.report_err(r);
            }
            other => {
                self.run_ui(other);
                Ok(())
            }
        };
        if let Err(e) = result {
            self.report(e);
        }
    }

    /// Imports what `from` holds (another browser's bookmarks and history,
    /// a bookmarks or passwords file); says how much came.
    fn import(&mut self, from: super::command::ImportFrom) -> Result<(), String> {
        use super::command::ImportFrom;
        use infrastructure::browser_import;
        let read = |file: &std::path::Path| {
            std::fs::read_to_string(file)
                .map_err(|e| i18n::notices::read_failed(&file.display(), &e))
        };
        // What could not be read, told after what came.
        let mut errors = Vec::new();
        let text = match from {
            ImportFrom::Browser(source) => {
                let added = match browser_import::read_bookmarks(&source) {
                    Ok(found) => self.add_bookmarks(source.name, found)?,
                    Err(e) => {
                        errors.push(e);
                        0
                    }
                };
                let pages = match browser_import::read_history(&source) {
                    Ok(found) => {
                        let visits = found.into_iter().filter_map(|v| {
                            Some(domain::Visit {
                                address: domain::WebAddress::parse(&v.url).ok()?,
                                title: v.title,
                                at: v.at,
                            })
                        });
                        self.browser
                            .import_history(visits)
                            .map_err(|e| e.to_string())?
                    }
                    Err(e) => {
                        errors.push(e);
                        0
                    }
                };
                if errors.len() == 2 {
                    return Err(errors.join(" "));
                }
                i18n::notices::browser_imported(added, pages, source.name)
            }
            ImportFrom::Html(file) => {
                let found = browser_import::parse_html(&read(&file)?);
                let from = i18n::notices::html_file();
                let added = self.add_bookmarks(from, found)?;
                i18n::notices::bookmarks_imported(added, from)
            }
            ImportFrom::Passwords(file) => {
                let found = browser_import::parse_passwords_csv(&read(&file)?)?;
                let items = found.into_iter().filter_map(|l| {
                    Some((
                        domain::WebAddress::parse(&l.url).ok()?,
                        l.username,
                        l.password,
                    ))
                });
                let added = self
                    .browser
                    .import_credentials(items)
                    .map_err(|e| e.to_string())?;
                i18n::notices::passwords_imported(added)
            }
        };
        self.notice = Some(Notice {
            error: !errors.is_empty(),
            text: std::iter::once(text)
                .chain(errors)
                .collect::<Vec<_>>()
                .join(" "),
        });
        Ok(())
    }

    /// Adds bookmarks `from` another browser in a folder named after it.
    fn add_bookmarks(
        &mut self,
        from: &str,
        found: Vec<infrastructure::browser_import::Imported>,
    ) -> Result<usize, String> {
        let items = found.into_iter().filter_map(|b| {
            let address = domain::WebAddress::parse(&b.url).ok()?;
            Some((address, b.title, b.folder))
        });
        self.browser
            .import_bookmarks(&i18n::notices::imported_folder(from), items)
            .map_err(|e| e.to_string())
    }

    /// Removes what was asked, and says what it did.
    fn clear_browsing_data(
        &mut self,
        what: super::command::ClearData,
    ) -> Result<(), application::BrowserError> {
        let mut done = Vec::new();
        if what.history {
            self.browser.clear_history()?;
            done.push(i18n::notices::cleared_history());
        }
        if what.cookies || what.cache {
            self.browser
                .engine_mut()
                .clear_site_data(what.cookies, what.cache);
            if what.cookies {
                done.push(i18n::notices::cleared_cookies());
            }
            if what.cache {
                done.push(i18n::notices::cleared_cache());
            }
        }
        self.library.settings.cleared = Some(i18n::notices::cleared(&done.join(", ")));
        Ok(())
    }

    /// Opens tab `id`'s page in a window of its own and closes the tab. Not
    /// in a private window (a new one would not share its cookies), nor for
    /// the last tab.
    fn detach_tab(&mut self, id: TabId, geometry: [f32; 4]) -> Result<(), String> {
        let session = self.browser.session();
        if self.is_private() || session.len() < 2 {
            return Ok(());
        }
        let Some(address) = session.tab(id).and_then(|t| t.address()) else {
            return Ok(());
        };
        infrastructure::profiles::open_tab_window(
            &self.profiles.current().id,
            address.as_str(),
            geometry,
        )
        .map_err(|e| i18n::notices::new_window_failed(&e))?;
        self.browser.close_tab(id);
        Ok(())
    }

    fn report_err(&mut self, result: Result<(), String>) {
        if let Err(e) = result {
            self.report(e);
        }
    }

    /// Applies a finished round, and starts one when due.
    fn drive_sync(&mut self, ctx: &egui::Context) {
        if let Some(records) = self.sync.poll()
            && let Err(e) = self.browser.apply_remote(&records)
        {
            self.report(e);
        }
        if self.sync.is_due(self.browser.has_unsynced_changes())
            && let Some(config) = self.sync.config()
        {
            let (device, name) = (config.device.clone(), config.device_name.clone());
            let record = self.browser.sync_record(device, &name);
            self.sync.start(record);
        }
        if let Some(wait) = self.sync.next_check() {
            ctx.request_repaint_after(wait);
        }
    }

    /// Commands that cannot fail.
    fn run_ui(&mut self, command: Command) {
        match command {
            Command::CloseTab(id) => self.browser.close_tab(id),
            Command::CloseActiveTab => self.browser.close_active_tab(),
            Command::Activate(id) => self.browser.activate(id),
            Command::MoveTab(id, to) => self.browser.move_tab(id, to),
            Command::CycleTabs(offset) => self.browser.cycle_tabs(offset),
            Command::Back => self.browser.go_back(),
            Command::Forward => self.browser.go_forward(),
            Command::Reload => self.browser.reload(),
            Command::HardReload => self.browser.reload_bypassing_cache(),
            Command::FocusOmnibox => {
                self.browser.engine().focus_window();
                self.omnibox.request_focus();
            }
            Command::OpenDevTools => {
                if let Some(tab) = self.browser.session().active_id() {
                    self.browser
                        .engine_mut()
                        .open_devtools(&PageSource::Tab(tab));
                }
            }
            Command::InspectExtension(id) => self
                .browser
                .engine_mut()
                .open_devtools(&PageSource::Background(id)),
            Command::ToggleLibrary(view) => self.library.toggle(view),
            Command::ToggleSiteInfo => self.toggle_site_info(),
            Command::ToggleDownloads => self.downloads.shown = !self.downloads.shown,
            Command::ToggleShield => self.toggle_shield(),
            Command::CheckForUpdates => self.updates.check_now(),
            Command::InstallUpdate => self.updates.updater.install(),
            Command::UpdateLater => self.updates.updater.later(),
            Command::SkipUpdate => self.updates.skip(),
            Command::RestartToUpdate => {
                self.updates.updater.schedule_restart();
                self.close_requested = true;
            }
            Command::SetMuted(tab, muted) => self.browser.engine_mut().set_muted(tab, muted),
            Command::ShowBookmarkFolder(folder) => self.library.show_folder(folder),
            Command::ResetZoom => {
                let host = self
                    .browser
                    .session()
                    .active()
                    .and_then(|t| t.address())
                    .and_then(|a| a.host().map(str::to_owned));
                let mut device = self.browser.settings().device.clone();
                if let Some(host) = host
                    && device.set_zoom(&host, 100)
                {
                    if let Err(e) = self.browser.update_device_settings(device.clone()) {
                        self.report(e);
                    }
                    self.browser.engine_mut().apply_device_settings(&device);
                }
            }
            Command::SetLanguage(choice) => {
                self.language = choice;
                if let Err(e) = super::language::choose_language(choice) {
                    self.report(e);
                }
            }
            Command::ApplyZoom => {
                let device = self.browser.settings().device.clone();
                self.browser.engine_mut().apply_device_settings(&device);
            }
            Command::NewTab
            | Command::ReopenClosedTab
            | Command::UpdateSettings(_)
            | Command::ClearBrowsingData(_)
            | Command::ResetDrm
            | Command::Import(_)
            | Command::MoveBookmark(..)
            | Command::Home
            | Command::Submit(_)
            | Command::Open(..)
            | Command::ToggleBookmark
            | Command::RemoveBookmark(_)
            | Command::ClearHistory
            | Command::SyncTurnOn
            | Command::SyncJoin(_)
            | Command::SyncTurnOff
            | Command::SyncNow
            | Command::InstallExtension(_)
            | Command::SetExtensionEnabled(..)
            | Command::RemoveExtension(_)
            | Command::InstallUserScript(_)
            | Command::SetUserScriptEnabled(..)
            | Command::RemoveUserScript(_)
            | Command::SetBlockerForSite(..)
            | Command::SetBlocker(_)
            | Command::Profile(_)
            | Command::OpenPrivateWindow
            | Command::DetachTab(..)
            | Command::DefaultBrowser(_)
            | Command::AnswerCredential(_)
            | Command::RemoveCredential(..)
            | Command::ToggleExtensionPopup { .. } => unreachable!("handled by run"),
        }
    }

    /// Keeps the keyboard where the user clicked last. The page is a native
    /// child view: a click on the window's own widgets (the address bar)
    /// doesn't take the keyboard back from it, and a click in the page
    /// leaves the widget looking focused.
    fn sync_keyboard_focus(&mut self, ctx: &egui::Context) {
        if self.browser.engine_mut().take_page_focused() {
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
                // A click on the page closes an open list (shown over the
                // page, egui never sees that click).
                m.close_popup();
            });
        }
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.browser.engine().focus_window();
        }
    }

    /// Saves the ad blocker's new settings, hands them to the hooks at
    /// once, and reloads the page to apply them.
    fn change_blocker(
        &mut self,
        settings: domain::Settings,
    ) -> Result<(), application::BrowserError> {
        self.browser.update_settings(settings)?;
        self.blocker.follow_settings(self.browser.settings());
        self.browser.reload();
        Ok(())
    }

    /// Opens the ad blocker's bar on the active site, or closes it (the
    /// padlock's bar closes).
    fn toggle_shield(&mut self) {
        if self.shield.take().is_some() {
            return;
        }
        let Some(address) = self.browser.session().active().and_then(|t| t.address()) else {
            return;
        };
        self.shield = Some(Shield {
            address: address.clone(),
        });
        self.site_info = None;
    }

    /// Shows the ad blocker's bar while the active page stays the same.
    fn shield_bar(&mut self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        let Some(shield) = &self.shield else {
            return;
        };
        let active = self.browser.session().active();
        if active.and_then(|t| t.address()) != Some(&shield.address) {
            self.shield = None;
            return;
        }
        let settings = self.browser.settings();
        let view = ShieldView {
            enabled: settings.blocker,
            on_site: settings.blocks_on(shield.address.host().unwrap_or("")),
            blocked: active.and_then(|t| self.browser.engine().blocked_count(t.id())),
            updated: self.blocker.handle.get().map(|b| b.updated()),
        };
        if !shield.show(ctx, &view, commands) {
            self.shield = None;
        }
    }

    /// The toolbar's shield for the active page.
    fn shield_button(&self) -> ShieldButton {
        let active = self.browser.session().active();
        let host = active.and_then(|t| t.address()).and_then(|a| a.host());
        ShieldButton {
            on: host.is_some_and(|h| self.browser.settings().blocks_on(h)),
            blocked: active.and_then(|t| self.browser.engine().blocked_count(t.id())),
        }
    }

    /// Opens the padlock's bar on the active site (asking its certificate),
    /// or closes it (the shield's bar closes).
    fn toggle_site_info(&mut self) {
        if self.site_info.take().is_some() {
            return;
        }
        self.shield = None;
        let Some(tab) = self.browser.session().active() else {
            return;
        };
        let Some(address) = tab.address().cloned() else {
            return;
        };
        self.site_info = Some(SiteInfo {
            address,
            certificates: None,
        });
    }

    /// Shows the padlock's bar while the active page stays the same.
    fn site_info_bar(&mut self, ctx: &egui::Context) {
        let Some(info) = &mut self.site_info else {
            return;
        };
        let active = self.browser.session().active();
        if active.and_then(|t| t.address()) != Some(&info.address) {
            self.site_info = None;
            return;
        }
        if info.address.is_secure() {
            info.certificates = active.and_then(|t| self.browser.engine().site_security(t.id()));
        }
        if !info.show(ctx) {
            self.site_info = None;
        }
    }

    /// Lays the extension popup over the page, under its button; closes it
    /// when it lost the focus or another tab shows.
    fn place_popup(&mut self, ctx: &egui::Context) {
        if self.hub.take_panel_blur() {
            self.bar.closed_by_page();
        }
        let active = self.browser.session().active_id();
        if self.bar.popup.as_ref().is_some_and(|p| p.tab != active) {
            self.bar.popup = None;
        }
        let Some(popup) = &self.bar.popup else {
            self.browser
                .engine_mut()
                .show_panel_page(None, Viewport::default());
            self.hub.panel_closed();
            return;
        };
        let ppp = ctx.pixels_per_point();
        let (width, height) = self.browser.engine().panel_size().unwrap_or((
            (DEFAULT_POPUP.x * ppp) as u32,
            (DEFAULT_POPUP.y * ppp) as u32,
        ));
        let screen = ctx.screen_rect();
        let right = (popup.anchor.right() * ppp).min(screen.right() * ppp);
        let area = Viewport {
            x: (right - width as f32).max(0.0).round() as i32,
            y: ((popup.anchor.bottom() + 2.0) * ppp).round() as i32,
            width,
            height,
        };
        let url = popup.url.clone();
        self.browser.engine_mut().show_panel_page(Some(&url), area);
    }

    fn persist(&mut self) {
        self.last_persist = Instant::now();
        if let Err(e) = self.browser.persist() {
            self.report(e);
        }
    }

    fn drain_engine_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if let EngineEvent::PageMessage { from, body } = &event {
                // The password manager's page script: a sign-in was sent.
                if let Some((username, password)) =
                    infrastructure::page_scripts::credentials::submitted(body)
                {
                    if let PageSource::Tab(tab) = from
                        && !self.is_private()
                    {
                        self.browser.offer_credential(*tab, &username, &password);
                    }
                    continue;
                }
                let mut host = BrowserHub {
                    browser: &mut self.browser,
                    extensions: self.extensions.extensions(),
                };
                self.hub.handle(from, body, &mut host);
                continue;
            }
            if self.downloads.handle(&event) {
                continue;
            }
            // A store's "Add to Chrome": installed here, followed in the
            // Extensions panel.
            if let EngineEvent::ExtensionOffered { id } = event {
                if !self.is_private() {
                    self.library.open = Some(LibraryView::Extensions);
                }
                self.run(Command::InstallExtension(id.to_string()));
                continue;
            }
            if let EngineEvent::FaviconChanged { tab, png } = event {
                self.favicons.changed(tab, png);
                continue;
            }
            if let EngineEvent::AudioChanged {
                tab,
                playing,
                muted,
            } = event
            {
                self.favicons.audio(tab, playing, muted);
                continue;
            }
            if let EngineEvent::FullScreenChanged { tab, on } = event {
                if on {
                    self.page_fullscreen = Some(tab);
                } else if self.page_fullscreen == Some(tab) {
                    self.page_fullscreen = None;
                }
                continue;
            }
            if let Err(e) = self.browser.handle(event) {
                self.report(e);
            }
        }
    }

    /// Links the OS opened with the browser while this window was open:
    /// each in a new tab, the window brought forward.
    fn open_handed_over(&mut self, ctx: &egui::Context) {
        let requests = self.default_browser.take_requests();
        if requests.is_empty() {
            return;
        }
        for input in requests {
            if let Err(e) = self.browser.submit(&input, application::Target::NewTab) {
                self.report(e);
            }
        }
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let app = if self.profiles.several() {
            format!("{APP_NAME} ({})", self.profiles.current().name)
        } else {
            APP_NAME.to_owned()
        };
        let mut title = match self.browser.session().active() {
            Some(tab) => format!("{} — {app}", tab.display_title()),
            None => app,
        };
        if self.is_private() {
            title.push_str(i18n::window::private_suffix());
        }
        if title != self.window_title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn notice_bar(&mut self, ctx: &egui::Context) {
        let Some(notice) = &self.notice else {
            return;
        };
        let mut dismissed = false;
        TopBottomPanel::bottom("notice").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if notice.error {
                    ui.colored_label(ui.visuals().error_fg_color, &notice.text);
                } else {
                    ui.label(&notice.text);
                }
                dismissed |= ui.small_button(i18n::common::ok()).clicked();
            });
        });
        if dismissed {
            self.notice = None;
        }
    }
}

/// What a private window refuses: it would write to the profile.
fn writes_profile(command: &Command) -> bool {
    matches!(
        command,
        Command::ToggleBookmark
            | Command::RemoveBookmark(_)
            | Command::Import(_)
            | Command::MoveBookmark(..)
            | Command::SyncTurnOn
            | Command::SyncJoin(_)
            | Command::SyncTurnOff
            | Command::SyncNow
            | Command::InstallExtension(_)
            | Command::SetExtensionEnabled(..)
            | Command::RemoveExtension(_)
            | Command::InstallUserScript(_)
            | Command::SetUserScriptEnabled(..)
            | Command::RemoveUserScript(_)
            | Command::Profile(_)
            | Command::AnswerCredential(_)
            | Command::RemoveCredential(..)
    )
}

impl BrowserApp {
    /// The tab strip, the toolbar, the bars and the side panel.
    fn show_chrome(&mut self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        let chrome = super::theme::Chrome::of(self.is_private());
        TopBottomPanel::top("tabs")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: tab_strip::TOP_MARGIN,
                bottom: 0,
            }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                super::theme::gradient(ui.painter(), tab_strip::area(ui), &chrome);
                self.favicons.update(ui.ctx(), self.browser.session());
                tab_strip::show(
                    ui,
                    self.browser.session(),
                    &self.favicons,
                    &chrome,
                    self.is_private(),
                    commands,
                );
            });
        let actions = if infrastructure::builtin_extensions() {
            self.bar.actions(ctx, self.extensions.extensions())
        } else {
            Vec::new()
        };
        let shield = self.shield_button();
        let downloads_tip = super::downloads::toolbar_tip(&self.downloads);
        let downloads_running = self.downloads.running() > 0;
        // The active site's zoom, when not 100%.
        let zoom = self
            .browser
            .session()
            .active()
            .and_then(|t| t.address())
            .and_then(|a| a.host())
            .map(|h| self.browser.settings().device.zoom_of(h))
            .filter(|&z| z != 100);
        TopBottomPanel::top("toolbar")
            .frame(super::theme::chrome_frame(&chrome))
            .show_separator_line(false)
            .show(ctx, |ui| {
                super::theme::chrome_style(ui, &chrome);
                toolbar::show(
                    ui,
                    self.browser.session().active(),
                    self.browser.is_active_bookmarked(),
                    &shield,
                    &mut self.omnibox,
                    &actions,
                    self.profiles.current(),
                    downloads_tip.as_deref().map(|tip| (tip, downloads_running)),
                    zoom,
                    commands,
                );
            });
        if let Some(query) = self.omnibox.stale_query().map(str::to_owned) {
            let found = self.browser.suggest(&query, super::omnibox::SUGGESTIONS);
            self.omnibox.set_suggestions(query, found);
        }
        self.omnibox.show_suggestions(ctx, &chrome, commands);
        self.site_info_bar(ctx);
        self.shield_bar(ctx, commands);
        if !self.browser.bookmarks().is_empty() {
            TopBottomPanel::top("bookmarks")
                .frame(
                    super::theme::chrome_frame(&chrome)
                        .inner_margin(egui::Margin::symmetric(10, 3)),
                )
                .show_separator_line(false)
                .show(ctx, |ui| {
                    super::theme::chrome_style(ui, &chrome);
                    bookmarks_bar::show(ui, self.browser.bookmarks(), commands);
                    let r = ui.max_rect();
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() + 3.0,
                        egui::Stroke::new(super::theme::HAIRLINE, chrome.hover),
                    );
                });
        }
        self.updates.banner(ctx, commands);
        self.default_browser.banner(ctx, commands);
        super::passwords::banner(ctx, self.browser.pending_credential(), commands);
        if self.library.open == Some(LibraryView::Profiles) && !self.is_private() {
            self.profiles.refresh(ctx);
        }
        let can_cancel = self.browser.engine().can_cancel_downloads();
        for action in self.downloads.show(ctx, can_cancel, commands) {
            match action {
                super::downloads::Action::Cancel(file) => {
                    self.browser.engine_mut().cancel_download(&file);
                }
            }
        }
        self.notice_bar(ctx);
        self.library.show(
            ctx,
            LibraryData {
                history: self.browser.history(),
                bookmarks: self.browser.bookmarks(),
                now: self.browser.now(),
                updater: &self.updates.updater,
                sync: SyncView {
                    config: self.sync.config(),
                    state: &self.sync.state,
                    devices: self.browser.remote_devices(),
                },
                extensions: ExtensionsView {
                    support: self.browser.engine().extension_support(),
                    origins: infrastructure::builtin_extensions()
                        .then(infrastructure::extension_server::Origins::desktop),
                    runner: &self.extensions,
                    engine: self.browser.engine(),
                    user_scripts: &self.user_scripts,
                },
                profiles: &self.profiles,
                private: self.is_private(),
                default_browser: if self.library.open == Some(LibraryView::About) {
                    self.default_browser.status()
                } else {
                    None
                },
                settings: super::settings_panel::SettingsView {
                    settings: self.browser.settings(),
                    language: self.language,
                    passwords: self.browser.engine().can_save_passwords(),
                    credentials: self.browser.credentials(),
                    clear_site_data: self.browser.engine().can_clear_site_data(),
                    reset_drm: self.drm.is_some(),
                    default_download_dir: infrastructure::downloads_dir(),
                },
            },
            commands,
        );
    }
}

impl eframe::App for BrowserApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.browser.engine_mut().pump();
        self.drain_engine_events();
        self.open_handed_over(ctx);
        if self.downloads.running() > 0 {
            let progress = self.browser.engine_mut().downloads();
            self.downloads.progress(&progress);
        }
        self.drive_sync(ctx);
        self.blocker.follow(ctx, self.browser.settings());
        self.extensions.poll(self.browser.engine_mut());
        self.user_scripts.poll(self.browser.engine_mut());
        for id in self.browser.engine_mut().take_stopped_backgrounds() {
            self.hub.background_closed(&id);
        }
        let mut host = BrowserHub {
            browser: &mut self.browser,
            extensions: self.extensions.extensions(),
        };
        self.hub.tabs_changed(&mut host);
        self.hub.pump_native(&mut host);
        if self.hub.waiting() {
            // Unanswered extension messages expire: look again shortly.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        if let Some(notice) = self.updates.poll() {
            self.notice = Some(notice);
        }

        self.sync_keyboard_focus(ctx);
        let mut commands = Vec::new();
        super::shortcuts::collect(ctx, &mut commands);
        for press in self.browser.engine_mut().take_key_presses() {
            commands.extend(super::shortcuts::from_page(&press));
        }

        // A video in full screen: the page alone, over the whole screen.
        let fullscreen = self.page_fullscreen.is_some()
            && self.page_fullscreen == self.browser.session().active_id();
        // The window follows: full screen with the page, back after (only
        // when the page asks: the user's own full screen stays).
        if fullscreen != self.window_fullscreen.is_some() {
            let maximized = match self.window_fullscreen {
                Some(maximized) => maximized,
                None => ctx.input(|i| i.viewport().maximized == Some(true)),
            };
            self.window_fullscreen = fullscreen.then_some(maximized);
            for command in super::window_frame::fullscreen_commands(fullscreen, maximized) {
                ctx.send_viewport_cmd(command);
            }
        }
        if !fullscreen {
            self.show_chrome(ctx, &mut commands);
        }
        let chrome = super::theme::Chrome::of(self.is_private());
        // Without the system's frame, a thin border around the page is
        // where the window is resized: the page's native view would take
        // the pointer.
        let border = if !fullscreen && super::window_frame::resizable(ctx) {
            super::window_frame::BORDER as i8
        } else {
            0
        };
        let page_frame = Frame::NONE.fill(chrome.night).inner_margin(egui::Margin {
            left: border,
            right: border,
            top: 0,
            bottom: border,
        });
        let page = CentralPanel::default()
            .frame(page_frame)
            .show(ctx, |ui| {
                // The active page's native view is laid over this area.
                let rect = ui.max_rect();
                self.browser
                    .set_viewport(page_viewport(rect, ctx.pixels_per_point()));
                rect
            })
            .inner;

        for command in commands {
            self.run(command);
        }
        self.place_popup(ctx);
        super::window_frame::resize_edges(ctx);
        // Everything is drawn: the page lets the tooltips and menus over it
        // show through.
        let holes = page_holes(floating_layers(ctx), page, ctx.pixels_per_point());
        self.browser.set_page_holes(&holes);

        if self.close_requested || self.browser.session().is_empty() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        self.sync_window_title(ctx);
        if self.browser.has_unsaved_changes() {
            match PERSIST_EVERY.checked_sub(self.last_persist.elapsed()) {
                Some(wait) if !wait.is_zero() => ctx.request_repaint_after(wait),
                _ => self.persist(),
            }
        }

        // GTK's loop only advances when we pump it.
        if cfg!(target_os = "linux") {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.persist();
        // A downloaded update still applies on a plain quit (MSI).
        self.updates.updater.schedule_on_quit();
        let action = match (self.updates.updater.exit_action.clone(), self.relaunch) {
            // A DRM reset restarts the app, with the update if one waits.
            (Some(ExitAction::RunMsi(msi)), true) => Some(ExitAction::RunMsiThenRelaunch(msi)),
            (None, true) => Some(ExitAction::Relaunch),
            (action, _) => action,
        };
        *self
            .exit_slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = action;
    }
}
