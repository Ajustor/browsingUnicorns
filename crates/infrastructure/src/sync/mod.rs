//! End-to-end encrypted sync through a relay that only stores opaque blobs.
//!
//! Every device of a group knows the same 32-byte [`SyncKey`], shared once by
//! its code (typed or scanned as a QR code). From it are derived:
//! - the group id, under which the relay files the records;
//! - the token the relay asks for (it keeps only its hash);
//! - the encryption key (XChaCha20-Poly1305): the relay never sees the data.
//!
//! The HTTP side is a [`BlobTransport`], so that the Android app can bring its
//! own client.

mod config;
#[cfg(feature = "desktop")]
mod http;
pub mod relay_api;

use application::ports::{SyncError, SyncStore};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload, rand_core::RngCore};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use data_encoding::{BASE32_NOPAD, HEXLOWER};
use domain::{DeviceId, DeviceRecord};
use sha2::{Digest, Sha256};

use crate::dto::DeviceRecordDto;

pub use config::{SyncConfig, SyncConfigFile, default_device_name, new_device_id};
#[cfg(feature = "desktop")]
pub use http::HttpTransport;

/// The secret shared by the devices of a sync group.
#[derive(Clone, PartialEq, Eq)]
pub struct SyncKey([u8; 32]);

impl std::fmt::Debug for SyncKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SyncKey(…)")
    }
}

const NONCE_LEN: usize = 24;
/// Prefix of the QR code text, so a scanner can tell it apart.
pub const QR_PREFIX: &str = "browsingunicorns-sync:";

impl SyncKey {
    pub fn generate() -> Self {
        let mut key = [0; 32];
        OsRng.fill_bytes(&mut key);
        Self(key)
    }

    /// The key as the user sees it: base32 in groups of four
    /// (`ABCD-EFGH-…`, 13 groups).
    pub fn code(&self) -> String {
        let raw = BASE32_NOPAD.encode(&self.0);
        raw.as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).expect("base32 is ASCII"))
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Reads a code typed by the user (any case, spaces and dashes ignored)
    /// or scanned from the QR code.
    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim();
        let code = code.strip_prefix(QR_PREFIX).unwrap_or(code);
        let cleaned: String = code
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let bytes = BASE32_NOPAD.decode(cleaned.as_bytes()).ok()?;
        Some(Self(bytes.try_into().ok()?))
    }

    /// What the QR code holds.
    pub fn qr_text(&self) -> String {
        format!("{QR_PREFIX}{}", self.code())
    }

    fn derive(&self, purpose: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"browsingUnicorns/");
        h.update(purpose.as_bytes());
        h.update(self.0);
        h.finalize().into()
    }

    /// Where the relay files this group's records.
    pub fn group_id(&self) -> String {
        HEXLOWER.encode(&self.derive("group")[..16])
    }

    /// What the relay asks for to read or write the group.
    pub fn token(&self) -> String {
        HEXLOWER.encode(&self.derive("token"))
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(&self.derive("encrypt").into())
    }

    /// Encrypts `plain` for the slot of `device` (bound to it: a blob moved
    /// to another device's slot no longer opens).
    pub fn seal(&self, device: &str, plain: &[u8]) -> Vec<u8> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let sealed = self
            .cipher()
            .encrypt(
                &nonce,
                Payload {
                    msg: plain,
                    aad: device.as_bytes(),
                },
            )
            .expect("encrypting in memory cannot fail");
        let mut out = nonce.to_vec();
        out.extend(sealed);
        out
    }

    /// `None` when `blob` was not sealed with this key for `device`.
    pub fn open(&self, device: &str, blob: &[u8]) -> Option<Vec<u8>> {
        if blob.len() < NONCE_LEN {
            return None;
        }
        let (nonce, sealed) = blob.split_at(NONCE_LEN);
        self.cipher()
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: device.as_bytes(),
                },
            )
            .ok()
    }
}

