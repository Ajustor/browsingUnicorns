//! Repositories that never write to disk, for private windows.

use std::sync::Mutex;

use application::ports::{Repository, RepositoryError};

/// The message of a write refused in a private window.
pub const NOT_IN_PRIVATE: &str = "Not available in a private window.";

/// Keeps the value in memory: `load` gives what was last saved (at first,
/// the value it was made with).
pub struct MemoryRepository<T> {
    value: Mutex<T>,
}

impl<T> MemoryRepository<T> {
    pub fn new(initial: T) -> Self {
        Self {
            value: Mutex::new(initial),
        }
    }
}

impl<T: Default> Default for MemoryRepository<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T: Clone + Send> Repository<T> for MemoryRepository<T> {
    fn load(&self) -> Result<T, RepositoryError> {
        Ok(self.value.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    fn save(&self, value: &T) -> Result<(), RepositoryError> {
        *self.value.lock().unwrap_or_else(|e| e.into_inner()) = value.clone();
        Ok(())
    }
}

/// Reads through another repository, refuses every write.
pub struct ReadOnlyRepository<R> {
    inner: R,
}

impl<R> ReadOnlyRepository<R> {
    pub fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<T, R: Repository<T>> Repository<T> for ReadOnlyRepository<R> {
    fn load(&self) -> Result<T, RepositoryError> {
        self.inner.load()
    }

    fn save(&self, _value: &T) -> Result<(), RepositoryError> {
        Err(RepositoryError(NOT_IN_PRIVATE.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_gives_back_the_last_saved_value() {
        let r = MemoryRepository::new(1u32);
        assert_eq!(r.load().unwrap(), 1);
        r.save(&7).unwrap();
        assert_eq!(r.load().unwrap(), 7);
    }

    #[test]
    fn read_only_reads_and_refuses_writes() {
        let r = ReadOnlyRepository::new(MemoryRepository::new(3u32));
        assert_eq!(r.load().unwrap(), 3);
        assert_eq!(r.save(&4).unwrap_err().0, NOT_IN_PRIVATE);
        assert_eq!(r.load().unwrap(), 3);
    }
}
