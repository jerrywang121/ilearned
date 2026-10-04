use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use fs4::fs_std::FileExt;

use crate::error::AppError;

/// A stable per-database advisory lock. It deliberately uses a sibling file
/// rather than the database itself so replacement cannot invalidate the lock.
pub(crate) struct DatabaseLock {
    _file: File,
}

impl DatabaseLock {
    pub(crate) fn shared(path: &Path) -> Result<Self, AppError> {
        Self::acquire(path, FileExt::try_lock_shared)
    }

    pub(crate) fn exclusive(path: &Path) -> Result<Self, AppError> {
        Self::acquire(path, |file| file.try_lock_exclusive())
    }

    fn acquire(
        path: &Path,
        lock: impl FnOnce(&File) -> std::io::Result<bool>,
    ) -> Result<Self, AppError> {
        let lock_path = lock_path(path);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .map_err(|_| AppError::Storage("unable to acquire database lock".to_string()))?;
        match lock(&file) {
            Ok(true) => Ok(Self { _file: file }),
            Ok(false) => Err(AppError::Storage(
                "database is in use by another ilearned process".to_string(),
            )),
            Err(_) => Err(AppError::Storage(
                "unable to acquire database lock".to_string(),
            )),
        }
    }
}

fn lock_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or(path.as_os_str());
    path.with_file_name(format!(".{}-ilearned.lock", name.to_string_lossy()))
}
