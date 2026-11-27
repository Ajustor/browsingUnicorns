use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use application::Browser;
use application::ports::{EngineEvent, PageSource, Viewport, WebEngine as _};
use domain::{GroupId, TabId};
use egui::{CentralPanel, Frame, ViewportCommand};
use infrastructure::{PrivateDataDir, WryEngine};
use updater::ExitAction;

use super::blocker::BlockerRunner;
use super::command::Command;
use super::extension_bar::{DEFAULT_POPUP, ExtensionBar};
use super::extensions::ExtensionsRunner;
use super::library::{Library, LibraryView};
use super::omnibox::Omnibox;
use super::page_holes::{floating_layers, page_holes, page_viewport};
use super::profiles::ProfilesRunner;
use super::shield::Shield;
use super::site_info::SiteInfo;
use super::sync::SyncRunner;
use super::tab_search::TabSearch;
use super::updates::{Notice, Updates};
use super::user_scripts::UserScriptsRunner;
use super::{APP_NAME, split_view};
use chrome::{crashed_page, floating_window};
use infrastructure::extension_hub::{BrowserHub, Hub};

// Its command dispatch and its chrome: child modules, they reach its
// private state.
mod chrome;
mod commands;

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
    tab_search: TabSearch,
    /// A new group whose name is being typed in its label.
    naming_group: Option<GroupId>,
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
    /// When idle tabs were last looked for, to put them to sleep.
    last_sleep_check: Instant,
    /// A page said it got the keyboard (its script, where the web view
    /// doesn't tell): menus close as for a click in it.
    page_focused: bool,
    /// What pages asked to use, waiting for the user (WebKitGTK).
    permission_asks: Vec<super::permissions::Ask>,
    find_bar: super::find_bar::FindBar,
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

