//! The Settings view of the side panel.

messages! {
    /// Section title.
    fn language() { en: "Language", fr: "Langue" }
    /// Under the Language title.
    fn language_hint() { en: "Of menus and messages.", fr: "Des menus et des messages." }
    /// The language choice that follows the system's.
    fn language_auto() { en: "Automatic", fr: "Automatique" }
    /// A site's zoom in the Zoom by site list.
    fn zoom_of(host: &str, percent: u32) { en: "{host}: {percent}%", fr: "{host}\u{a0}: {percent}\u{a0}%" }
}
