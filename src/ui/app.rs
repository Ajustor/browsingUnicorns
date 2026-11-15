use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use application::Browser;
use application::ports::{EngineEvent, Viewport};
use egui::{CentralPanel, Frame, TopBottomPanel, ViewportCommand};
use infrastructure::WryEngine;

use super::command::Command;
use super::library::Library;
use super::omnibox::Omnibox;
use super::{APP_NAME, bookmarks_bar, tab_strip, toolbar};

pub struct BrowserApp {
    browser: Browser<WryEngine>,
    events: Receiver<EngineEvent>,
    omnibox: Omnibox,
    library: Library,
    error: Option<String>,
    window_title: String,
    last_persist: Instant,
}

/// History and session are written at most this often (and on exit).
const PERSIST_EVERY: Duration = Duration::from_secs(5);

impl BrowserApp {
    pub fn new(browser: Browser<WryEngine>, events: Receiver<EngineEvent>) -> Self {
        Self {
            browser,
            events,
            omnibox: Omnibox::default(),
            library: Library::default(),
            error: None,
            window_title: String::new(),
            last_persist: Instant::now(),
        }
    }

    fn report(&mut self, error: impl std::fmt::Display) {
        log::error!("{error}");
        self.error = Some(error.to_string());
    }

    fn run(&mut self, command: Command) {
        let result = match command {
            Command::NewTab => self.browser.open_tab(None).map(|_| {
                self.omnibox.request_focus();
            }),
            Command::Home => self.browser.go_home(),
            Command::Submit(input) => self.browser.submit_omnibox(&input).map(|_| ()),
            Command::CloseTab(id) => {
                self.browser.close_tab(id);
                Ok(())
            }
            Command::CloseActiveTab => {
                self.browser.close_active_tab();
                Ok(())
            }
            Command::Activate(id) => {
                self.browser.activate(id);
                Ok(())
            }
            Command::CycleTabs(offset) => {
                self.browser.cycle_tabs(offset);
                Ok(())
            }
            Command::Back => {
                self.browser.go_back();
                Ok(())
            }
            Command::Forward => {
                self.browser.go_forward();
                Ok(())
            }
            Command::Reload => {
                self.browser.reload();
                Ok(())
            }
            Command::FocusOmnibox => {
                self.omnibox.request_focus();
                Ok(())
            }
            Command::Open(address, target) => self.browser.load(address, target),
            Command::ToggleBookmark => self.browser.toggle_bookmark().map(|_| ()),
            Command::RemoveBookmark(address) => self.browser.remove_bookmark(&address),
            Command::ClearHistory => self.browser.clear_history(),
            Command::ToggleLibrary(view) => {
                self.library.toggle(view);
                Ok(())
            }
        };
        if let Err(e) = result {
            self.report(e);
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
}

impl eframe::App for BrowserApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.browser.engine().pump();
        self.drain_engine_events();

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
        if let Some(error) = self.error.clone() {
            TopBottomPanel::bottom("error").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    if ui.small_button("OK").clicked() {
                        self.error = None;
                    }
                });
            });
        }
        self.library.show(
            ctx,
            self.browser.history(),
            self.browser.bookmarks(),
            self.browser.now(),
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

        if self.browser.session().is_empty() {
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
    }
}
