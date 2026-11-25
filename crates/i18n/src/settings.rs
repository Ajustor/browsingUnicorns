//! The Settings view of the side panel.

messages! {
    /// Section title.
    fn language() { en: "Language", fr: "Langue" }
    /// Under the Language title.
    fn language_hint() { en: "Of menus and messages.", fr: "Langue des menus et des messages." }
    /// The language choice that follows the system's.
    fn language_auto() { en: "Automatic", fr: "Automatique" }

    /// Section title.
    fn home_page() { en: "Home page", fr: "Page d’accueil" }
    fn home_page_hint() { en: "Opened by new tabs and the Home button.", fr: "Ouverte par les nouveaux onglets et le bouton Accueil." }
    /// The home page typed is not one.
    fn not_an_address() { en: "Not a web address.", fr: "Ce n’est pas une adresse web." }

    /// Section title.
    fn search_engine() { en: "Search engine", fr: "Moteur de recherche" }
    fn search_engine_hint() { en: "What the address bar searches with.", fr: "Utilisé par la barre d’adresse pour les recherches." }

    /// Section title.
    fn tabs() { en: "Tabs", fr: "Onglets" }
    fn tabs_hint() { en: "When they no longer fit in the tab strip.", fr: "Quand ils ne tiennent plus dans la barre d’onglets." }
    /// The tabs keep their width; the wheel scrolls them.
    fn tabs_scroll() { en: "Scroll them (mouse wheel)", fr: "Les faire défiler (molette)" }
    /// The tabs get narrower to fit.
    fn tabs_shrink() { en: "Shrink them to fit", fr: "Les réduire pour qu’ils tiennent" }

    /// Section title.
    fn downloads() { en: "Downloads", fr: "Téléchargements" }
    /// Before the downloads' folder.
    fn save_to() { en: "Save to", fr: "Enregistrer dans" }
    /// The downloads' folder when the system's is unknown.
    fn system_downloads_folder() { en: "the Downloads folder", fr: "le dossier Téléchargements" }
    /// Picks the downloads' folder.
    fn change_folder() { en: "Change…", fr: "Changer…" }
    /// The folder picker's title.
    fn folder_dialog_title() { en: "Where downloads go", fr: "Où enregistrer les téléchargements" }
    /// Back to the system's folder.
    fn downloads_folder() { en: "Downloads folder", fr: "Dossier Téléchargements" }
    fn downloads_folder_tip() { en: "Back to the system's Downloads folder", fr: "Revenir au dossier Téléchargements du système" }
    fn ask_where_to_save() { en: "Ask where to save each file", fr: "Demander où enregistrer chaque fichier" }

    /// Section title.
    fn passwords() { en: "Passwords", fr: "Mots de passe" }
    fn offer_to_save_passwords() { en: "Offer to save the passwords I type", fr: "Proposer d’enregistrer les mots de passe que je saisis" }
    fn offer_to_save_passwords_tip() {
        en: "The browser keeps them for this profile, encrypted, fills them in next time and syncs them with your devices. A password manager extension (Bitwarden…) works either way.",
        fr: "Le navigateur les garde pour ce profil, chiffrés, les remplit la fois suivante et les synchronise avec vos appareils. Une extension de gestion de mots de passe (Bitwarden…) fonctionne dans tous les cas.",
    }

    /// Section title.
    fn zoom_by_site() { en: "Zoom by site", fr: "Zoom par site" }
    /// A site's zoom in the Zoom by site list.
    fn zoom_of(host: &str, percent: u32) { en: "{host}: {percent}%", fr: "{host}\u{a0}: {percent}\u{a0}%" }
    /// Forgets a site's zoom.
    fn zoom_reset_tip() { en: "Back to 100%", fr: "Revenir à 100\u{a0}%" }

    /// Section title.
    fn import_bookmarks() { en: "Import from another browser", fr: "Importer depuis un autre navigateur" }
    fn import_bookmarks_hint() {
        en: "Bookmarks (into a folder named after the browser) and history.",
        fr: "Les favoris (dans un dossier au nom du navigateur) et l’historique.",
    }
    /// Imports passwords exported as CSV.
    fn import_passwords() { en: "Passwords (CSV file)…", fr: "Mots de passe (fichier CSV)…" }
    fn import_passwords_tip() {
        en: "The file browsers and password managers export. Chrome / Edge: Settings > Passwords > Export passwords. Firefox: Passwords > ⋯ > Export passwords. Delete the file afterwards: it holds them in clear.",
        fr: "Le fichier qu’exportent les navigateurs et gestionnaires de mots de passe. Chrome / Edge\u{a0}: Paramètres > Mots de passe > Exporter les mots de passe. Firefox\u{a0}: Mots de passe > ⋯ > Exporter les mots de passe. Supprimez ensuite le fichier\u{a0}: ils y sont en clair.",
    }
    /// The passwords file picker's title and filter.
    fn import_passwords_title() { en: "Passwords exported from a browser", fr: "Mots de passe exportés d’un navigateur" }
    fn import_passwords_filter() { en: "Passwords (CSV)", fr: "Mots de passe (CSV)" }
    /// Imports from a browser found here.
    fn import_from(browser: &str) { en: "From {browser}", fr: "Depuis {browser}" }
    fn import_html() { en: "From an HTML file…", fr: "Depuis un fichier HTML…" }
    fn import_html_tip() {
        en: "The file browsers export (Safari: File > Export Bookmarks)",
        fr: "Le fichier qu’exportent les navigateurs (Safari\u{a0}: Fichier > Exporter les signets)",
    }
    /// The file picker's title.
    fn import_dialog_title() { en: "Bookmarks exported from a browser", fr: "Favoris exportés d’un navigateur" }
    /// The file picker's filter.
    fn import_filter() { en: "Bookmarks", fr: "Favoris" }

    /// Section title.
    fn clear_browsing_data() { en: "Clear browsing data", fr: "Effacer les données de navigation" }
    fn clear_history() { en: "History", fr: "Historique" }
    fn clear_cookies() { en: "Cookies and site data (signs you out of sites)", fr: "Cookies et données des sites (vous déconnecte des sites)" }
    fn clear_cache() { en: "Cached images and files", fr: "Images et fichiers en cache" }
    /// The button.
    fn clear_now() { en: "Clear now", fr: "Effacer maintenant" }

    /// Section title: the DRM data of streaming sites.
    fn protected_videos() { en: "Protected videos", fr: "Vidéos protégées" }
    /// Under the Protected videos title.
    fn protected_videos_hint() {
        en: "If Netflix or another streaming site loads forever, reset its DRM data.",
        fr: "Si Netflix ou un autre site de streaming charge sans fin, réinitialisez ses données DRM.",
    }
    /// The button.
    fn reset_drm() { en: "Reset DRM data and restart", fr: "Réinitialiser les DRM et redémarrer" }
    fn reset_drm_tip() {
        en: "Restarts the app: your tabs reopen. Sites stay signed in.",
        fr: "Redémarre l’appli\u{a0}: vos onglets se rouvrent. Vous restez connecté aux sites.",
    }
}
