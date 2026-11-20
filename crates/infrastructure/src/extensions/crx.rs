//! The `.crx` package of a Chrome extension: a header with the publisher's
//! public key(s) and signatures, then a zip of the extension's files.
//!
//! CRX3: `Cr24`, version 3, header length, then a protobuf `CrxFileHeader`.
//! CRX2: `Cr24`, version 2, key length, signature length, key, signature.
//!
//! The signatures are not checked: packages come from the Chrome Web Store
//! over HTTPS, or are files the user chose.

use domain::ExtensionId;
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct Crx<'a> {
    pub id: ExtensionId,
    /// DER public key, written into the manifest as `key` so the unpacked
    /// extension keeps its Web Store id.
    pub public_key: Vec<u8>,
    pub zip: &'a [u8],
}

pub fn id_of_key(public_key: &[u8]) -> ExtensionId {
    ExtensionId::from_key_hash(&Sha256::digest(public_key))
}

fn u32_at(bytes: &[u8], at: usize) -> Result<usize, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        .ok_or_else(|| i18n::errors::infrastructure::truncated_package().to_owned())
}

fn slice(bytes: &[u8], from: usize, len: usize) -> Result<&[u8], String> {
    from.checked_add(len)
        .and_then(|end| bytes.get(from..end))
        .ok_or_else(|| i18n::errors::infrastructure::truncated_package().to_owned())
}

pub fn parse(bytes: &[u8]) -> Result<Crx<'_>, String> {
    if bytes.get(..4) != Some(b"Cr24") {
        return Err(i18n::errors::infrastructure::not_a_crx().into());
    }
    match u32_at(bytes, 4)? {
        2 => {
            let key_len = u32_at(bytes, 8)?;
            let sig_len = u32_at(bytes, 12)?;
            let key = slice(bytes, 16, key_len)?;
            let zip_at = 16 + key_len + sig_len;
            Ok(Crx {
                id: id_of_key(key),
                public_key: key.to_vec(),
                zip: bytes
                    .get(zip_at..)
                    .ok_or_else(i18n::errors::infrastructure::truncated_package)?,
            })
        }
        3 => {
            let header_len = u32_at(bytes, 8)?;
            let header = slice(bytes, 12, header_len)?;
            let (keys, crx_id) = read_header(header)?;
            // The key the id was derived from (there can be several proofs,
            // the Web Store adds its own).
            let key = match crx_id {
                Some(id) => keys.into_iter().find(|k| id_of_key(k) == id),
                None => keys.into_iter().next(),
            }
            .ok_or_else(i18n::errors::infrastructure::no_key_for_package_id)?;
            Ok(Crx {
                id: id_of_key(&key),
                public_key: key,
                zip: &bytes[12 + header_len..],
            })
        }
        v => Err(i18n::errors::infrastructure::unsupported_package_version(v)),
    }
}

/// The public keys of a `CrxFileHeader`, and the id it was signed for.
fn read_header(header: &[u8]) -> Result<(Vec<Vec<u8>>, Option<ExtensionId>), String> {
    let mut keys = Vec::new();
    let mut id = None;
    for field in Fields(header) {
        let (number, value) = field?;
        match (number, value) {
            // sha256_with_rsa, sha256_with_ecdsa: AsymmetricKeyProof
            (2 | 3, Value::Bytes(proof)) => {
                for f in Fields(proof) {
                    if let (1, Value::Bytes(key)) = f? {
                        keys.push(key.to_vec());
                    }
                }
            }
            // signed_header_data: SignedData { crx_id }
            (10000, Value::Bytes(signed)) => {
                for f in Fields(signed) {
                    if let (1, Value::Bytes(crx_id)) = f? {
                        id = Some(ExtensionId::from_key_hash(crx_id));
                    }
                }
            }
            _ => {}
        }
    }
    Ok((keys, id))
}

enum Value<'a> {
    Int,
    Bytes(&'a [u8]),
}

/// The fields of a protobuf message (just what the CRX header needs).
struct Fields<'a>(&'a [u8]);

