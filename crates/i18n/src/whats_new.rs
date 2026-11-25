//! The "What's new" view of the side panel (src/ui/whats_new.rs).

use std::fmt::Display;

messages! {
    /// Over the notes, for those who read English.
    fn in_french() {
        en: "The release notes are written in French.",
        fr: "Les notes de version sont écrites en français.",
    }
    /// A card's title.
    fn version(v: &dyn Display) { en: "Version {v}", fr: "Version {v}" }
    /// The chip beside the version running.
    fn installed() { en: "Installed", fr: "Installée" }
    /// The button under the last card.
    fn older() { en: "Older versions", fr: "Versions précédentes" }
    /// The About view's button.
    fn see_whats_new() { en: "What’s new", fr: "Voir les nouveautés" }
    /// Over an update's notes, in the About view.
    fn in_this_update(v: &dyn Display) { en: "New in {v}", fr: "Nouveau dans la {v}" }
}
