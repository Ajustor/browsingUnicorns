//! The user's profiles: the list, kept on disk and shared by every window
//! (each window runs one profile), and the profiles' data folders.

use std::fmt;

use domain::{Profile, ProfileError, ProfileId, Profiles};

use crate::ports::{ProfileStore, Repository, RepositoryError};

#[derive(Debug)]
pub enum ProfilesError {
    Profile(ProfileError),
    Storage(RepositoryError),
    /// The profile runs, in this window or another one.
    InUse,
}

impl fmt::Display for ProfilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Profile(e) => e.fmt(f),
            Self::Storage(e) => e.fmt(f),
            Self::InUse => f.write_str("this profile is open in a window: close it first"),
        }
    }
}

impl std::error::Error for ProfilesError {}

impl From<ProfileError> for ProfilesError {
    fn from(e: ProfileError) -> Self {
        Self::Profile(e)
    }
}

impl From<RepositoryError> for ProfilesError {
    fn from(e: RepositoryError) -> Self {
        Self::Storage(e)
    }
}

pub struct ProfileManager {
    profiles: Profiles,
    current: ProfileId,
    repository: Box<dyn Repository<Profiles>>,
    store: Box<dyn ProfileStore>,
}

impl ProfileManager {
    /// The list as saved; `current` is the profile this window runs (an
    /// unknown one is added under its id).
    pub fn load(
        repository: Box<dyn Repository<Profiles>>,
        store: Box<dyn ProfileStore>,
        current: ProfileId,
    ) -> Result<Self, ProfilesError> {
        let mut manager = Self {
            profiles: repository.load()?,
            current,
            repository,
            store,
        };
        if manager.profiles.get(&manager.current).is_none() {
            let id = manager.current.clone();
            let mut items = manager.profiles.items().to_vec();
            items.push(Profile {
                name: id.to_string(),
                id,
                color: items.len() as u8,
            });
            manager.profiles = Profiles::from_items(items);
            manager.repository.save(&manager.profiles)?;
        }
        Ok(manager)
    }

    pub fn profiles(&self) -> &Profiles {
        &self.profiles
    }

    pub fn current(&self) -> &Profile {
        self.profiles
            .get(&self.current)
            .expect("the current profile is in the list")
    }

    /// Whether `id` runs in a window (this one included).
    pub fn is_open(&self, id: &ProfileId) -> bool {
        id == &self.current || self.store.is_open(id)
    }

    /// Picks up changes made from other windows.
    pub fn reload(&mut self) -> Result<(), ProfilesError> {
        let profiles = self.repository.load()?;
        if profiles.get(&self.current).is_some() {
            self.profiles = profiles;
        }
        Ok(())
    }

    /// Changes the list as saved now (other windows may have changed it),
    /// and saves it.
    fn change<T>(
        &mut self,
        f: impl FnOnce(&mut Profiles) -> Result<T, ProfileError>,
    ) -> Result<T, ProfilesError> {
        self.reload()?;
        let mut profiles = self.profiles.clone();
        let value = f(&mut profiles)?;
        self.repository.save(&profiles)?;
        self.profiles = profiles;
        Ok(value)
    }

    /// Adds a profile, with its empty data folder.
    pub fn add(&mut self, name: &str) -> Result<ProfileId, ProfilesError> {
        let id = self.change(|p| p.add(name))?;
        self.store.create(&id)?;
        Ok(id)
    }

    pub fn rename(&mut self, id: &ProfileId, name: &str) -> Result<(), ProfilesError> {
        self.change(|p| p.rename(id, name))
    }

    pub fn set_color(&mut self, id: &ProfileId, color: u8) -> Result<(), ProfilesError> {
        self.change(|p| p.set_color(id, color))
    }

    /// Removes a profile that no window runs, and deletes its data.
    pub fn remove(&mut self, id: &ProfileId) -> Result<(), ProfilesError> {
        if self.is_open(id) {
            return Err(ProfilesError::InUse);
        }
        self.change(|p| p.remove(id))?;
        self.store.delete(id)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct Shared {
        saved: Arc<Mutex<Option<Profiles>>>,
        folders: Arc<Mutex<HashSet<String>>>,
        open: Arc<Mutex<HashSet<String>>>,
    }

    impl Repository<Profiles> for Shared {
        fn load(&self) -> Result<Profiles, RepositoryError> {
            Ok(self.saved.lock().unwrap().clone().unwrap_or_default())
        }
        fn save(&self, value: &Profiles) -> Result<(), RepositoryError> {
            *self.saved.lock().unwrap() = Some(value.clone());
            Ok(())
        }
    }

    impl ProfileStore for Shared {
        fn create(&self, id: &ProfileId) -> Result<(), RepositoryError> {
            self.folders.lock().unwrap().insert(id.to_string());
            Ok(())
        }
        fn delete(&self, id: &ProfileId) -> Result<(), RepositoryError> {
            self.folders.lock().unwrap().remove(id.as_str());
            Ok(())
        }
        fn is_open(&self, id: &ProfileId) -> bool {
            self.open.lock().unwrap().contains(id.as_str())
        }
    }

    fn manager(shared: &Shared, current: &str) -> ProfileManager {
        ProfileManager::load(
            Box::new(shared.clone()),
            Box::new(shared.clone()),
            ProfileId::parse(current).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn windows_share_the_list() {
        let shared = Shared::default();
        let mut first = manager(&shared, "default");
        let mut second = manager(&shared, "default");
        let work = first.add("Work").unwrap();
        assert!(shared.folders.lock().unwrap().contains("work"));
        // The second window adds on top of the first one's change.
        second.add("Home").unwrap();
        first.reload().unwrap();
        assert_eq!(first.profiles().items().len(), 3);
        assert!(first.profiles().get(&work).is_some());
    }

    #[test]
    fn open_profiles_are_not_removed() {
        let shared = Shared::default();
        let mut m = manager(&shared, "default");
        let work = m.add("Work").unwrap();
        shared.open.lock().unwrap().insert("work".into());
        assert!(matches!(m.remove(&work), Err(ProfilesError::InUse)));
        assert!(matches!(
            m.remove(&ProfileId::default_profile()),
            Err(ProfilesError::InUse)
        ));
        shared.open.lock().unwrap().clear();
        m.remove(&work).unwrap();
        assert!(!shared.folders.lock().unwrap().contains("work"));
        assert_eq!(m.profiles().items().len(), 1);
    }

    #[test]
    fn an_unknown_current_profile_is_added() {
        let shared = Shared::default();
        let m = manager(&shared, "test");
        assert_eq!(m.current().name, "test");
        assert_eq!(shared.load().unwrap().items().len(), 2);
    }
}
