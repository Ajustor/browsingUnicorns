//! The connection and certificate a site showed: told by the web view
//! (WebView2's DevTools protocol, `Security.visibleSecurityStateChanged`),
//! decoded here.

use sha2::{Digest, Sha256};
use x509_parser::prelude::{FromDer, GeneralName, X509Certificate, X509Name};

/// One certificate of a site's chain (the site's own first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertificateInfo {
    /// Common name of the subject, or the whole name.
    pub subject: String,
    /// Organization and common name of the issuer.
    pub issuer: String,
    /// The DNS names it covers (subject alternative names).
    pub names: Vec<String>,
    /// `YYYY-MM-DD HH:MM UTC`.
    pub valid_from: String,
    pub valid_until: String,
    pub expired: bool,
    pub serial: String,
    /// SHA-256 of the DER bytes, `AB:CD:...`.
    pub sha256: String,
}

/// How a page's connection is secured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteSecurity {
    /// Protocol, key exchange and cipher (`TLS 1.3, X25519, AES_128_GCM`).
    pub connection: String,
    /// The site's certificate first, then its issuers'.
    pub chain: Vec<CertificateInfo>,
}

/// A site's security, or why it isn't known.
pub type Certificates = Result<SiteSecurity, String>;

fn common_name(name: &X509Name) -> Option<String> {
    name.iter_common_name()
        .next()
        .and_then(|cn| cn.as_str().ok())
        .map(str::to_owned)
}

fn organization(name: &X509Name) -> Option<String> {
    name.iter_organization()
        .next()
        .and_then(|o| o.as_str().ok())
        .map(str::to_owned)
}

