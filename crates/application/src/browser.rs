use std::collections::{HashMap, HashSet};

use domain::{
    Bookmarks, BrowsingHistory, Credentials, DeviceId, DeviceRecord, GroupColor, GroupId, Offer,
    RemoteTab, SeenCookies, Session, SessionSnapshot, Settings, SiteCookie, Suggestion, TabId,
    Timestamp, Visit, WebAddress, cookies_to_import, cookies_to_share, resolve_omnibox, suggest,
};

use crate::BrowserError;
use crate::ports::{EngineEvent, Profile, Viewport, WebEngine};
use crate::sync::RemoteDevice;

/// Where to load a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The active tab (a new one when there is none).
    ActiveTab,
    NewTab,
}

/// A sign-in typed in a page, that the user is asked to save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingCredential {
    pub tab: TabId,
    pub origin: String,
    pub username: String,
    pub password: String,
    /// It changes the password of a saved user name.
    pub update: bool,
}

/// The user's answer to a [`PendingCredential`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialAnswer {
    Save,
    NotNow,
    /// Never for this site.
    Never,
}

/// A user name typed alone (the first step of a two-step sign-in) is kept
/// this long for the password typed next.
const USERNAME_KEPT_FOR: Timestamp = 10 * 60;

/// The browser window's use cases.
pub struct Browser<E: WebEngine> {
    session: Session,
    settings: Settings,
    engine: E,
    profile: Profile,
    history: BrowsingHistory,
    history_dirty: bool,
    bookmarks: Bookmarks,
    saved_session: SessionSnapshot,
    remote_devices: Vec<RemoteDevice>,
    /// Bookmarks, settings or history changed since the last sync record.
    unsynced: bool,
    /// The tabs closed, last closed last: their address and where they were.
    closed: Vec<(WebAddress, usize)>,
    credentials: Credentials,
    pending_credential: Option<PendingCredential>,
    /// By tab: the origin, user name and time of a user name typed alone.
    typed_usernames: HashMap<TabId, (String, String, Timestamp)>,
    seen_cookies: SeenCookies,
}

/// How many closed tabs can be reopened.
const CLOSED_TABS: usize = 25;

impl<E: WebEngine> Browser<E> {
    /// A browser with no tab. Call [`Self::load_profile`] then
    /// [`Self::restore_session`] to start it.
    pub fn new(engine: E, settings: Settings, profile: Profile) -> Self {
        Self {
            session: Session::new(),
            settings,
            engine,
            profile,
            history: BrowsingHistory::default(),
            history_dirty: false,
            bookmarks: Bookmarks::default(),
            saved_session: SessionSnapshot::default(),
            remote_devices: Vec::new(),
            unsynced: false,
            closed: Vec::new(),
            credentials: Credentials::default(),
            pending_credential: None,
            typed_usernames: HashMap::new(),
            seen_cookies: SeenCookies::default(),
        }
    }

    /// Reads the saved history and bookmarks. Whatever fails to load is
    /// left empty; the errors are returned.
    pub fn load_profile(&mut self) -> Vec<BrowserError> {
        let mut errors = Vec::new();
        match self.profile.history.load() {
            Ok(history) => self.history = history,
            Err(e) => errors.push(e.into()),
        }
        match self.profile.bookmarks.load() {
            Ok(bookmarks) => self.bookmarks = bookmarks,
            Err(e) => errors.push(e.into()),
        }
        match self.profile.credentials.load() {
            Ok(credentials) => self.credentials = credentials,
            Err(e) => errors.push(e.into()),
        }
        match self.profile.seen_cookies.load() {
            Ok(seen) => self.seen_cookies = seen,
            Err(e) => errors.push(e.into()),
        }
        errors
    }

    /// Reopens the tabs of the last run, or the home page when there were
    /// none. Returns how many tabs were restored.
    pub fn restore_session(&mut self) -> Result<usize, BrowserError> {
        let snapshot = self.profile.session.load().unwrap_or_default();
        let mut ids = Vec::with_capacity(snapshot.tabs.len());
        // Each new tab opens after the last one.
        for address in &snapshot.tabs {
            ids.push(self.open_tab(Some(address.clone()))?);
        }
        self.session.restore_layout(&ids, &snapshot);
        match snapshot.active.and_then(|i| ids.get(i)) {
            Some(&id) => self.activate(id),
            None if ids.is_empty() => {
                self.open_tab(None)?;
            }
            None => {}
        }
        self.saved_session = snapshot;
        Ok(ids.len())
    }

    /// Whether [`Self::persist`] has something to write.
    pub fn has_unsaved_changes(&self) -> bool {
        self.history_dirty || self.session.snapshot() != self.saved_session
    }

