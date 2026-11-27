//! Command dispatch: what the window does for each [`Command`] its
//! chrome, its shortcuts and its pages ask for.

use application::ports::{PageSource, WebEngine as _};
use domain::TabId;

use super::BrowserApp;
use crate::ui::command::{self, Command};
use crate::ui::language;
use crate::ui::profiles::ProfileCommand;
use crate::ui::shield::Shield;
use crate::ui::site_info::SiteInfo;
use crate::ui::updates::Notice;

impl BrowserApp {
    pub(super) fn run(&mut self, command: Command) {
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
            Command::RemoveCredential {
                origin,
                username,
                never,
            } => {
                if never {
                    self.browser.allow_saving_for(&origin)
                } else {
                    self.browser.remove_credential(&origin, &username)
                }
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
    fn import(&mut self, from: command::ImportFrom) -> Result<(), String> {
        use command::ImportFrom;
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
        what: command::ClearData,
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

    /// Commands that cannot fail.
    fn run_ui(&mut self, command: Command) {
        match command {
            Command::CloseTab(id) => self.browser.close_tab(id),
            Command::CloseActiveTab => self.browser.close_active_tab(),
            Command::Activate(id) => self.browser.activate(id),
            Command::MoveTab(id, to) => self.browser.move_tab(id, to),
            Command::SetPinned(id, pinned) => self.browser.set_pinned(id, pinned),
            Command::NewGroup(id) => {
                if let Some(group) = self.browser.new_group(id) {
                    // Named at once, as in other browsers.
                    self.tab_search.close();
                    self.naming_group = Some(group);
                }
            }
            Command::AddToGroup(id, group) => self.browser.add_to_group(id, group),
            Command::RemoveFromGroup(id) => self.browser.remove_from_group(id),
            Command::RenameGroup(group, name) => self.browser.rename_group(group, &name),
            Command::SetGroupColor(group, color) => self.browser.set_group_color(group, color),
            Command::SetGroupCollapsed(group, collapsed) => {
                self.browser.set_group_collapsed(group, collapsed);
            }
            Command::Ungroup(group) => self.browser.ungroup(group),
            Command::CloseGroup(group) => self.browser.close_group(group),
            Command::ToggleTabSearch => {
                self.browser.engine().focus_window();
                self.tab_search.toggle();
            }
            Command::CycleTabs(offset) => self.browser.cycle_tabs(offset),
            Command::Back => self.browser.go_back(),
            Command::Forward => self.browser.go_forward(),
            Command::Reload => self.browser.reload(),
            Command::ReloadTab(tab) => self.browser.reload_tab(tab),
            Command::OpenFind => self.find_bar.open(),
            Command::FindInPage {
                query,
                backwards,
                restart,
            } => self.browser.find_in_page(&query, backwards, restart),
            Command::Print => self.browser.print(),
            Command::AnswerPermission(id, allow) => {
                self.permission_asks.retain(|a| a.id != id);
                self.browser.engine_mut().answer_permission(id, allow);
            }
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
                if let Err(e) = language::choose_language(choice) {
                    self.report(e);
                }
            }
            Command::ApplyZoom => {
                let device = self.browser.settings().device.clone();
                self.browser.engine_mut().apply_device_settings(&device);
            }
            Command::SetTabPosition(position) => {
                self.change_device(|d| d.tab_position = position);
            }
            Command::SetSideTabsWidth(width) => {
                let (min, _, max) = domain::SIDE_TABS_WIDTH;
                self.change_device(|d| d.side_tabs_width = width.clamp(min, max));
            }
            Command::SetSideTabsCollapsed(collapsed) => {
                self.change_device(|d| d.side_tabs_collapsed = collapsed);
            }
            Command::SplitWithActive(tab) => self.browser.split_with_active(tab),
            Command::SplitWithNewTab => {
                if let Err(e) = self.browser.split_with_new_tab() {
                    self.report(e);
                }
                self.omnibox.request_focus();
            }
            Command::Unsplit(tab) => self.browser.unsplit(tab),
            Command::SetSplitRatio(tab, ratio) => self.browser.set_split_ratio(tab, ratio),
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
            | Command::RemoveCredential { .. }
            | Command::ToggleExtensionPopup { .. } => unreachable!("handled by run"),
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

    /// Changes this device's settings (saved when they changed).
    fn change_device(&mut self, f: impl FnOnce(&mut domain::DeviceSettings)) {
        let mut device = self.browser.settings().device.clone();
        f(&mut device);
        if device != self.browser.settings().device
            && let Err(e) = self.browser.update_device_settings(device)
        {
            self.report(e);
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
            | Command::RemoveCredential { .. }
    )
}
