//! Profiles: their view and menu (src/ui/profiles.rs).

messages! {
    /// The view's title.
    fn heading() { en: "Profiles", fr: "Profils" }
    fn intro() {
        en: "Each profile has its own history, bookmarks, tabs, extensions, cookies and sync, and opens in its own window.",
        fr: "Chaque profil a son propre historique, ses favoris, ses onglets, ses extensions, ses cookies et sa synchronisation, et s’ouvre dans sa propre fenêtre.",
    }
    /// Under the current profile's name.
    fn this_window() { en: "This window", fr: "Cette fenêtre" }
    fn rename() { en: "Rename", fr: "Renommer" }
    /// A color of the avatar.
    fn color_tip() { en: "Color", fr: "Couleur" }
    fn no_other() { en: "No other profile.", fr: "Aucun autre profil." }
    /// Another profile's avatar.
    fn open_in_new_window_tip() { en: "Open in a new window", fr: "Ouvrir dans une nouvelle fenêtre" }
    /// Another profile already has its window.
    fn is_open() { en: "open", fr: "ouvert" }
    fn open() { en: "Open", fr: "Ouvrir" }
    fn remove() { en: "Remove", fr: "Supprimer" }
    fn confirm_remove(name: &str) {
        en: "Remove {name} and delete its history, bookmarks, tabs, extensions and cookies?",
        fr: "Supprimer {name} et effacer son historique, ses favoris, ses onglets, ses extensions et ses cookies\u{a0}?",
    }
    fn cancel() { en: "Cancel", fr: "Annuler" }
    /// Over the new profile's field.
    fn new_profile() { en: "New profile:", fr: "Nouveau profil\u{a0}:" }
    fn name_hint() { en: "Name (Work, Personal…)", fr: "Nom (Travail, Perso…)" }
    fn add() { en: "Add", fr: "Ajouter" }
    fn opens_in_new_window() { en: "A new profile opens in a new window.", fr: "Un nouveau profil s’ouvre dans une nouvelle fenêtre." }
}
