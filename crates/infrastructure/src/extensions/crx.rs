//! The `.crx` package of a Chrome extension: a header with the publisher's
//! public key(s) and signatures, then a zip of the extension's files.
//!
//! CRX3: `Cr24`, version 3, header length, then a protobuf `CrxFileHeader`.
//! (CRX2, signed with SHA-1, is refused, as Chrome has since version 75;
//! the stores serve CRX3.)
//!
//! The signatures are checked, as Chrome does: the id comes from a public
//! key, and only the holder of its private key can sign a package with it
//! (a file can't claim a password manager's id, and with it the right to
//! talk to its desktop app). Every proof must hold, one of them by the key
//! of the id.

use domain::ExtensionId;
use ring::signature::{self, UnparsedPublicKey, VerificationAlgorithm};
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
        3 => {
            let header_len = u32_at(bytes, 8)?;
            let header = slice(bytes, 12, header_len)?;
            let zip = &bytes[12 + header_len..];
            let header = read_header(header)?;
            // The id the package was signed for (there can be several
            // proofs, the Web Store adds its own), and its key's proof.
            let crx_id = header
                .crx_id
                .ok_or_else(i18n::errors::infrastructure::no_key_for_package_id)?;
            let key = header
                .proofs
                .iter()
                .map(|p| p.key)
                .find(|k| id_of_key(k) == crx_id)
                .ok_or_else(i18n::errors::infrastructure::no_key_for_package_id)?;
            // Signed: a prefix, the signed header data and its length, the zip.
            let signed_len = u32::try_from(header.signed_data.len())
                .map_err(|_| i18n::errors::infrastructure::bad_header().to_owned())?
                .to_le_bytes();
            let message = [
                &b"CRX3 SignedData\x00"[..],
                &signed_len,
                header.signed_data,
                zip,
            ]
            .concat();
            if !header
                .proofs
                .iter()
                .all(|p| verify(p.algorithm, p.key, &message, p.signature))
            {
                return Err(i18n::errors::infrastructure::bad_package_signature().into());
            }
            Ok(Crx {
                id: crx_id,
                public_key: key.to_vec(),
                zip,
            })
        }
        v => Err(i18n::errors::infrastructure::unsupported_package_version(v)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Algorithm {
    RsaSha256,
    EcdsaP256Sha256,
}

/// One signature of a CRX3 header: `AsymmetricKeyProof`.
struct Proof<'a> {
    algorithm: Algorithm,
    /// DER `SubjectPublicKeyInfo`.
    key: &'a [u8],
    signature: &'a [u8],
}

/// What a `CrxFileHeader` holds.
struct Header<'a> {
    proofs: Vec<Proof<'a>>,
    /// `signed_header_data`, as signed.
    signed_data: &'a [u8],
    /// The id it was signed for.
    crx_id: Option<ExtensionId>,
}

