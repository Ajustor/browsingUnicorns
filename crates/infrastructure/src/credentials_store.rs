//! The saved sign-ins, in a file sealed for the user where the system
//! offers it (DPAPI on Windows, see [`crate::seal`]; elsewhere the file is
//! private to the user), and the cookies already shared by sync.

use std::path::{Path, PathBuf};

use application::ports::{Repository, RepositoryError};
use domain::{Credentials, SeenCookies};
use serde::{Deserialize, Serialize};

use crate::dto::CredentialsDto;
use crate::{files, seal};

/// [`Credentials`] in a JSON file, sealed when the system can.
pub struct CredentialsRepository {
    path: PathBuf,
}

impl CredentialsRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Repository<Credentials> for CredentialsRepository {
    fn load(&self) -> Result<Credentials, RepositoryError> {
        let Some(text) = files::read(&self.path)? else {
            return Ok(Credentials::default());
        };
        // Unix: written readable by all by older versions.
        files::make_private(&self.path);
        let json = seal::open(&text).map_err(|e| files::error(&self.path, e))?;
        let dto: CredentialsDto =
            serde_json::from_str(&json).map_err(|e| files::error(&self.path, e))?;
        Ok(dto.into())
    }

    fn save(&self, credentials: &Credentials) -> Result<(), RepositoryError> {
        let json = serde_json::to_string(&CredentialsDto::from(credentials))
            .map_err(|e| files::error(&self.path, e))?;
        files::write(&self.path, &seal::seal(&json))
    }
}

#[derive(Default, Serialize, Deserialize)]
struct SeenFile {
    seen: Vec<String>,
}

/// [`SeenCookies`] in a JSON file (fingerprints are hashes).
pub struct SeenCookiesRepository {
    path: PathBuf,
}

impl SeenCookiesRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl Repository<SeenCookies> for SeenCookiesRepository {
    fn load(&self) -> Result<SeenCookies, RepositoryError> {
        let file: SeenFile = match files::read(&self.path)? {
            Some(text) => serde_json::from_str(&text).map_err(|e| files::error(&self.path, e))?,
            None => SeenFile::default(),
        };
        Ok(SeenCookies::from_fingerprints(file.seen))
    }

    fn save(&self, seen: &SeenCookies) -> Result<(), RepositoryError> {
        let file = SeenFile {
            seen: seen.fingerprints().to_vec(),
        };
        let text = serde_json::to_string(&file).map_err(|e| files::error(&self.path, e))?;
        files::write(&self.path, &text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_round_trip_sealed_where_possible() {
        let dir = tempfile::tempdir().unwrap();
        let repo = CredentialsRepository::new(dir.path().join("credentials.json"));
        assert!(repo.load().unwrap().items().is_empty());
        let mut c = Credentials::default();
        c.save("https://a.com", "me", "hunter2", 1);
        c.never_for("https://b.com", 2);
        c.remove("https://c.com", "x", 3);
        repo.save(&c).unwrap();
        assert_eq!(repo.load().unwrap(), c);
        let raw = std::fs::read_to_string(repo.path()).unwrap();
        if seal::AVAILABLE {
            assert!(seal::is_sealed(&raw));
            assert!(!raw.contains("hunter2"));
        }
    }

    #[test]
    fn seen_cookies_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let repo = SeenCookiesRepository::new(dir.path().join("seen.json"));
        let seen = SeenCookies::from_fingerprints(vec!["ab".into(), "cd".into()]);
        repo.save(&seen).unwrap();
        assert_eq!(repo.load().unwrap(), seen);
    }
}
