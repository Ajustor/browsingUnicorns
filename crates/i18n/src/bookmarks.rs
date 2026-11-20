//! Bookmarks: the bar, the Bookmarks view, the folders (src/ui/bookmarks_bar.rs, library.rs).

messages! {
    /// The Bookmarks view when empty; `star` is the star's icon.
    fn no_bookmarks(star: &str) {
        en: "No bookmarks. {star} in the address bar or Ctrl+D saves the page; Settings imports other browsers'.",
        fr: "Aucun favori. {star} dans la barre d’adresse ou Ctrl+D enregistre la page\u{a0}; les Réglages importent ceux des autres navigateurs.",
    }
    /// A bookmark's remove button.
    fn remove_tip() { en: "Remove bookmark", fr: "Supprimer le favori" }
    /// A bookmark's folder button.
    fn move_tip() { en: "Move to a folder", fr: "Déplacer dans un dossier" }
    /// The choice of no folder, among the folders.
    fn no_folder() { en: "No folder", fr: "Aucun dossier" }
    /// The field naming a new folder.
    fn new_folder_hint() { en: "New folder (Work/Projects)", fr: "Nouveau dossier (Travail/Projets)" }
    /// Moves the bookmark into the folder typed.
    fn move_ok() { en: "OK", fr: "OK" }
    /// A folder of the bookmarks bar.
    fn show_folder_tip() { en: "Show this folder's bookmarks", fr: "Afficher les favoris de ce dossier" }
}
