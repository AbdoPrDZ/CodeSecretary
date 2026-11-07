//! Single-instance guard: an exclusive OS lock on `<data>/agent.lock`.
//!
//! The OS releases the lock when the process exits, including on crash, so a
//! stale lock file never blocks the next start.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("another secretary agent is already running for this user")]
    AlreadyRunning,
    #[error("could not lock {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Holds the lock until dropped.
#[derive(Debug)]
pub struct InstanceLock {
    _file: File,
}

impl InstanceLock {
    pub fn acquire(path: &Path) -> Result<Self, LockError> {
        let io_err = |source| LockError::Io {
            path: path.to_owned(),
            source,
        };
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .map_err(io_err)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(LockError::AlreadyRunning),
            Err(TryLockError::Error(source)) => Err(io_err(source)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_fails_until_first_is_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("agent.lock");

        let first = InstanceLock::acquire(&path).unwrap();
        assert!(matches!(
            InstanceLock::acquire(&path),
            Err(LockError::AlreadyRunning)
        ));

        drop(first);
        InstanceLock::acquire(&path).expect("lock is free after drop");
    }
}
