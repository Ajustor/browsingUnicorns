//! The ad blocker's shield and its panel.

messages! {
    /// What the blocker did on the page.
    fn items_blocked(n: u32) -> String {
        en: { if n == 1 { "1 item blocked on this page.".into() } else { format!("{n} items blocked on this page.") } },
        fr: { if n <= 1 { format!("{n} élément bloqué sur cette page.") } else { format!("{n} éléments bloqués sur cette page.") } },
    }
    /// Tooltip of the toolbar's shield, blocking.
    fn on_for_site() { en: "Ad blocker: on for this site", fr: "Bloqueur de pubs\u{a0}: activé sur ce site" }
    /// Tooltip of the toolbar's shield, not blocking.
    fn off_for_site() { en: "Ad blocker: off for this site", fr: "Bloqueur de pubs\u{a0}: désactivé sur ce site" }
    /// The panel's status, the blocker off everywhere.
    fn blocker_off() { en: "The ad blocker is off.", fr: "Le bloqueur de pubs est désactivé." }
    /// The panel's status, the blocker off for this site.
    fn not_blocking_here() { en: "Not blocking on this site.", fr: "Aucun blocage sur ce site." }
    /// The panel's status where the web view doesn't tell what it blocked.
    fn active() { en: "Active: blocking ads and trackers.", fr: "Actif\u{a0}: bloque les pubs et les traqueurs." }
    /// The switch for this site.
    fn block_on_this_site() { en: "Block on this site", fr: "Bloquer sur ce site" }
    /// The switch for every site.
    fn ad_blocker() { en: "Ad blocker", fr: "Bloqueur de pubs" }
    /// The lists' line, before their first download.
    fn lists_not_downloaded() {
        en: "Lists: EasyList, EasyPrivacy, uBlock filters, Twitch (vaft) · not downloaded yet",
        fr: "Listes\u{a0}: EasyList, EasyPrivacy, filtres uBlock, Twitch (vaft) · pas encore téléchargées",
    }
    /// The lists' line, downloaded `secs` seconds ago.
    fn lists_updated(secs: u64) -> String {
        en: {
            let age = match secs {
                0..60 => "just now".to_owned(),
                60..3_600 => format!("{} min ago", secs / 60),
                3_600..86_400 => format!("{} h ago", secs / 3_600),
                _ => format!("{} d ago", secs / 86_400),
            };
            format!("Lists: EasyList, EasyPrivacy, uBlock filters, Twitch (vaft) · updated {age}")
        },
        fr: {
            let age = match secs {
                0..60 => "à l’instant".to_owned(),
                60..3_600 => format!("il y a {}\u{a0}min", secs / 60),
                3_600..86_400 => format!("il y a {}\u{a0}h", secs / 3_600),
                _ => format!("il y a {}\u{a0}j", secs / 86_400),
            };
            format!("Listes\u{a0}: EasyList, EasyPrivacy, filtres uBlock, Twitch (vaft) · mises à jour {age}")
        },
    }
}

#[cfg(test)]
mod tests {
    use super::in_lang::*;
    use crate::Lang;

    #[test]
    fn the_lists_tell_their_age() {
        assert!(lists_updated(Lang::En, 10).ends_with("updated just now"));
        assert!(lists_updated(Lang::En, 300).ends_with("updated 5 min ago"));
        assert!(lists_updated(Lang::En, 2 * 86_400).ends_with("updated 2 d ago"));
        assert!(lists_updated(Lang::Fr, 10).ends_with("mises à jour à l’instant"));
        assert!(lists_updated(Lang::Fr, 3 * 3_600).ends_with("mises à jour il y a 3\u{a0}h"));
        assert!(lists_updated(Lang::Fr, 2 * 86_400).ends_with("il y a 2\u{a0}j"));
    }
}
