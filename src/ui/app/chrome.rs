//! The window's chrome: the tab strip, the toolbar, the bars and the side
//! panel around the page, and what stands in for a page (an extension's
//! window, a crashed page).

use application::ports::WebEngine as _;
use egui::TopBottomPanel;

use super::BrowserApp;
use crate::ui::command::Command;
use crate::ui::extensions::ExtensionsView;
use crate::ui::library::{LibraryData, LibraryView};
use crate::ui::shield::ShieldView;
use crate::ui::sync_panel::SyncView;
use crate::ui::toolbar::{self, ShieldButton};
use crate::ui::{
    bookmarks_bar, downloads, omnibox, panel_kit, passwords, permissions, settings_panel,
    side_tabs, tab_common, tab_strip, theme,
};

impl BrowserApp {
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

    /// The tab strip, the toolbar, the bars and the side panel.
    pub(super) fn show_chrome(&mut self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        let chrome = theme::Chrome::of(self.is_private());
        // A group gone (closed, ungrouped) is no longer named.
        if let Some(group) = self.naming_group
            && self.browser.session().group(group).is_none()
        {
            self.naming_group = None;
        }
        // Extensions' windows float over their opener's page: not listed.
        let hidden: Vec<domain::TabId> = self
            .hub
            .popup_windows()
            .into_iter()
            .map(|(tab, _)| tab)
            .filter(|&t| {
                self.browser
                    .session()
                    .tab(t)
                    .is_some_and(|t| t.opener().is_some())
            })
            .collect();
        let device = &self.browser.settings().device;
        let side = device.tab_position == domain::TabPosition::Left;
        let (side_width, folded) = (device.side_tabs_width, device.side_tabs_collapsed);
        // The tabs at the top: their strip is the title bar. On the left,
        // the toolbar is (see below).
        let strip = (!side).then(|| {
            TopBottomPanel::top("tabs")
                .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: tab_strip::TOP_MARGIN,
                    bottom: 0,
                }))
                .show_separator_line(false)
                .show(ctx, |ui| {
                    theme::gradient(ui.painter(), tab_strip::area(ui), &chrome);
                    self.favicons.update(ui.ctx(), self.browser.session());
                    tab_strip::show(
                        ui,
                        self.browser.session(),
                        &self.favicons,
                        &chrome,
                        self.is_private(),
                        &mut self.naming_group,
                        self.browser.settings().device.shrink_tabs,
                        &hidden,
                        commands,
                    );
                })
                .response
                .rect
        });
        let actions = if infrastructure::builtin_extensions() {
            self.bar.actions(ctx, self.extensions.extensions())
        } else {
            Vec::new()
        };
        let shield = self.shield_button();
        let downloads_tip = downloads::toolbar_tip(&self.downloads);
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
        let toolbar_frame = if side {
            // Up to the window's top edge, which it now grabs.
            theme::chrome_frame(&chrome).inner_margin(egui::Margin {
                left: 8,
                right: 0,
                top: 6,
                bottom: 6,
            })
        } else {
            theme::chrome_frame(&chrome)
        };
        let toolbar = TopBottomPanel::top("toolbar")
            .frame(toolbar_frame)
            .show_separator_line(false)
            .show(ctx, |ui| {
                theme::chrome_style(ui, &chrome);
                if side {
                    // The title bar: its empty space moves the window, the
                    // window's buttons end it (macOS: its own, on the left).
                    let r = ui.max_rect();
                    let bar = egui::Rect::from_min_max(
                        egui::pos2(0.0, 0.0),
                        egui::pos2(r.right(), r.bottom() + 6.0),
                    );
                    tab_common::drag_window_by(ui, bar);
                }
                ui.horizontal(|ui| {
                    if side && cfg!(target_os = "macos") {
                        ui.add_space(tab_common::MACOS_BUTTONS);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if side && !cfg!(target_os = "macos") {
                            // In a scope: they set their spacing to 0.
                            ui.scope(tab_common::window_buttons);
                            ui.add_space(6.0);
                        }
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
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
                                &self.updates.updater.state,
                                commands,
                            );
                        });
                    });
                });
            })
            .response
            .rect;
        if let Some(query) = self.omnibox.stale_query().map(str::to_owned) {
            let found = self.browser.suggest(&query, omnibox::SUGGESTIONS);
            self.omnibox.set_suggestions(query, found);
        }
        self.omnibox.show_suggestions(ctx, &chrome, commands);
        self.tab_search.show(
            ctx,
            strip.map_or(toolbar.bottom(), |s| s.bottom()) + 6.0,
            self.browser.session(),
            &self.favicons,
            &chrome,
            commands,
        );
        self.site_info_bar(ctx);
        self.shield_bar(ctx, commands);
        if !self.browser.bookmarks().is_empty() {
            TopBottomPanel::top("bookmarks")
                .frame(theme::chrome_frame(&chrome).inner_margin(egui::Margin::symmetric(10, 3)))
                .show_separator_line(false)
                .show(ctx, |ui| {
                    theme::chrome_style(ui, &chrome);
                    bookmarks_bar::show(ui, self.browser.bookmarks(), commands);
                    let r = ui.max_rect();
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() + 3.0,
                        egui::Stroke::new(theme::HAIRLINE, chrome.hover),
                    );
                });
        }
        if side {
            self.favicons.update(ctx, self.browser.session());
            side_tabs::show(
                ctx,
                side_tabs::SideTabs {
                    session: self.browser.session(),
                    favicons: &self.favicons,
                    chrome: &chrome,
                    private: self.is_private(),
                    width: side_width,
                    folded,
                    hidden: &hidden,
                },
                &mut self.naming_group,
                commands,
            );
        }
        self.default_browser.banner(ctx, commands);
        // Asked in the tab where it was typed only.
        let active = self.browser.session().active().map(|t| t.id());
        let pending = self
            .browser
            .pending_credential()
            .filter(|p| Some(p.tab) == active);
        passwords::banner(ctx, pending, commands);
        // Requests of closed tabs went with their view.
        let open = self.browser.session();
        self.permission_asks.retain(|a| open.tab(a.tab).is_some());
        let ask = self.permission_asks.iter().find(|a| Some(a.tab) == active);
        permissions::bar(ctx, ask, commands);
        self.find_bar.show(ctx, commands);
        if self.library.open == Some(LibraryView::Profiles) && !self.is_private() {
            self.profiles.refresh(ctx);
        }
        let can_cancel = self.browser.engine().can_cancel_downloads();
        for action in self.downloads.show(ctx, can_cancel, commands) {
            match action {
                downloads::Action::Cancel(file) => {
                    self.browser.engine_mut().cancel_download(&file);
                }
            }
        }
        self.notice_bar(ctx);
        let unseen_notes = !self.is_private()
            && self.browser.settings().device.seen_release != env!("CARGO_PKG_VERSION");
        if unseen_notes && self.library.open == Some(LibraryView::WhatsNew) {
            self.mark_notes_seen();
        }
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
                settings: settings_panel::SettingsView {
                    settings: self.browser.settings(),
                    language: self.language,
                    passwords: self.browser.engine().can_save_passwords(),
                    credentials: self.browser.credentials(),
                    clear_site_data: self.browser.engine().can_clear_site_data(),
                    reset_drm: self.drm.is_some(),
                    default_download_dir: infrastructure::downloads_dir(),
                },
                unseen_notes,
                update_waiting: matches!(
                    self.updates.updater.state,
                    updater::UpdateState::Available(_) | updater::UpdateState::Ready(_)
                ),
            },
            commands,
        );
    }
}

