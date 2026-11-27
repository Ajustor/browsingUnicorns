use std::path::PathBuf;

use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
use data_encoding::HEXLOWER;
use domain::DeviceId;
use serde::{Deserialize, Serialize};

use super::SyncKey;
use crate::{files, seal};

/// This device's sync settings. The key is a secret: whoever has it reads
/// the group's data (passwords and cookies included), so it is stored
/// sealed like the passwords.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncConfig {
    pub key: SyncKey,
    pub device: DeviceId,
    pub device_name: String,
    pub relay_url: String,
}

#[derive(Serialize, Deserialize)]
struct ConfigDto {
    code: String,
    device: String,
    device_name: String,
    relay_url: String,
}

/// Random id for a device joining a group.
pub fn new_device_id() -> DeviceId {
    let mut bytes = [0; 8];
    OsRng.fill_bytes(&mut bytes);
    DeviceId::new(HEXLOWER.encode(&bytes)).expect("hex is a valid device id")
}

/// The computer's name, else "Desktop".
pub fn default_device_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .map(|n| n.trim().to_owned())
        .find(|n| !n.is_empty())
        .unwrap_or_else(|| "Desktop".into())
}

/// `sync.json` in the profile directory; absent while sync is off.
pub struct SyncConfigFile {
    path: PathBuf,
}

impl SyncConfigFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `None` when sync is off or the file is unreadable (logged).
    pub fn load(&self) -> Option<SyncConfig> {
        let text = match files::read(&self.path) {
            Ok(text) => text?,
            Err(e) => {
                log::warn!("sync settings: {e}");
                return None;
            }
        };
        let dto: ConfigDto = serde_json::from_str(&text)
            .inspect_err(|e| log::warn!("sync settings: {e}"))
            .ok()?;
        let code = seal::open(&dto.code)
            .inspect_err(|e| log::warn!("sync settings: {e}"))
            .ok()?;
        let config = SyncConfig {
            key: SyncKey::from_code(&code)?,
            device: DeviceId::new(dto.device)?,
            device_name: dto.device_name,
            relay_url: dto.relay_url,
        };
        // A key written in clear by an older version is sealed now.
        if seal::AVAILABLE
            && !seal::is_sealed(&dto.code)
            && let Err(e) = self.save(&config)
        {
            log::warn!("sync settings: {e}");
        }
        Some(config)
    }

    /// Writes the settings, the key sealed for the user where the system
    /// can ([`seal`]).
    pub fn save(&self, config: &SyncConfig) -> Result<(), application::ports::RepositoryError> {
        let dto = ConfigDto {
            code: seal::seal(&config.key.code()),
            device: config.device.to_string(),
            device_name: config.device_name.clone(),
            relay_url: config.relay_url.clone(),
        };
        let text = serde_json::to_string_pretty(&dto).map_err(|e| files::error(&self.path, e))?;
        files::write(&self.path, &text)
    }

    /// Turns sync off on this device.
    pub fn delete(&self) -> std::io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let file = SyncConfigFile::new(dir.path().join("sync.json"));
        assert!(file.load().is_none());
        let config = SyncConfig {
            key: SyncKey::generate(),
            device: new_device_id(),
            device_name: "Desktop".into(),
            relay_url: "https://relay.test".into(),
        };
        file.save(&config).unwrap();
        let raw = std::fs::read_to_string(dir.path().join("sync.json")).unwrap();
        assert_eq!(raw.contains(&config.key.code()), !seal::AVAILABLE);
        assert_eq!(file.load(), Some(config));
        file.delete().unwrap();
        assert!(file.load().is_none());
        file.delete().unwrap();
    }

    #[test]
    fn a_clear_key_is_read_then_sealed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync.json");
        let key = SyncKey::generate();
        let clear = ConfigDto {
            code: key.code(),
            device: new_device_id().to_string(),
            device_name: "Old".into(),
            relay_url: "https://relay.test".into(),
        };
        std::fs::write(&path, serde_json::to_string(&clear).unwrap()).unwrap();
        let file = SyncConfigFile::new(&path);
        let loaded = file.load().unwrap();
        assert_eq!(loaded.key, key);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert_eq!(raw.contains(&key.code()), !seal::AVAILABLE);
        assert_eq!(file.load(), Some(loaded));
    }

    #[test]
    fn device_ids_are_random_hex() {
        let a = new_device_id();
        assert_eq!(a.as_str().len(), 16);
        assert_ne!(a, new_device_id());
        assert!(!default_device_name().is_empty());
    }
}
