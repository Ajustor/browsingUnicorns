use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use application::Browser;
use application::ports::{EngineEvent, PageSource, Viewport, WebEngine as _};
use egui::{CentralPanel, Frame, TopBottomPanel, ViewportCommand};
use infrastructure::WryEngine;
use updater::ExitAction;

use super::command::Command;
use super::extension_bar::{DEFAULT_POPUP, ExtensionBar};
use super::extensions::{ExtensionsRunner, ExtensionsView};
use super::library::{Library, LibraryData, LibraryView};
use super::omnibox::Omnibox;
use super::profiles::{ProfileCommand, ProfilesRunner};
use super::site_info::SiteInfo;
use super::sync::SyncRunner;
use super::sync_panel::SyncView;
use super::updates::{Notice, Updates};
use super::user_scripts::UserScriptsRunner;
use super::{APP_NAME, bookmarks_bar, tab_strip, toolbar};
use infrastructure::extension_hub::{BrowserHub, Hub};

/// What `main` runs once the window is gone (apply an update).
pub type ExitSlot = Arc<Mutex<Option<ExitAction>>>;

pub struct BrowserApp {
    browser: Browser<WryEngine>,
    events: Receiver<EngineEvent>,
    updates: Updates,
    sync: SyncRunner,
    extensions: ExtensionsRunner,
    user_scripts: UserScriptsRunner,
    hub: Hub,
    bar: ExtensionBar,
    profiles: ProfilesRunner,
    exit_slot: ExitSlot,
    omnibox: Omnibox,
    library: Library,
    notice: Option<Notice>,
    /// The padlock's bar, when open.
    site_info: Option<SiteInfo>,
    window_title: String,
    last_persist: Instant,
    close_requested: bool,
}

/// Extensions, user scripts and the storage of their page scripts.
pub struct AddOns {
    pub extensions: ExtensionsRunner,
    /// The extensions' toolbar buttons and popups.
    pub bar: ExtensionBar,
    pub user_scripts: UserScriptsRunner,
    /// The browser side of the extension APIs (storage, messages, tabs...).
    pub hub: Hub,
}

/// History and session are written at most this often (and on exit).
const PERSIST_EVERY: Duration = Duration::from_secs(5);

impl BrowserApp {
    pub fn new(
        browser: Browser<WryEngine>,
        events: Receiver<EngineEvent>,
        mut updates: Updates,
        sync: SyncRunner,
        add_ons: AddOns,
        profiles: ProfilesRunner,
        exit_slot: ExitSlot,
    ) -> Self {
        let AddOns {
            extensions,
            bar,
            user_scripts,
            hub,
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
            profiles,
            exit_slot,
            omnibox: Omnibox::default(),
            library: Library::default(),
            notice: None,
            site_info: None,
            window_title: String::new(),
            last_persist: Instant::now(),
            close_requested: false,
        }
    }

    fn report(&mut self, error: impl std::fmt::Display) {
        log::error!("{error}");
        self.notice = Some(Notice {
            error: true,
            text: error.to_string(),
            file: None,
        });
    }

    fn run(&mut self, command: Command) {
        let result = match command {
            Command::NewTab => self.browser.open_tab(None).map(|_| {
                self.browser.engine().focus_window();
                self.omnibox.request_focus();
            }),
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
            other => {
                self.run_ui(other);
                Ok(())
            }
        };
        if let Err(e) = result {
            self.report(e);
        }
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
            Command::CycleTabs(offset) => self.browser.cycle_tabs(offset),
            Command::Back => self.browser.go_back(),
            Command::Forward => self.browser.go_forward(),
            Command::Reload => self.browser.reload(),
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
            Command::CheckForUpdates => self.updates.check_now(),
            Command::InstallUpdate => self.updates.updater.install(),
            Command::UpdateLater => self.updates.updater.later(),
            Command::SkipUpdate => self.updates.skip(),
            Command::RestartToUpdate => {
                self.updates.updater.schedule_restart();
                self.close_requested = true;
            }
            Command::NewTab
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
            | Command::Profile(_)
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
            });
        }
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.browser.engine().focus_window();
        }
    }

    /// Opens the padlock's bar on the active site (asking its certificate),
    /// or closes it.
    fn toggle_site_info(&mut self) {
        if self.site_info.take().is_some() {
            return;
        }
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
                let mut host = BrowserHub {
                    browser: &mut self.browser,
                    extensions: self.extensions.extensions(),
                };
                self.hub.handle(from, body, &mut host);
                continue;
            }
            if let Some(notice) = super::downloads::notice(&event) {
                self.notice = Some(notice);
                continue;
            }
            if let Err(e) = self.browser.handle(event) {
                self.report(e);
            }
        }
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let app = if self.profiles.several() {
            format!("{APP_NAME} ({})", self.profiles.current().name)
        } else {
            APP_NAME.to_owned()
        };
        let title = match self.browser.session().active() {
            Some(tab) => format!("{} — {app}", tab.display_title()),
            None => app,
        };
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
                if let Some(file) = &notice.file {
                    if ui.small_button("Open").clicked() {
                        super::downloads::open(file);
                        dismissed = true;
                    }
                    if ui.small_button("Show in folder").clicked() {
                        super::downloads::reveal(file);
                        dismissed = true;
                    }
                }
                dismissed |= ui.small_button("OK").clicked();
            });
        });
        if dismissed {
            self.notice = None;
        }
    }
}

