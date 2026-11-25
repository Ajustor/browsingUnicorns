//! The app's updates (src/ui/updates.rs).

messages! {
    /// A check found nothing new.
    fn up_to_date() { en: "browsingUnicorns is up to date.", fr: "browsingUnicorns est à jour." }
    /// A check or download failed.
    fn failed(e: &dyn std::fmt::Display) { en: "Update: {e}", fr: "Mise à jour\u{a0}: {e}" }
    /// The banner, a new version out.
    fn available(version: &dyn std::fmt::Display) {
        en: "browsingUnicorns {version} is available.",
        fr: "browsingUnicorns {version} est disponible.",
    }
    /// The banner's button that installs it.
    fn install() { en: "Install", fr: "Installer" }
    /// The banner's button that hides it until the next start.
    fn later() { en: "Later", fr: "Plus tard" }
    /// The banner's button that never offers this version again.
    fn skip_version() { en: "Skip this version", fr: "Ignorer cette version" }
    /// The banner's link to the release notes.
    fn whats_new() { en: "What's new", fr: "Nouveautés" }
    /// The banner, while the new version downloads.
    fn downloading(version: &dyn std::fmt::Display) {
        en: "Downloading browsingUnicorns {version}…",
        fr: "Téléchargement de browsingUnicorns {version}…",
    }
    /// The banner, the new version downloaded.
    fn ready(version: &dyn std::fmt::Display) {
        en: "browsingUnicorns {version} is ready.",
        fr: "browsingUnicorns {version} est prêt.",
    }
    /// The banner's button that restarts on the new version.
    fn restart_now() { en: "Restart now", fr: "Redémarrer maintenant" }
    /// The toolbar's button, a new version out.
    fn update_button() { en: "Update", fr: "Mettre à jour" }
    /// The toolbar's button, while it downloads.
    fn downloading_button() { en: "Updating…", fr: "Mise à jour…" }
    /// The toolbar's button, the new version downloaded.
    fn restart_button() { en: "Restart", fr: "Redémarrer" }
}
