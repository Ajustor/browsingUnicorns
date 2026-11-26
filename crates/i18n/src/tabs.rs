//! The tab strip and the tabs' menus (src/ui/tab_strip.rs).

messages! {
    /// Tooltip of the + after the tabs.
    fn new_tab() {
        en: "New tab (Ctrl+T)\nRight-click: reopen the last tab closed (Ctrl+Shift+T)",
        fr: "Nouvel onglet (Ctrl+T)\nClic droit\u{a0}: rouvrir le dernier onglet fermé (Ctrl+Shift+T)",
    }
    /// Tooltip of the cross of a tab.
    fn close_tab() { en: "Close tab (Ctrl+W)", fr: "Fermer l’onglet (Ctrl+W)" }
    /// Tooltip of the speaker of a tab playing sound.
    fn mute_tab() { en: "Mute this tab", fr: "Couper le son de cet onglet" }
    /// Tooltip of the speaker of a muted tab.
    fn unmute_tab() { en: "Unmute this tab", fr: "Rétablir le son de cet onglet" }
    /// The pill before the tabs of a private window.
    fn private_pill() { en: "Private", fr: "Privée" }
    /// Tooltip of that pill.
    fn private_window() {
        en: "Private window: no history, cookies deleted at closing",
        fr: "Fenêtre privée\u{a0}: pas d’historique, cookies effacés à la fermeture",
    }
    /// Tab menu: pin the tab.
    fn pin_tab() { en: "Pin tab", fr: "Épingler l’onglet" }
    /// Tab menu: unpin the tab.
    fn unpin_tab() { en: "Unpin tab", fr: "Désépingler l’onglet" }
    /// Tab menu: put the tab in a new group.
    fn add_to_new_group() { en: "Add tab to new group", fr: "Ajouter l’onglet à un nouveau groupe" }
    /// Tab menu: the submenu listing the groups.
    fn add_to_group() { en: "Add tab to group", fr: "Ajouter l’onglet au groupe" }
    /// Tab menu: take the tab out of its group.
    fn remove_from_group() { en: "Remove from group", fr: "Retirer du groupe" }
    /// Tab menu, and the menu of a group: close.
    fn close() { en: "Close", fr: "Fermer" }
    /// A group without a name, in the menus.
    fn unnamed_group(tabs: usize) -> String {
        en: { if tabs == 1 { "Unnamed group (1 tab)".into() } else { format!("Unnamed group ({tabs} tabs)") } },
        fr: { if tabs <= 1 { format!("Groupe sans nom ({tabs} onglet)") } else { format!("Groupe sans nom ({tabs} onglets)") } },
    }
    /// Tooltip of a group's label.
    fn group_tip() {
        en: "Click: collapse or expand\nRight-click: name, colour…",
        fr: "Clic\u{a0}: replier ou déplier\nClic droit\u{a0}: nom, couleur…",
    }
    /// Hint of the group's name field, in its menu.
    fn group_name() { en: "Name this group", fr: "Nommer ce groupe" }
    /// Group menu: take every tab out of the group.
    fn ungroup() { en: "Ungroup", fr: "Dissocier" }
    /// Group menu: close every tab of the group.
    fn close_group() { en: "Close group", fr: "Fermer le groupe" }
    /// Tooltip of the button that searches the tabs.
    fn search_tabs() { en: "Search tabs (Ctrl+Shift+A)", fr: "Rechercher dans les onglets (Ctrl+Shift+A)" }
    /// Hint of the tab search's field.
    fn search_tabs_hint() { en: "Search open tabs", fr: "Rechercher parmi les onglets ouverts" }
    /// The tab search, finding nothing.
    fn no_tab_found() { en: "No tab matches", fr: "Aucun onglet ne correspond" }
    /// Under the tab search's list: its keys.
    fn search_tabs_keys(arrows: &str) {
        en: "{arrows} choose · Enter go · Esc close",
        fr: "{arrows} choisir · Entrée aller · Échap fermer",
    }
    /// Tab menu: show the tabs in a column on the left.
    fn tabs_on_left() { en: "Show tabs on the left", fr: "Afficher les onglets à gauche" }
    /// Tab menu: show the tabs in the strip at the top.
    fn tabs_on_top() { en: "Show tabs at the top", fr: "Afficher les onglets en haut" }
    /// Tooltip of the button that narrows the tabs' column to their icons.
    fn collapse_column() { en: "Show icons only", fr: "N’afficher que les icônes" }
    /// Tooltip of the button that widens the tabs' column again.
    fn expand_column() { en: "Show tab titles", fr: "Afficher les titres des onglets" }
    /// Tab menu (another tab than the active one): split view with it.
    fn split_with_active() { en: "Show beside the active tab", fr: "Afficher à côté de l’onglet actif" }
    /// Tab menu (the active tab): split view with a new tab.
    fn split_with_new_tab() { en: "Split with a new tab", fr: "Scinder avec un nouvel onglet" }
    /// Tab menu (a tab of a split view): back to one tab at a time.
    fn unsplit() { en: "Close split view", fr: "Fermer l’écran scindé" }
    /// Tooltip of the bar between the two pages of a split view.
    fn split_divider() { en: "Drag to resize · double-click: half and half", fr: "Tirer pour redimensionner · double-clic\u{a0}: moitié-moitié" }
}
