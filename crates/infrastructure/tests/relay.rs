//! End-to-end check against a running relay (`relay/sync`):
//!
//! ```sh
//! cd relay/sync && npx wrangler dev --local --port 8787
//! BU_TEST_RELAY=http://127.0.0.1:8787 cargo test -p infrastructure --test relay -- --ignored
//! ```
#![cfg(feature = "desktop")]

use application::ports::SyncStore;
use application::sync::sync_round;
use domain::{Bookmarks, DeviceRecord, Settings, WebAddress};
use infrastructure::sync::{EncryptedSyncStore, HttpTransport, SyncKey, new_device_id};

fn record(name: &str, bookmark: &str) -> DeviceRecord {
    let mut bookmarks = Bookmarks::default();
    bookmarks.add(WebAddress::parse(bookmark).unwrap(), name, 1);
    DeviceRecord {
        device: new_device_id(),
        name: name.into(),
        updated_at: 1,
        bookmarks,
        history: vec![],
        tabs: vec![],
        settings: Settings::default(),
    }
}

#[test]
#[ignore = "needs a relay: set BU_TEST_RELAY"]
fn two_devices_sync_through_the_relay() {
    let relay = std::env::var("BU_TEST_RELAY").expect("BU_TEST_RELAY");
    let key = SyncKey::generate();
    let desktop = EncryptedSyncStore::new(key.clone(), HttpTransport::new(&relay));
    let phone = EncryptedSyncStore::new(key, HttpTransport::new(&relay));

    let desktop_record = record("Desktop", "https://desktop.test/");
    let phone_record = record("Phone", "https://phone.test/");
    assert!(sync_round(&desktop, &desktop_record).unwrap().is_empty());
    let seen_by_phone = sync_round(&phone, &phone_record).unwrap();
    assert_eq!(seen_by_phone, vec![desktop_record.clone()]);
    let seen_by_desktop = sync_round(&desktop, &desktop_record).unwrap();
    assert_eq!(seen_by_desktop, vec![phone_record.clone()]);

    // Another group, same relay: refused (its token does not match).
    let intruder = EncryptedSyncStore::new(SyncKey::generate(), HttpTransport::new(&relay));
    assert!(intruder.pull().unwrap().is_empty(), "its own, empty group");

    phone.leave(&phone_record.device).unwrap();
    assert!(sync_round(&desktop, &desktop_record).unwrap().is_empty());
    desktop.leave(&desktop_record.device).unwrap();
}
