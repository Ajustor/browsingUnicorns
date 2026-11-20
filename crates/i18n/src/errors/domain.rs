//! Errors of the `domain` crate, and the few texts it makes for the user.

messages! {
    /// The title of a tab without a title nor a host.
    fn untitled_tab() { en: "New tab", fr: "Nouvel onglet" }
    /// An address that does not parse.
    fn invalid_address(raw: &str) { en: "invalid address: {raw}", fr: "adresse invalide\u{a0}: {raw}" }
    /// An address whose scheme the browser does not load.
    fn unsupported_scheme(scheme: &str) { en: "unsupported scheme: {scheme}", fr: "protocole non pris en charge\u{a0}: {scheme}" }
    /// Adding or renaming a profile to a blank name.
    fn profile_needs_a_name() { en: "a profile needs a name", fr: "un profil doit avoir un nom" }
    /// Adding or renaming a profile to another one's name.
    fn profile_name_taken() { en: "another profile has this name", fr: "un autre profil porte ce nom" }
    /// A profile that is not (or no longer) there.
    fn no_such_profile() { en: "no such profile", fr: "profil introuvable" }
    /// Removing the default profile (or deleting its data).
    fn default_profile_kept() { en: "the default profile cannot be removed", fr: "le profil par défaut ne peut pas être supprimé" }
}
