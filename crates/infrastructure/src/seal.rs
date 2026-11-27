//! Secrets sealed for the user where the system offers it: DPAPI on
//! Windows. Elsewhere the text is kept as is, protected only by the
//! permissions of the file (see [`crate::files`]).

/// Starts a sealed text (what follows: base64 of the sealed bytes).
pub const SEALED: &str = "sealed1:";

/// Whether this build seals ([`seal`] returns something else than its input).
pub const AVAILABLE: bool = cfg!(all(windows, feature = "desktop"));

/// `plain` sealed for the user, or as is where the system cannot seal.
pub fn seal(plain: &str) -> String {
    match os::close(plain.as_bytes()) {
        Some(sealed) => format!("{SEALED}{}", data_encoding::BASE64.encode(&sealed)),
        None => plain.to_owned(),
    }
}

/// Whether `text` was written by [`seal`] on a system that seals.
pub fn is_sealed(text: &str) -> bool {
    text.starts_with(SEALED)
}

/// What [`seal`] made of `text`; a text that was not sealed comes back as is.
pub fn open(text: &str) -> Result<String, String> {
    let Some(sealed) = text.strip_prefix(SEALED) else {
        return Ok(text.to_owned());
    };
    let bytes = data_encoding::BASE64
        .decode(sealed.trim().as_bytes())
        .map_err(|e| e.to_string())?;
    let plain = os::open(&bytes)
        .ok_or_else(|| i18n::errors::infrastructure::cannot_unseal().to_string())?;
    String::from_utf8(plain).map_err(|e| e.to_string())
}

#[cfg(all(windows, feature = "desktop"))]
mod os {
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

/// No sealing here (macOS, Linux, Android): no keychain is used, to stay
/// light; the files holding secrets are private to the user (`0600` in a
/// `0700` folder), and Android keeps them in the app's private storage.
#[cfg(not(all(windows, feature = "desktop")))]
mod os {
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
    fn seals_where_available_and_opens_back() {
        let sealed = seal("hunter2");
        assert_eq!(is_sealed(&sealed), AVAILABLE);
        assert_eq!(sealed.contains("hunter2"), !AVAILABLE);
        assert_eq!(open(&sealed).unwrap(), "hunter2");
        assert_eq!(open("plain").unwrap(), "plain");
        assert!(open(&format!("{SEALED}not base64!")).is_err());
    }
}
