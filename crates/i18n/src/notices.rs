//! The notices bar at the bottom of the window (src/ui/app.rs).

messages! {
    /// The system refused to make the app the default browser.
    fn default_browser_failed(e: &dyn std::fmt::Display) {
        en: "Could not make browsingUnicorns the default browser: {e}",
        fr: "Impossible de faire de browsingUnicorns le navigateur par défaut\u{a0}: {e}",
    }
    /// A private window did not open.
    fn private_window_failed(e: &dyn std::fmt::Display) {
        en: "Could not open a private window: {e}",
        fr: "Impossible d’ouvrir une fenêtre privée\u{a0}: {e}",
    }
    /// A tab dragged out of the window did not get its own.
    fn new_window_failed(e: &dyn std::fmt::Display) {
        en: "Could not open a new window: {e}",
        fr: "Impossible d’ouvrir une nouvelle fenêtre\u{a0}: {e}",
    }
    /// A file to import could not be read.
    fn read_failed(file: &dyn std::fmt::Display, e: &dyn std::fmt::Display) {
        en: "Could not read {file}: {e}",
        fr: "Impossible de lire {file}\u{a0}: {e}",
    }
    /// Where bookmarks imported from an HTML file come from, in the two
    /// messages below.
    fn html_file() { en: "HTML file", fr: "un fichier HTML" }
    /// The folder the imported bookmarks go in; `from` a browser's name or
    /// [`html_file`].
    fn imported_folder(from: &str) { en: "Imported from {from}", fr: "Importés depuis {from}" }
    /// How many bookmarks came, and from where.
    fn bookmarks_imported(n: usize, from: &str) -> String {
        en: {
            if n == 1 { format!("1 bookmark imported from {from}.") } else { format!("{n} bookmarks imported from {from}.") }
        },
        fr: {
            if n <= 1 { format!("{n} favori importé depuis {from}.") } else { format!("{n} favoris importés depuis {from}.") }
        },
    }
    /// How many bookmarks and pages of history came from a browser.
    fn browser_imported(bookmarks: usize, pages: usize, from: &str) -> String {
        en: {
            let b = if bookmarks == 1 { "1 bookmark".to_owned() } else { format!("{bookmarks} bookmarks") };
            let p = if pages == 1 { "1 page of history".to_owned() } else { format!("{pages} pages of history") };
            format!("Imported from {from}: {b} and {p}.")
        },
        fr: {
            let b = if bookmarks <= 1 { format!("{bookmarks} favori") } else { format!("{bookmarks} favoris") };
            let p = if pages <= 1 { format!("{pages} page d’historique") } else { format!("{pages} pages d’historique") };
            format!("Importé depuis {from}\u{a0}: {b} et {p}.")
        },
    }
    /// How many passwords came from a file.
    fn passwords_imported(n: usize) -> String {
        en: {
            if n == 1 { "1 password imported.".to_owned() } else { format!("{n} passwords imported.") }
        },
        fr: {
            if n <= 1 { format!("{n} mot de passe importé.") } else { format!("{n} mots de passe importés.") }
        },
    }
    /// What Clear browsing data removed: a list of the three below.
    fn cleared(what: &str) { en: "Cleared: {what}.", fr: "Données effacées\u{a0}: {what}." }
    /// In [`cleared`].
    fn cleared_history() { en: "history", fr: "historique" }
    /// In [`cleared`].
    fn cleared_cookies() { en: "cookies and site data", fr: "cookies et données des sites" }
    /// In [`cleared`].
    fn cleared_cache() { en: "cache", fr: "cache" }
    /// The DRM reset could not be asked for.
    fn drm_reset_failed(e: &str) { en: "Could not reset the DRM data: {e}", fr: "Impossible de réinitialiser les DRM\u{a0}: {e}" }
}

#[cfg(test)]
mod tests {
    use super::in_lang::*;
    use crate::Lang;

    #[test]
    fn imported_bookmarks_are_counted() {
        assert_eq!(
            bookmarks_imported(Lang::En, 1, "Chrome"),
            "1 bookmark imported from Chrome."
        );
        assert_eq!(
            bookmarks_imported(Lang::En, 3, "Chrome"),
            "3 bookmarks imported from Chrome."
        );
        assert_eq!(
            bookmarks_imported(Lang::Fr, 0, "Chrome"),
            "0 favori importé depuis Chrome."
        );
        assert_eq!(
            bookmarks_imported(Lang::Fr, 3, html_file(Lang::Fr)),
            "3 favoris importés depuis un fichier HTML."
        );
    }

    #[test]
    fn errors_are_wrapped() {
        let e = "access denied";
        assert_eq!(
            read_failed(Lang::En, &"a.html", &e),
            "Could not read a.html: access denied"
        );
        assert_eq!(
            read_failed(Lang::Fr, &"a.html", &e),
            "Impossible de lire a.html\u{a0}: access denied"
        );
    }
}