/// An extension's window (a passkey prompt...) in the middle of the page
/// area, over a dimmed backdrop: its title and a close button above, the
/// page in it. `size`: what it asked for (points). Returns where its page
/// goes.
pub(super) fn floating_window(
    ui: &mut egui::Ui,
    area: egui::Rect,
    size: Option<(u32, u32)>,
    title: &str,
    tab: domain::TabId,
    commands: &mut Vec<Command>,
) -> egui::Rect {
    const BAR: f32 = 36.0;
    let painter = ui.painter();
    painter.rect_filled(area, 0.0, egui::Color32::from_black_alpha(150));
    let (w, h) = size.map_or((420.0, 600.0), |(w, h)| (w as f32, h as f32));
    let room = area.shrink(24.0);
    let page_size = egui::vec2(w.min(room.width()), h.min(room.height() - BAR).max(120.0));
    let frame = egui::Rect::from_center_size(area.center(), page_size + egui::vec2(0.0, BAR));
    painter.rect_filled(frame.expand(1.0), 12.0, theme::GRADIENT[1]);
    let bar = egui::Rect::from_min_size(frame.min, egui::vec2(frame.width(), BAR));
    painter.rect_filled(
        bar,
        egui::CornerRadius {
            nw: 12,
            ne: 12,
            sw: 0,
            se: 0,
        },
        theme::GRADIENT[1],
    );
    // The close button on the right, the title on the left of what is left.
    let inner = bar.shrink2(egui::vec2(12.0, 0.0));
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let close = theme::hover_fill(&mut right, |ui| {
        ui.add(
            egui::Button::new(
                egui::RichText::new(egui_phosphor::regular::X)
                    .size(16.0)
                    .color(egui::Color32::WHITE),
            )
            .frame(false),
        )
    })
    .on_hover_text(i18n::common::close());
    if close.clicked() {
        commands.push(Command::CloseTab(tab));
    }
    let title_area = egui::Rect::from_min_max(
        inner.min,
        egui::pos2((close.rect.left() - 8.0).max(inner.left()), inner.bottom()),
    );
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(title_area)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    left.add(
        egui::Label::new(
            egui::RichText::new(format!("{}  {title}", egui_phosphor::regular::PUZZLE_PIECE))
                .color(egui::Color32::WHITE)
                .strong(),
        )
        .truncate(),
    );
    egui::Rect::from_min_max(egui::pos2(frame.left(), bar.bottom()), frame.max)
}

/// Where a crashed page was: what happened, and Reload.
pub(super) fn crashed_page(
    ui: &mut egui::Ui,
    area: egui::Rect,
    commands: &mut Vec<Command>,
    tab: domain::TabId,
) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(area)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.add_space((area.height() / 2.0 - 90.0).max(0.0));
    child.label(
        egui::RichText::new(egui_phosphor::regular::SMILEY_SAD)
            .size(56.0)
            .color(theme::CHROME_TEXT_WEAK),
    );
    child.add_space(8.0);
    child.label(
        egui::RichText::new(i18n::tabs::page_crashed())
            .size(20.0)
            .color(theme::CHROME_TEXT),
    );
    child
        .label(egui::RichText::new(i18n::tabs::page_crashed_hint()).color(theme::CHROME_TEXT_WEAK));
    child.add_space(12.0);
    if panel_kit::primary_button(
        &mut child,
        format!(
            "{} {}",
            egui_phosphor::regular::ARROW_CLOCKWISE,
            i18n::tabs::reload_page()
        ),
        true,
    )
    .clicked()
    {
        commands.push(Command::ReloadTab(tab));
    }
}