    /// Saves what changed since the last call: history and open tabs.
    pub fn persist(&mut self) -> Result<(), BrowserError> {
        if self.history_dirty {
            self.profile.history.save(&self.history)?;
            self.history_dirty = false;
        }
        let snapshot = self.session.snapshot();
        if snapshot != self.saved_session {
            self.profile.session.save(&snapshot)?;
            self.saved_session = snapshot;
        }
        Ok(())
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn now(&self) -> Timestamp {
        self.profile.clock.now()
    }

    pub fn history(&self) -> &BrowsingHistory {
        &self.history
    }

    pub fn bookmarks(&self) -> &Bookmarks {
        &self.bookmarks
    }

    pub fn clear_history(&mut self) -> Result<(), BrowserError> {
        self.history.clear();
        self.profile.history.save(&self.history)?;
        self.history_dirty = false;
        Ok(())
    }

    pub fn is_active_bookmarked(&self) -> bool {
        self.session
            .active()
            .and_then(|t| t.address())
            .is_some_and(|a| self.bookmarks.contains(a))
    }

    /// Bookmarks the active page, or removes its bookmark. Returns whether
    /// it is bookmarked afterwards.
    pub fn toggle_bookmark(&mut self) -> Result<bool, BrowserError> {
        let Some(tab) = self.session.active() else {
            return Ok(false);
        };
        let Some(address) = tab.address().cloned() else {
            return Ok(false);
        };
        let title = tab.title().to_owned();
        let saved = self.bookmarks.toggle(address, title, self.now());
        self.unsynced = true;
        self.profile.bookmarks.save(&self.bookmarks)?;
        Ok(saved)
    }

    pub fn remove_bookmark(&mut self, address: &WebAddress) -> Result<(), BrowserError> {
        if self.bookmarks.remove(address, self.now()) {
            self.unsynced = true;
            self.profile.bookmarks.save(&self.bookmarks)?;
        }
        Ok(())
    }

    /// Adds bookmarks of another browser under `root` (their folders kept
    /// inside it); those already saved stay where they are. Returns how
    /// many were added.
    pub fn import_bookmarks(
        &mut self,
        root: &str,
        items: impl IntoIterator<Item = (WebAddress, String, String)>,
    ) -> Result<usize, BrowserError> {
        let now = self.now();
        let mut added = 0;
        for (address, title, folder) in items {
            let folder = if folder.is_empty() {
                root.to_owned()
            } else {
                format!("{root}/{folder}")
            };
            if self.bookmarks.add_in(address, title, &folder, now) {
                added += 1;
            }
        }
        if added > 0 {
            self.unsynced = true;
            self.profile.bookmarks.save(&self.bookmarks)?;
        }
        Ok(added)
    }

    /// Adds another browser's history (its pages at their last visit).
    /// Returns how many visits were new.
    pub fn import_history(
        &mut self,
        visits: impl IntoIterator<Item = Visit>,
    ) -> Result<usize, BrowserError> {
        let known: HashSet<(WebAddress, Timestamp)> = self
            .history
            .visits()
            .iter()
            .map(|v| (v.address.clone(), v.at))
            .collect();
        let new: Vec<Visit> = visits
            .into_iter()
            .filter(|v| {
                BrowsingHistory::is_recordable(&v.address)
                    && !known.contains(&(v.address.clone(), v.at))
            })
            .collect();
        if self.history.merge(&new) {
            self.profile.history.save(&self.history)?;
        }
        Ok(new.len())
    }

    /// Moves a bookmark to `folder` (empty: out of any).
    pub fn move_bookmark(
        &mut self,
        address: &WebAddress,
        folder: &str,
    ) -> Result<(), BrowserError> {
        if self.bookmarks.move_to(address, folder, self.now()) {
            self.unsynced = true;
            self.profile.bookmarks.save(&self.bookmarks)?;
        }
        Ok(())
    }

    /// Other devices of the sync group, as of the last sync.
    pub fn remote_devices(&self) -> &[RemoteDevice] {
        &self.remote_devices
    }

    /// Whether something worth syncing changed since [`Self::sync_record`].
    pub fn has_unsynced_changes(&self) -> bool {
        self.unsynced
    }

    /// This device's state for the other devices.
    pub fn sync_record(&mut self, device: DeviceId, name: &str) -> DeviceRecord {
        self.unsynced = false;
        let cookies = self.cookies_to_share();
        DeviceRecord {
            device,
            name: name.to_owned(),
            updated_at: self.now(),
            bookmarks: self.bookmarks.clone(),
            history: self.history.recent(DeviceRecord::HISTORY_LIMIT).to_vec(),
            tabs: self
                .session
                .tabs()
                .iter()
                .filter(|t| !t.is_private())
                .filter_map(|t| {
                    Some(RemoteTab {
                        address: t.address()?.clone(),
                        title: t.title().to_owned(),
                    })
                })
                .collect(),
            settings: self.settings.clone(),
            credentials: self.credentials.clone(),
            cookies,
        }
    }

    /// The engine's cookies of the sites recently visited, remembered as
    /// seen: once deleted here (signed out), the others' copies of them
    /// don't come back.
    fn cookies_to_share(&mut self) -> Vec<SiteCookie> {
        if !self.settings.device.sync_cookies {
            return Vec::new();
        }
        let Some(cookies) = self.engine.site_cookies() else {
            return Vec::new();
        };
        let mut hosts: Vec<String> = Vec::new();
        for visit in self.history.visits().iter().rev() {
            if let Some(host) = visit.address.host() {
                let host = host.to_ascii_lowercase();
                if !hosts.contains(&host) {
                    hosts.push(host);
                }
            }
        }
        let shared = cookies_to_share(cookies, &hosts, self.now(), DeviceRecord::COOKIES_BUDGET);
        if self
            .seen_cookies
            .add(shared.iter().map(SiteCookie::fingerprint))
        {
            // Best effort: at worst, a cookie deleted here comes back once.
            let _ = self.profile.seen_cookies.save(&self.seen_cookies);
        }
        shared
    }

    /// Sets the other devices' cookies that this one never had.
    fn import_cookies(&mut self, records: &[DeviceRecord]) -> Result<(), BrowserError> {
        if !self.settings.device.sync_cookies || records.iter().all(|r| r.cookies.is_empty()) {
            return Ok(());
        }
        let Some(local) = self.engine.site_cookies() else {
            return Ok(());
        };
        let mut newest_first: Vec<_> = records.iter().collect();
        newest_first.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
        let import = cookies_to_import(
            &local,
            newest_first.iter().flat_map(|r| &r.cookies),
            self.seen_cookies.set(),
            self.now(),
        );
        if import.is_empty() {
            return Ok(());
        }
        self.engine.set_site_cookies(&import);
        self.seen_cookies
            .add(import.iter().map(SiteCookie::fingerprint));
        self.profile.seen_cookies.save(&self.seen_cookies)?;
        Ok(())
    }

    /// The saved sign-ins.
    pub fn credentials(&self) -> &Credentials {
        &self.credentials
    }

    /// The sign-in the user is asked to save, if any.
    pub fn pending_credential(&self) -> Option<&PendingCredential> {
        self.pending_credential.as_ref()
    }

    /// A page of `tab` sent a sign-in form. With `password` empty, the first
    /// step of a two-step one (the user name only), kept for the next step.
    /// Asks to save it when it is new (see [`Self::pending_credential`]).
    pub fn offer_credential(&mut self, tab: TabId, username: &str, password: &str) {
        if !self.settings.device.save_passwords {
            return;
        }
        let Some(t) = self.session.tab(tab) else {
            return;
        };
        if t.is_private() {
            return;
        }
        let Some(origin) = t.address().and_then(WebAddress::origin) else {
            return;
        };
        let now = self.now();
        let username = username.trim();
        if password.is_empty() {
            if !username.is_empty() {
                self.typed_usernames
                    .insert(tab, (origin, username.to_owned(), now));
            }
            return;
        }
        let username = match self.typed_usernames.get(&tab) {
            Some((o, typed, at))
                if username.is_empty()
                    && *o == origin
                    && now.saturating_sub(*at) <= USERNAME_KEPT_FOR =>
            {
                typed.clone()
            }
            _ => username.to_owned(),
        };
        let update = match self.credentials.offer(&origin, &username, password) {
            Offer::Nothing => return,
            Offer::Save => false,
            Offer::Update => true,
        };
        self.pending_credential = Some(PendingCredential {
            tab,
            origin,
            username,
            password: password.to_owned(),
            update,
        });
    }

    /// What the user answered to [`Self::pending_credential`].
    pub fn answer_credential(&mut self, answer: CredentialAnswer) -> Result<(), BrowserError> {
        let Some(p) = self.pending_credential.take() else {
            return Ok(());
        };
        self.typed_usernames.remove(&p.tab);
        let now = self.now();
        match answer {
            CredentialAnswer::NotNow => return Ok(()),
            CredentialAnswer::Save => {
                self.credentials
                    .save(&p.origin, &p.username, &p.password, now)
            }
            CredentialAnswer::Never => self.credentials.never_for(&p.origin, now),
        }
        self.save_credentials()
    }

    /// Deletes a saved sign-in (with an empty `username`, the "never for
    /// this site" entry of `origin`).
    pub fn remove_credential(&mut self, origin: &str, username: &str) -> Result<(), BrowserError> {
        if self.credentials.remove(origin, username, self.now()) {
            self.save_credentials()?;
        }
        Ok(())
    }

    /// Adds sign-ins of another browser (page address, user name,
    /// password): those not saved yet; one already saved keeps its
    /// password. Returns how many were added.
    pub fn import_credentials(
        &mut self,
        items: impl IntoIterator<Item = (WebAddress, String, String)>,
    ) -> Result<usize, BrowserError> {
        let now = self.now();
        let mut added = 0;
        for (address, username, password) in items {
            let Some(origin) = address.origin() else {
                continue;
            };
            let username = username.trim();
            if self.credentials.offer(&origin, username, &password) == Offer::Save {
                self.credentials.save(&origin, username, &password, now);
                added += 1;
            }
        }
        if added > 0 {
            self.save_credentials()?;
        }
        Ok(added)
    }

    fn save_credentials(&mut self) -> Result<(), BrowserError> {
        self.unsynced = true;
        self.profile.credentials.save(&self.credentials)?;
        Ok(())
    }

    /// Fills `tab`'s sign-in form with the latest sign-in saved for the
    /// origin of `address`.
    fn fill_credentials(&mut self, tab: TabId, address: &WebAddress) {
        if !self.settings.device.save_passwords {
            return;
        }
        let Some(origin) = address.origin() else {
            return;
        };
        if let Some(c) = self.credentials.for_origin(&origin).first() {
            let (username, password) = (c.username.clone(), c.password.clone());
            self.engine
                .fill_credentials(tab, &origin, &username, &password);
        }
    }

    /// Merges the other devices' records into this one's data and saves
    /// what changed.
    pub fn apply_remote(&mut self, records: &[DeviceRecord]) -> Result<(), BrowserError> {
        let mut bookmarks_changed = false;
        let mut settings_changed = false;
        let mut credentials_changed = false;
        for r in records {
            bookmarks_changed |= self.bookmarks.merge(&r.bookmarks);
            self.history_dirty |= self.history.merge(&r.history);
            settings_changed |= self.settings.merge(&r.settings);
            credentials_changed |= self.credentials.merge(&r.credentials);
        }
        self.remote_devices = records
            .iter()
            .map(|r| RemoteDevice {
                id: r.device.clone(),
                name: r.name.clone(),
                updated_at: r.updated_at,
                tabs: r.tabs.clone(),
            })
            .collect();
        self.remote_devices.sort_by(|a, b| a.name.cmp(&b.name));
        if bookmarks_changed {
            self.profile.bookmarks.save(&self.bookmarks)?;
        }
        if settings_changed {
            self.profile.settings.save(&self.settings)?;
        }
        if credentials_changed {
            self.profile.credentials.save(&self.credentials)?;
        }
        self.import_cookies(records)
    }

    /// Replaces the settings (they win over older ones on other devices).
    pub fn update_settings(&mut self, mut settings: Settings) -> Result<(), BrowserError> {
        settings.updated_at = self.now();
        self.settings = settings;
        self.unsynced = true;
        self.profile.settings.save(&self.settings)?;
        Ok(())
    }

    /// Replaces this device's own settings (not synced).
    pub fn update_device_settings(
        &mut self,
        device: domain::DeviceSettings,
    ) -> Result<(), BrowserError> {
        if self.settings.device != device {
            self.settings.device = device;
            self.profile.settings.save(&self.settings)?;
        }
        Ok(())
    }

    /// Loads `address` in the active tab or a new one.
    pub fn load(&mut self, address: WebAddress, target: Target) -> Result<(), BrowserError> {
        match (target, self.session.active_id()) {
            (Target::ActiveTab, Some(id)) => self.engine.navigate(id, &address)?,
            _ => {
                self.open_tab_in(Some(address), self.active_is_private())?;
            }
        }
        Ok(())
    }

    pub fn engine(&self) -> &E {
        &self.engine
    }

    pub fn engine_mut(&mut self) -> &mut E {
        &mut self.engine
    }

    /// Opens a tab for a link of `opener`'s page, next to it, and leaves
    /// `opener` shown. Without `opener` (or when it is closed), a tab after
    /// the last one, shown.
    pub fn open_link_tab(
        &mut self,
        opener: Option<TabId>,
        address: WebAddress,
    ) -> Result<TabId, BrowserError> {
        let Some(id) = opener.and_then(|o| self.session.open_tab_from(o)) else {
            return self.open_tab_in(Some(address), self.active_is_private());
        };
        let opened = if self.session.tab(id).is_some_and(|t| t.is_private()) {
            self.engine.open_private_view(id, &address)
        } else {
            self.engine.open_view(id, &address)
        };
        if let Err(e) = opened {
            self.session.close_tab(id);
            return Err(e.into());
        }
        Ok(id)
    }

    /// Moves tab `id` to index `to` of the tab strip.
    pub fn move_tab(&mut self, id: TabId, to: usize) {
        self.session.move_tab(id, to);
    }

    /// Pins tab `id` (first, small, out of its group) or unpins it.
    pub fn set_pinned(&mut self, id: TabId, pinned: bool) {
        self.session.set_pinned(id, pinned);
    }

    /// Puts tab `id` in a new group of its own.
    pub fn new_group(&mut self, id: TabId) -> Option<GroupId> {
        self.session.new_group(id)
    }

    /// Moves tab `id` to the end of `group`.
    pub fn add_to_group(&mut self, id: TabId, group: GroupId) {
        self.session.add_to_group(id, group);
    }

    /// Takes tab `id` out of its group.
    pub fn remove_from_group(&mut self, id: TabId) {
        self.session.remove_from_group(id);
    }

    /// Takes all the tabs of `group` out of it.
    pub fn ungroup(&mut self, group: GroupId) {
        self.session.ungroup(group);
    }

    pub fn rename_group(&mut self, group: GroupId, name: &str) {
        self.session.rename_group(group, name);
    }

    pub fn set_group_color(&mut self, group: GroupId, color: GroupColor) {
        self.session.set_group_color(group, color);
    }

    /// Collapses or expands `group`; collapsed with the active tab, another
    /// one is shown.
    pub fn set_group_collapsed(&mut self, group: GroupId, collapsed: bool) {
        let before = self.session.active_id();
        self.session.set_group_collapsed(group, collapsed);
        if self.session.active_id() != before {
            self.engine.show(self.session.active_id());
        }
    }

    /// Closes every tab of `group`.
    pub fn close_group(&mut self, group: GroupId) {
        for id in self.session.group_tabs(group) {
            self.close_tab(id);
        }
    }

    /// What the address bar proposes for `input`: past searches and pages
    /// of the history.
    pub fn suggest(&self, input: &str, limit: usize) -> Vec<Suggestion> {
        suggest(input, &self.history, &self.settings.search_engine, limit)
    }

    /// Opens a tab on `address` (the home page when `None`) and shows it.
    pub fn open_tab(&mut self, address: Option<WebAddress>) -> Result<TabId, BrowserError> {
        self.open_tab_in(address, false)
    }

    /// [`Self::open_tab`], private or not: a private tab's pages stay out of
    /// the history, the saved session and sync.
    pub fn open_tab_in(
        &mut self,
        address: Option<WebAddress>,
        private: bool,
    ) -> Result<TabId, BrowserError> {
        let address = address.unwrap_or_else(|| self.settings.home_page.clone());
        let id = if private {
            self.session.open_private_tab()
        } else {
            self.session.open_tab()
        };
        let opened = if private {
            self.engine.open_private_view(id, &address)
        } else {
            self.engine.open_view(id, &address)
        };
        if let Err(e) = opened {
            self.session.close_tab(id);
            return Err(e.into());
        }
        self.engine.show(Some(id));
        Ok(id)
    }

    pub fn close_tab(&mut self, id: TabId) {
        // Not a private one: it would come back in the open.
        let index = self.session.tabs().iter().position(|t| t.id() == id);
        if let (Some(index), Some(tab)) = (index, self.session.tab(id))
            && !tab.is_private()
            && let Some(address) = tab.address()
        {
            if self.closed.len() == CLOSED_TABS {
                self.closed.remove(0);
            }
            self.closed.push((address.clone(), index));
        }
        self.typed_usernames.remove(&id);
        if self.session.close_tab(id) {
            self.engine.close_view(id);
            self.engine.show(self.session.active_id());
        }
    }

    /// Whether a closed tab can be reopened.
    pub fn can_reopen_closed_tab(&self) -> bool {
        !self.closed.is_empty()
    }

    /// Reopens the last tab closed where it was, shown (Ctrl+Shift+T).
    pub fn reopen_closed_tab(&mut self) -> Result<Option<TabId>, BrowserError> {
        let Some((address, index)) = self.closed.pop() else {
            return Ok(None);
        };
        let id = self.open_tab(Some(address))?;
        self.session.move_tab(id, index);
        Ok(Some(id))
    }

    pub fn close_active_tab(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.close_tab(id);
        }
    }