fn read_header(header: &[u8]) -> Result<Header<'_>, String> {
    let mut out = Header {
        proofs: Vec::new(),
        signed_data: &[],
        crx_id: None,
    };
    for field in Fields(header) {
        let (number, value) = field?;
        match (number, value) {
            // sha256_with_rsa, sha256_with_ecdsa: AsymmetricKeyProof
            // { public_key = 1, signature = 2 }
            (2 | 3, Value::Bytes(proof)) => {
                let (mut key, mut sig) = (None, None);
                for f in Fields(proof) {
                    match f? {
                        (1, Value::Bytes(k)) => key = Some(k),
                        (2, Value::Bytes(s)) => sig = Some(s),
                        _ => {}
                    }
                }
                let (Some(key), Some(signature)) = (key, sig) else {
                    return Err(i18n::errors::infrastructure::bad_header().into());
                };
                out.proofs.push(Proof {
                    algorithm: if number == 2 {
                        Algorithm::RsaSha256
                    } else {
                        Algorithm::EcdsaP256Sha256
                    },
                    key,
                    signature,
                });
            }
            // signed_header_data: SignedData { crx_id = 1 }
            (10000, Value::Bytes(signed)) => {
                out.signed_data = signed;
                for f in Fields(signed) {
                    if let (1, Value::Bytes(crx_id)) = f?
                        && crx_id.len() == 16
                    {
                        out.crx_id = Some(ExtensionId::from_key_hash(crx_id));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Whether `signature` of `message` holds for `key` (DER
/// `SubjectPublicKeyInfo`).
fn verify(algorithm: Algorithm, key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    const RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
    const EC: &[u8] = &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01];
    let Some((oid, raw)) = spki(key) else {
        return false;
    };
    // Keys of 1024 bits included: older Web Store extensions have them.
    let (expected, verifier): (&[u8], &dyn VerificationAlgorithm) = match algorithm {
        Algorithm::RsaSha256 => (
            RSA,
            &signature::RSA_PKCS1_1024_8192_SHA256_FOR_LEGACY_USE_ONLY,
        ),
        Algorithm::EcdsaP256Sha256 => (EC, &signature::ECDSA_P256_SHA256_ASN1),
    };
    if oid != expected {
        return false;
    }
    UnparsedPublicKey::new(verifier, raw)
        .verify(message, signature)
        .is_ok()
}

/// One DER element at the start of `input`: its tag, contents, and what
/// follows.
fn der(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 || rest.len() < n {
            return None;
        }
        let len = rest[..n]
            .iter()
            .fold(0usize, |len, &b| (len << 8) | usize::from(b));
        (len, &rest[n..])
    };
    if rest.len() < len {
        return None;
    }
    Some((tag, &rest[..len], &rest[len..]))
}

/// The algorithm (OID) and the key of a `SubjectPublicKeyInfo`: the
/// `RSAPublicKey` for RSA, the point for elliptic curves.
fn spki(key: &[u8]) -> Option<(&[u8], &[u8])> {
    let (0x30, info, _) = der(key)? else {
        return None;
    };
    let (0x30, algorithm, rest) = der(info)? else {
        return None;
    };
    let (0x06, oid, _) = der(algorithm)? else {
        return None;
    };
    let (0x03, bits, _) = der(rest)? else {
        return None;
    };
    // A bit string: the count of unused bits (none) first.
    let (&0, raw) = bits.split_first()? else {
        return None;
    };
    Some((oid, raw))
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

    /// A DER element.
    fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        match content.len() {
            n @ 0..0x80 => out.push(n as u8),
            n @ 0x80..0x100 => out.extend([0x81, n as u8]),
            n => out.extend([0x82, (n >> 8) as u8, n as u8]),
        }
        out.extend(content);
        out
    }

    fn spki(algorithm: &[u8], key: &[u8]) -> Vec<u8> {
        tlv(
            0x30,
            &[tlv(0x30, algorithm), tlv(0x03, &[&[0u8][..], key].concat())].concat(),
        )
    }

    /// A 2048-bit RSA key (PKCS #1), made for these tests only.
    const RSA_KEY: &str = "\
        MIIEogIBAAKCAQEA41ZpoQtGYOSuxs/wqIJIJLe3rfXZ+E5CrsN/l1jYugrbmVVDEx40KNXx/FBR3FQD7INEzbK7ykuTgJspp0dR\
        ppuIUnLDprKWLgM356Y9J0YzysMwxEQ6dxDQnf0UZfcah1XsKn/Z4+n/YFCjDytx9EcLx7WJvJHUdhjbGsG2lIE9WpZ7nRXDfsyA\
        KA1fDsdwsAmBHJvIY1bvTJf3HPScqIYYihoZqc24p6NXwcZv6Jku8GhB5XI30OhUl2NVLkL3t2p297WFPepj9Rt9YD+lYYzBauQQ\
        MpMaYQi3aIVhMc4UgJrigoaRc5Vykrvb9mzzvLjUHJ8WKW9MGxWBzCTBEQIDAQABAoH/EiQ+7e9VM8Y/ORfyJhnypb+66cGeYkJL\
        eIKJ5JOZ862sRVnLrlnRFiiesq12MDd3WLIDJkCxZDzuYxX+wkh4Tbiu4MvfNWKNH+VGHq7RLBiiWF4OaWfzHAsreJI81fMHgVQ4\
        7g1uX3xjShGtz1o6VClkDJm1Z1XZtxctOkeFyV6PSA/VgK/23Pw/FJbfiDtRg8krZqF+0qv68IK/PGlKX/gJDdZnjeDmdHH26WKz\
        9eJs9+rCq93lGj83y8k+AiHq5OtYsBjyxfdkEp3Nw3W4UT88jvI98c7ZxNOcc8AwkS1DxkEEyoEn9ZLY+RDVzmmpVwn/68ZlWUBF\
        NU6kMbIBAoGBAPwxrSPIUiSnIlSl0gsqZ0v1zPq7Fr1m19ztYGoHwQ0sdhRkkDFbjRRJ3XuHReXY4qpKqbXIRMGiyf/0sJiIZWTJ\
        n8yNwfPWscWicdp1tNpqqSl2cgN9UJd5ZrjlIFs7eaw2yYuUhS79n2p4dIRz3nr0ndtA61YxbtaHihnqCZ8RAoGBAObEtLosvIPy\
        CWPblwVNoLlD0Q7tchVvC3weRd6uPONANoZ+9ChsH0ZUiWqHC6j4AkvNrKRhwfXBaJTyAtJwMbrnEGH03Lb/V48pRTZbGW+4fYUx\
        6Bk/8AXHflKHbDnmP/VngEldWHWotIX6rf7WR6g1fA+Mmoc7B3qhCTELDQIBAoGAZ8elgTkZHH4usXi/U9ryQR74aCF/PRfKMt74\
        MwnX1qz6NnAfqkALcVLoMsI68FftBrjM18J/ENQFni+6UDqoxu31tX6MVuR9qV+szIJtlwgXbrmQN4FlHcBWljWrt/9klYrCK49F\
        s02pQW4qPKtV3XE2WJiBf6m9+8gw+3ME3JECgYEAttH/m771WlLVvZap/jmRqzRe20ShrPlcT+hr0g6XjjHBAKxzIdMGs+ElPI/g\
        XOyeZQ98Rn5r+xZOlDFmqvDXdQq5n+EtOozwASrg+j8R18CBMJRpHBfbP7jcCuXeh8Axy2lD6NtayVvuKHaAgfOgsVIuBuONSLUr\
        J6uMbJ4koAECgYEArfIfI9gVzKMa5oPu+zNROwjcum5jQn3ibCv4OL9VDIvmHOI7i7+ApX0OwRMwidXQ1EUqYcBaZA/jLg/He1mq\
        Ys7znfh7HzaaXjLDQefcW6PWh0AlpWjWmETubmP1PCH+L/BLxDEykmLTGSt7/RPRVpNqk0HNVRf2huIRMxftmJ4=";

    /// A key signing packages in these tests.
    pub(crate) enum TestKey {
        Rsa(signature::RsaKeyPair),
        Ec(signature::EcdsaKeyPair),
    }

    impl TestKey {
        fn rsa() -> Self {
            let der = data_encoding::BASE64.decode(RSA_KEY.as_bytes()).unwrap();
            Self::Rsa(signature::RsaKeyPair::from_der(&der).unwrap())
        }

        /// A new P-256 key.
        pub(crate) fn ec() -> Self {
            use signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair};
            let rng = ring::rand::SystemRandom::new();
            let pkcs8 =
                EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
            Self::Ec(
                EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
                    .unwrap(),
            )
        }

        /// DER `SubjectPublicKeyInfo`, as in a `.crx`.
        pub(crate) fn public_key(&self) -> Vec<u8> {
            use signature::KeyPair;
            match self {
                Self::Rsa(k) => spki(
                    &[
                        tlv(
                            0x06,
                            &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01],
                        ),
                        tlv(0x05, &[]),
                    ]
                    .concat(),
                    k.public_key().as_ref(),
                ),
                Self::Ec(k) => spki(
                    &[
                        tlv(0x06, &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01]),
                        tlv(0x06, &[0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]),
                    ]
                    .concat(),
                    k.public_key().as_ref(),
                ),
            }
        }

        fn sign(&self, message: &[u8]) -> Vec<u8> {
            let rng = ring::rand::SystemRandom::new();
            match self {
                Self::Rsa(k) => {
                    let mut sig = vec![0; k.public().modulus_len()];
                    k.sign(&signature::RSA_PKCS1_SHA256, &rng, message, &mut sig)
                        .unwrap();
                    sig
                }
                Self::Ec(k) => k.sign(&rng, message).unwrap().as_ref().to_vec(),
            }
        }

        /// The proof field of a header: 2 for RSA, 3 for ECDSA.
        fn field(&self) -> u64 {
            match self {
                Self::Rsa(_) => 2,
                Self::Ec(_) => 3,
            }
        }
    }

    /// The publisher of the test packages (always the same key).
    pub(crate) fn publisher() -> &'static TestKey {
        static KEY: std::sync::OnceLock<TestKey> = std::sync::OnceLock::new();
        KEY.get_or_init(TestKey::rsa)
    }

    /// A CRX3 around `zip` for the id of `id_key`, with a proof by each of
    /// `signers` (the key and the signature they claim, in turn).
    fn crx3_with(id_key: &[u8], proofs: &[(&TestKey, Vec<u8>)], zip: &[u8]) -> Vec<u8> {
        let hash = Sha256::digest(id_key);
        let signed_data = field(1, &hash[..16]);
        let message = [
            &b"CRX3 SignedData\x00"[..],
            &(signed_data.len() as u32).to_le_bytes(),
            &signed_data,
            zip,
        ]
        .concat();
        let mut header = Vec::new();
        for (signer, key) in proofs {
            let proof = [field(1, key), field(2, &signer.sign(&message))].concat();
            header.extend(field(signer.field(), &proof));
        }
        header.extend(field(10000, &signed_data));
        let mut out = b"Cr24".to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend((header.len() as u32).to_le_bytes());
        out.extend(header);
        out.extend(zip);
        out
    }

    /// A CRX3 signed by `key`, the store adding its own proof.
    pub(crate) fn crx3_by(key: &TestKey, zip: &[u8]) -> Vec<u8> {
        let store = TestKey::ec();
        crx3_with(
            &key.public_key(),
            &[(key, key.public_key()), (&store, store.public_key())],
            zip,
        )
    }

    /// A CRX3 of [`publisher`] around `zip`.
    pub(crate) fn crx3(zip: &[u8]) -> Vec<u8> {
        crx3_by(publisher(), zip)
    }

    #[test]
    fn reads_crx3() {
        let crx = crx3(b"PK zip");
        let parsed = parse(&crx).unwrap();
        assert_eq!(parsed.public_key, publisher().public_key());
        assert_eq!(parsed.id, id_of_key(&publisher().public_key()));
        assert_eq!(parsed.zip, b"PK zip");

        // Signed with an elliptic curve key.
        let ec = TestKey::ec();
        let crx = crx3_by(&ec, b"PK");
        assert_eq!(parse(&crx).unwrap().id, id_of_key(&ec.public_key()));
    }

    #[test]
    fn a_changed_package_is_refused() {
        let mut crx = crx3(b"PK zip");
        *crx.last_mut().unwrap() ^= 1;
        assert_eq!(
            parse(&crx).unwrap_err(),
            i18n::errors::infrastructure::bad_package_signature()
        );
    }

    #[test]
    fn an_id_takes_its_private_key() {
        // A package claiming the publisher's key (and id), signed by another.
        let thief = TestKey::ec();
        let stolen = publisher().public_key();
        let crx = crx3_with(&stolen, &[(&thief, stolen.clone())], b"PK");
        assert_eq!(
            parse(&crx).unwrap_err(),
            i18n::errors::infrastructure::bad_package_signature()
        );
        // The id of a key the header has no proof by.
        let crx = crx3_with(&stolen, &[(&thief, thief.public_key())], b"PK");
        assert_eq!(
            parse(&crx).unwrap_err(),
            i18n::errors::infrastructure::no_key_for_package_id()
        );
        // Every proof must hold, the store's too.
        let store = TestKey::ec();
        let crx = crx3_with(
            &stolen,
            &[(publisher(), stolen.clone()), (&thief, store.public_key())],
            b"PK",
        );
        assert!(parse(&crx).is_err());
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"PK\x03\x04").is_err());
        assert!(parse(b"Cr24\x03\x00\x00\x00\xff\xff\x00\x00").is_err());
        assert!(parse(b"Cr24\x09\x00\x00\x00").is_err());
        // CRX2 (SHA-1): no longer.
        let mut crx2 = b"Cr24".to_vec();
        for n in [2u32, 3, 2] {
            crx2.extend(n.to_le_bytes());
        }
        crx2.extend(b"keysgPK");
        assert!(parse(&crx2).is_err());
        // An unsigned CRX3.
        let mut unsigned = b"Cr24".to_vec();
        unsigned.extend(3u32.to_le_bytes());
        unsigned.extend(0u32.to_le_bytes());
        unsigned.extend(b"PK");
        assert!(parse(&unsigned).is_err());
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
