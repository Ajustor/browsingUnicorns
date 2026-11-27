//! Errors of the `updater` crate the user reads (a failed check or update).
//! Its staging and file-system failures stay in English: technical.

messages! {
    /// The manifest or the asset could not be fetched.
    fn request_failed(url: &str, e: &dyn std::fmt::Display) {
        en: "request to {url} failed: {e}",
        fr: "échec de la requête vers {url}\u{a0}: {e}",
    }
    /// The manifest's body could not be read.
    fn manifest_unread(e: &dyn std::fmt::Display) {
        en: "reading release manifest: {e}",
        fr: "lecture du manifeste de publication\u{a0}: {e}",
    }
    /// The manifest is not the expected JSON.
    fn manifest_unparsed(e: &dyn std::fmt::Display) {
        en: "parsing release manifest: {e}",
        fr: "manifeste de publication illisible\u{a0}: {e}",
    }
    /// The manifest carries no signature.
    fn manifest_unsigned() {
        en: "release manifest is not signed; refusing to update",
        fr: "le manifeste de publication n’est pas signé\u{a0}; mise à jour refusée",
    }
    /// The manifest's signature does not match its content or our key.
    fn manifest_bad_signature() {
        en: "release manifest signature is invalid; refusing to update",
        fr: "la signature du manifeste de publication est invalide\u{a0}; mise à jour refusée",
    }
    /// The running version is not a semantic version.
    fn bad_current_version(e: &dyn std::fmt::Display) {
        en: "bad current version: {e}",
        fr: "version actuelle invalide\u{a0}: {e}",
    }
    /// The manifest's version is not a semantic version.
    fn bad_release_version(version: &str, e: &dyn std::fmt::Display) {
        en: "bad release version {version:?}: {e}",
        fr: "version publiée invalide {version:?}\u{a0}: {e}",
    }
    /// No binary is published for this system (portable installs).
    fn no_binary_for_platform() {
        en: "no prebuilt binary for this platform",
        fr: "aucun binaire précompilé pour cette plateforme",
    }
    /// No AppImage is published for this system.
    fn no_appimage_for_platform() {
        en: "no AppImage for this platform",
        fr: "aucune AppImage pour cette plateforme",
    }
    /// The release lacks the file this install needs.
    fn release_without_asset(version: &dyn std::fmt::Display, asset: &str) {
        en: "release v{version} has no asset {asset}",
        fr: "la version {version} n’a pas de fichier {asset}",
    }
    /// The asset's download broke off.
    fn download_failed(asset: &str, e: &dyn std::fmt::Display) {
        en: "downloading {asset}: {e}",
        fr: "téléchargement de {asset}\u{a0}: {e}",
    }
    /// An AppImage update while the app does not run from one.
    fn not_in_an_appimage() {
        en: "not running from an AppImage",
        fr: "l’application n’est pas lancée depuis une AppImage",
    }
    /// The manifest gives no checksum for the asset.
    fn no_checksum() {
        en: "release asset has no sha256; refusing to install",
        fr: "le fichier publié n’a pas de SHA-256\u{a0}; installation refusée",
    }
    /// The downloaded asset is not the one published.
    fn checksum_mismatch(expected: &str, actual: &str) {
        en: "checksum mismatch (expected {expected}, got {actual})",
        fr: "somme de contrôle différente (attendue {expected}, obtenue {actual})",
    }
}
