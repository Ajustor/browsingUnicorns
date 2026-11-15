//! Browser profiles: separate people or uses (work, personal...), each with
//! its own history, bookmarks, tabs, extensions, cookies and sync.

use std::fmt;

/// A profile's id, also the name of its folder: lowercase letters, digits
/// and `-`. [`ProfileId::default_profile`] is the profile that existed
/// before profiles did.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProfileId(String);

const DEFAULT_ID: &str = "default";
const MAX_ID_LEN: usize = 32;

impl ProfileId {
    pub fn default_profile() -> Self {
        Self(DEFAULT_ID.to_owned())
    }

    pub fn parse(id: &str) -> Option<Self> {
        let valid = !id.is_empty()
            && id.len() <= MAX_ID_LEN
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !id.starts_with('-')
            && !id.ends_with('-');
        valid.then(|| Self(id.to_owned()))
    }

    /// `Work stuff!` → `work-stuff`; `None` when nothing is left.
    fn slug(name: &str) -> Option<Self> {
        let mut slug = String::new();
        for c in name.chars().flat_map(char::to_lowercase) {
            match c {
                'a'..='z' | '0'..='9' => slug.push(c),
                'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => slug.push('a'),
                'é' | 'è' | 'ê' | 'ë' => slug.push('e'),
                'î' | 'ï' | 'í' | 'ì' => slug.push('i'),
                'ô' | 'ö' | 'ó' | 'ò' | 'õ' => slug.push('o'),
                'ù' | 'û' | 'ü' | 'ú' => slug.push('u'),
                'ç' => slug.push('c'),
                'ñ' => slug.push('n'),
                _ if !slug.is_empty() && !slug.ends_with('-') => slug.push('-'),
                _ => {}
            }
        }
        slug.truncate(MAX_ID_LEN - 3);
        Self::parse(slug.trim_end_matches('-'))
    }

    pub fn is_default(&self) -> bool {
        self.0 == DEFAULT_ID
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProfileId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A profile as the user sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub id: ProfileId,
    pub name: String,
    /// Index in the avatar palette.
    pub color: u8,
}

/// Number of avatar colors the UI offers.
pub const PROFILE_COLORS: u8 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    EmptyName,
    NameTaken,
    NotFound,
    /// The default profile holds the data from before profiles.
    DefaultProfile,
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyName => "a profile needs a name",
            Self::NameTaken => "another profile has this name",
            Self::NotFound => "no such profile",
            Self::DefaultProfile => "the default profile cannot be removed",
        })
    }
}

impl std::error::Error for ProfileError {}

/// Every profile; the default one is always there, first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profiles {
    items: Vec<Profile>,
}

impl Default for Profiles {
    fn default() -> Self {
        Self {
            items: vec![Profile {
                id: ProfileId::default_profile(),
                name: "Default".to_owned(),
                color: 0,
            }],
        }
    }
}

impl Profiles {
    /// The saved list, repaired: the default profile first, no duplicate id.
    pub fn from_items(items: Vec<Profile>) -> Self {
        let mut profiles = Self::default();
        for p in items {
            if p.id.is_default() {
                profiles.items[0].name = p.name;
                profiles.items[0].color = p.color % PROFILE_COLORS;
            } else if profiles.get(&p.id).is_none() {
                profiles.items.push(Profile {
                    color: p.color % PROFILE_COLORS,
                    ..p
                });
            }
        }
        profiles
    }

    pub fn items(&self) -> &[Profile] {
        &self.items
    }

    pub fn get(&self, id: &ProfileId) -> Option<&Profile> {
        self.items.iter().find(|p| &p.id == id)
    }

