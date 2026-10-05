use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use tempfile::Builder;
use uuid::Uuid;

use crate::error::AppError;

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const REQUIRED_SCHEMA_OBJECTS: [&str; 3] = ["schema_migrations", "experiences", "experiences_fts"];

/// Convert an existing ilearned plaintext SQLite database to SQLCipher format.
pub fn encrypt_database(path: &Path, key: &str) -> Result<(), AppError> {
    if key.is_empty() {
        return Err(AppError::InvalidInput(
            "database encryption key must not be empty".to_string(),
        ));
    }

    // Hold this across every migration stage. The stable sibling lock remains
    // locked even after the source database is atomically replaced.
    let _database_lock = super::lock::DatabaseLock::exclusive(path)?;
    validate_input(path)?;

    #[cfg(unix)]
    let source_permissions = source_permissions(path)?;
    #[cfg(not(unix))]
    source_permissions(path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = create_temporary_database(parent)?;

    #[cfg(unix)]
    let result = encrypt_database_inner(path, key, &temporary, source_permissions);
    #[cfg(not(unix))]
    let result = encrypt_database_inner(path, key, &temporary, ());
    if result.is_err() {
        remove_database_files(&temporary);
    }
    result
}

fn encrypt_database_inner(
    path: &Path,
    key: &str,
    temporary: &Path,
    #[cfg(unix)] source_permissions: Option<std::fs::Permissions>,
    #[cfg(not(unix))] source_permissions: (),
) -> Result<(), AppError> {
    let source = open_existing(path)?;
    validate_schema(&source)?;
    let (busy, _, _) = source
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|_| {
            AppError::Storage("unable to checkpoint the plaintext database".to_string())
        })?;
    validate_checkpoint_result(busy)?;

    source
        .execute(
            "ATTACH DATABASE ?1 AS encrypted KEY ?2",
            rusqlite::params![temporary.to_string_lossy(), key],
        )
        .map_err(|_| AppError::Storage("unable to create the encrypted database".to_string()))?;
    let export_result = source.query_row("SELECT sqlcipher_export('encrypted')", [], |_| Ok(()));
    let detach_result = source.execute("DETACH DATABASE encrypted", []);
    export_result
        .and(detach_result.map(|_| ()))
        .map_err(|_| AppError::Storage("unable to export the encrypted database".to_string()))?;
    drop(source);

    verify_encrypted_database(temporary, key)?;
    apply_source_permissions(temporary, source_permissions)?;
    // Remove plaintext sidecars while the plaintext source is still intact.
    // A failure leaves its logical contents usable and prevents replacement.
    remove_sidecars(path)?;
    replace_source(path, temporary)
}

fn validate_input(path: &Path) -> Result<(), AppError> {
    let metadata = fs::metadata(path).map_err(|_| {
        AppError::InvalidInput("database encryption requires an existing regular file".to_string())
    })?;
    if !metadata.is_file() {
        return Err(AppError::InvalidInput(
            "database encryption requires an existing regular file".to_string(),
        ));
    }
    let mut header = [0; SQLITE_HEADER.len()];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| AppError::Storage("unable to read database header".to_string()))?;
    if header != *SQLITE_HEADER {
        return Err(AppError::InvalidInput(
            "database encryption requires a plaintext SQLite database".to_string(),
        ));
    }
    Ok(())
}

fn open_existing(path: &Path) -> Result<Connection, AppError> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|_| AppError::Storage("unable to open plaintext database".to_string()))
}

fn validate_checkpoint_result(busy: i64) -> Result<(), AppError> {
    if busy == 0 {
        return Ok(());
    }
    Err(AppError::Storage(
        "plaintext database checkpoint is busy".to_string(),
    ))
}

fn validate_schema(connection: &Connection) -> Result<(), AppError> {
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type IN ('table', 'trigger') AND name IN (?1, ?2, ?3)",
            REQUIRED_SCHEMA_OBJECTS,
            |row| row.get(0),
        )
        .map_err(|_| AppError::Storage("unable to validate database schema".to_string()))?;
    if count != REQUIRED_SCHEMA_OBJECTS.len() as i64 {
        return Err(AppError::InvalidInput(
            "database does not contain the expected ilearned schema".to_string(),
        ));
    }
    Ok(())
}

/// Creates a secure same-directory temporary file. `tempfile` defaults to
/// owner-only (`0o600`) permissions on Unix and uses the platform's secure
/// temporary-file creation defaults on targets without Unix mode bits.
fn create_temporary_database(parent: &Path) -> Result<PathBuf, AppError> {
    let uuid = Uuid::new_v4().to_string();
    let temporary = Builder::new()
        .prefix(&format!(".ilearned-encrypt-{uuid}-"))
        .suffix(".db")
        .tempfile_in(parent)
        .map_err(|_| {
            AppError::Storage("unable to create temporary encrypted database".to_string())
        })?;
    let (file, path) = temporary.keep().map_err(|_| {
        AppError::Storage("unable to retain temporary encrypted database".to_string())
    })?;
    drop(file);
    Ok(path)
}

