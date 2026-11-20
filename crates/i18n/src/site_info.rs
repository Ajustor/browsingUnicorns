//! The padlock's bar: the site's connection and permissions (src/ui/site_info.rs).

messages! {
    /// The bar's line for an HTTPS page.
    fn connection_secure() { en: "Connection is secure (HTTPS).", fr: "La connexion est sécurisée (HTTPS)." }
    /// The bar's line for a page not encrypted.
    fn connection_not_secure() {
        en: "Connection is not secure: what you send or see can be read on the way.",
        fr: "La connexion n’est pas sécurisée\u{a0}: ce que vous envoyez ou voyez peut être lu en chemin.",
    }
    /// While the engine looks the certificate up.
    fn reading_certificate() { en: "Reading the certificate…", fr: "Lecture du certificat…" }
    /// The certificate could not be read.
    fn certificate_unavailable(e: &dyn std::fmt::Display) {
        en: "Certificate not available: {e}",
        fr: "Certificat indisponible\u{a0}: {e}",
    }
    /// The connection's protocol and cipher.
    fn connection_of(connection: &str) { en: "Connection: {connection}", fr: "Connexion\u{a0}: {connection}" }
    /// Title of the site's own certificate.
    fn certificate_of(subject: &str) { en: "Certificate: {subject}", fr: "Certificat\u{a0}: {subject}" }
    /// Title of a certificate up the chain.
    fn issuer_certificate_of(subject: &str) {
        en: "Issuer certificate: {subject}",
        fr: "Certificat de l’émetteur\u{a0}: {subject}",
    }
    /// A certificate's row: whom it was issued to.
    fn issued_to() { en: "Issued to", fr: "Délivré à" }
    /// A certificate's row: who issued it.
    fn issued_by() { en: "Issued by", fr: "Délivré par" }
    /// A certificate's row: the names it covers.
    fn covers() { en: "Covers", fr: "Couvre" }
    /// A certificate's row: its start.
    fn valid_from() { en: "Valid from", fr: "Valide à partir du" }
    /// A certificate's row: its end.
    fn valid_until() { en: "Valid until", fr: "Valide jusqu’au" }
    /// A certificate's row: its serial number.
    fn serial_number() { en: "Serial number", fr: "Numéro de série" }
    /// Under an expired (or not yet valid) certificate.
    fn certificate_expired() {
        en: "This certificate is not valid at this date.",
        fr: "Ce certificat n’est pas valide à cette date.",
    }
}

#[cfg(test)]
mod tests {
    use super::in_lang::*;
    use crate::Lang;

    #[test]
    fn errors_are_wrapped() {
        let e = "timeout";
        assert_eq!(
            certificate_unavailable(Lang::En, &e),
            "Certificate not available: timeout"
        );
        assert_eq!(
            certificate_unavailable(Lang::Fr, &e),
            "Certificat indisponible\u{a0}: timeout"
        );
    }
}
