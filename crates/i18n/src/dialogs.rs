//! The system's file and folder pickers' titles and filters (rfd), and confirmations.

messages! {
    /// Title of the picker of a packaged extension.
    fn install_extension() { en: "Install an extension", fr: "Installer une extension" }
    /// Its filter's name (.crx files).
    fn chrome_extension_filter() { en: "Chrome extension", fr: "Extension Chrome" }
    /// Title of the picker of an unpacked extension's folder.
    fn extension_folder() {
        en: "Extension folder (with manifest.json)",
        fr: "Dossier de l'extension (avec manifest.json)",
    }
    /// Title of the picker of the downloads' folder.
    fn downloads_folder() { en: "Where downloads go", fr: "Emplacement des téléchargements" }
    /// Title of the picker of bookmarks exported to HTML.
    fn bookmarks_file() { en: "Bookmarks exported from a browser", fr: "Favoris exportés d'un navigateur" }
    /// Its filter's name (.html files).
    fn bookmarks_filter() { en: "Bookmarks", fr: "Favoris" }
}
