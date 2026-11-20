//! Errors of the `infrastructure` crate, and the few texts it shows itself.

messages! {
    // Private windows.

    /// A change refused in a private window (saving, installing).
    fn not_in_private_window() { en: "Not available in a private window.", fr: "Indisponible dans une fenêtre privée." }

    // Profiles.

    /// Deleting the data of the default profile.
    fn default_profile_kept() { en: "the default profile is kept", fr: "le profil par défaut est conservé" }

    // Extensions.

    /// A store (`Chrome Web Store`, `Edge Add-ons`) serving nothing for the id.
    fn no_extension_with_id(store: &str) { en: "{store}: no extension with this id", fr: "{store}\u{a0}: aucune extension avec cet identifiant" }
    /// The downloaded package is another extension than the one asked for.
    fn package_is_another_extension(found: &str, expected: &str) {
        en: "the package is extension {found}, not {expected}",
        fr: "le paquet est l’extension {found}, pas {expected}",
    }
    /// The folder chosen for an unpacked extension has no manifest.
    fn no_manifest_in_folder(folder: &dyn std::fmt::Display) {
        en: "{folder} has no manifest.json: choose the extension's folder",
        fr: "{folder} n’a pas de manifest.json\u{a0}: choisissez le dossier de l’extension",
    }
    /// The previous version's files could not be removed.
    fn installed_version_not_replaced(e: &dyn std::fmt::Display) {
        en: "could not replace the installed version: {e}",
        fr: "impossible de remplacer la version installée\u{a0}: {e}",
    }
    /// A file that is not a `.crx`.
    fn not_a_crx() { en: "not a Chrome extension package (.crx)", fr: "pas un paquet d’extension Chrome (.crx)" }
    /// A `.crx` of a format version the browser does not read.
    fn unsupported_package_version(version: usize) {
        en: "unsupported package version {version}",
        fr: "version de paquet non prise en charge\u{a0}: {version}",
    }
    /// A `.crx` cut short.
    fn truncated_package() { en: "truncated package", fr: "paquet incomplet" }
    /// A `.crx` whose header is cut short.
    fn truncated_header() { en: "truncated header", fr: "en-tête du paquet incomplet" }
    /// A `.crx` whose header does not read.
    fn bad_header() { en: "bad header", fr: "en-tête du paquet invalide" }
    /// A `.crx` signed by no key matching its id.
    fn no_key_for_package_id() { en: "no public key matches the package's id", fr: "aucune clé publique ne correspond à l’identifiant du paquet" }
    /// A `manifest.json` that is not a JSON object.
    fn manifest_not_an_object() { en: "manifest.json is not an object", fr: "manifest.json n’est pas un objet" }
    /// A `manifest.json` without `name`.
    fn manifest_without_name() { en: "the manifest has no name", fr: "le manifeste n’a pas de nom" }

    // User scripts.

    /// Installing from something that is not a web address.
    fn paste_a_user_script_address() {
        en: "Paste the address of a .user.js script (Greasy Fork, GitHub…).",
        fr: "Collez l’adresse d’un script .user.js (Greasy Fork, GitHub…).",
    }
    /// A downloaded script (or `@require`) that is not text.
    fn not_utf8_text(url: &str) { en: "{url}: not UTF-8 text", fr: "{url}\u{a0}: pas du texte UTF-8" }
    /// A file without a user script's metadata.
    fn not_a_user_script() {
        en: "not a user script (no ==UserScript== block with a @name)",
        fr: "pas un script utilisateur (aucun bloc ==UserScript== avec un @name)",
    }
    /// A user script without `@match` nor `@include`.
    fn script_runs_on_no_page() {
        en: "the script runs on no page (no @match or @include)",
        fr: "le script ne s’applique à aucune page (ni @match ni @include)",
    }

    // Sync.

    /// The relay could not be reached.
    fn relay_unreachable(e: &dyn std::fmt::Display) { en: "relay unreachable: {e}", fr: "relais injoignable\u{a0}: {e}" }
    /// The relay's answer could not be read.
    fn relay_failed(e: &dyn std::fmt::Display) { en: "relay: {e}", fr: "relais\u{a0}: {e}" }
    /// The relay answered with an error status and its message.
    fn relay_refused(status: u16, message: &str) { en: "relay: {status} {message}", fr: "relais\u{a0}: {status} {message}" }
    /// The relay's answer is not what it should be.
    fn relay_answer_unreadable(e: &dyn std::fmt::Display) { en: "relay answer: {e}", fr: "réponse du relais illisible\u{a0}: {e}" }
    /// This device's record could not be encoded for sending.
    fn record_not_encoded(e: &dyn std::fmt::Display) { en: "encoding the record: {e}", fr: "encodage des données\u{a0}: {e}" }

    // Web engine.

    /// The engine has no view for the tab.
    fn no_view_for_tab(tab: &dyn std::fmt::Display) { en: "no view for tab {tab}", fr: "aucune vue pour l’onglet {tab}" }
    /// Title of the dialog asking where to save a download.
    fn save_as() { en: "Save as", fr: "Enregistrer sous" }
    /// A secure page without a certificate to show.
    fn no_certificate() { en: "no certificate", fr: "aucun certificat" }
    /// Where the web view does not tell how pages are secured.
    fn web_view_does_not_tell() { en: "this system's web view doesn't tell", fr: "le moteur web de ce système ne l’indique pas" }

    // Default browser.

    /// Windows' Settings (default apps) did not open.
    fn settings_not_opened(code: isize) {
        en: "could not open the Settings (error {code})",
        fr: "impossible d’ouvrir les Paramètres (erreur {code})",
    }
    /// `xdg-settings` refused: its message, and the likely cause.
    fn xdg_settings_failed(e: &str) {
        en: "xdg-settings: {e} (is browsingUnicorns installed with its .desktop file?)",
        fr: "xdg-settings\u{a0}: {e} (browsingUnicorns est-il installé avec son fichier .desktop\u{a0}?)",
    }
    /// Making the browser the default one, on macOS.
    fn not_available_on_macos() { en: "not available on macOS", fr: "indisponible sur macOS" }

    // Bookmark import.

    /// A browser's bookmarks file without bookmarks.
    fn no_bookmarks_in_file() { en: "no bookmarks in this file", fr: "aucun favori dans ce fichier" }
}
