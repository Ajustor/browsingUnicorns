//! What one device shares with the others, and how it is merged in.

use std::fmt;

use crate::{Bookmarks, Credentials, Settings, SiteCookie, Timestamp, Visit, WebAddress};

/// Identifies a device in a sync group. Random, chosen once per install.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeviceId(String);

impl DeviceId {
    /// `raw` must be 1 to 64 characters among `[A-Za-z0-9_-]` (it is part
    /// of the relay's URLs).
    pub fn new(raw: impl Into<String>) -> Option<Self> {
        let raw = raw.into();
        let valid = !raw.is_empty()
            && raw.len() <= 64
            && raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        valid.then_some(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A tab open on a device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTab {
    pub address: WebAddress,
    pub title: String,
}

/// One device's state as the others see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    pub device: DeviceId,
    /// Shown to the user ("Pixel 9", "Desktop").
    pub name: String,
    pub updated_at: Timestamp,
    pub bookmarks: Bookmarks,
    /// Its latest visits (not the whole history, to keep records small).
    pub history: Vec<Visit>,
    pub tabs: Vec<RemoteTab>,
    pub settings: Settings,
    /// The saved sign-ins (user names and passwords).
    pub credentials: Credentials,
    /// The sites' cookies, for the others to be signed in too (empty when
    /// the device does not share them).
    pub cookies: Vec<SiteCookie>,
}

impl DeviceRecord {
    /// Visits a record carries at most.
    pub const HISTORY_LIMIT: usize = 2_000;
    /// What its cookies may weigh at most, roughly (bytes): the relay takes
    /// records of 1 MiB at most.
    pub const COOKIES_BUDGET: usize = 256 * 1024;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_ids_are_url_safe() {
        assert!(DeviceId::new("a1-B_2").is_some());
        assert!(DeviceId::new("").is_none());
        assert!(DeviceId::new("a/b").is_none());
        assert!(DeviceId::new("é").is_none());
        assert!(DeviceId::new("x".repeat(65)).is_none());
        assert_eq!(DeviceId::new("abc").unwrap().to_string(), "abc");
    }
}
