//! Where each profile keeps its data, the list of profiles, and the lock a
//! window holds on the profile it runs.
//!
//! The default profile lives where the browser always kept its data
//! (`<data dir>/browsing-unicorns`, settings in the config dir); the others
//! in `profiles/<id>/` under it, settings included.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use application::ports::{ProfileStore, Repository, RepositoryError};
use domain::{Profile, ProfileId, Profiles};
use serde::{Deserialize, Serialize};

use crate::files;
use crate::settings_store::TomlSettingsRepository;

const LOCK_FILE: &str = "profile.lock";

/// The folders of every profile, under the default profile's one.
#[derive(Debug, Clone)]
pub struct ProfileDirs {
    root: PathBuf,
}

impl ProfileDirs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Under [`crate::profile_dir`].
    pub fn default_location() -> Option<Self> {
        crate::profile_dir().map(Self::new)
    }

    pub fn dir(&self, id: &ProfileId) -> PathBuf {
        if id.is_default() {
            self.root.clone()
        } else {
            self.root.join("profiles").join(id.as_str())
        }
    }

    pub fn settings(&self, id: &ProfileId) -> Option<TomlSettingsRepository> {
        if id.is_default() {
            TomlSettingsRepository::default_location()
        } else {
            Some(TomlSettingsRepository::new(
                self.dir(id).join("settings.toml"),
            ))
        }
    }

    /// The list of profiles, shared by every window.
    pub fn list(&self) -> ProfileList {
        ProfileList {
            path: self.root.join("profiles.json"),
        }
    }
}

impl ProfileStore for ProfileDirs {
    fn create(&self, id: &ProfileId) -> Result<(), RepositoryError> {
        let dir = self.dir(id);
        std::fs::create_dir_all(&dir).map_err(|e| files::error(&dir, e))
    }

    fn delete(&self, id: &ProfileId) -> Result<(), RepositoryError> {
        if id.is_default() {
            return Err(RepositoryError("the default profile is kept".into()));
        }
        let dir = self.dir(id);
        match std::fs::remove_dir_all(&dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(files::error(&dir, e)),
            _ => Ok(()),
        }
    }

    fn is_open(&self, id: &ProfileId) -> bool {
        is_locked(&self.dir(id))
    }
}

/// Starts the browser on profile `id`, in a new window (its own process).
pub fn open_window(id: &ProfileId) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("--profile")
        .arg(id.as_str())
        .spawn()
        .map(drop)
}

/// Starts a private window on profile `id` (its own process).
pub fn open_private_window(id: &ProfileId) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .arg("--profile")
        .arg(id.as_str())
        .arg("--private")
        .spawn()
        .map(drop)
}

/// Held by the window that runs a profile, until it closes (the OS
/// releases it if the process dies).
pub struct ProfileLock {
    _file: File,
}

impl ProfileLock {
    /// `None` when another window holds it (or the folder is unusable).
    pub fn acquire(dir: &Path) -> Option<Self> {
        std::fs::create_dir_all(dir).ok()?;
        let file = open_lock(dir).ok()?;
        match file.try_lock() {
            Ok(()) => Some(Self { _file: file }),
            Err(TryLockError::WouldBlock) => None,
            Err(TryLockError::Error(e)) => {
                log::warn!("profile lock in {}: {e}", dir.display());
                None
            }
        }
    }
}

fn open_lock(dir: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(LOCK_FILE))
}

/// Whether a window holds `dir`'s lock: one held by this process counts
/// too (the lock belongs to the open file, not the process).
fn is_locked(dir: &Path) -> bool {
    if !dir.join(LOCK_FILE).exists() {
        return false;
    }
    match open_lock(dir).map(|f| f.try_lock()) {
        Ok(Ok(())) => false,
        Ok(Err(TryLockError::WouldBlock)) => true,
        _ => false,
    }
}

/// `profiles.json`.
pub struct ProfileList {
    path: PathBuf,
}

#[derive(Default, Serialize, Deserialize)]
struct ProfilesFile {
    profiles: Vec<ProfileDto>,
}

#[derive(Serialize, Deserialize)]
struct ProfileDto {
    id: String,
    name: String,
    #[serde(default)]
    color: u8,
}

impl Repository<Profiles> for ProfileList {
    fn load(&self) -> Result<Profiles, RepositoryError> {
        let file: ProfilesFile = match files::read(&self.path)? {
            Some(text) => serde_json::from_str(&text).map_err(|e| files::error(&self.path, e))?,
            None => ProfilesFile::default(),
        };
        Ok(Profiles::from_items(
            file.profiles
                .into_iter()
                .filter_map(|p| {
                    Some(Profile {
                        id: ProfileId::parse(&p.id)?,
                        name: p.name,
                        color: p.color,
                    })
                })
                .collect(),
        ))
    }

    fn save(&self, profiles: &Profiles) -> Result<(), RepositoryError> {
        let file = ProfilesFile {
            profiles: profiles
                .items()
                .iter()
                .map(|p| ProfileDto {
                    id: p.id.to_string(),
                    name: p.name.clone(),
                    color: p.color,
                })
                .collect(),
        };
        let text = serde_json::to_string_pretty(&file).map_err(|e| files::error(&self.path, e))?;
        files::write(&self.path, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_profile_keeps_the_old_folder() {
        let dirs = ProfileDirs::new("/data/bu");
        assert_eq!(
            dirs.dir(&ProfileId::default_profile()),
            PathBuf::from("/data/bu")
        );
        assert_eq!(
            dirs.dir(&ProfileId::parse("work").unwrap()),
            Path::new("/data/bu").join("profiles").join("work")
        );
    }

    #[test]
    fn the_list_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let list = ProfileDirs::new(dir.path()).list();
        assert_eq!(list.load().unwrap(), Profiles::default());
        let mut profiles = Profiles::default();
        profiles.add("Work").unwrap();
        list.save(&profiles).unwrap();
        assert_eq!(list.load().unwrap(), profiles);
    }

    #[test]
    fn a_held_lock_shows_the_profile_open() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::new(dir.path());
        let work = ProfileId::parse("work").unwrap();
        dirs.create(&work).unwrap();
        assert!(!dirs.is_open(&work));
        let lock = ProfileLock::acquire(&dirs.dir(&work)).unwrap();
        assert!(dirs.is_open(&work));
        assert!(ProfileLock::acquire(&dirs.dir(&work)).is_none());
        drop(lock);
        assert!(!dirs.is_open(&work));
        dirs.delete(&work).unwrap();
        assert!(!dirs.dir(&work).exists());
    }
}
