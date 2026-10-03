use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::models::SourceRecord;

/// Schema version stored in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i64 = 2;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_multi_source.sql"),
];

/// Returned when no database file exists at the expected path.
#[derive(Debug)]
pub struct NotInstalled(pub PathBuf);

impl fmt::Display for NotInstalled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no database at {}; run `gurd update` to install one",
            self.0.display()
        )
    }
}

impl std::error::Error for NotInstalled {}

/// A read-only handle used for all lookups.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(NotInstalled(path.to_owned()).into());
        }
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(path, flags)
            .with_context(|| format!("cannot open {}", path.display()))?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .with_context(|| format!("{} is not a SQLite database", path.display()))?;
        match version {
            SCHEMA_VERSION => {}
            0 => bail!("{} is not a gurd database", path.display()),
            v if v > SCHEMA_VERSION => bail!(
                "database schema {v} is newer than this version of gurd supports ({SCHEMA_VERSION}); upgrade gurd"
            ),
            v => bail!(
                "database schema {v} is older than {SCHEMA_VERSION}; run `gurd update` to rebuild it"
            ),
        }
        Ok(Self {
            conn,
            path: path.to_owned(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn sources(&self) -> Result<Vec<SourceRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT slug, title, code_system, provider, version, release_date, license, attribution, url,
                    file_name, upstream_checksum, sha256, retrieved_at, imported_at,
                    importer_version, origin, redistributable, stale_after_days,
                    CAST(julianday('now') - julianday(release_date) AS INTEGER), notice
             FROM sources ORDER BY slug",
        )?;
        let rows = stmt.query_map([], |row| {
            let stale_after_days: Option<i64> = row.get(17)?;
            let age_days: Option<i64> = row.get(18)?;
            Ok(SourceRecord {
                slug: row.get(0)?,
                title: row.get(1)?,
                code_system: row.get(2)?,
                provider: row.get(3)?,
                version: row.get(4)?,
                release_date: row.get(5)?,
                license: row.get(6)?,
                attribution: row.get(7)?,
                url: row.get(8)?,
                file_name: row.get(9)?,
                upstream_checksum: row.get(10)?,
                sha256: row.get(11)?,
                retrieved_at: row.get(12)?,
                imported_at: row.get(13)?,
                importer_version: row.get(14)?,
                origin: row.get(15)?,
                redistributable: row.get(16)?,
                stale_after_days,
                age_days,
                stale: matches!((age_days, stale_after_days), (Some(age), Some(max)) if age > max),
                notice: row.get(19)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// Creates a new, empty database with the current schema. Refuses to touch an existing file:
/// databases are always built fresh and then moved into place.
pub fn create(path: &Path) -> Result<Connection> {
    if path.exists() {
        bail!(
            "refusing to create database: {} already exists",
            path.display()
        );
    }
    let mut conn =
        Connection::open(path).with_context(|| format!("cannot create {}", path.display()))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    let tx = conn.transaction()?;
    for migration in MIGRATIONS {
        tx.execute_batch(migration)?;
    }
    tx.execute(
        "INSERT INTO meta (key, value) VALUES
           ('built_by', ?1),
           ('built_at', strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        [concat!("gurd ", env!("CARGO_PKG_VERSION"))],
    )?;
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(conn)
}
