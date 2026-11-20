//! Sync: its view and pairing (src/ui/sync_panel.rs, sync.rs).

messages! {
    /// The view while sync is off.
    fn intro() {
        en: "Sync bookmarks, history, open tabs and settings with your other devices. Everything is encrypted on the device: the relay cannot read it.",
        fr: "Synchronisez favoris, historique, onglets ouverts et réglages avec vos autres appareils. Tout est chiffré sur l'appareil\u{a0}: le relais ne peut pas le lire.",
    }
    fn turn_on() { en: "Turn on sync", fr: "Activer la synchronisation" }
    fn join_prompt() {
        en: "Already on another device? Enter its sync code:",
        fr: "Déjà sur un autre appareil\u{a0}? Saisissez son code de synchronisation\u{a0}:",
    }
    /// Joins the group of the code typed.
    fn join() { en: "Join", fr: "Rejoindre" }
    /// The code typed is not a sync code.
    fn invalid_code() {
        en: "This code is not valid: check it on the other device.",
        fr: "Ce code n'est pas valide\u{a0}: vérifiez-le sur l'autre appareil.",
    }
    fn this_device(name: &str) { en: "This device: {name}", fr: "Cet appareil\u{a0}: {name}" }
    fn not_synced() { en: "Not synced yet.", fr: "Pas encore synchronisé." }
    fn syncing() { en: "Syncing…", fr: "Synchronisation…" }
    /// The last round; `ago` is `history::ago`.
    fn synced(ago: &str) { en: "Synced {ago}.", fr: "Synchronisé {ago}." }
    fn sync_now() { en: "Sync now", fr: "Synchroniser maintenant" }
    fn turn_off() { en: "Turn off", fr: "Désactiver" }
    fn turn_off_tip() { en: "Leave the sync group on this device", fr: "Quitter le groupe de synchronisation sur cet appareil" }
    /// Section with the code.
    fn add_device() { en: "Add a device", fr: "Ajouter un appareil" }
    fn add_device_how() {
        en: "On the other device, choose \"Join\" and scan this QR code or type the code. Keep it secret: it gives access to your data.",
        fr: "Sur l'autre appareil, choisissez «\u{a0}Rejoindre\u{a0}» et scannez ce code QR ou saisissez le code. Gardez-le secret\u{a0}: il donne accès à vos données.",
    }
    fn show_code() { en: "Show the code", fr: "Afficher le code" }
    fn copy() { en: "Copy", fr: "Copier" }
    fn hide() { en: "Hide", fr: "Masquer" }
    /// Section title.
    fn other_devices() { en: "Other devices", fr: "Autres appareils" }
    fn no_other_devices() { en: "None yet.", fr: "Aucun pour l'instant." }
    /// Another device: its name, its open tabs, when it last synced
    /// (`ago` is `history::ago`).
    fn device_summary(name: &str, tabs: usize, ago: &str) -> String {
        en: { if tabs == 1 { format!("{name} · 1 tab · {ago}") } else { format!("{name} · {tabs} tabs · {ago}") } },
        fr: { if tabs <= 1 { format!("{name} · {tabs} onglet · {ago}") } else { format!("{name} · {tabs} onglets · {ago}") } },
    }
}

#[cfg(test)]
mod tests {
    use super::in_lang::device_summary;
    use crate::Lang;

    #[test]
    fn tabs_are_counted() {
        assert_eq!(device_summary(Lang::En, "PC", 1, "now"), "PC · 1 tab · now");
        assert_eq!(
            device_summary(Lang::En, "PC", 0, "now"),
            "PC · 0 tabs · now"
        );
        assert_eq!(
            device_summary(Lang::Fr, "PC", 0, "à l'instant"),
            "PC · 0 onglet · à l'instant"
        );
        assert_eq!(
            device_summary(Lang::Fr, "PC", 3, "il y a 5 min"),
            "PC · 3 onglets · il y a 5 min"
        );
    }
}
