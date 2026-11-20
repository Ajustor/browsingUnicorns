//! The About view of the side panel (src/ui/library.rs).

use std::fmt::Display;

messages! {
    /// The version running.
    fn version(v: &dyn Display) { en: "Version {v}", fr: "Version {v}" }
    fn checking() { en: "Checking for updates…", fr: "Recherche de mises à jour…" }
    fn up_to_date() { en: "Up to date.", fr: "À jour." }
    fn available(v: &dyn Display) { en: "Version {v} is available.", fr: "La version {v} est disponible." }
    fn downloading(v: &dyn Display) { en: "Downloading version {v}…", fr: "Téléchargement de la version {v}…" }
    fn ready(v: &dyn Display) {
        en: "Version {v} is ready: restart to use it.",
        fr: "La version {v} est prête\u{a0}: redémarrez pour l'utiliser.",
    }
    fn check_failed(e: &dyn Display) {
        en: "Update check failed: {e}",
        fr: "La recherche de mises à jour a échoué\u{a0}: {e}",
    }
    /// The button.
    fn check_for_updates() { en: "Check for updates", fr: "Rechercher des mises à jour" }
    fn is_default() { en: "browsingUnicorns is your default browser.", fr: "browsingUnicorns est votre navigateur par défaut." }
    fn is_not_default() { en: "browsingUnicorns is not your default browser.", fr: "browsingUnicorns n'est pas votre navigateur par défaut." }
    /// The button.
    fn make_default() { en: "Make it the default browser", fr: "En faire le navigateur par défaut" }
}
