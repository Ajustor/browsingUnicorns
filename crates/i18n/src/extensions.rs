//! Extensions: their view, toolbar buttons and popups (src/ui/extensions.rs, extension_bar.rs).

messages! {
    /// The view's title.
    fn heading() { en: "Extensions", fr: "Extensions" }
    fn unsupported() {
        en: "Extensions need the Windows web view (WebView2): the web view of this system cannot run them.",
        fr: "Les extensions demandent la vue web de Windows (WebView2)\u{a0}: celle de ce système ne peut pas les faire fonctionner.",
    }
    fn partly_supported() {
        en: "On this system the browser runs the extensions' blocking rules and page scripts; their background pages and popups do not run.",
        fr: "Sur ce système, le navigateur applique les règles de blocage et les scripts de page des extensions\u{a0}; leurs pages d’arrière-plan et leurs pop-ups ne fonctionnent pas.",
    }
    /// Over the field.
    fn input_label() {
        en: "Chrome Web Store or Edge Add-ons link, extension id, .crx file or unpacked folder:",
        fr: "Lien Chrome Web Store ou Edge Add-ons, identifiant d’extension, fichier .crx ou dossier décompressé\u{a0}:",
    }
    /// The input names nothing installable.
    fn invalid_input() {
        en: "Paste a Chrome Web Store or Edge Add-ons link, an extension id, or the path of a .crx file or of an unpacked extension's folder.",
        fr: "Collez un lien Chrome Web Store ou Edge Add-ons, un identifiant d’extension, ou le chemin d’un fichier .crx ou du dossier d’une extension décompressée.",
    }
    fn install() { en: "Install", fr: "Installer" }
    fn web_store_tip() { en: "Find extensions in the Chrome Web Store", fr: "Trouver des extensions sur le Chrome Web Store" }
    fn choose_crx() { en: "Choose .crx…", fr: "Choisir un .crx…" }
    fn choose_crx_tip() { en: "Install a packaged extension from this computer", fr: "Installer une extension empaquetée depuis cet ordinateur" }
    /// The file picker's title.
    fn crx_dialog_title() { en: "Install an extension", fr: "Installer une extension" }
    /// The file picker's filter.
    fn crx_filter() { en: "Chrome extension", fr: "Extension Chrome" }
    fn load_unpacked() { en: "Load unpacked…", fr: "Charger un dossier…" }
    fn load_unpacked_tip() {
        en: "Install an extension from its folder (the one with manifest.json); load it again to take its changes",
        fr: "Installer une extension depuis son dossier (celui avec manifest.json)\u{a0}; la recharger pour prendre en compte ses modifications",
    }
    /// The folder picker's title.
    fn folder_dialog_title() { en: "Extension folder (with manifest.json)", fr: "Dossier de l’extension (avec manifest.json)" }
    /// While an install runs.
    fn installing(what: &str) { en: "Installing {what}…", fr: "Installation de {what}…" }
    /// An install went well.
    fn installed(name: &str, version: &str) { en: "{name} {version} installed.", fr: "{name} {version} installée." }
    fn none() { en: "No extensions.", fr: "Aucune extension." }
    /// Under the list, when the browser serves the extensions' pages.
    fn note_with_popups() {
        en: "Pages already open see a change after a reload. Extensions with a popup have a button next to the bookmark star; Options opens their settings in a tab.",
        fr: "Les pages déjà ouvertes voient un changement après un rechargement. Les extensions avec un pop-up ont un bouton à côté de l’étoile des favoris\u{a0}; Options ouvre leurs réglages dans un onglet.",
    }
    /// Under the list, when the web view serves them.
    fn note_in_tabs() {
        en: "Pages already open see a change after a reload. Toolbar buttons do not exist here: Popup and Options open the extension's pages in a tab.",
        fr: "Les pages déjà ouvertes voient un changement après un rechargement. Les boutons de barre d’outils n’existent pas ici\u{a0}: Pop-up et Options ouvrent les pages de l’extension dans un onglet.",
    }
    /// An extension's checkbox, when enabled.
    fn disable_tip() { en: "Disable", fr: "Désactiver" }
    /// An extension's checkbox, when disabled.
    fn enable_tip() { en: "Enable", fr: "Activer" }
    /// Opens the extension's popup.
    fn popup() { en: "Popup", fr: "Pop-up" }
    /// Opens the extension's options page.
    fn options() { en: "Options", fr: "Options" }
    fn inspect() { en: "Inspect", fr: "Inspecter" }
    fn inspect_tip() { en: "Developer tools of its background page", fr: "Outils de développement de sa page d’arrière-plan" }
    fn remove() { en: "Remove", fr: "Supprimer" }
}
