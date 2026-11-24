//! One sync round, run off the UI thread.

use domain::{DeviceId, DeviceRecord};

use crate::ports::{SyncError, SyncStore};

/// Publishes `own` then returns the other devices' records, to hand to
/// [`crate::Browser::apply_remote`].
pub fn sync_round(
    store: &dyn SyncStore,
    own: &DeviceRecord,
) -> Result<Vec<DeviceRecord>, SyncError> {
    store.push(own)?;
    let mut others = store.pull()?;
    others.retain(|r| r.device != own.device);
    Ok(others)
}

/// What the user sees of another device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteDevice {
    pub id: DeviceId,
    pub name: String,
    pub updated_at: domain::Timestamp,
    pub tabs: Vec<domain::RemoteTab>,
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use domain::{Bookmarks, Settings};

    use super::*;

    #[derive(Default)]
    struct MemoryStore(Mutex<Vec<DeviceRecord>>);

    impl SyncStore for MemoryStore {
        fn push(&self, record: &DeviceRecord) -> Result<(), SyncError> {
            let mut all = self.0.lock().unwrap();
            all.retain(|r| r.device != record.device);
            all.push(record.clone());
            Ok(())
        }
        fn pull(&self) -> Result<Vec<DeviceRecord>, SyncError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn leave(&self, device: &DeviceId) -> Result<(), SyncError> {
            self.0.lock().unwrap().retain(|r| &r.device != device);
            Ok(())
        }
    }

    fn record(id: &str) -> DeviceRecord {
        DeviceRecord {
            device: DeviceId::new(id).unwrap(),
            name: id.into(),
            updated_at: 1,
            bookmarks: Bookmarks::default(),
            history: vec![],
            tabs: vec![],
            settings: Settings::default(),
            credentials: Default::default(),
            cookies: vec![],
        }
    }

    #[test]
    fn a_round_publishes_and_returns_the_others_only() {
        let store = MemoryStore::default();
        assert!(sync_round(&store, &record("desktop")).unwrap().is_empty());
        let others = sync_round(&store, &record("phone")).unwrap();
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].device.as_str(), "desktop");
        store.leave(&DeviceId::new("desktop").unwrap()).unwrap();
        assert!(sync_round(&store, &record("phone")).unwrap().is_empty());
    }
}