fn date(t: &x509_parser::time::ASN1Time) -> String {
    let t = t.to_datetime();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute()
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// Decodes one DER certificate.
pub fn parse(der: &[u8]) -> Result<CertificateInfo, String> {
    let (_, cert) = X509Certificate::from_der(der).map_err(|e| e.to_string())?;
    let subject = common_name(cert.subject()).unwrap_or_else(|| cert.subject().to_string());
    let issuer = match (organization(cert.issuer()), common_name(cert.issuer())) {
        (Some(o), Some(cn)) if o != cn => format!("{cn} ({o})"),
        (_, Some(cn)) => cn,
        (Some(o), None) => o,
        (None, None) => cert.issuer().to_string(),
    };
    let names = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|san| {
            san.value
                .general_names
                .iter()
                .filter_map(|n| match n {
                    GeneralName::DNSName(d) => Some((*d).to_owned()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let validity = cert.validity();
    Ok(CertificateInfo {
        subject,
        issuer,
        names,
        valid_from: date(&validity.not_before),
        valid_until: date(&validity.not_after),
        expired: !validity.is_valid(),
        serial: hex(cert.raw_serial()),
        sha256: hex(&Sha256::digest(der)),
    })
}

/// Decodes the parameters of `Security.visibleSecurityStateChanged`:
/// `{"visibleSecurityState": {"certificateSecurityState": {...}}}`.
pub fn from_security_state(json: &str) -> Certificates {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let state = value
        .pointer("/visibleSecurityState/certificateSecurityState")
        .ok_or_else(i18n::errors::infrastructure::no_certificate)?;
    let text = |key: &str| state.get(key).and_then(|v| v.as_str()).unwrap_or("");
    let connection = [text("protocol"), text("keyExchangeGroup"), text("cipher")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    let chain = state
        .get("certificate")
        .and_then(|c| c.as_array())
        .ok_or_else(i18n::errors::infrastructure::no_certificate)?
        .iter()
        .filter_map(|c| c.as_str())
        .map(|b64| {
            let der = data_encoding::BASE64
                .decode(b64.as_bytes())
                .map_err(|e| e.to_string())?;
            parse(&der)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if chain.is_empty() {
        return Err(i18n::errors::infrastructure::no_certificate().into());
    }
    Ok(SiteSecurity { connection, chain })
}

/// Keeps `slot` up to date with the security of `view`'s page (from its
/// first load on); `changed` tells the UI.
#[cfg(windows)]
pub fn watch(
    view: &wry::WebView,
    slot: std::rc::Rc<std::cell::RefCell<Option<Certificates>>>,
    changed: impl Fn() + 'static,
) -> windows_core::Result<()> {
    use webview2_com::{
        CallDevToolsProtocolMethodCompletedHandler, DevToolsProtocolEventReceivedEventHandler,
        take_pwstr,
    };
    use windows_core::{HSTRING, PWSTR};
    use wry::WebViewExtWindows as _;

    let webview = view.webview();
    let handler = DevToolsProtocolEventReceivedEventHandler::create(Box::new(move |_, args| {
        let Some(args) = args else { return Ok(()) };
        let mut json = PWSTR::null();
        unsafe { args.ParameterObjectAsJson(&mut json)? };
        *slot.borrow_mut() = Some(from_security_state(&take_pwstr(json)));
        changed();
        Ok(())
    }));
    unsafe {
        let receiver = webview.GetDevToolsProtocolEventReceiver(&HSTRING::from(
            "Security.visibleSecurityStateChanged",
        ))?;
        receiver.add_DevToolsProtocolEventReceived(&handler, &mut 0)?;
        webview.CallDevToolsProtocolMethod(
            &HSTRING::from("Security.enable"),
            &HSTRING::from("{}"),
            &CallDevToolsProtocolMethodCompletedHandler::create(Box::new(|_, _| Ok(()))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SITE: &[u8] = include_bytes!("../testdata/site.der");
    const SITE_B64: &str = "MIIB3TCCAYSgAwIBAgIUcuyZIL+IN1BX+YM61lD/OVYGKqMwCgYIKoZIzj0EAwIwLzEWMBQGA1UECgwNVW5pY29ybiBUZXN0czEVMBMGA1UEAwwMZXhhbXBsZS50ZXN0MB4XDTI2MTExNjA3MjU0OVoXDTM2MTExMzA3MjU0OVowLzEWMBQGA1UECgwNVW5pY29ybiBUZXN0czEVMBMGA1UEAwwMZXhhbXBsZS50ZXN0MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEhX479PQPB9dO3RsvnZl62cBqg6U47QM8obr57mtFygKreFkKuH9VoYEYaxaZfVRTl8EgChvcf6Y7X4hhtfspwaN+MHwwHQYDVR0OBBYEFAmahWAJ7C4eTp9/bBjcfyN2YfgoMB8GA1UdIwQYMBaAFAmahWAJ7C4eTp9/bBjcfyN2YfgoMA8GA1UdEwEB/wQFMAMBAf8wKQYDVR0RBCIwIIIMZXhhbXBsZS50ZXN0ghB3d3cuZXhhbXBsZS50ZXN0MAoGCCqGSM49BAMCA0cAMEQCIA8p3b0uLm+Le+a8d7WBDJ7hnU9S/qs45Y2gCJ4BucRWAiAkPdUpi9XMEdhS8ZTNyAZRP1jdQh7YZf6IDD3yiVwdlw==";

    #[test]
    fn reads_a_certificate() {
        let cert = parse(SITE).unwrap();
        assert_eq!(cert.subject, "example.test");
        // Self-signed: issued by itself.
        assert_eq!(cert.issuer, "example.test (Unicorn Tests)");
        assert_eq!(cert.names, ["example.test", "www.example.test"]);
        assert_eq!(cert.valid_until, "2036-11-13 07:25 UTC");
        assert!(cert.sha256.starts_with("35:9B:77:AC:D1:3E"));
    }

    #[test]
    fn reads_the_security_state() {
        let json = serde_json::json!({ "visibleSecurityState": {
            "securityState": "secure",
            "certificateSecurityState": {
                "protocol": "TLS 1.3",
                "keyExchange": "",
                "keyExchangeGroup": "X25519",
                "cipher": "AES_128_GCM",
                "certificate": [SITE_B64],
            },
        }})
        .to_string();
        let security = from_security_state(&json).unwrap();
        assert_eq!(security.connection, "TLS 1.3, X25519, AES_128_GCM");
        assert_eq!(security.chain[0].subject, "example.test");
    }

    #[test]
    fn no_certificate_is_an_error() {
        // Plain http: no certificate state.
        let http = r#"{"visibleSecurityState": {"securityState": "insecure"}}"#;
        assert!(from_security_state(http).is_err());
        let empty =
            r#"{"visibleSecurityState": {"certificateSecurityState": {"certificate": []}}}"#;
        assert!(from_security_state(empty).is_err());
        let bad = r#"{"visibleSecurityState": {"certificateSecurityState": {"certificate": ["not base64!"]}}}"#;
        assert!(from_security_state(bad).is_err());
    }
}
