//! Errors of the `application` crate.

messages! {
    /// The web engine failed (opening a view, loading a page…).
    fn web_engine_failed(e: &dyn std::fmt::Display) { en: "web engine: {e}", fr: "moteur web\u{a0}: {e}" }
    /// Reading or saving the user's data failed.
    fn storage_failed(e: &dyn std::fmt::Display) { en: "storage: {e}", fr: "stockage\u{a0}: {e}" }
    /// Removing a profile that a window runs.
    fn profile_in_use() { en: "this profile is open in a window: close it first", fr: "ce profil est ouvert dans une fenêtre\u{a0}: fermez-la d’abord" }
    /// Opening a private tab where the web engine cannot keep it apart.
    fn private_tabs_unavailable() { en: "private tabs are not available here", fr: "les onglets privés ne sont pas disponibles ici" }
}
