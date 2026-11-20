//! The ad blocker's shield and its panel.

messages! {
    /// What the blocker did on the page.
    fn items_blocked(n: u32) -> String {
        en: { if n == 1 { "1 item blocked on this page.".into() } else { format!("{n} items blocked on this page.") } },
        fr: { if n <= 1 { format!("{n} élément bloqué sur cette page.") } else { format!("{n} éléments bloqués sur cette page.") } },
    }
}
