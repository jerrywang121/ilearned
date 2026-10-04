use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use uuid::Uuid;

use crate::error::AppError;

const SQLITE_HEADER: &[u8; 16] = b"SQLite format 3\0";
const REQUIRED_SCHEMA_OBJECTS: [&str; 3] = ["schema_migrations", "experiences", "experiences_fts"];

/// Convert an existing ilearned plaintext SQLite database to SQLCipher format.
pub fn encrypt_database(path: &Path, key: &str) -> Result<(), AppError> {
    validate_input(path, key)?;

    let source_permissions = source_permissions(path)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temporary = parent.join(format!(".ilearned-encrypt-{}.db", Uuid::new_v4()));

    let result = encrypt_database_inner(path, key, &temporary, source_permissions);
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
    source
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .map_err(|_| {
            AppError::Storage("unable to checkpoint the plaintext database".to_string())
        })?;

    create_temporary_database(temporary)?;
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
    replace_source(path, temporary)?;
    remove_sidecars(path);
    Ok(())
}

fn validate_input(path: &Path, key: &str) -> Result<(), AppError> {
    if key.is_empty() {
        return Err(AppError::InvalidInput(
            "database encryption key must not be empty".to_string(),
        ));
    }
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

fn create_temporary_database(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    let result = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    };
    #[cfg(not(unix))]
    let result = OpenOptions::new().write(true).create_new(true).open(path);

    result
        .map(|_| ())
        .map_err(|_| AppError::Storage("unable to create temporary encrypted database".to_string()))
}

fn verify_encrypted_database(path: &Path, key: &str) -> Result<(), AppError> {
    let connection = open_existing(path)?;
    connection
        .pragma_update(None, "key", key)
        .and_then(|_| connection.pragma_update(None, "cipher_memory_security", "ON"))
        .map_err(|_| AppError::DatabaseKey("unable to verify encrypted database".to_string()))?;
    connection
        .execute_batch("PRAGMA cipher_integrity_check")
        .map_err(|_| AppError::DatabaseKey("unable to verify encrypted database".to_string()))?;
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
    fs::rename(source, &backup).map_err(|_| {
        AppError::Storage("unable to stage plaintext database replacement".to_string())
    })?;
    if fs::rename(temporary, source).is_err() {
        let _ = fs::rename(&backup, source);
        return Err(AppError::Storage(
            "unable to replace plaintext database".to_string(),
        ));
    }
    fs::remove_file(backup)
        .map_err(|_| AppError::Storage("unable to remove plaintext database backup".to_string()))
}

#[cfg(all(not(unix), not(windows)))]
fn replace_source(source: &Path, temporary: &Path) -> Result<(), AppError> {
    fs::rename(temporary, source)
        .map_err(|_| AppError::Storage("unable to replace plaintext database".to_string()))
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), suffix))
}

fn remove_database_files(path: &Path) {
    let _ = fs::remove_file(path);
    remove_sidecars(path);
}

fn remove_sidecars(path: &Path) {
    let _ = fs::remove_file(sidecar_path(path, "-wal"));
    let _ = fs::remove_file(sidecar_path(path, "-shm"));
}
