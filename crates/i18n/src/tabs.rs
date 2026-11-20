//! The tab strip and the tabs' menus (src/ui/tab_strip.rs).

messages! {
    /// The title of a tab with neither title nor address.
    fn untitled() { en: "New tab", fr: "Nouvel onglet" }
    /// Tooltip of the + after the tabs.
    fn new_tab() {
        en: "New tab (Ctrl+T)\nRight-click: reopen the last tab closed (Ctrl+Shift+T)",
        fr: "Nouvel onglet (Ctrl+T)\nClic droit\u{a0}: rouvrir le dernier onglet fermé (Ctrl+Shift+T)",
    }
    /// Tooltip of the cross of a tab.
    fn close_tab() { en: "Close tab (Ctrl+W)", fr: "Fermer l'onglet (Ctrl+W)" }
    /// Tooltip of the speaker of a tab playing sound.
    fn mute_tab() { en: "Mute this tab", fr: "Couper le son de cet onglet" }
    /// Tooltip of the speaker of a muted tab.
    fn unmute_tab() { en: "Unmute this tab", fr: "Rétablir le son de cet onglet" }
    /// The pill before the tabs of a private window.
    fn private_pill() { en: "Private", fr: "Privée" }
    /// Tooltip of that pill.
    fn private_window() {
        en: "Private window: no history, cookies deleted at closing",
        fr: "Fenêtre privée\u{a0}: pas d'historique, cookies effacés à la fermeture",
    }
}
