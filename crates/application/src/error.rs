use std::fmt;

use crate::ports::{EngineError, RepositoryError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserError {
    Engine(EngineError),
    Storage(RepositoryError),
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(e) => f.write_str(&i18n::errors::application::web_engine_failed(e)),
            Self::Storage(e) => f.write_str(&i18n::errors::application::storage_failed(e)),
        }
    }
}

impl From<RepositoryError> for BrowserError {
    fn from(e: RepositoryError) -> Self {
        Self::Storage(e)
    }
}

impl std::error::Error for BrowserError {}

impl From<EngineError> for BrowserError {
    fn from(e: EngineError) -> Self {
        Self::Engine(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use i18n::Lang;
    use i18n::errors::application::in_lang::storage_failed;

    #[test]
    fn errors_speak_the_language_they_are_shown_in() {
        let e = BrowserError::Engine(EngineError("boom".into()));
        assert_eq!(e.to_string(), "web engine: boom");
        assert_eq!(
            i18n::with_lang(Lang::Fr, || e.to_string()),
            "moteur web\u{a0}: boom"
        );
        assert_eq!(storage_failed(Lang::Fr, &"disk"), "stockage\u{a0}: disk");
    }
}
