use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use application::Browser;
use application::ports::{EngineEvent, Viewport, WebEngine as _};
use egui::{CentralPanel, Frame, TopBottomPanel, ViewportCommand};
use infrastructure::WryEngine;
use updater::ExitAction;

use super::command::Command;
use super::extensions::{ExtensionsRunner, ExtensionsView};
use super::library::{Library, LibraryData};
use super::omnibox::Omnibox;
use super::sync::SyncRunner;
use super::sync_panel::SyncView;
use super::updates::{Notice, Updates};
use super::{APP_NAME, bookmarks_bar, tab_strip, toolbar};

/// What `main` runs once the window is gone (apply an update).
pub type ExitSlot = Arc<Mutex<Option<ExitAction>>>;

pub struct BrowserApp {
    browser: Browser<WryEngine>,
    events: Receiver<EngineEvent>,
    updates: Updates,
    sync: SyncRunner,
    extensions: ExtensionsRunner,
    exit_slot: ExitSlot,
    omnibox: Omnibox,
    library: Library,
    notice: Option<Notice>,
    window_title: String,
    last_persist: Instant,
    close_requested: bool,
}

/// History and session are written at most this often (and on exit).
const PERSIST_EVERY: Duration = Duration::from_secs(5);

impl BrowserApp {
    pub fn new(
        browser: Browser<WryEngine>,
        events: Receiver<EngineEvent>,
        mut updates: Updates,
        sync: SyncRunner,
        extensions: ExtensionsRunner,
        exit_slot: ExitSlot,
    ) -> Self {
        updates.check_if_due();
        Self {
            browser,
            events,
            updates,
            sync,
            extensions,
            exit_slot,
            omnibox: Omnibox::default(),
            library: Library::default(),
            notice: None,
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
        });
    }

    fn run(&mut self, command: Command) {
        let result = match command {
            Command::NewTab => self.browser.open_tab(None).map(|_| {
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
            Command::FocusOmnibox => self.omnibox.request_focus(),
            Command::ToggleLibrary(view) => self.library.toggle(view),
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
            | Command::RemoveExtension(_) => unreachable!("handled by run"),
        }
    }

    fn persist(&mut self) {
        self.last_persist = Instant::now();
        if let Err(e) = self.browser.persist() {
            self.report(e);
        }
    }

    fn drain_engine_events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            if let Err(e) = self.browser.handle(event) {
                self.report(e);
            }
        }
    }

    fn sync_window_title(&mut self, ctx: &egui::Context) {
        let title = match self.browser.session().active() {
            Some(tab) => format!("{} — {APP_NAME}", tab.display_title()),
            None => APP_NAME.to_owned(),
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
                dismissed = ui.small_button("OK").clicked();
            });
        });
        if dismissed {
            self.notice = None;
        }
    }
}

impl eframe::App for BrowserApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.browser.engine().pump();
        self.drain_engine_events();
        self.drive_sync(ctx);
        self.extensions.poll(self.browser.engine_mut());
        if let Some(notice) = self.updates.poll() {
            self.notice = Some(notice);
        }

        let mut commands = Vec::new();
        super::shortcuts::collect(ctx, &mut commands);

        TopBottomPanel::top("tabs").show(ctx, |ui| {
            tab_strip::show(ui, self.browser.session(), &mut commands);
        });
        TopBottomPanel::top("toolbar").show(ctx, |ui| {
            toolbar::show(
                ui,
                self.browser.session().active(),
                self.browser.is_active_bookmarked(),
                &mut self.omnibox,
                &mut commands,
            );
        });
        if !self.browser.bookmarks().is_empty() {
            TopBottomPanel::top("bookmarks").show(ctx, |ui| {
                bookmarks_bar::show(ui, self.browser.bookmarks(), &mut commands);
            });
        }
        self.updates.banner(ctx, &mut commands);
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
                    runner: &self.extensions,
                },
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
