//! User scripts: their view (src/ui/user_scripts.rs).

messages! {
    /// Their part's title in the Extensions view.
    fn heading() { en: "User scripts", fr: "Scripts utilisateur" }
    /// Over the field.
    fn input_label() {
        en: "Address of a .user.js script (Greasy Fork, OpenUserJS, GitHub…):",
        fr: "Adresse d'un script .user.js (Greasy Fork, OpenUserJS, GitHub…)\u{a0}:",
    }
    fn install() { en: "Install", fr: "Installer" }
    fn greasy_fork_tip() { en: "Find user scripts on Greasy Fork", fr: "Trouver des scripts utilisateur sur Greasy Fork" }
    /// An install went well.
    fn installed(name: &str, version: &str) {
        en: "{name} {version} installed: it runs from the next page load.",
        fr: "{name} {version} installé\u{a0}: il s'exécute dès le prochain chargement de page.",
    }
    fn none() { en: "No user scripts.", fr: "Aucun script utilisateur." }
    fn remove() { en: "Remove", fr: "Supprimer" }
}
