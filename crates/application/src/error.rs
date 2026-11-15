use std::fmt;

use crate::ports::EngineError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserError {
    Engine(EngineError),
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(e) => write!(f, "web engine: {e}"),
        }
    }
}

impl std::error::Error for BrowserError {}

impl From<EngineError> for BrowserError {
    fn from(e: EngineError) -> Self {
        Self::Engine(e)
    }
}
