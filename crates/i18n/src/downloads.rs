//! The downloads bar and list (src/ui/downloads.rs).

messages! {
    /// Tooltip of the bar's cross.
    fn close_bar() { en: "Close the downloads bar", fr: "Fermer la barre des téléchargements" }
    /// The button that removes the finished downloads.
    fn clear() { en: "Clear", fr: "Effacer" }
    /// Its tooltip.
    fn clear_tip() {
        en: "Remove the finished downloads from the list",
        fr: "Retirer de la liste les téléchargements terminés",
    }
    /// The button that opens the downloads' folder.
    fn open_folder() { en: "Open folder", fr: "Ouvrir le dossier" }
    /// A finished download whose size is unknown.
    fn done() { en: "Done", fr: "Terminé" }
    /// A finished download and its size.
    fn done_size(size: &str) { en: "Done · {size}", fr: "Terminé · {size}" }
    /// A download that failed.
    fn failed() { en: "Failed", fr: "Échec" }
    /// A download the user cancelled.
    fn cancelled() { en: "Cancelled", fr: "Annulé" }
    /// Tooltip of the folder button of a finished download.
    fn show_in_folder() { en: "Show in folder", fr: "Afficher dans le dossier" }
    /// Tooltip of the button that starts a failed download again.
    fn download_again() { en: "Download again", fr: "Télécharger à nouveau" }
    /// Tooltip of a finished download's card.
    fn open_file(name: &str) { en: "Open {name}", fr: "Ouvrir {name}" }
    /// A download under way of known size.
    fn progress(received: &str, total: &str, percent: u64) {
        en: "{received} of {total} · {percent}%",
        fr: "{received} sur {total} · {percent}\u{a0}%",
    }
    /// A download under way of unknown size.
    fn downloading(received: &str) { en: "{received} · downloading…", fr: "{received} · téléchargement…" }
    /// A download not started yet.
    fn starting() { en: "Starting…", fr: "Démarrage…" }
    /// A size under 1 KB.
    fn bytes(n: u64) { en: "{n} B", fr: "{n}\u{a0}o" }
    /// A size of 1 KB or more: `value` in the `unit`th of KB, MB, GB, TB,
    /// with `decimals` decimals.
    fn size(value: f64, decimals: usize, unit: usize) -> String {
        en: { format!("{value:.decimals$} {}", ["KB", "MB", "GB", "TB"][unit]) },
        fr: {
            let value = format!("{value:.decimals$}").replace('.', ",");
            format!("{value}\u{a0}{}", ["Ko", "Mo", "Go", "To"][unit])
        },
    }
    /// Tooltip of the toolbar's downloads button: `running` under way.
    fn toolbar_tip(running: usize) -> String {
        en: {
            match running {
                0 => "Downloads".into(),
                n => format!("Downloads: {n} in progress"),
            }
        },
        fr: {
            match running {
                0 => "Téléchargements".into(),
                n => format!("Téléchargements\u{a0}: {n} en cours"),
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::in_lang::*;
    use crate::Lang;

    #[test]
    fn sizes_follow_the_language() {
        assert_eq!(size(Lang::En, 3.5, 2, 0), "3.50 KB");
        assert_eq!(size(Lang::Fr, 3.5, 2, 0), "3,50\u{a0}Ko");
        assert_eq!(size(Lang::Fr, 12.0, 1, 1), "12,0\u{a0}Mo");
        assert_eq!(bytes(Lang::Fr, 512), "512\u{a0}o");
    }

    #[test]
    fn the_toolbar_tip_counts_what_runs() {
        assert_eq!(toolbar_tip(Lang::En, 0), "Downloads");
        assert_eq!(toolbar_tip(Lang::En, 2), "Downloads: 2 in progress");
        assert_eq!(
            toolbar_tip(Lang::Fr, 1),
            "Téléchargements\u{a0}: 1 en cours"
        );
    }
}
