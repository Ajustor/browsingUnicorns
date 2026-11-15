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
            Self::Engine(e) => write!(f, "web engine: {e}"),
            Self::Storage(e) => write!(f, "storage: {e}"),
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