/// Moves opaque blobs to and from the relay. Blocking.
pub trait BlobTransport: Send + Sync {
    fn put(&self, group: &str, token: &str, device: &str, blob: &[u8]) -> Result<(), SyncError>;
    /// Every device's blob in the group, by device id.
    fn list(&self, group: &str, token: &str) -> Result<Vec<(String, Vec<u8>)>, SyncError>;
    fn delete(&self, group: &str, token: &str, device: &str) -> Result<(), SyncError>;
}

/// [`SyncStore`] that encrypts records with the group's key before handing
/// them to a [`BlobTransport`].
pub struct EncryptedSyncStore<T> {
    key: SyncKey,
    transport: T,
}

impl<T: BlobTransport> EncryptedSyncStore<T> {
    pub fn new(key: SyncKey, transport: T) -> Self {
        Self { key, transport }
    }
}

impl<T: BlobTransport> SyncStore for EncryptedSyncStore<T> {
    fn push(&self, record: &DeviceRecord) -> Result<(), SyncError> {
        let json = serde_json::to_vec(&DeviceRecordDto::from(record))
            .map_err(|e| SyncError(format!("encoding the record: {e}")))?;
        let device = record.device.as_str();
        let blob = self.key.seal(device, &json);
        self.transport
            .put(&self.key.group_id(), &self.key.token(), device, &blob)
    }

    fn pull(&self) -> Result<Vec<DeviceRecord>, SyncError> {
        let blobs = self
            .transport
            .list(&self.key.group_id(), &self.key.token())?;
        Ok(blobs
            .into_iter()
            .filter_map(|(device, blob)| {
                // A blob that does not open (other key, tampered) is skipped.
                let plain = self.key.open(&device, &blob)?;
                let dto: DeviceRecordDto = serde_json::from_slice(&plain).ok()?;
                let record = dto.into_record()?;
                (record.device.as_str() == device).then_some(record)
            })
            .collect())
    }

