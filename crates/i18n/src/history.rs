//! The History view of the side panel (src/ui/library.rs).

messages! {
    /// No visit (or none matching the filter).
    fn no_pages() { en: "No pages.", fr: "Aucune page." }
    /// The parts of the history, by age.
    fn last_hour() { en: "Last hour", fr: "Dernière heure" }
    fn last_day() { en: "Last 24 hours", fr: "Dernières 24\u{a0}heures" }
    fn this_week() { en: "This week", fr: "Cette semaine" }
    fn this_month() { en: "This month", fr: "Ce mois-ci" }
    fn older() { en: "Older", fr: "Plus ancien" }
    /// The button under the list.
    fn clear_history() { en: "Clear history", fr: "Effacer l’historique" }
    /// How long ago, for an age in seconds: short, next to a page or a
    /// device, and after "Synced" ("just now", "5 min ago", "3 h ago",
    /// "2 d ago").
    fn ago(secs: u64) -> String {
        en: {
            match secs {
                0..60 => "just now".into(),
                60..3_600 => format!("{} min ago", secs / 60),
                3_600..86_400 => format!("{} h ago", secs / 3_600),
                _ => format!("{} d ago", secs / 86_400),
            }
        },
        fr: {
            match secs {
                0..60 => "à l’instant".into(),
                60..3_600 => format!("il y a {}\u{a0}min", secs / 60),
                3_600..86_400 => format!("il y a {}\u{a0}h", secs / 3_600),
                _ => format!("il y a {}\u{a0}j", secs / 86_400),
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::in_lang::ago;
    use crate::Lang;

    #[test]
    fn ages_in_both_languages() {
        assert_eq!(ago(Lang::En, 59), "just now");
        assert_eq!(ago(Lang::En, 300), "5 min ago");
        assert_eq!(ago(Lang::En, 3 * 86_400), "3 d ago");
        assert_eq!(ago(Lang::Fr, 0), "à l’instant");
        assert_eq!(ago(Lang::Fr, 300), "il y a 5\u{a0}min");
        assert_eq!(ago(Lang::Fr, 7_200), "il y a 2\u{a0}h");
        assert_eq!(ago(Lang::Fr, 3 * 86_400), "il y a 3\u{a0}j");
    }
}