impl eframe::App for BrowserApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.browser.engine_mut().pump();
        self.drain_engine_events();
        self.drive_sync(ctx);
        self.extensions.poll(self.browser.engine_mut());
        self.user_scripts.poll(self.browser.engine_mut());
        let mut host = BrowserHub {
            browser: &mut self.browser,
            extensions: self.extensions.extensions(),
        };
        self.hub.tabs_changed(&mut host);
        if let Some(notice) = self.updates.poll() {
            self.notice = Some(notice);
        }

        self.sync_keyboard_focus(ctx);
        let mut commands = Vec::new();
        super::shortcuts::collect(ctx, &mut commands);
        for press in self.browser.engine_mut().take_key_presses() {
            commands.extend(super::shortcuts::from_page(&press));
        }

        TopBottomPanel::top("tabs")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 6,
                bottom: 0,
            }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                super::theme::gradient(ui.painter(), ui.max_rect().expand2(egui::vec2(8.0, 6.0)));
                tab_strip::show(ui, self.browser.session(), &mut commands);
            });
        let actions = if infrastructure::builtin_extensions() {
            self.bar.actions(ctx, self.extensions.extensions())
        } else {
            Vec::new()
        };
        TopBottomPanel::top("toolbar")
            .frame(super::theme::chrome_frame())
            .show_separator_line(false)
            .show(ctx, |ui| {
                super::theme::chrome_style(ui);
                toolbar::show(
                    ui,
                    self.browser.session().active(),
                    self.browser.is_active_bookmarked(),
                    &mut self.omnibox,
                    &actions,
                    self.profiles.current(),
                    &mut commands,
                );
            });
        self.site_info_bar(ctx);
        if !self.browser.bookmarks().is_empty() {
            TopBottomPanel::top("bookmarks")
                .frame(super::theme::chrome_frame().inner_margin(egui::Margin::symmetric(10, 3)))
                .show_separator_line(false)
                .show(ctx, |ui| {
                    super::theme::chrome_style(ui);
                    bookmarks_bar::show(ui, self.browser.bookmarks(), &mut commands);
                    let r = ui.max_rect();
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() + 3.0,
                        egui::Stroke::new(1.0, super::theme::NIGHT_HOVER),
                    );
                });
        }
        self.updates.banner(ctx, &mut commands);
        if self.library.open == Some(LibraryView::Profiles) {
            self.profiles.refresh(ctx);
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
            },
            &mut commands,
        );
        CentralPanel::default().frame(Frame::NONE).show(ctx, |ui| {
            // The active page's native view is laid over this area.
            let rect = ui.max_rect();
            let ppp = ctx.pixels_per_point();
            self.browser.set_viewport(Viewport {
                x: (rect.min.x * ppp).round() as i32,
                y: (rect.min.y * ppp).round() as i32,
                width: (rect.width() * ppp).round().max(1.0) as u32,
                height: (rect.height() * ppp).round().max(1.0) as u32,
            });
        });

        for command in commands {
            self.run(command);
        }
        self.place_popup(ctx);

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
        *self
            .exit_slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            self.updates.updater.exit_action.clone();
    }
}
