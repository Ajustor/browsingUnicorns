//! The toolbar: navigation buttons, zoom, the bookmark star.

messages! {
    /// Tooltip of the star of a page not bookmarked.
    fn bookmark_this_page() { en: "Bookmark this page (Ctrl+D)", fr: "Ajouter aux favoris (Ctrl+D)" }
    /// Tooltip of the star of a bookmarked page.
    fn remove_bookmark() { en: "Remove bookmark (Ctrl+D)", fr: "Retirer des favoris (Ctrl+D)" }
    /// Tooltip of the back arrow.
    fn back() { en: "Back (Alt+←)", fr: "Page précédente (Alt+←)" }
    /// Tooltip of the forward arrow.
    fn forward() { en: "Forward (Alt+→)", fr: "Page suivante (Alt+→)" }
    /// Tooltip of the reload button.
    fn reload() {
        en: "Reload (F5)\nShift+click or Ctrl+Shift+R: reload without the cache",
        fr: "Actualiser (F5)\nShift+clic ou Ctrl+Shift+R\u{a0}: actualiser sans le cache",
    }
    /// Tooltip of the home button.
    fn home() { en: "Home (Alt+Home)", fr: "Page d'accueil (Alt+Home)" }
    /// Tooltip of the padlock of an HTTPS page.
    fn connection_secure() { en: "Connection is secure: certificate", fr: "Connexion sécurisée\u{a0}: certificat" }
    /// Tooltip of the globe of a page not encrypted.
    fn connection_not_secure() { en: "Connection is not secure", fr: "Connexion non sécurisée" }
    /// Tooltip of the profile's avatar.
    fn profile_of(name: &str) { en: "Profile: {name} (profiles)", fr: "Profil\u{a0}: {name} (profils)" }
    /// Tooltip of the mask.
    fn new_private_window() { en: "New private window (Ctrl+Shift+N)", fr: "Nouvelle fenêtre privée (Ctrl+Shift+N)" }
    /// Tooltip of the books, which open the side panel.
    fn history_and_bookmarks() { en: "History and bookmarks (Ctrl+H)", fr: "Historique et favoris (Ctrl+H)" }
    /// Tooltip of the developer tools' button.
    fn developer_tools() { en: "Developer tools (F12)", fr: "Outils de développement (F12)" }
    /// The zoom button's label.
    fn zoom_percent(percent: u32) { en: "{percent}%", fr: "{percent}\u{a0}%" }
    /// Tooltip of the zoom button.
    fn zoom_of_site() {
        en: "Zoom of this site (Ctrl+wheel, Ctrl +/-): click for 100%",
        fr: "Zoom de ce site (Ctrl+molette, Ctrl +/-)\u{a0}: cliquer pour revenir à 100\u{a0}%",
    }
}

#[cfg(test)]
mod tests {
    use super::in_lang::*;
    use crate::Lang;

    #[test]
    fn the_zoom_is_a_percentage() {
        assert_eq!(zoom_percent(Lang::En, 125), "125%");
        assert_eq!(zoom_percent(Lang::Fr, 125), "125\u{a0}%");
    }

    #[test]
    fn the_profile_is_named() {
        assert_eq!(profile_of(Lang::En, "Work"), "Profile: Work (profiles)");
        assert_eq!(profile_of(Lang::Fr, "Work"), "Profil\u{a0}: Work (profils)");
    }
}
