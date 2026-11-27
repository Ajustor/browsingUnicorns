//! Ed25519 signature of `latest.json`.
//!
//! The asset's SHA-256 comes from the manifest, so on its own it only guards
//! against a corrupted download: whoever can change the manifest (the site,
//! a compromised Pages deploy) can change both. The release workflow signs
//! the manifest with a key kept in a GitHub secret (`UPDATE_SIGNING_KEY`,
//! see `scripts/sign-manifest.py`), and the updater refuses a manifest whose
//! signature does not check against the public key built into the app.
//!
//! What is signed is a canonical payload, not the JSON bytes, so the
//! signature can live in the manifest itself (one request, no window where
//! a cached `.sig` and a new manifest disagree). Each field is written as
//! `<byte length>:<UTF-8 bytes>\n`, after a fixed header:
//!
//! ```text
//! update-manifest-v1\n
//! version, page_url, notes, <number of assets>,
//! then for each asset in order: name, url, sha256
//! ```
//!
//! `scripts/sign-manifest.py` builds the same payload; keep them in sync.

use std::fmt::Write as _;

use i18n::errors::updater as msg;
use ring::signature::{ED25519, UnparsedPublicKey};

use crate::Manifest;

/// The release key's public half, hex (Ed25519, 32 bytes).
pub(crate) const UPDATE_PUBLIC_KEY: &str = include_str!("../update-signing-key.pub");

const HEADER: &str = "update-manifest-v1\n";

fn push_field(out: &mut String, value: &str) {
    let _ = writeln!(out, "{}:{value}", value.len());
}

/// The bytes the release workflow signs.
pub(crate) fn signed_payload(m: &Manifest) -> String {
    let mut out = String::from(HEADER);
    push_field(&mut out, &m.version);
    push_field(&mut out, &m.page_url);
    push_field(&mut out, &m.notes);
    push_field(&mut out, &m.assets.len().to_string());
    for a in &m.assets {
        push_field(&mut out, &a.name);
        push_field(&mut out, &a.url);
        push_field(&mut out, &a.sha256);
    }
    out
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim().as_bytes();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let nibble = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
    s.chunks(2)
        .map(|p| Some(nibble(p[0])? << 4 | nibble(p[1])?))
        .collect()
}

/// Check `m.signature` against `public_key_hex`. Nothing else in the
/// manifest may be used before this succeeds.
pub(crate) fn verify_manifest(m: &Manifest, public_key_hex: &str) -> Result<(), String> {
    if m.signature.trim().is_empty() {
        return Err(msg::manifest_unsigned().into());
    }
    let bad = || -> String { msg::manifest_bad_signature().into() };
    let key = decode_hex(public_key_hex).filter(|k| k.len() == 32);
    let sig = decode_hex(&m.signature).filter(|s| s.len() == 64);
    let (Some(key), Some(sig)) = (key, sig) else {
        return Err(bad());
    };
    UnparsedPublicKey::new(&ED25519, key)
        .verify(signed_payload(m).as_bytes(), &sig)
        .map_err(|_| bad())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ring::signature::{Ed25519KeyPair, KeyPair};

    /// A throwaway key for tests (not the release key).
    pub fn test_key() -> Ed25519KeyPair {
        Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap()
    }

    pub fn test_public_key_hex() -> String {
        hex(test_key().public_key().as_ref())
    }

    pub fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// `json` with a `signature` field made with [`test_key`].
    pub fn sign_json(json: &str) -> String {
        let m: Manifest = serde_json::from_str(json).unwrap();
        let sig = hex(test_key().sign(signed_payload(&m).as_bytes()).as_ref());
        let mut v: serde_json::Value = serde_json::from_str(json).unwrap();
        v["signature"] = sig.into();
        v.to_string()
    }

    const JSON: &str = r#"{"version":"1.2.0","notes":"é\nnotes","page_url":"https://p/","assets":[{"name":"a","url":"https://u/a","sha256":"00"}]}"#;

    fn parse(json: &str) -> Manifest {
        serde_json::from_str(json).unwrap()
    }

    fn en<T>(f: impl FnOnce() -> T) -> T {
        i18n::with_lang(i18n::Lang::En, f)
    }

    #[test]
    fn release_public_key_is_a_valid_ed25519_key() {
        let key = decode_hex(UPDATE_PUBLIC_KEY).unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn payload_is_length_prefixed() {
        assert_eq!(
            signed_payload(&parse(JSON)),
            "update-manifest-v1\n5:1.2.0\n10:https://p/\n8:é\nnotes\n1:1\n1:a\n11:https://u/a\n2:00\n"
        );
    }

    #[test]
    fn valid_signature_is_accepted() {
        let m = parse(&sign_json(JSON));
        assert!(verify_manifest(&m, &test_public_key_hex()).is_ok());
    }

    #[test]
    fn tampered_manifest_is_rejected() {
        let signed = sign_json(JSON);
        for (from, to) in [
            ("https://u/a", "https://evil/a"),
            (r#""sha256":"00""#, r#""sha256":"11""#),
            ("1.2.0", "1.3.0"),
            ("notes", "Notes"),
        ] {
            let m = parse(&signed.replace(from, to));
            let err = en(|| verify_manifest(&m, &test_public_key_hex())).unwrap_err();
            assert!(err.contains("signature is invalid"), "{from}: {err}");
        }
        // Signed by another key.
        let m = parse(&signed);
        let other = hex(Ed25519KeyPair::from_seed_unchecked(&[8; 32])
            .unwrap()
            .public_key()
            .as_ref());
        assert!(verify_manifest(&m, &other).is_err());
        // Garbage signature.
        let mut m = parse(&signed);
        m.signature = "zz".into();
        assert!(verify_manifest(&m, &test_public_key_hex()).is_err());
    }

    #[test]
    fn missing_signature_is_rejected() {
        let err = en(|| verify_manifest(&parse(JSON), &test_public_key_hex())).unwrap_err();
        assert!(err.contains("not signed"), "{err}");
    }
}