    fn check_name(&self, name: &str, except: Option<&ProfileId>) -> Result<String, ProfileError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ProfileError::EmptyName);
        }
        let taken = self
            .items
            .iter()
            .any(|p| Some(&p.id) != except && p.name.to_lowercase() == name.to_lowercase());
        if taken {
            return Err(ProfileError::NameTaken);
        }
        Ok(name.to_owned())
    }

    /// Adds a profile named `name`, with an id made from it and the next
    /// color.
    pub fn add(&mut self, name: &str) -> Result<ProfileId, ProfileError> {
        let name = self.check_name(name, None)?;
        let base = ProfileId::slug(&name).unwrap_or_else(|| ProfileId("profile".to_owned()));
        let id = std::iter::once(base.clone())
            .chain((2..).map(|n| ProfileId(format!("{base}-{n}"))))
            .find(|id| !id.is_default() && self.get(id).is_none())
            .expect("an unused id");
        let color = (self.items.len() % PROFILE_COLORS as usize) as u8;
        self.items.push(Profile {
            id: id.clone(),
            name,
            color,
        });
        Ok(id)
    }

    pub fn rename(&mut self, id: &ProfileId, name: &str) -> Result<(), ProfileError> {
        let name = self.check_name(name, Some(id))?;
        let profile = self
            .items
            .iter_mut()
            .find(|p| &p.id == id)
            .ok_or(ProfileError::NotFound)?;
        profile.name = name;
        Ok(())
    }

    pub fn set_color(&mut self, id: &ProfileId, color: u8) -> Result<(), ProfileError> {
        let profile = self
            .items
            .iter_mut()
            .find(|p| &p.id == id)
            .ok_or(ProfileError::NotFound)?;
        profile.color = color % PROFILE_COLORS;
        Ok(())
    }

    pub fn remove(&mut self, id: &ProfileId) -> Result<Profile, ProfileError> {
        if id.is_default() {
            return Err(ProfileError::DefaultProfile);
        }
        let index = self
            .items
            .iter()
            .position(|p| &p.id == id)
            .ok_or(ProfileError::NotFound)?;
        Ok(self.items.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_folder_safe() {
        assert!(ProfileId::parse("work-2").is_some());
        for bad in ["", "Work", "a/b", "..", "-a", "a-", &"a".repeat(40)] {
            assert_eq!(ProfileId::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_default_profile_is_always_first() {
        let p = Profiles::from_items(vec![
            Profile {
                id: ProfileId::parse("work").unwrap(),
                name: "Work".into(),
                color: 3,
            },
            Profile {
                id: ProfileId::default_profile(),
                name: "Me".into(),
                color: 1,
            },
        ]);
        assert_eq!(p.items()[0].id, ProfileId::default_profile());
        assert_eq!(p.items()[0].name, "Me");
        assert_eq!(p.items().len(), 2);
    }

    #[test]
    fn ids_come_from_names_and_stay_unique() {
        let mut p = Profiles::default();
        assert_eq!(
            p.add("  Travail perso ! ").unwrap().as_str(),
            "travail-perso"
        );
        assert_eq!(p.add("Élève").unwrap().as_str(), "eleve");
        assert_eq!(
            p.add("TRAVAIL PERSO !").unwrap_err(),
            ProfileError::NameTaken
        );
        assert_eq!(p.add("Travail  perso").unwrap().as_str(), "travail-perso-2");
        assert_eq!(p.add("Default ").unwrap_err(), ProfileError::NameTaken);
        assert_eq!(p.add("default x").unwrap().as_str(), "default-x");
        assert_eq!(p.add("😀").unwrap().as_str(), "profile");
        assert_eq!(p.add("   ").unwrap_err(), ProfileError::EmptyName);
    }

    #[test]
    fn rename_and_remove() {
        let mut p = Profiles::default();
        let work = p.add("Work").unwrap();
        p.rename(&work, "Job").unwrap();
        assert_eq!(p.get(&work).unwrap().name, "Job");
        assert_eq!(p.rename(&work, "default"), Err(ProfileError::NameTaken));
        p.rename(&work, "JOB").unwrap();
        assert_eq!(
            p.remove(&ProfileId::default_profile()),
            Err(ProfileError::DefaultProfile)
        );
        assert_eq!(p.remove(&work).unwrap().name, "JOB");
        assert_eq!(p.remove(&work), Err(ProfileError::NotFound));
    }
}
