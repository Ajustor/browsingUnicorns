//! The password manager: its bar after a sign-in (src/ui/passwords.rs) and
//! its part of the Settings view.

messages! {
    /// The bar's question for a new sign-in.
    fn save_offer(site: &str) {
        en: "Save the password for {site}?",
        fr: "Enregistrer le mot de passe pour {site}\u{a0}?",
    }
    /// The bar's question for a new sign-in with a user name.
    fn save_offer_for(user: &str, site: &str) {
        en: "Save the password of {user} for {site}?",
        fr: "Enregistrer le mot de passe de {user} pour {site}\u{a0}?",
    }
    /// The bar's question when a saved password changed.
    fn update_offer(user: &str, site: &str) {
        en: "Update the saved password of {user} for {site}?",
        fr: "Mettre à jour le mot de passe enregistré de {user} pour {site}\u{a0}?",
    }
    fn save() { en: "Save", fr: "Enregistrer" }
    fn update() { en: "Update", fr: "Mettre à jour" }
    fn not_now() { en: "Not now", fr: "Pas maintenant" }
    fn never() { en: "Never for this site", fr: "Jamais pour ce site" }
    /// Under the bar's question.
    fn synced_tip() {
        en: "Kept for this profile, encrypted, and synced with your devices (end-to-end encrypted) when sync is on.",
        fr: "Gardé pour ce profil, chiffré, et synchronisé avec vos appareils (chiffré de bout en bout) quand la synchronisation est active.",
    }

    /// Settings: the list's title.
    fn saved() { en: "Saved passwords", fr: "Mots de passe enregistrés" }
    /// Settings: the list is empty.
    fn none_saved() { en: "No password saved yet.", fr: "Aucun mot de passe enregistré pour l’instant." }
    /// Settings: a site whose passwords are never saved.
    fn never_saved() { en: "never saved", fr: "jamais enregistré" }
    /// Settings: a sign-in without user name.
    fn no_username() { en: "(no user name)", fr: "(sans identifiant)" }
    fn remove_tip() { en: "Delete (on your synced devices too)", fr: "Supprimer (sur vos appareils synchronisés aussi)" }
    fn copy_tip() { en: "Copy the password", fr: "Copier le mot de passe" }

    /// Settings: sharing the sites' cookies through sync.
    fn sync_cookies() {
        en: "Stay signed in to sites on my synced devices",
        fr: "Rester connecté aux sites sur mes appareils synchronisés",
    }
    fn sync_cookies_tip() {
        en: "Sync shares the cookies of the sites you visit with your other devices (end-to-end encrypted): signed in here, signed in there. Some sites (Google, banks…) may still ask you to sign in again.",
        fr: "La synchronisation partage les cookies des sites visités avec vos autres appareils (chiffrés de bout en bout)\u{a0}: connecté ici, connecté là-bas. Certains sites (Google, banques…) peuvent quand même redemander la connexion.",
    }
}
