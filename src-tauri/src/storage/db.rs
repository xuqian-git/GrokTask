//! SQLite open helpers: bundled SQLite, WAL, foreign_keys, busy_timeout.

use crate::paths;
use crate::storage::migrations::{self, MigrationError};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration: {0}")]
    Migration(#[from] MigrationError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub fn open_default() -> Result<Connection, DbError> {
    let path = paths::history_db();
    open_path(&path)
}

pub fn open_path(path: &Path) -> Result<Connection, DbError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrations::migrate(&conn)?;
    Ok(conn)
}

pub fn open_memory() -> Result<Connection, DbError> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrations::migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        ",
    )?;
    Ok(())
}

/// Confirm WAL mode is active (file-backed DBs).
pub fn journal_mode(conn: &Connection) -> Result<String, DbError> {
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    Ok(mode.to_lowercase())
}

pub fn db_path() -> PathBuf {
    paths::history_db()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn file_db_uses_wal() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("history.sqlite3");
        let conn = open_path(&path).unwrap();
        let mode = journal_mode(&conn).unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn foreign_keys_on() {
        let conn = open_memory().unwrap();
        let on: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }
}