    /// Closes every private tab; returns their ids.
    pub fn close_private_tabs(&mut self) -> Vec<TabId> {
        let ids: Vec<TabId> = self
            .session
            .tabs()
            .iter()
            .filter(|t| t.is_private())
            .map(|t| t.id())
            .collect();
        for &id in &ids {
            self.close_tab(id);
        }
        ids
    }

    /// Whether the active tab is private: the tabs it opens are too.
    fn active_is_private(&self) -> bool {
        self.session.active().is_some_and(|t| t.is_private())
    }

    pub fn activate(&mut self, id: TabId) {
        if self.session.activate(id) {
            self.engine.show(Some(id));
        }
    }

    /// Activates the tab `offset` places away from the active one.
    pub fn cycle_tabs(&mut self, offset: isize) {
        if let Some(id) = self.session.neighbour(offset) {
            self.activate(id);
        }
    }

    /// Loads what was typed in the address bar in the active tab (in a new
    /// one when there is none). Returns `false` when there was nothing to load.
    pub fn submit_omnibox(&mut self, input: &str) -> Result<bool, BrowserError> {
        self.submit(input, Target::ActiveTab)
    }

    /// Loads typed input (address, host name or search) in `target`.
    /// Returns `false` when there was nothing to load.
    pub fn submit(&mut self, input: &str, target: Target) -> Result<bool, BrowserError> {
        let Some(address) = resolve_omnibox(input, &self.settings.search_engine) else {
            return Ok(false);
        };
        self.load(address, target)?;
        Ok(true)
    }

