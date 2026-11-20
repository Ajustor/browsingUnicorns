//! Offering to be the default browser (src/ui/default_browser.rs).

messages! {
    /// The bar's question.
    fn offer() {
        en: "Make browsingUnicorns your default browser? Links you open in other apps will open here.",
        fr: "Faire de browsingUnicorns votre navigateur par défaut\u{a0}? Les liens ouverts dans d’autres applications s’ouvriront ici.",
    }
    /// The bar's yes.
    fn set_as_default() { en: "Set as default", fr: "Définir par défaut" }
    /// The bar's not now: it comes back at the next start.
    fn not_now() { en: "Not now", fr: "Pas maintenant" }
    /// The bar's never.
    fn never_ask() { en: "Don't ask again", fr: "Ne plus demander" }
}