/// A tab unseen this long goes to sleep (its page unloaded).
const SLEEP_AFTER: Duration = Duration::from_secs(30 * 60);
/// How often the idle tabs are looked for.
const SLEEP_CHECK_EVERY: Duration = Duration::from_secs(30);

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
        favicons_dir: Option<std::path::PathBuf>,
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
        let mut app = Self {
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
            tab_search: TabSearch::default(),
            naming_group: None,
            page_fullscreen: None,
            favicons: super::favicons::Favicons::new(favicons_dir),
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
            last_sleep_check: Instant::now(),
            page_focused: false,
            permission_asks: Vec::new(),
            find_bar: Default::default(),
            close_requested: false,
            drm,
            relaunch: false,
            private_data,
        };
        app.offer_release_notes();
        app
    }

    /// After an update, the side panel opens on what changed (not at the
    /// first start: nothing was seen before, nothing changed then).
    fn offer_release_notes(&mut self) {
        let seen = &self.browser.settings().device.seen_release;
        if !self.is_private() && !seen.is_empty() && seen != env!("CARGO_PKG_VERSION") {
            self.library.open = Some(LibraryView::WhatsNew);
        }
    }

    /// This version's notes were shown.
    fn mark_notes_seen(&mut self) {
        let mut device = self.browser.settings().device.clone();
        device.seen_release = env!("CARGO_PKG_VERSION").into();
        if let Err(e) = self.browser.update_device_settings(device) {
            log::warn!("release notes: {e}");
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

    /// Applies a finished round, and starts one when due.
    fn drive_sync(&mut self, ctx: &egui::Context) {
        if let Some(records) = self.sync.poll() {
            // The round succeeded: what its record carried is synced.
            self.browser.mark_synced();
            if let Err(e) = self.browser.apply_remote(&records) {
                self.report(e);
            }
        }
        let changes = self.browser.has_unsynced_changes();
        if self.sync.is_due(changes)
            && let Some(config) = self.sync.config()
        {
            let (device, name) = (config.device.clone(), config.device_name.clone());
            let record = self.browser.sync_record(device, &name);
            self.sync.start(record);
        }
        if let Some(wait) = self.sync.next_check(changes) {
            ctx.request_repaint_after(wait);
        }
    }

    /// Keeps the keyboard where the user clicked last. The page is a native
    /// child view: a click on the window's own widgets (the address bar)
    /// doesn't take the keyboard back from it, and a click in the page
    /// leaves the widget looking focused.
    fn sync_keyboard_focus(&mut self, ctx: &egui::Context) {
        let page_script_said = std::mem::take(&mut self.page_focused);
        if self.browser.engine_mut().take_page_focused() || page_script_said {
            ctx.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
                // A click on the page closes an open list (shown over the
                // page, egui never sees that click).
                m.close_popup();
            });
            close_context_menu(ctx);
        }
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.browser.engine().focus_window();
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

    /// The extension's window shown (the active tab is one), with the size
    /// it asked for.
    fn active_popup_window(&self) -> Option<(domain::TabId, Option<(u32, u32)>)> {
        let active = self.browser.session().active_id()?;
        self.hub
            .popup_windows()
            .into_iter()
            .find(|(tab, _)| *tab == active)
    }

    /// An extension's window stays over the tab that opened it: back on
    /// that tab, the window shows again (until it closes).
    fn front_popup_window(&mut self) {
        let Some(active) = self.browser.session().active_id() else {
            return;
        };
        let window = self.hub.popup_windows().into_iter().find(|(tab, _)| {
            self.browser.session().tab(*tab).and_then(|t| t.opener()) == Some(active)
        });
        if let Some((window, _)) = window {
            self.browser.activate(window);
        }
    }

    /// Puts the tabs left unseen [`SLEEP_AFTER`] to sleep, every
    /// [`SLEEP_CHECK_EVERY`] (unless the user turned it off). Those playing
    /// sound stay awake.
    fn sleep_idle_tabs(&mut self, ctx: &egui::Context) {
        // Off: nothing to wake up for.
        if !self.browser.settings().device.sleep_tabs {
            return;
        }
        if let Some(wait) = SLEEP_CHECK_EVERY.checked_sub(self.last_sleep_check.elapsed())
            && !wait.is_zero()
        {
            ctx.request_repaint_after(wait);
            return;
        }
        self.last_sleep_check = Instant::now();
        ctx.request_repaint_after(SLEEP_CHECK_EVERY);
        let playing: Vec<domain::TabId> = self
            .browser
            .session()
            .tabs()
            .iter()
            .map(|t| t.id())
            .filter(|&t| self.favicons.sound(t).is_some())
            .collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let asleep = self
            .browser
            .discard_idle(now, SLEEP_AFTER.as_secs(), &playing);
        if !asleep.is_empty() {
            log::info!("{} tab(s) put to sleep", asleep.len());
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
            if let EngineEvent::PageMessage { from, url, body } = &event {
                // The password manager's page script: a sign-in was sent
                // (in the top frame: only its script there has the token).
                if infrastructure::page_scripts::credentials::is_report(body) {
                    if let Some((username, password)) =
                        infrastructure::page_scripts::credentials::submitted(body)
                        && let PageSource::Tab(tab) = from
                        && !self.is_private()
                    {
                        self.browser.offer_credential(*tab, &username, &password);
                    }
                    continue;
                }
                // Ctrl/Cmd+F, Ctrl/Cmd+P typed in a page (WebKit leaves them).
                if let Some(key) = infrastructure::page_scripts::focus::shortcut(body) {
                    self.run(match key {
                        'f' => Command::OpenFind,
                        _ => Command::Print,
                    });
                    continue;
                }
                // A page got the keyboard (WebKit, where the web view
                // doesn't say): as WebView2's own focus event.
                if infrastructure::page_scripts::focus::focused(body) {
                    if let PageSource::Tab(tab) = from {
                        self.page_focused = true;
                        if let Err(e) = self.browser.handle(EngineEvent::PageFocused { tab: *tab })
                        {
                            self.report(e);
                        }
                    }
                    continue;
                }
                let mut host = BrowserHub {
                    browser: &mut self.browser,
                    extensions: self.extensions.extensions(),
                };
                self.hub.handle_at(from, url.as_deref(), body, &mut host);
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
            if let EngineEvent::PermissionRequested {
                id,
                tab,
                host,
                kind,
            } = event
            {
                self.permission_asks.push(super::permissions::Ask {
                    id,
                    tab,
                    host,
                    kind,
                });
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
}

/// Where egui keeps the open right-click menu (`egui::menu`'s
/// `CONTEXT_MENU_ID_STR`, private to it).
const CONTEXT_MENU_ID: &str = "__egui::context_menu";

/// Closes the open right-click menu (a tab's): egui keeps it open until it
/// sees a click elsewhere, and a click in the page is the web view's.
fn close_context_menu(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<egui::menu::BarState>(egui::Id::new(CONTEXT_MENU_ID)));
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
            self.front_popup_window();
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
                // The active page's native view is laid over this area (two
                // views, side by side, in a split view).
                let rect = ui.max_rect();
                let ppp = ctx.pixels_per_point();
                self.browser.set_viewport(page_viewport(rect, ppp));
                if self.updates.window_open() {
                    // The update's window is over them: their native views
                    // would cover it.
                    self.browser.engine_mut().show(None);
                } else if let Some((window, size)) = self.active_popup_window() {
                    // An extension's window over its opener: alone, framed.
                    let title = self
                        .browser
                        .session()
                        .tab(window)
                        .map(|t| t.display_title().to_owned())
                        .unwrap_or_default();
                    let area = floating_window(ui, rect, size, &title, window, &mut commands);
                    self.browser
                        .show_panes(&[(window, page_viewport(area, ppp))]);
                } else {
                    let panes = split_view::panes(ui, rect, self.browser.session(), &mut commands);
                    // A crashed page has no view: its message shows there.
                    for &(tab, area) in &panes {
                        if self.browser.is_crashed(tab) {
                            crashed_page(ui, area, &mut commands, tab);
                        }
                    }
                    let panes: Vec<_> = panes
                        .into_iter()
                        .map(|(tab, area)| (tab, page_viewport(area, ppp)))
                        .collect();
                    self.browser.show_panes(&panes);
                }
                rect
            })
            .inner;

        self.updates.window(ctx, &mut commands);
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
        self.sleep_idle_tabs(ctx);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame with a label that has a right-click menu; whether the menu
    /// is open.
    fn frame(ctx: &egui::Context, events: Vec<egui::Event>) -> bool {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            events,
            ..Default::default()
        };
        let mut open = false;
        let _ = ctx.run(input, |ctx| {
            CentralPanel::default().show(ctx, |ui| {
                let label = ui.add(egui::Label::new("tab").sense(egui::Sense::click()));
                label.context_menu(|ui| {
                    let _ = ui.button("Close");
                });
                open = label.context_menu_opened();
            });
        });
        open
    }

    #[test]
    fn a_click_in_the_page_closes_a_right_click_menu() {
        let ctx = egui::Context::default();
        frame(&ctx, Vec::new());
        let at = egui::pos2(15.0, 12.0);
        let click = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&ctx, vec![egui::Event::PointerMoved(at)]);
        frame(&ctx, vec![click(true)]);
        frame(&ctx, vec![click(false)]);
        assert!(frame(&ctx, Vec::new()), "the menu opened");
        // The page took the click: egui saw none.
        close_context_menu(&ctx);
        assert!(!frame(&ctx, Vec::new()));
    }
}
