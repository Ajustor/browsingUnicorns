//! The interface's language: the one chosen on this computer (in
//! `device.toml`, whatever the profile), or the system's.

use domain::LanguageChoice;
use i18n::Lang;
use infrastructure::DeviceFile;

/// The language chosen on this computer (`Auto` when unknown).
pub fn device_language() -> LanguageChoice {
    let Some(file) = DeviceFile::default_location() else {
        return LanguageChoice::Auto;
    };
    file.language().unwrap_or_else(|e| {
        log::warn!("language setting ignored: {e}");
        LanguageChoice::Auto
    })
}

/// Keeps `choice` for this computer and speaks it at once.
pub fn choose_language(choice: LanguageChoice) -> Result<(), application::ports::RepositoryError> {
    apply_language(choice);
    match DeviceFile::default_location() {
        Some(file) => file.set_language(choice),
        None => Ok(()),
    }
}

/// Makes the interface speak `choice` (the system's language for `Auto`).
pub fn apply_language(choice: LanguageChoice) {
    let lang = effective(choice, sys_locale::get_locale().as_deref());
    if lang != i18n::current() {
        log::info!("interface in {}", lang.code());
        i18n::set(lang);
    }
}

/// The language for `choice` on a system whose locale is `system`.
fn effective(choice: LanguageChoice, system: Option<&str>) -> Lang {
    match choice {
        LanguageChoice::Auto => Lang::from_locale(system.unwrap_or("")),
        LanguageChoice::En => Lang::En,
        LanguageChoice::Fr => Lang::Fr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_the_system() {
        assert_eq!(effective(LanguageChoice::Auto, Some("fr-FR")), Lang::Fr);
        assert_eq!(effective(LanguageChoice::Auto, Some("de-DE")), Lang::En);
        assert_eq!(effective(LanguageChoice::Auto, None), Lang::En);
    }

    #[test]
    fn a_chosen_language_wins() {
        assert_eq!(effective(LanguageChoice::En, Some("fr-FR")), Lang::En);
        assert_eq!(effective(LanguageChoice::Fr, Some("en-US")), Lang::Fr);
    }
}
