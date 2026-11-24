//! The saved sign-ins, in a file sealed for the user where the system
//! offers it (DPAPI on Windows; elsewhere the file lives in the user's
//! private data folder), and the cookies already shared by sync.

use std::path::{Path, PathBuf};

use application::ports::{Repository, RepositoryError};
use domain::{Credentials, SeenCookies};
use serde::{Deserialize, Serialize};

use crate::dto::CredentialsDto;
use crate::files;

/// Starts a sealed file (what follows: base64 of the sealed JSON).
const SEALED: &str = "sealed1:";

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
        let json = match text.strip_prefix(SEALED) {
            Some(sealed) => {
                let bytes = data_encoding::BASE64
                    .decode(sealed.trim().as_bytes())
                    .map_err(|e| files::error(&self.path, e))?;
                let plain = seal::open(&bytes).ok_or_else(|| {
                    files::error(&self.path, i18n::errors::infrastructure::cannot_unseal())
                })?;
                String::from_utf8(plain).map_err(|e| files::error(&self.path, e))?
            }
            None => text,
        };
        let dto: CredentialsDto =
            serde_json::from_str(&json).map_err(|e| files::error(&self.path, e))?;
        Ok(dto.into())
    }

    fn save(&self, credentials: &Credentials) -> Result<(), RepositoryError> {
        let json = serde_json::to_string(&CredentialsDto::from(credentials))
            .map_err(|e| files::error(&self.path, e))?;
        let text = match seal::close(json.as_bytes()) {
            Some(sealed) => format!("{SEALED}{}", data_encoding::BASE64.encode(&sealed)),
            None => json,
        };
        files::write(&self.path, &text)
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

#[cfg(all(windows, feature = "desktop"))]
mod seal {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows_core::PCWSTR;

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        }
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        // SAFETY: the system allocated `cbData` bytes at `pbData`, freed here.
        unsafe {
            let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(Some(HLOCAL(out.pbData.cast())));
            bytes
        }
    }

    /// Seals `plain` for the current Windows user.
    pub fn close(plain: &[u8]) -> Option<Vec<u8>> {
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: the input outlives the call, the output is taken above.
        unsafe {
            CryptProtectData(
                &blob(plain),
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .ok()?;
        Some(take(out))
    }

    pub fn open(sealed: &[u8]) -> Option<Vec<u8>> {
        let mut out = CRYPT_INTEGER_BLOB::default();
        // SAFETY: as in `close`.
        unsafe {
            CryptUnprotectData(
                &blob(sealed),
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
        .ok()?;
        Some(take(out))
    }
}

/// No sealing here: the file is written as is.
#[cfg(not(all(windows, feature = "desktop")))]
mod seal {
    pub fn close(_plain: &[u8]) -> Option<Vec<u8>> {
        None
    }

    pub fn open(_sealed: &[u8]) -> Option<Vec<u8>> {
        None
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
        if cfg!(all(windows, feature = "desktop")) {
            assert!(raw.starts_with(SEALED));
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
