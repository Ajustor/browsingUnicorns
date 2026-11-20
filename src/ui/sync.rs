//! Sync on the desktop: pairing, background rounds and their status.
//!
//! A round (push this device's record, pull the others) runs on a worker
//! thread; its result is applied to the browser on the UI thread.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use application::ports::{SyncError, SyncStore};
use application::sync::sync_round;
use domain::DeviceRecord;
use infrastructure::sync::{
    EncryptedSyncStore, HttpTransport, SyncConfig, SyncConfigFile, SyncKey, default_device_name,
    new_device_id,
};

/// The relay deployed from `relay/sync`. `BU_SYNC_RELAY` overrides it (tests).
pub const DEFAULT_RELAY: &str = "https://browsing-unicorns-sync.minecities.workers.dev";

/// Rounds run this long after a local change...
const AFTER_CHANGE: Duration = Duration::from_secs(15);
/// ...and at least this often otherwise.
const EVERY: Duration = Duration::from_secs(5 * 60);
/// The first round waits this long after start-up.
const STARTUP_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoundState {
    Never,
    Running,
    Done(Instant),
    Failed(String),
}

pub struct SyncRunner {
    file: SyncConfigFile,
    active: Option<Active>,
    pub state: RoundState,
    last_round: Option<Instant>,
    /// No round before this (lets the restored tabs load at start-up).
    not_before: Instant,
    repaint: Arc<dyn Fn() + Send + Sync>,
    tx: Sender<Result<Vec<DeviceRecord>, SyncError>>,
    rx: Receiver<Result<Vec<DeviceRecord>, SyncError>>,
}

struct Active {
    config: SyncConfig,
    store: Arc<dyn SyncStore>,
}

fn relay_url() -> String {
    std::env::var("BU_SYNC_RELAY").unwrap_or_else(|_| DEFAULT_RELAY.to_owned())
}

fn store_for(config: &SyncConfig) -> Arc<dyn SyncStore> {
    Arc::new(EncryptedSyncStore::new(
        config.key.clone(),
        HttpTransport::new(&config.relay_url),
    ))
}

impl SyncRunner {
    pub fn new(file: SyncConfigFile, repaint: impl Fn() + Send + Sync + 'static) -> Self {
        let active = file.load().map(|config| Active {
            store: store_for(&config),
            config,
        });
        let (tx, rx) = mpsc::channel();
        Self {
            file,
            active,
            state: RoundState::Never,
            last_round: None,
            not_before: Instant::now() + STARTUP_DELAY,
            repaint: Arc::new(repaint),
            tx,
            rx,
        }
    }

    pub fn config(&self) -> Option<&SyncConfig> {
        self.active.as_ref().map(|a| &a.config)
    }

    /// Starts a new group with a fresh key.
    pub fn turn_on(&mut self) -> Result<(), String> {
        self.activate(SyncKey::generate())
    }

    /// Joins the group of another device from its code.
    pub fn join(&mut self, code: &str) -> Result<(), String> {
        let key = SyncKey::from_code(code).ok_or_else(i18n::sync::invalid_code)?;
        self.activate(key)
    }

    fn activate(&mut self, key: SyncKey) -> Result<(), String> {
        let config = SyncConfig {
            key,
            device: new_device_id(),
            device_name: default_device_name(),
            relay_url: relay_url(),
        };
        self.file.save(&config).map_err(|e| e.to_string())?;
        self.active = Some(Active {
            store: store_for(&config),
            config,
        });
        self.state = RoundState::Never;
        self.last_round = None;
        self.not_before = Instant::now();
        Ok(())
    }

    /// Leaves the group: this device's record is removed from the relay (in
    /// the background, best effort) and the key forgotten here.
    pub fn turn_off(&mut self) -> Result<(), String> {
        if let Some(active) = self.active.take() {
            let device = active.config.device.clone();
            std::thread::spawn(move || {
                if let Err(e) = active.store.leave(&device) {
                    log::warn!("leaving the sync group: {e}");
                }
            });
        }
        self.state = RoundState::Never;
        self.file.delete().map_err(|e| e.to_string())
    }

    /// Whether a round should start now, given unsynced local changes.
    pub fn is_due(&self, changes: bool) -> bool {
        if self.active.is_none()
            || self.state == RoundState::Running
            || Instant::now() < self.not_before
        {
            return false;
        }
        match self.last_round {
            None => true,
            Some(t) => t.elapsed() >= EVERY || (changes && t.elapsed() >= AFTER_CHANGE),
        }
    }

    /// When to look again for a due round.
    pub fn next_check(&self) -> Option<Duration> {
        self.active.as_ref()?;
        let wait = match self.last_round {
            Some(last) => AFTER_CHANGE.saturating_sub(last.elapsed()),
            None => self.not_before.saturating_duration_since(Instant::now()),
        };
        Some(wait.max(Duration::from_secs(1)))
    }

    /// Runs a round in the background with this device's `record`.
    pub fn start(&mut self, record: DeviceRecord) {
        let Some(active) = &self.active else {
            return;
        };
        self.state = RoundState::Running;
        let (store, tx, repaint) = (active.store.clone(), self.tx.clone(), self.repaint.clone());
        std::thread::spawn(move || {
            let _ = tx.send(sync_round(store.as_ref(), &record));
            repaint();
        });
    }

    /// The other devices' records once a round is over.
    pub fn poll(&mut self) -> Option<Vec<DeviceRecord>> {
        let result = self.rx.try_recv().ok()?;
        self.last_round = Some(Instant::now());
        match result {
            Ok(records) => {
                self.state = RoundState::Done(Instant::now());
                Some(records)
            }
            Err(e) => {
                log::warn!("sync: {e}");
                self.state = RoundState::Failed(e.to_string());
                None
            }
        }
    }

    /// Lets the next frame start a round right away.
    pub fn sync_now(&mut self) {
        self.last_round = None;
        self.not_before = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runner(dir: &tempfile::TempDir) -> SyncRunner {
        SyncRunner::new(SyncConfigFile::new(dir.path().join("sync.json")), || {})
    }

    #[test]
    fn off_until_turned_on_and_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = runner(&dir);
        assert!(r.config().is_none());
        assert!(!r.is_due(true));
        r.turn_on().unwrap();
        assert!(r.is_due(false), "first round right away");
        let code = r.config().unwrap().key.code();
        assert_eq!(
            runner(&dir).config().unwrap().key.code(),
            code,
            "kept across runs"
        );
        r.turn_off().unwrap();
        assert!(runner(&dir).config().is_none());
    }

    #[test]
    fn joining_needs_a_valid_code() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = runner(&dir);
        assert!(r.join("nope").is_err());
        let key = SyncKey::generate();
        r.join(&key.code().to_lowercase()).unwrap();
        assert_eq!(r.config().unwrap().key, key);
    }

    #[test]
    fn rounds_are_spaced_out() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = runner(&dir);
        r.turn_on().unwrap();
        r.last_round = Some(Instant::now());
        assert!(!r.is_due(false));
        assert!(!r.is_due(true), "changes wait a little");
        r.last_round = Some(Instant::now() - AFTER_CHANGE);
        assert!(r.is_due(true));
        assert!(!r.is_due(false));
        r.last_round = Some(Instant::now() - EVERY);
        assert!(r.is_due(false));
        r.sync_now();
        assert!(r.is_due(false));
        r.state = RoundState::Running;
        assert!(!r.is_due(true), "one round at a time");
    }
}
