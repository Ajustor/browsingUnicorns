//! What pages ask to use (src/ui/permissions.rs).

messages! {
    /// The bar: `host` asks to use `what` (one of the below).
    fn ask(host: &dyn std::fmt::Display, what: &dyn std::fmt::Display) {
        en: "{host} wants to use {what}.",
        fr: "{host} veut utiliser {what}.",
    }
    fn camera() { en: "your camera", fr: "votre caméra" }
    fn microphone() { en: "your microphone", fr: "votre micro" }
    fn camera_and_microphone() { en: "your camera and microphone", fr: "votre caméra et votre micro" }
    fn screen() { en: "your screen (to share it)", fr: "votre écran (pour le partager)" }
    fn location() { en: "your location", fr: "votre position" }
    fn notifications() { en: "notifications", fr: "les notifications" }
    /// The bar's buttons.
    fn allow() { en: "Allow", fr: "Autoriser" }
    fn deny() { en: "Deny", fr: "Refuser" }
    /// Tooltip of the bar: how long the answer holds.
    fn until_closed() {
        en: "The answer holds for this site until the browser is closed.",
        fr: "La réponse vaut pour ce site jusqu’à la fermeture du navigateur.",
    }
}
