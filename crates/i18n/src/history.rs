//! The History view of the side panel (src/ui/library.rs).

messages! {
    /// No visit (or none matching the filter).
    fn no_pages() { en: "No pages.", fr: "Aucune page." }
    /// The button under the list.
    fn clear_history() { en: "Clear history", fr: "Effacer l’historique" }
    /// How long ago, for an age in seconds: short, next to a page or a
    /// device ("now", "5 min", "3 h", "2 d").
    fn ago(secs: u64) -> String {
        en: {
            match secs {
                0..60 => "now".into(),
                60..3_600 => format!("{} min", secs / 60),
                3_600..86_400 => format!("{} h", secs / 3_600),
                _ => format!("{} d", secs / 86_400),
            }
        },
        fr: {
            match secs {
                0..60 => "à l’instant".into(),
                60..3_600 => format!("il y a {} min", secs / 60),
                3_600..86_400 => format!("il y a {} h", secs / 3_600),
                _ => format!("il y a {} j", secs / 86_400),
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
        assert_eq!(ago(Lang::En, 59), "now");
        assert_eq!(ago(Lang::En, 300), "5 min");
        assert_eq!(ago(Lang::En, 3 * 86_400), "3 d");
        assert_eq!(ago(Lang::Fr, 0), "à l’instant");
        assert_eq!(ago(Lang::Fr, 300), "il y a 5 min");
        assert_eq!(ago(Lang::Fr, 7_200), "il y a 2 h");
        assert_eq!(ago(Lang::Fr, 3 * 86_400), "il y a 3 j");
    }
}