fn verify_encrypted_database(path: &Path, key: &str) -> Result<(), AppError> {
    let connection = open_existing(path)?;
    connection
        .pragma_update(None, "key", key)
        .and_then(|_| connection.pragma_update(None, "cipher_memory_security", "ON"))
        .map_err(|_| AppError::DatabaseKey("unable to verify encrypted database".to_string()))?;
    let integrity_results = cipher_integrity_results(&connection)?;
    validate_cipher_integrity_results(&integrity_results)?;
    validate_schema(&connection)?;
    Ok(())
}

#[cfg(unix)]
fn source_permissions(path: &Path) -> Result<Option<std::fs::Permissions>, AppError> {
    fs::metadata(path)
        .map(|metadata| Some(metadata.permissions()))
        .map_err(|_| AppError::Storage("unable to inspect database permissions".to_string()))
}

#[cfg(not(unix))]
fn source_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(unix)]
fn apply_source_permissions(
    path: &Path,
    source_permissions: Option<std::fs::Permissions>,
) -> Result<(), AppError> {
    if let Some(permissions) = source_permissions {
        fs::set_permissions(path, permissions).map_err(|_| {
            AppError::Storage("unable to preserve database permissions".to_string())
        })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_source_permissions(_path: &Path, _source_permissions: ()) -> Result<(), AppError> {
    Ok(())
}

#[cfg(unix)]
fn replace_source(source: &Path, temporary: &Path) -> Result<(), AppError> {
    fs::rename(temporary, source)
        .map_err(|_| AppError::Storage("unable to replace plaintext database".to_string()))
}

#[cfg(windows)]
fn replace_source(source: &Path, temporary: &Path) -> Result<(), AppError> {
    let backup = source.with_file_name(format!(".ilearned-encrypt-backup-{}.db", Uuid::new_v4()));
    replace_with_rollback(
        source,
        temporary,
        &backup,
        |from, to| fs::rename(from, to),
        |path| fs::remove_file(path),
    )
}

#[cfg(all(not(unix), not(windows)))]
fn replace_source(source: &Path, temporary: &Path) -> Result<(), AppError> {
    fs::rename(temporary, source)
        .map_err(|_| AppError::Storage("unable to replace plaintext database".to_string()))
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), suffix))
}

fn cipher_integrity_results(connection: &Connection) -> Result<Vec<String>, AppError> {
    let mut statement = connection
        .prepare("PRAGMA cipher_integrity_check")
        .map_err(|_| AppError::DatabaseKey("unable to verify encrypted database".to_string()))?;
    statement
        .query_map([], |row| row.get(0))
        .and_then(Iterator::collect)
        .map_err(|_| AppError::DatabaseKey("unable to verify encrypted database".to_string()))
}

fn validate_cipher_integrity_results(results: &[String]) -> Result<(), AppError> {
    // The bundled SQLCipher build emits no row on a clean database. A returned
    // row must explicitly report success; any diagnostic indicates corruption.
    if results
        .iter()
        .all(|result| result.trim().eq_ignore_ascii_case("ok"))
    {
        return Ok(());
    }
    Err(AppError::DatabaseKey(
        "encrypted database integrity verification failed".to_string(),
    ))
}

#[cfg(any(windows, test))]
fn replace_with_rollback<R, D>(
    source: &Path,
    temporary: &Path,
    backup: &Path,
    mut rename: R,
    mut remove: D,
) -> Result<(), AppError>
where
    R: FnMut(&Path, &Path) -> std::io::Result<()>,
    D: FnMut(&Path) -> std::io::Result<()>,
{
    rename(source, backup).map_err(|_| {
        AppError::Storage("unable to stage plaintext database replacement".to_string())
    })?;
    if rename(temporary, source).is_err() {
        return match rename(backup, source) {
            Ok(()) => Err(AppError::Storage(
                "unable to replace plaintext database; source restored".to_string(),
            )),
            Err(_) => Err(AppError::Storage(
                "unable to replace plaintext database and restore the source".to_string(),
            )),
        };
    }
    if remove(backup).is_ok() {
        return Ok(());
    }

    let encrypted_removal_failed = remove(source).is_err();
    match rename(backup, source) {
        Ok(()) if encrypted_removal_failed => Err(AppError::Storage(
            "unable to remove plaintext backup and encrypted replacement; source restored"
                .to_string(),
        )),
        Ok(()) => Err(AppError::Storage(
            "unable to remove plaintext backup; encrypted replacement removed and source restored"
                .to_string(),
        )),
        Err(_) if encrypted_removal_failed => Err(AppError::Storage(
            "unable to remove plaintext backup and encrypted replacement, and restore the source"
                .to_string(),
        )),
        Err(_) => Err(AppError::Storage(
            "unable to remove plaintext backup and restore the source after removing the encrypted replacement"
                .to_string(),
        )),
    }
}

fn remove_database_files(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = remove_sidecars(path);
}

