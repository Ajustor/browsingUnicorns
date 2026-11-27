//! Saved sign-ins: offered, answered, imported and filled in.

use domain::{Credentials, Offer, TabId, Timestamp, WebAddress};

use super::Browser;
use crate::BrowserError;
use crate::ports::WebEngine;

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

impl<E: WebEngine> Browser<E> {
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
    pub(super) fn fill_credentials(&mut self, tab: TabId, address: &WebAddress) {
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
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

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
}
