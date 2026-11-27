//! The password manager and the shared cookies on Android: what the app
//! shows (the "save this password?" bar, the saved passwords) and the
//! cookies of its WebViews, read and written by Kotlin.

use application::CredentialAnswer;
use domain::SiteCookie;
use infrastructure::page_scripts::credentials::{is_report, submitted};

use crate::{MobileBrowser, MobileError, PageTarget};

/// A sign-in the user is asked to save.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CredentialOffer {
    pub origin: String,
    pub username: String,
    /// It changes the password of a saved user name.
    pub update: bool,
}

/// A saved sign-in (`never`: "never for this site", no user name).
#[derive(Debug, Clone, uniffi::Record)]
pub struct SavedCredential {
    pub origin: String,
    pub username: String,
    pub never: bool,
}

#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum CredentialChoice {
    Save,
    NotNow,
    Never,
}

/// A cookie of the WebViews.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CookieInfo {
    /// With a leading dot, its subdomains too.
    pub domain: String,
    pub name: String,
    pub value: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// `Strict`, `Lax`, `None` or empty.
    pub same_site: String,
    /// Seconds since the epoch; `None`: a session cookie.
    pub expires: Option<i64>,
}

impl From<CookieInfo> for SiteCookie {
    fn from(c: CookieInfo) -> Self {
        Self {
            domain: c.domain,
            name: c.name,
            value: c.value,
            path: c.path,
            secure: c.secure,
            http_only: c.http_only,
            same_site: c.same_site,
            expires: c.expires,
        }
    }
}

impl From<&SiteCookie> for CookieInfo {
    fn from(c: &SiteCookie) -> Self {
        Self {
            domain: c.domain.clone(),
            name: c.name.clone(),
            value: c.value.clone(),
            path: c.path.clone(),
            secure: c.secure,
            http_only: c.http_only,
            same_site: c.same_site.clone(),
            expires: c.expires,
        }
    }
}

impl MobileBrowser {
    /// A sign-in reported by the password manager's page script, handled
    /// here; `false` for any other message. `BUBridge` is in every frame:
    /// a report without the token (that only the script of the top frame
    /// has) is dropped, so a frame cannot offer a password for the site it
    /// sits in.
    pub(crate) fn credential_message(&self, from: &PageTarget, message: &str) -> bool {
        if !is_report(message) {
            return false;
        }
        let Some((username, password)) = submitted(message) else {
            return true;
        };
        if let PageTarget::Tab { tab } = from {
            self.lock()
                .browser
                .offer_credential(domain::TabId::new(*tab), &username, &password);
        }
        true
    }
}

#[uniffi::export]
impl MobileBrowser {
    /// The sign-in the user is asked to save, if any.
    pub fn pending_credential(&self) -> Option<CredentialOffer> {
        self.lock()
            .browser
            .pending_credential()
            .map(|p| CredentialOffer {
                origin: p.origin.clone(),
                username: p.username.clone(),
                update: p.update,
            })
    }

    pub fn answer_credential(&self, choice: CredentialChoice) -> Result<(), MobileError> {
        let answer = match choice {
            CredentialChoice::Save => CredentialAnswer::Save,
            CredentialChoice::NotNow => CredentialAnswer::NotNow,
            CredentialChoice::Never => CredentialAnswer::Never,
        };
        self.lock()
            .browser
            .answer_credential(answer)
            .map_err(MobileError::new)
    }

    /// The saved sign-ins and the sites never saved, by site.
    pub fn saved_credentials(&self) -> Vec<SavedCredential> {
        let inner = self.lock();
        let mut all: Vec<_> = inner
            .browser
            .credentials()
            .items()
            .iter()
            .map(|c| SavedCredential {
                origin: c.origin.clone(),
                username: c.username.clone(),
                never: c.never,
            })
            .collect();
        all.sort_by(|a, b| (&a.origin, &a.username).cmp(&(&b.origin, &b.username)));
        all
    }

    pub fn remove_credential(&self, origin: String, username: String) -> Result<(), MobileError> {
        self.lock()
            .browser
            .remove_credential(&origin, &username)
            .map_err(MobileError::new)
    }

    /// Offer to save the passwords typed in pages, and fill them in.
    pub fn set_save_passwords(&self, on: bool) -> Result<(), MobileError> {
        self.change_device_settings(|d| d.save_passwords = on)
    }

    /// Share the sites' cookies with the other devices of the sync group.
    pub fn set_sync_cookies(&self, on: bool) -> Result<(), MobileError> {
        self.change_device_settings(|d| d.sync_cookies = on)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookies_convert_both_ways() {
        let info = CookieInfo {
            domain: ".a.com".into(),
            name: "sid".into(),
            value: "v".into(),
            path: "/".into(),
            secure: true,
            http_only: true,
            same_site: "Lax".into(),
            expires: Some(9),
        };
        let cookie = SiteCookie::from(info.clone());
        assert_eq!(cookie.domain, ".a.com");
        assert_eq!(CookieInfo::from(&cookie).expires, Some(9));
    }
}
