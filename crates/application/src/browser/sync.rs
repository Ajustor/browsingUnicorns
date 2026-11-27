//! Sync with the other devices, cookies included.

use domain::{DeviceId, DeviceRecord, RemoteTab, SiteCookie, cookies_to_import, cookies_to_share};

use super::{Browser, Stores};
use crate::BrowserError;
use crate::ports::WebEngine;
use crate::sync::RemoteDevice;

impl<E: WebEngine> Browser<E> {
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
            // Saved on the next persist: at worst, a cookie deleted here
            // comes back once.
            self.mark_dirty(Stores::SEEN_COOKIES);
        }
        shared
    }

    /// Sets the other devices' cookies that this one never had.
    fn import_cookies(&mut self, records: &[DeviceRecord]) {
        if !self.settings.device.sync_cookies || records.iter().all(|r| r.cookies.is_empty()) {
            return;
        }
        let Some(local) = self.engine.site_cookies() else {
            return;
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
            return;
        }
        self.engine.set_site_cookies(&import);
        self.seen_cookies
            .add(import.iter().map(SiteCookie::fingerprint));
        self.mark_dirty(Stores::SEEN_COOKIES);
    }

    /// Merges the other devices' records into this one's data and saves
    /// what changed (the history on the next persist). What fails to save
    /// stays to save: the next persist tries again.
    pub fn apply_remote(&mut self, records: &[DeviceRecord]) -> Result<(), BrowserError> {
        for r in records {
            if self.bookmarks.merge(&r.bookmarks) {
                self.mark_dirty(Stores::BOOKMARKS);
            }
            if self.history.merge(&r.history) {
                self.mark_dirty(Stores::HISTORY);
            }
            if self.settings.merge(&r.settings) {
                self.mark_dirty(Stores::SETTINGS);
            }
            if self.credentials.merge(&r.credentials) {
                self.mark_dirty(Stores::CREDENTIALS);
            }
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
        self.import_cookies(records);
        self.save_dirty(
            Stores::BOOKMARKS | Stores::SETTINGS | Stores::CREDENTIALS | Stores::SEEN_COOKIES,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::CredentialAnswer;
    use super::super::test_support::*;
    use super::*;

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

    #[test]
    fn a_store_failing_to_save_keeps_the_others_saved_and_is_retried() {
        let store = Store::default();
        let fail = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let mut profile = store.profile();
        profile.bookmarks = Box::new(FlakyRepository(
            MemoryRepository(store.bookmarks.clone()),
            fail.clone(),
        ));
        let mut b = Browser::new(FakeEngine::default(), Settings::default(), profile);
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

        assert!(b.apply_remote(std::slice::from_ref(&record)).is_err());
        assert!(store.settings.lock().unwrap().is_some(), "saved anyway");
        assert!(store.bookmarks.lock().unwrap().is_none());
        assert!(b.has_unsaved_changes());
        assert!(b.persist().is_err(), "still failing");
        fail.store(false, std::sync::atomic::Ordering::SeqCst);
        b.persist().unwrap();
        assert_eq!(store.bookmarks.lock().unwrap().as_ref().unwrap().len(), 1);
        assert!(!b.has_unsaved_changes());
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
}
