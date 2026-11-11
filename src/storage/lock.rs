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
        // The lock file is a sibling of the database. Create its parent first
        // so opening a new configured database can reach SQLite initialization.
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| {
                AppError::Storage(format!(
                    "cannot create database directory {}: {error}",
                    parent.display()
                ))
            })?;
        }
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
            .open(&lock_path)
            .map_err(|error| {
                AppError::Storage(format!(
                    "cannot open database lock file {}: {error}",
                    lock_path.display()
                ))
            })?;
        match lock(&file) {
            Ok(true) => Ok(Self { _file: file }),
            Ok(false) => Err(AppError::Storage(
                "database is in use by another ilearned process".to_string(),
            )),
            Err(error) => Err(AppError::Storage(format!(
                "unable to acquire database lock {}: {error}",
                lock_path.display()
            ))),
        }
    }
}

fn lock_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or(path.as_os_str());
    path.with_file_name(format!(".{}-ilearned.lock", name.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::{lock_path, DatabaseLock};

    #[test]
    fn lock_operation_error_includes_path_and_os_cause() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("store.db");
        let expected_lock_path = lock_path(&db_path);

        let error = match DatabaseLock::acquire(&db_path, |_| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "simulated lock failure",
            ))
        }) {
            Ok(_) => panic!("lock operation unexpectedly succeeded"),
            Err(error) => error.to_string(),
        };

        assert!(
            error.contains("unable to acquire database lock"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains(&expected_lock_path.display().to_string()),
            "unexpected error: {error}"
        );
        assert!(
            error.contains("simulated lock failure"),
            "unexpected error: {error}"
        );
    }
}