    pub fn go_home(&mut self) -> Result<(), BrowserError> {
        self.load(self.settings.home_page.clone(), Target::ActiveTab)
    }

    pub fn go_back(&mut self) {
        if let Some(tab) = self.session.active()
            && tab.history().can_go_back()
        {
            self.engine.go_back(tab.id());
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(tab) = self.session.active()
            && tab.history().can_go_forward()
        {
            self.engine.go_forward(tab.id());
        }
    }

    pub fn reload(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.engine.reload(id);
        }
    }

    /// Reloads the active tab without the cache (Ctrl+Shift+R).
    pub fn reload_bypassing_cache(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.engine.reload_bypassing_cache(id);
        }
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.engine.set_viewport(viewport);
    }

    /// See [`WebEngine::set_page_holes`].
    pub fn set_page_holes(&mut self, holes: &[Viewport]) {
        self.engine.set_page_holes(holes);
    }

    /// Applies what the engine reported. Events about closed tabs are ignored.
    pub fn handle(&mut self, event: EngineEvent) -> Result<(), BrowserError> {
        match event {
            EngineEvent::LoadStarted { tab, address } => {
                if let Some(t) = self.session.tab_mut(tab) {
                    t.load_started(address);
                }
            }
            EngineEvent::LoadFinished { tab, address } => {
                if self.session.tab(tab).is_some_and(|t| !t.is_private()) {
                    self.fill_credentials(tab, &address);
                }
                if let Some(t) = self.session.tab_mut(tab) {
                    t.load_finished(address.clone());
                    let title = t.title().to_owned();
                    if !t.is_private()
                        && self
                            .history
                            .record(address.clone(), self.profile.clock.now())
                    {
                        // The title may have come before the end of the load.
                        self.history.retitle(&address, &title);
                        self.history_dirty = true;
                        self.unsynced = true;
                    }
                }
            }
            EngineEvent::TitleChanged { tab, title } => {
                if let Some(t) = self.session.tab_mut(tab) {
                    t.set_title(title);
                    if !t.is_private()
                        && let Some(address) = t.address()
                        && self.history.retitle(address, t.title())
                    {
                        self.history_dirty = true;
                    }
                }
            }
            EngineEvent::NewTabRequested { opener, address } => {
                self.open_link_tab(opener, address)?;
            }
            // Kept for the site's next pages; this device's, not synced.
            EngineEvent::ZoomChanged { host, percent } => {
                if self.settings.device.set_zoom(&host, percent) {
                    self.profile.settings.save(&self.settings)?;
                }
            }
            // Answered by the presentation (page scripts' bridge).
            EngineEvent::PageMessage { .. } => {}
            // Shown by the presentation.
            EngineEvent::FullScreenChanged { .. }
            | EngineEvent::FaviconChanged { .. }
            | EngineEvent::AudioChanged { .. } => {}
            // Shown by the presentation.
            EngineEvent::DownloadStarted { .. }
            | EngineEvent::DownloadFinished { .. }
            | EngineEvent::ExtensionOffered { .. } => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::ports::{Clock, EngineError, Repository, RepositoryError};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        Open(u64, String),
        Close(u64),
        Show(Option<u64>),
        Navigate(u64, String),
        Back(u64),
        Forward(u64),
        Reload(u64),
        Viewport(Viewport),
        Fill(u64, String, String),
        SetCookies(Vec<String>),
    }

    #[derive(Default)]
    struct FakeEngine {
        calls: Vec<Call>,
        fail_open: bool,
        cookies: Option<Vec<SiteCookie>>,
    }

    impl WebEngine for FakeEngine {
        fn open_view(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
            if self.fail_open {
                return Err(EngineError("boom".into()));
            }
            self.calls.push(Call::Open(tab.get(), address.to_string()));
            Ok(())
        }
        fn open_private_view(
            &mut self,
            tab: TabId,
            address: &WebAddress,
        ) -> Result<(), EngineError> {
            self.open_view(tab, address)
        }
        fn close_view(&mut self, tab: TabId) {
            self.calls.push(Call::Close(tab.get()));
        }
        fn show(&mut self, tab: Option<TabId>) {
            self.calls.push(Call::Show(tab.map(TabId::get)));
        }
        fn navigate(&mut self, tab: TabId, address: &WebAddress) -> Result<(), EngineError> {
            self.calls
                .push(Call::Navigate(tab.get(), address.to_string()));
            Ok(())
        }
        fn go_back(&mut self, tab: TabId) {
            self.calls.push(Call::Back(tab.get()));
        }
        fn go_forward(&mut self, tab: TabId) {
            self.calls.push(Call::Forward(tab.get()));
        }
        fn reload(&mut self, tab: TabId) {
            self.calls.push(Call::Reload(tab.get()));
        }
        fn set_viewport(&mut self, viewport: Viewport) {
            self.calls.push(Call::Viewport(viewport));
        }
        fn fill_credentials(&mut self, tab: TabId, origin: &str, username: &str, _: &str) {
            self.calls
                .push(Call::Fill(tab.get(), origin.into(), username.into()));
        }
        fn site_cookies(&mut self) -> Option<Vec<SiteCookie>> {
            self.cookies.clone()
        }
        fn set_site_cookies(&mut self, cookies: &[SiteCookie]) {
            self.calls.push(Call::SetCookies(
                cookies.iter().map(|c| c.value.clone()).collect(),
            ));
            if let Some(all) = &mut self.cookies {
                all.extend_from_slice(cookies);
            }
        }
    }

    /// A repository whose content the test can read and preset.
    struct MemoryRepository<T>(Arc<Mutex<Option<T>>>);

    impl<T: Clone + Default + Send> Repository<T> for MemoryRepository<T> {
        fn load(&self) -> Result<T, RepositoryError> {
            Ok(self.0.lock().unwrap().clone().unwrap_or_default())
        }
        fn save(&self, value: &T) -> Result<(), RepositoryError> {
            *self.0.lock().unwrap() = Some(value.clone());
            Ok(())
        }
    }

    struct FailingRepository;

    impl<T> Repository<T> for FailingRepository {
        fn load(&self) -> Result<T, RepositoryError> {
            Err(RepositoryError("disk".into()))
        }
        fn save(&self, _: &T) -> Result<(), RepositoryError> {
            Err(RepositoryError("disk".into()))
        }
    }

    struct FixedClock;

    impl Clock for FixedClock {
        fn now(&self) -> domain::Timestamp {
            1_000
        }
    }

    type Cell<T> = Arc<Mutex<Option<T>>>;

    #[derive(Default)]
    struct Store {
        settings: Cell<Settings>,
        history: Cell<BrowsingHistory>,
        bookmarks: Cell<Bookmarks>,
        session: Cell<SessionSnapshot>,
        credentials: Cell<Credentials>,
        seen_cookies: Cell<SeenCookies>,
    }

    impl Store {
        fn profile(&self) -> Profile {
            Profile {
                settings: Box::new(MemoryRepository(self.settings.clone())),
                history: Box::new(MemoryRepository(self.history.clone())),
                bookmarks: Box::new(MemoryRepository(self.bookmarks.clone())),
                session: Box::new(MemoryRepository(self.session.clone())),
                credentials: Box::new(MemoryRepository(self.credentials.clone())),
                seen_cookies: Box::new(MemoryRepository(self.seen_cookies.clone())),
                clock: Box::new(FixedClock),
            }
        }
    }

    fn browser_with(store: &Store) -> Browser<FakeEngine> {
        Browser::new(FakeEngine::default(), Settings::default(), store.profile())
    }

    fn browser() -> Browser<FakeEngine> {
        browser_with(&Store::default())
    }

    fn finish(b: &mut Browser<FakeEngine>, tab: TabId, url: &str) {
        b.handle(EngineEvent::LoadFinished {
            tab,
            address: addr(url),
        })
        .unwrap();
    }

    #[test]
    fn finished_loads_are_recorded_with_their_title() {
        let store = Store::default();
        let mut b = browser_with(&store);
        let id = b.open_tab(None).unwrap();
        b.handle(EngineEvent::TitleChanged {
            tab: id,
            title: "early".into(),
        })
        .unwrap();
        finish(&mut b, id, "https://a.com/");
        b.handle(EngineEvent::TitleChanged {
            tab: id,
            title: "A".into(),
        })
        .unwrap();
        assert_eq!(b.history().len(), 1);
        assert_eq!(b.history().visits()[0].title, "A");
        assert_eq!(b.history().visits()[0].at, 1_000);

        assert!(
            store.history.lock().unwrap().is_none(),
            "saved only on persist"
        );
        assert!(b.has_unsaved_changes());
        b.persist().unwrap();
        assert!(!b.has_unsaved_changes());
        assert_eq!(store.history.lock().unwrap().as_ref().unwrap().len(), 1);

        b.clear_history().unwrap();
        assert!(store.history.lock().unwrap().as_ref().unwrap().is_empty());
    }

    #[test]
    fn bookmarks_are_toggled_on_the_active_page_and_saved() {
        let store = Store::default();
        let mut b = browser_with(&store);
        assert!(!b.toggle_bookmark().unwrap());
        let id = b.open_tab(None).unwrap();
        assert!(!b.toggle_bookmark().unwrap(), "nothing loaded yet");
        finish(&mut b, id, "https://a.com/");
        assert!(b.toggle_bookmark().unwrap());
        assert!(b.is_active_bookmarked());
        assert_eq!(store.bookmarks.lock().unwrap().as_ref().unwrap().len(), 1);
        b.remove_bookmark(&addr("https://a.com/")).unwrap();
        assert!(!b.is_active_bookmarked());
        assert!(store.bookmarks.lock().unwrap().as_ref().unwrap().is_empty());
    }

    #[test]
    fn profile_errors_are_reported_and_leave_empty_data() {
        let mut b = Browser::new(
            FakeEngine::default(),
            Settings::default(),
            Profile {
                settings: Box::new(FailingRepository),
                history: Box::new(FailingRepository),
                bookmarks: Box::new(FailingRepository),
                session: Box::new(FailingRepository),
                credentials: Box::new(FailingRepository),
                seen_cookies: Box::new(FailingRepository),
                clock: Box::new(FixedClock),
            },
        );
        assert_eq!(b.load_profile().len(), 4);
        assert!(b.history().is_empty());
        // An unreadable session falls back to the home page.
        assert_eq!(b.restore_session().unwrap(), 0);
        assert_eq!(b.session().len(), 1);
    }

    #[test]
    fn session_is_saved_and_restored_in_order() {
        let store = Store::default();
        let mut b = browser_with(&store);
        b.restore_session().unwrap();
        let first = b.session().active_id().unwrap();
        finish(&mut b, first, "https://a.com/");
        let second = b.open_tab(None).unwrap();
        finish(&mut b, second, "https://b.com/");
        b.activate(first);
        b.persist().unwrap();

        let saved = store.session.lock().unwrap().clone().unwrap();
        assert_eq!(
            saved.tabs,
            vec![addr("https://a.com/"), addr("https://b.com/")]
        );
        assert_eq!(saved.active, Some(0));

        let mut restored = browser_with(&store);
        assert_eq!(restored.restore_session().unwrap(), 2);
        let calls = take_calls(&mut restored);
        assert!(calls.contains(&Call::Open(1, "https://a.com/".into())));
        assert!(calls.contains(&Call::Open(2, "https://b.com/".into())));
        assert_eq!(calls.last(), Some(&Call::Show(Some(1))));
        assert_eq!(restored.session().active_id().map(TabId::get), Some(1));
    }

    fn device(id: &str) -> DeviceId {
        DeviceId::new(id).unwrap()
    }

    #[test]
    fn the_sync_record_carries_bookmarks_history_tabs_and_settings() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        finish(&mut b, id, "https://a.com/");
        b.toggle_bookmark().unwrap();
        assert!(b.has_unsynced_changes());
        let record = b.sync_record(device("desktop"), "Desktop");
        assert!(!b.has_unsynced_changes());
        assert_eq!(record.name, "Desktop");
        assert_eq!(record.bookmarks.len(), 1);
        assert_eq!(record.history.len(), 1);
        assert_eq!(record.tabs.len(), 1);
        assert_eq!(record.tabs[0].address, addr("https://a.com/"));
    }

    #[test]
    fn remote_records_are_merged_and_saved() {
        let store = Store::default();
        let mut b = browser_with(&store);
        let mut phone = browser();
        let id = phone.open_tab(None).unwrap();
        finish(&mut phone, id, "https://phone.com/");
        phone.toggle_bookmark().unwrap();
        phone
            .update_settings(Settings {
                home_page: addr("https://home.com/"),
                ..Settings::default()
            })
            .unwrap();
        let record = phone.sync_record(device("phone"), "Phone");

        b.apply_remote(std::slice::from_ref(&record)).unwrap();
        assert!(b.bookmarks().contains(&addr("https://phone.com/")));
        assert_eq!(b.history().len(), 1);
        assert_eq!(b.settings().home_page, addr("https://home.com/"));
        assert_eq!(b.remote_devices().len(), 1);
        assert_eq!(b.remote_devices()[0].name, "Phone");
        assert_eq!(b.remote_devices()[0].tabs.len(), 1);
        // Bookmarks and settings are saved right away, history on persist.
        assert_eq!(store.bookmarks.lock().unwrap().as_ref().unwrap().len(), 1);
        assert!(store.settings.lock().unwrap().is_some());
        assert!(b.has_unsaved_changes());
    }

    fn signed_in_browser() -> (Store, Browser<FakeEngine>, TabId) {
        let store = Store::default();
        let mut settings = Settings::default();
        settings.device.save_passwords = true;
        let mut b = Browser::new(FakeEngine::default(), settings, store.profile());
        let id = b.open_tab(None).unwrap();
        finish(&mut b, id, "https://a.com/login");
        (store, b, id)
    }

    #[test]
    fn typed_sign_ins_are_offered_saved_and_filled_in() {
        let (store, mut b, id) = signed_in_browser();
        b.offer_credential(id, "me", "");
        assert!(b.pending_credential().is_none(), "the user name alone");
        b.offer_credential(id, "", "secret");
        let pending = b.pending_credential().unwrap();
        assert_eq!(pending.origin, "https://a.com");
        assert_eq!(pending.username, "me", "from the first step");
        assert!(!pending.update);
        b.answer_credential(CredentialAnswer::Save).unwrap();
        assert!(b.pending_credential().is_none());
        assert!(b.has_unsynced_changes());
        let saved = store.credentials.lock().unwrap().clone().unwrap();
        assert_eq!(saved.for_origin("https://a.com")[0].password, "secret");

        // The same again: nothing to ask. Another password: an update.
        b.offer_credential(id, "me", "secret");
        assert!(b.pending_credential().is_none());
        b.offer_credential(id, "me", "new");
        assert!(b.pending_credential().unwrap().update);
        b.answer_credential(CredentialAnswer::NotNow).unwrap();

        take_calls(&mut b);
        finish(&mut b, id, "https://a.com/again");
        assert!(take_calls(&mut b).contains(&Call::Fill(
            id.get(),
            "https://a.com".into(),
            "me".into()
        )));
    }

    #[test]
    fn never_means_never_and_the_setting_turns_it_all_off() {
        let (_, mut b, id) = signed_in_browser();
        b.offer_credential(id, "me", "pw");
        b.answer_credential(CredentialAnswer::Never).unwrap();
        b.offer_credential(id, "me", "pw");
        assert!(b.pending_credential().is_none());
        b.remove_credential("https://a.com", "").unwrap();
        b.offer_credential(id, "me", "pw");
        assert!(b.pending_credential().is_some());

        let mut settings = Settings::default();
        settings.device.save_passwords = false;
        let mut off = Browser::new(FakeEngine::default(), settings, Store::default().profile());
        let id = off.open_tab(None).unwrap();
        finish(&mut off, id, "https://a.com/");
        off.offer_credential(id, "me", "pw");
        assert!(off.pending_credential().is_none());
    }

    fn cookie(domain: &str, value: &str) -> SiteCookie {
        SiteCookie {
            domain: domain.into(),
            name: "sid".into(),
            value: value.into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            same_site: String::new(),
            expires: None,
        }
    }

    #[test]
    fn cookies_go_to_the_others_and_come_back_only_once() {
        let mut settings = Settings::default();
        settings.device.sync_cookies = true;
        let store = Store::default();
        let mut b = Browser::new(FakeEngine::default(), settings.clone(), store.profile());
        b.engine_mut().cookies = Some(vec![cookie("a.com", "mine"), cookie("ads.net", "x")]);
        let id = b.open_tab(None).unwrap();
        finish(&mut b, id, "https://a.com/");
        let record = b.sync_record(device("desktop"), "Desktop");
        assert_eq!(
            record.cookies,
            vec![cookie("a.com", "mine")],
            "visited sites only"
        );

        let mut phone = Browser::new(FakeEngine::default(), settings, Store::default().profile());
        phone.engine_mut().cookies = Some(vec![]);
        phone.apply_remote(std::slice::from_ref(&record)).unwrap();
        assert_eq!(
            take_calls(&mut phone),
            vec![Call::SetCookies(vec!["mine".into()])]
        );
        // Signed out on the phone: the desktop's copy does not come back.
        phone.engine_mut().cookies = Some(vec![]);
        phone.apply_remote(std::slice::from_ref(&record)).unwrap();
        assert!(take_calls(&mut phone).is_empty());

        // Sync of cookies off: nothing shared.
        b.update_device_settings(domain::DeviceSettings {
            sync_cookies: false,
            ..Default::default()
        })
        .unwrap();
        assert!(b.sync_record(device("desktop"), "D").cookies.is_empty());
    }

    #[test]
    fn credentials_travel_in_the_records() {
        let (_, mut b, id) = signed_in_browser();
        b.offer_credential(id, "me", "pw");
        b.answer_credential(CredentialAnswer::Save).unwrap();
        let record = b.sync_record(device("desktop"), "Desktop");
        let store = Store::default();
        let mut phone = browser_with(&store);
        phone.apply_remote(std::slice::from_ref(&record)).unwrap();
        assert_eq!(phone.credentials().saved().len(), 1);
        assert!(store.credentials.lock().unwrap().is_some());
    }

    #[test]
    fn imported_sign_ins_add_only_the_new_ones() {
        let (store, mut b, id) = signed_in_browser();
        b.offer_credential(id, "me", "mine");
        b.answer_credential(CredentialAnswer::Save).unwrap();
        let origin = b.credentials().saved()[0].origin.clone();
        let added = b
            .import_credentials([
                (
                    addr(&format!("{origin}/login")),
                    "me".into(),
                    "theirs".into(),
                ),
                (addr("https://other.org/x"), " bob ".into(), "pw".into()),
                (addr("https://other.org/y"), "bob".into(), "pw".into()),
            ])
            .unwrap();
        assert_eq!(added, 1);
        let mine = b.credentials().for_origin(&origin);
        assert_eq!(mine[0].password, "mine");
        let other = b.credentials().for_origin("https://other.org");
        assert_eq!(other[0].username, "bob");
        assert!(store.credentials.lock().unwrap().is_some());
    }

    #[test]
    fn imported_history_skips_known_visits() {
        let store = Store::default();
        let mut b = browser_with(&store);
        let visit = |url: &str, at| Visit {
            address: addr(url),
            title: "T".into(),
            at,
        };
        let pages = || {
            [
                visit("https://a.org/", 10),
                visit("https://b.org/", 20),
                visit("about:blank", 30),
            ]
        };
        assert_eq!(b.import_history(pages()).unwrap(), 2);
        assert_eq!(b.history().len(), 2);
        assert!(store.history.lock().unwrap().is_some());
        assert_eq!(b.import_history(pages()).unwrap(), 0);
        assert_eq!(b.history().len(), 2);
    }

    #[test]
    fn load_targets_the_active_or_a_new_tab() {
        let mut b = browser();
        b.load(addr("https://a.com/"), Target::ActiveTab).unwrap();
        assert_eq!(b.session().len(), 1);
        b.load(addr("https://b.com/"), Target::ActiveTab).unwrap();
        assert_eq!(b.session().len(), 1);
        b.load(addr("https://c.com/"), Target::NewTab).unwrap();
        assert_eq!(b.session().len(), 2);
    }

    fn addr(s: &str) -> WebAddress {
        WebAddress::parse(s).unwrap()
    }

    fn take_calls(b: &mut Browser<FakeEngine>) -> Vec<Call> {
        std::mem::take(&mut b.engine_mut().calls)
    }

    #[test]
    fn open_tab_loads_the_home_page_and_shows_it() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        assert_eq!(
            take_calls(&mut b),
            vec![
                Call::Open(id.get(), "https://duckduckgo.com/".into()),
                Call::Show(Some(id.get())),
            ]
        );
        assert_eq!(b.session().active_id(), Some(id));
    }

    #[test]
    fn a_failed_open_leaves_no_tab() {
        let mut b = browser();
        b.engine_mut().fail_open = true;
        assert!(b.open_tab(None).is_err());
        assert!(b.session().is_empty());
    }

    #[test]
    fn closing_shows_the_next_active_tab() {
        let mut b = browser();
        let first = b.open_tab(None).unwrap();
        let second = b.open_tab(None).unwrap();
        take_calls(&mut b);
        b.close_tab(second);
        assert_eq!(
            take_calls(&mut b),
            vec![Call::Close(second.get()), Call::Show(Some(first.get()))]
        );
        b.close_active_tab();
        assert!(b.session().is_empty());
    }

    #[test]
    fn omnibox_navigates_the_active_tab() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        take_calls(&mut b);
        assert!(b.submit_omnibox("rust-lang.org").unwrap());
        assert!(!b.submit_omnibox("  ").unwrap());
        assert_eq!(
            take_calls(&mut b),
            vec![Call::Navigate(id.get(), "https://rust-lang.org/".into())]
        );
    }

    #[test]
    fn omnibox_without_tabs_opens_one() {
        let mut b = browser();
        b.submit_omnibox("hello").unwrap();
        assert_eq!(b.session().len(), 1);
    }

    #[test]
    fn back_and_forward_follow_the_mirrored_history() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        b.go_back();
        b.go_forward();
        take_calls(&mut b);
        for url in ["https://a.com/", "https://b.com/"] {
            b.handle(EngineEvent::LoadFinished {
                tab: id,
                address: addr(url),
            })
            .unwrap();
        }
        b.go_back();
        b.go_forward();
        assert_eq!(take_calls(&mut b), vec![Call::Back(id.get())]);
    }

    #[test]
    fn events_update_the_tab() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        b.handle(EngineEvent::LoadStarted {
            tab: id,
            address: addr("https://a.com/"),
        })
        .unwrap();
        assert!(b.session().active().unwrap().is_loading());
        b.handle(EngineEvent::TitleChanged {
            tab: id,
            title: "A".into(),
        })
        .unwrap();
        assert_eq!(b.session().active().unwrap().display_title(), "A");
        // Events for unknown tabs are ignored.
        b.handle(EngineEvent::TitleChanged {
            tab: TabId::new(42),
            title: "x".into(),
        })
        .unwrap();
    }

    #[test]
    fn links_open_in_a_tab_next_to_their_page_which_stays_shown() {
        let mut b = browser();
        let page = b.open_tab(None).unwrap();
        let last = b.open_tab(None).unwrap();
        b.activate(page);
        take_calls(&mut b);
        b.handle(EngineEvent::NewTabRequested {
            opener: Some(page),
            address: addr("https://link.com/"),
        })
        .unwrap();
        let tabs: Vec<TabId> = b.session().tabs().iter().map(|t| t.id()).collect();
        assert_eq!(tabs.len(), 3);
        let link = tabs[1];
        assert_eq!(tabs, vec![page, link, last]);
        assert_eq!(b.session().active_id(), Some(page));
        assert_eq!(
            take_calls(&mut b),
            vec![Call::Open(link.get(), "https://link.com/".into())],
            "opened but not shown"
        );
    }

    #[test]
    fn closed_tabs_reopen_where_they_were_last_first() {
        let mut b = browser();
        let a = b.open_tab(None).unwrap();
        finish(&mut b, a, "https://a.com/");
        let m = b.open_tab(None).unwrap();
        finish(&mut b, m, "https://m.com/");
        let z = b.open_tab(None).unwrap();
        finish(&mut b, z, "https://z.com/");
        b.close_tab(m);
        b.close_tab(a);
        let private = b.open_tab_in(Some(addr("https://p.com/")), true).unwrap();
        finish(&mut b, private, "https://p.com/");
        b.close_tab(private);
        let addresses = |b: &Browser<_>| -> Vec<String> {
            b.session()
                .tabs()
                .iter()
                .map(|t| t.address().unwrap().to_string())
                .collect()
        };
        // The private one is not kept.
        let first = b.reopen_closed_tab().unwrap().unwrap();
        assert_eq!(b.session().active_id(), Some(first));
        finish(&mut b, first, "https://a.com/");
        assert_eq!(addresses(&b), ["https://a.com/", "https://z.com/"]);
        let second = b.reopen_closed_tab().unwrap().unwrap();
        finish(&mut b, second, "https://m.com/");
        assert_eq!(
            addresses(&b),
            ["https://a.com/", "https://m.com/", "https://z.com/"]
        );
        assert!(!b.can_reopen_closed_tab());
        assert_eq!(b.reopen_closed_tab().unwrap(), None);
    }

    #[test]
    fn new_window_requests_without_opener_open_a_shown_tab() {
        let mut b = browser();
        b.open_tab(None).unwrap();
        let id = b.open_link_tab(None, addr("https://popup.com/")).unwrap();
        assert_eq!(b.session().len(), 2);
        assert_eq!(b.session().active_id(), Some(id));
    }

    #[test]
    fn tabs_are_moved_and_saved_in_their_new_order() {
        let store = Store::default();
        let mut b = browser_with(&store);
        let a = b.open_tab(None).unwrap();
        finish(&mut b, a, "https://a.com/");
        let c = b.open_tab(None).unwrap();
        finish(&mut b, c, "https://c.com/");
        b.move_tab(c, 0);
        b.persist().unwrap();
        let saved = store.session.lock().unwrap().clone().unwrap();
        assert_eq!(
            saved.tabs,
            vec![addr("https://c.com/"), addr("https://a.com/")]
        );
    }

    #[test]
    fn suggestions_come_from_the_history() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        finish(&mut b, id, "https://duckduckgo.com/?q=rust+book");
        finish(&mut b, id, "https://doc.rust-lang.org/book/");
        let s = b.suggest("rust", 8);
        assert_eq!(s.len(), 2);
        assert!(matches!(&s[0], Suggestion::Search { terms, .. } if terms == "rust book"));
    }

    #[test]
    fn cycling_and_reload_target_the_active_tab() {
        let mut b = browser();
        let first = b.open_tab(None).unwrap();
        let second = b.open_tab(None).unwrap();
        b.cycle_tabs(1);
        assert_eq!(b.session().active_id(), Some(first));
        take_calls(&mut b);
        b.reload();
        b.go_home().unwrap();
        b.set_viewport(Viewport::default());
        assert_eq!(
            take_calls(&mut b),
            vec![
                Call::Reload(first.get()),
                Call::Navigate(first.get(), "https://duckduckgo.com/".into()),
                Call::Viewport(Viewport::default()),
            ]
        );
        assert_ne!(first, second);
    }
}