fn remove_sidecars(path: &Path) -> Result<(), AppError> {
    for suffix in ["-wal", "-shm"] {
        match fs::remove_file(sidecar_path(path, suffix)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(AppError::Storage(
                    "unable to remove plaintext database sidecars".to_string(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_cipher_integrity_failure_rejects_verification() {
        assert!(validate_cipher_integrity_results(&["page 1 is corrupted".to_string()]).is_err());
    }

    #[test]
    fn busy_checkpoint_is_rejected() {
        let error = validate_checkpoint_result(1).unwrap_err();

        assert!(error.to_string().contains("checkpoint is busy"));
    }

    #[test]
    fn sidecar_cleanup_failure_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("database.db");
        fs::create_dir(sidecar_path(&database, "-wal")).unwrap();

        let error = remove_sidecars(&database).unwrap_err();

        assert!(error.to_string().contains("sidecars"));
    }

    #[cfg(unix)]
    #[test]
    fn temporary_database_is_owner_only_before_source_permissions_are_copied() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = create_temporary_database(dir.path()).unwrap();

        assert_eq!(fs::metadata(path).unwrap().permissions().mode() & 0o077, 0);
    }

    #[cfg(windows)]
    #[test]
    fn windows_replace_source_accepts_real_filesystem_callbacks() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.db");
        let temporary = directory.path().join("temporary.db");
        fs::write(&source, b"plaintext").unwrap();
        fs::write(&temporary, b"encrypted").unwrap();

        replace_source(&source, &temporary).unwrap();

        assert_eq!(fs::read(&source).unwrap(), b"encrypted");
        assert!(!temporary.exists());
    }

    #[test]
    fn replacement_restore_failure_is_reported() {
        let error = replace_with_rollback(
            Path::new("source.db"),
            Path::new("temporary.db"),
            Path::new("backup.db"),
            |from, to| match (from, to) {
                (from, to) if from == Path::new("source.db") && to == Path::new("backup.db") => {
                    Ok(())
                }
                _ => Err(std::io::Error::other("simulated failure")),
            },
            |_| Ok(()),
        )
        .unwrap_err();

        assert!(error.to_string().contains("restore"));
    }

    #[test]
    fn replacement_backup_cleanup_failure_restores_the_source() {
        use std::{cell::RefCell, rc::Rc};

        let renames = Rc::new(RefCell::new(Vec::new()));
        let removals = Rc::new(RefCell::new(Vec::new()));
        let rename_log = Rc::clone(&renames);
        let removal_log = Rc::clone(&removals);
        let error = replace_with_rollback(
            Path::new("source.db"),
            Path::new("temporary.db"),
            Path::new("backup.db"),
            move |from, to| {
                rename_log
                    .borrow_mut()
                    .push((from.to_path_buf(), to.to_path_buf()));
                Ok(())
            },
            move |path| {
                removal_log.borrow_mut().push(path.to_path_buf());
                if path == Path::new("backup.db") {
                    Err(std::io::Error::other("simulated cleanup failure"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("source restored"));
        assert_eq!(
            *removals.borrow(),
            vec![PathBuf::from("backup.db"), PathBuf::from("source.db")]
        );
        assert_eq!(
            *renames.borrow(),
            vec![
                (PathBuf::from("source.db"), PathBuf::from("backup.db")),
                (PathBuf::from("temporary.db"), PathBuf::from("source.db")),
                (PathBuf::from("backup.db"), PathBuf::from("source.db")),
            ]
        );
    }

    #[test]
    fn backup_cleanup_and_encrypted_removal_failure_still_attempts_restore() {
        use std::{cell::RefCell, rc::Rc};

        let renames = Rc::new(RefCell::new(Vec::new()));
        let rename_log = Rc::clone(&renames);
        let error = replace_with_rollback(
            Path::new("source.db"),
            Path::new("temporary.db"),
            Path::new("backup.db"),
            move |from, to| {
                rename_log
                    .borrow_mut()
                    .push((from.to_path_buf(), to.to_path_buf()));
                if from == Path::new("backup.db") && to == Path::new("source.db") {
                    Err(std::io::Error::other("simulated restore failure"))
                } else {
                    Ok(())
                }
            },
            |path| {
                Err(std::io::Error::other(format!(
                    "simulated removal failure: {path:?}"
                )))
            },
        )
        .unwrap_err();

        assert!(error.to_string().contains("plaintext backup"));
        assert!(error.to_string().contains("restore"));
        assert_eq!(
            *renames.borrow(),
            vec![
                (PathBuf::from("source.db"), PathBuf::from("backup.db")),
                (PathBuf::from("temporary.db"), PathBuf::from("source.db")),
                (PathBuf::from("backup.db"), PathBuf::from("source.db")),
            ]
        );
    }

    #[cfg(not(unix))]
    #[test]
    fn temporary_database_uses_platform_secure_creation() {
        let dir = tempfile::tempdir().unwrap();
        let path = create_temporary_database(dir.path()).unwrap();

        assert!(path.is_file());
    }
}