    fn leave(&self, device: &DeviceId) -> Result<(), SyncError> {
        self.transport
            .delete(&self.key.group_id(), &self.key.token(), device.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use domain::{Bookmarks, Settings, WebAddress};

    use super::*;

    #[test]
    fn codes_round_trip_whatever_the_typing() {
        let key = SyncKey::generate();
        let code = key.code();
        assert_eq!(code.split('-').count(), 13);
        assert_eq!(SyncKey::from_code(&code), Some(key.clone()));
        let sloppy = format!("  {} ", code.to_lowercase().replace('-', " "));
        assert_eq!(SyncKey::from_code(&sloppy), Some(key.clone()));
        assert_eq!(SyncKey::from_code(&key.qr_text()), Some(key.clone()));
        assert_eq!(SyncKey::from_code("ABCD"), None);
        assert_eq!(SyncKey::from_code("not base32 !"), None);
    }

    #[test]
    fn ids_are_derived_and_distinct() {
        let key = SyncKey::generate();
        assert_eq!(key.group_id().len(), 32);
        assert_eq!(key.token().len(), 64);
        assert_ne!(key.group_id(), key.token()[..32]);
        assert_ne!(key.group_id(), SyncKey::generate().group_id());
        assert_eq!(format!("{key:?}"), "SyncKey(…)", "never printed");
    }

    #[test]
    fn sealed_blobs_open_only_with_the_key_and_the_device() {
        let key = SyncKey::generate();
        let blob = key.seal("desktop", b"hello");
        assert_ne!(&blob[NONCE_LEN..], b"hello");
        assert_eq!(key.open("desktop", &blob).as_deref(), Some(&b"hello"[..]));
        assert_eq!(key.open("phone", &blob), None);
        assert_eq!(SyncKey::generate().open("desktop", &blob), None);
        let mut tampered = blob.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert_eq!(key.open("desktop", &tampered), None);
        assert_eq!(key.open("desktop", b"short"), None);
    }

    type Group = (String, BTreeMap<String, Vec<u8>>);

    /// A relay in memory that checks the token like the real one.
    #[derive(Default)]
    struct FakeRelay {
        /// Group → (token, blobs by device).
        groups: Mutex<BTreeMap<String, Group>>,
    }

    impl BlobTransport for &FakeRelay {
        fn put(
            &self,
            group: &str,
            token: &str,
            device: &str,
            blob: &[u8],
        ) -> Result<(), SyncError> {
            let mut groups = self.groups.lock().unwrap();
            let (owner, blobs) = groups
                .entry(group.into())
                .or_insert_with(|| (token.into(), BTreeMap::new()));
            if owner != token {
                return Err(SyncError("forbidden".into()));
            }
            blobs.insert(device.into(), blob.to_vec());
            Ok(())
        }
        fn list(&self, group: &str, token: &str) -> Result<Vec<(String, Vec<u8>)>, SyncError> {
            let groups = self.groups.lock().unwrap();
            match groups.get(group) {
                Some((owner, _)) if owner != token => Err(SyncError("forbidden".into())),
                Some((_, blobs)) => Ok(blobs.clone().into_iter().collect()),
                None => Ok(vec![]),
            }
        }
        fn delete(&self, group: &str, _token: &str, device: &str) -> Result<(), SyncError> {
            if let Some((_, blobs)) = self.groups.lock().unwrap().get_mut(group) {
                blobs.remove(device);
            }
            Ok(())
        }
    }

    fn record(device: &str) -> DeviceRecord {
        let mut bookmarks = Bookmarks::default();
        bookmarks.add(WebAddress::parse("https://a.com/").unwrap(), "A", 7);
        DeviceRecord {
            device: DeviceId::new(device).unwrap(),
            name: device.into(),
            updated_at: 9,
            bookmarks,
            history: vec![],
            tabs: vec![],
            settings: Settings::default(),
        }
    }

    #[test]
    fn records_go_through_the_relay_encrypted() {
        let relay = FakeRelay::default();
        let key = SyncKey::generate();
        let desktop = EncryptedSyncStore::new(key.clone(), &relay);
        let phone = EncryptedSyncStore::new(key.clone(), &relay);
        desktop.push(&record("desktop")).unwrap();
        phone.push(&record("phone")).unwrap();

        let pulled = desktop.pull().unwrap();
        assert_eq!(pulled.len(), 2);
        assert_eq!(pulled[0], record("desktop"));
        // The relay holds no plain text.
        let raw = relay.groups.lock().unwrap();
        let blob = &raw[&key.group_id()].1["desktop"];
        assert!(!String::from_utf8_lossy(blob).contains("a.com"));
        drop(raw);

        phone.leave(&DeviceId::new("phone").unwrap()).unwrap();
        assert_eq!(desktop.pull().unwrap().len(), 1);
    }

    #[test]
    fn another_group_cannot_read_or_overwrite() {
        let relay = FakeRelay::default();
        let mine = EncryptedSyncStore::new(SyncKey::generate(), &relay);
        mine.push(&record("desktop")).unwrap();
        let other = EncryptedSyncStore::new(SyncKey::generate(), &relay);
        assert!(other.pull().unwrap().is_empty(), "its own empty group");
    }

    #[test]
    fn a_blob_filed_under_another_device_is_ignored() {
        let relay = FakeRelay::default();
        let key = SyncKey::generate();
        let store = EncryptedSyncStore::new(key.clone(), &relay);
        store.push(&record("desktop")).unwrap();
        // Copy the desktop's blob into a "phone" slot.
        let blob = relay.groups.lock().unwrap()[&key.group_id()].1["desktop"].clone();
        (&relay)
            .put(&key.group_id(), &key.token(), "phone", &blob)
            .unwrap();
        assert_eq!(store.pull().unwrap().len(), 1);
    }
}