fn varint(bytes: &mut &[u8]) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&b, rest) = bytes
            .split_first()
            .ok_or_else(i18n::errors::infrastructure::truncated_header)?;
        *bytes = rest;
        value |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(i18n::errors::infrastructure::bad_header().into())
}

impl<'a> Iterator for Fields<'a> {
    type Item = Result<(u64, Value<'a>), String>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }
        let mut read = || -> Result<(u64, Value<'a>), String> {
            let tag = varint(&mut self.0)?;
            let value = match tag & 7 {
                0 => {
                    varint(&mut self.0)?;
                    Value::Int
                }
                1 | 5 => {
                    let n = if tag & 7 == 1 { 8 } else { 4 };
                    self.0 = self
                        .0
                        .get(n..)
                        .ok_or_else(i18n::errors::infrastructure::truncated_header)?;
                    Value::Int
                }
                2 => {
                    let len = usize::try_from(varint(&mut self.0)?).map_err(|e| e.to_string())?;
                    let bytes = self
                        .0
                        .get(..len)
                        .ok_or_else(i18n::errors::infrastructure::truncated_header)?;
                    self.0 = &self.0[len..];
                    Value::Bytes(bytes)
                }
                _ => return Err(i18n::errors::infrastructure::bad_header().into()),
            };
            Ok((tag >> 3, value))
        };
        let item = read();
        if item.is_err() {
            self.0 = &[];
        }
        Some(item)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn varint_bytes(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return out;
            }
            out.push(b | 0x80);
        }
    }

    fn field(number: u64, bytes: &[u8]) -> Vec<u8> {
        let mut out = varint_bytes(number << 3 | 2);
        out.extend(varint_bytes(bytes.len() as u64));
        out.extend(bytes);
        out
    }

    /// A CRX3 with `key` (plus another proof) around `zip`.
    pub(crate) fn crx3(key: &[u8], zip: &[u8]) -> Vec<u8> {
        let hash = Sha256::digest(key);
        let mut header = field(2, &[field(1, b"other key"), field(2, b"sig")].concat());
        header.extend(field(2, &[field(1, key), field(2, b"sig")].concat()));
        header.extend(field(10000, &field(1, &hash[..16])));
        let mut out = b"Cr24".to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend((header.len() as u32).to_le_bytes());
        out.extend(header);
        out.extend(zip);
        out
    }

    #[test]
    fn reads_crx3() {
        let crx = crx3(b"publisher key", b"PK zip");
        let parsed = parse(&crx).unwrap();
        assert_eq!(parsed.public_key, b"publisher key");
        assert_eq!(parsed.id, id_of_key(b"publisher key"));
        assert_eq!(parsed.zip, b"PK zip");
    }

    #[test]
    fn reads_crx2() {
        let mut crx = b"Cr24".to_vec();
        for n in [2u32, 3, 2] {
            crx.extend(n.to_le_bytes());
        }
        crx.extend(b"keysgPK");
        let parsed = parse(&crx).unwrap();
        assert_eq!(parsed.public_key, b"key");
        assert_eq!(parsed.zip, b"PK");
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"PK\x03\x04").is_err());
        assert!(parse(b"Cr24\x03\x00\x00\x00\xff\xff\x00\x00").is_err());
        assert!(parse(b"Cr24\x09\x00\x00\x00").is_err());
    }

    #[test]
    fn errors_speak_the_current_language() {
        let fr = |bytes: &[u8]| i18n::with_lang(i18n::Lang::Fr, || parse(bytes).err().unwrap());
        assert_eq!(fr(b"PK\x03\x04"), "pas un paquet d’extension Chrome (.crx)");
        assert_eq!(
            fr(b"Cr24\x09\x00\x00\x00"),
            "version de paquet non prise en charge\u{a0}: 9"
        );
        assert_eq!(
            parse(b"PK\x03\x04").err().unwrap(),
            "not a Chrome extension package (.crx)"
        );
    }
}
