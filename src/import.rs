//! Building and installing databases.
//!
//! A database is always built from scratch into a temporary file next to the target,
//! validated, and only then renamed over the installed one. Any failure before the
//! rename removes the temporary file and leaves the installed database untouched.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};

use crate::database::{self, Database};
use crate::models::{Kind, NameType, Section};
use crate::normalize::normalize;
use crate::sources::{Input, Source};

/// One dataset to import: an adapter and its input file.
pub struct Job<'a> {
    pub source: &'a dyn Source,
    pub input: &'a Input,
    /// Provider-published checksum the input was verified against, e.g. `md5:...`.
    pub upstream_checksum: Option<String>,
    /// When the input was obtained (ISO 8601), if known; defaults to the file's time.
    pub retrieved_at: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Counts {
    pub concepts: u64,
    pub names: u64,
    pub identifiers: u64,
    pub relationships: u64,
    pub attributes: u64,
}

#[derive(Debug)]
pub struct Imported {
    pub slug: String,
    pub version: String,
    pub counts: Counts,
}

/// Builds a database from `jobs` and atomically installs it at `dest`.
/// The previously installed database, if any, is kept as `<dest>.bak`.
pub fn install(dest: &Path, jobs: &[Job]) -> Result<Vec<Imported>> {
    if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let tmp = temp_path(dest);
    remove_db_files(&tmp);
    match build(&tmp, jobs).and_then(|r| replace(&tmp, dest).map(|()| r)) {
        Ok(report) => Ok(report),
        Err(err) => {
            remove_db_files(&tmp);
            Err(err)
        }
    }
}

/// Builds a complete, validated database at `path`, which must not exist.
pub fn build(path: &Path, jobs: &[Job]) -> Result<Vec<Imported>> {
    if jobs.is_empty() {
        bail!("no sources to import");
    }
    let mut conn = database::create(path)?;
    // The file is discarded on any failure, so durability during the build is pointless.
    conn.execute_batch("PRAGMA journal_mode = OFF; PRAGMA synchronous = OFF;")?;

    let mut report = Vec::new();
    for job in jobs {
        let info = job.source.info();
        let release = job.source.release(job.input)?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO sources (stale_after_days, slug, title, code_system, provider, version, release_date, license,
                 attribution, url, file_name, upstream_checksum, sha256, retrieved_at,
                 imported_at, importer_version, origin, redistributable, notice)
             VALUES (?15, ?1, ?2, ?14, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?16, ?10,
                 COALESCE(?17, strftime('%Y-%m-%dT%H:%M:%SZ', ?11, 'unixepoch'),
                          strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                 strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?12, 'builtin', ?13, ?18)",
            params![
                info.slug,
                info.title,
                info.provider,
                release.version,
                release.release_date,
                info.license,
                info.attribution,
                info.url,
                job.input.file_name,
                job.input.sha256,
                job.input.modified.map(|s| s as i64),
                env!("CARGO_PKG_VERSION"),
                info.redistributable,
                info.code_system,
                info.stale_after_days,
                job.upstream_checksum,
                job.retrieved_at,
                info.notice,
            ],
        )?;
        let source_id = tx.last_insert_rowid();
        let mut sink = ImportSink {
            conn: &tx,
            source_id,
            counts: Counts::default(),
        };
        job.source
            .import(job.input, &mut sink)
            .with_context(|| format!("importing {} failed", info.slug))?;
        let counts = sink.counts;
        tx.commit()?;
        report.push(Imported {
            slug: info.slug,
            version: release.version,
            counts,
        });
    }

    conn.execute_batch(
        "INSERT INTO names_tok (names_tok) VALUES ('rebuild');
         INSERT INTO names_tri (names_tri) VALUES ('rebuild');",
    )?;

    validate(&conn)?;
    for job in jobs {
        let slug = job.source.info().slug;
        let id: i64 = conn.query_row("SELECT id FROM sources WHERE slug = ?1", [&slug], |r| {
            r.get(0)
        })?;
        job.source
            .validate(&conn, id)
            .with_context(|| format!("validation of {slug} failed"))?;
    }

    conn.execute_batch(
        "UPDATE meta SET value = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE key = 'built_at';
         ANALYZE;
         VACUUM;",
    )?;
    drop(conn);

    File::open(path)?.sync_all()?;
    // Final check through the same path lookups use.
    Database::open(path)?;
    Ok(report)
}

/// Checks every source-independent invariant of a built database.
fn validate(conn: &Connection) -> Result<()> {
    let integrity: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" {
        bail!("integrity check failed: {integrity}");
    }
    let fk_violations: i64 =
        conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })?;
    if fk_violations > 0 {
        bail!("{fk_violations} foreign key violations");
    }
    let empty: Vec<String> = conn
        .prepare(
            "SELECT slug FROM sources s
             WHERE NOT EXISTS (SELECT 1 FROM concepts c WHERE c.source_id = s.id)",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if !empty.is_empty() {
        bail!("no concepts imported from: {}", empty.join(", "));
    }
    // Navigation paths may mention relationships a particular release happens not to
    // contain (e.g. no packs), but a source none of whose paths can be followed is broken.
    let mut stmt = conn.prepare(
        "SELECT s.slug, n.path FROM navigation n JOIN sources s ON s.id = n.source_id
         ORDER BY s.slug",
    )?;
    let paths: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut usable = std::collections::BTreeMap::<String, bool>::new();
    for (slug, path) in paths {
        let mut ok = true;
        for predicate in path.split(' ') {
            ok &= conn.query_row(
                "SELECT EXISTS (SELECT 1 FROM relationships r JOIN sources s ON s.id = r.source_id
                                WHERE s.slug = ?1 AND r.predicate = ?2)",
                params![slug, predicate],
                |r| r.get::<_, bool>(0),
            )?;
        }
        *usable.entry(slug).or_default() |= ok;
    }
    if let Some((slug, _)) = usable.iter().find(|(_, ok)| !**ok) {
        bail!("none of {slug}'s navigation paths match its relationships (unknown predicate)");
    }
    Ok(())
}

/// Moves the built database into place, keeping the old one as `.bak`.
fn replace(tmp: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        let bak = sibling(dest, "bak");
        let _ = fs::remove_file(&bak);
        // A hard link keeps `dest` in place until the atomic rename below.
        if fs::hard_link(dest, &bak).is_err() {
            fs::copy(dest, &bak).with_context(|| format!("cannot back up {}", dest.display()))?;
        }
    }
    fs::rename(tmp, dest)
        .with_context(|| format!("cannot move new database to {}", dest.display()))?;
    if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty()) {
        // Persist the rename. Not supported everywhere, so best effort.
        let _ = File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}

fn temp_path(dest: &Path) -> PathBuf {
    sibling(dest, &format!("tmp-{}", std::process::id()))
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

fn remove_db_files(path: &Path) {
    let _ = fs::remove_file(path);
    let mut journal = path.as_os_str().to_owned();
    journal.push("-journal");
    let _ = fs::remove_file(PathBuf::from(journal));
}

/// The only write path from adapters into the database. Every row it writes is
/// stamped with the adapter's source id.
pub struct ImportSink<'c> {
    conn: &'c Connection,
    source_id: i64,
    counts: Counts,
}

impl ImportSink<'_> {
    /// Adds a concept and its preferred name; returns the concept id.
    pub fn concept(
        &mut self,
        source_code: &str,
        kind: Kind,
        source_type: &str,
        name: &str,
    ) -> Result<i64> {
        self.conn
            .prepare_cached(
                "INSERT INTO concepts (source_id, source_code, kind, source_type, name)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                self.source_id,
                source_code,
                kind.as_str(),
                source_type,
                name
            ])?;
        let id = self.conn.last_insert_rowid();
        self.counts.concepts += 1;
        Ok(id)
    }

    pub fn name(
        &mut self,
        concept_id: i64,
        name: &str,
        name_type: NameType,
        source_type: &str,
        source_ref: Option<&str>,
    ) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO names (concept_id, source_id, name, norm, name_type, source_type, source_ref)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?
            .execute(params![
                concept_id,
                self.source_id,
                name,
                normalize(name),
                name_type.as_str(),
                source_type,
                source_ref
            ])?;
        self.counts.names += 1;
        Ok(())
    }

    /// Records an identifier. Duplicates for the same concept are ignored.
    pub fn identifier(&mut self, concept_id: i64, system: &str, value: &str) -> Result<()> {
        let n = self
            .conn
            .prepare_cached(
                "INSERT OR IGNORE INTO identifiers (concept_id, source_id, system, value)
                 VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![concept_id, self.source_id, system, value])?;
        self.counts.identifiers += n as u64;
        Ok(())
    }

    pub fn relationship(
        &mut self,
        subject_id: i64,
        predicate: &str,
        object_id: i64,
        source_predicate: &str,
    ) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO relationships (source_id, subject_id, predicate, object_id, source_predicate)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![self.source_id, subject_id, predicate, object_id, source_predicate])?;
        self.counts.relationships += 1;
        Ok(())
    }

    /// Declares how to collect `section` for concepts of `from`: follow `path` (predicates,
    /// in order) and keep the concepts of kind `to` that are reached.
    pub fn navigation(
        &mut self,
        from: Kind,
        section: Section,
        path: &[&str],
        to: Kind,
    ) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT OR IGNORE INTO navigation (source_id, from_kind, section, path, to_kind)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![
                self.source_id,
                from.as_str(),
                section.as_str(),
                path.join(" "),
                to.as_str()
            ])?;
        Ok(())
    }

    /// Declares how attribute `key` is displayed: its label, its position (lower first)
    /// and whether the summary page shows it.
    pub fn attribute_key(
        &mut self,
        key: &str,
        label: &str,
        rank: i64,
        summary: bool,
    ) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT OR REPLACE INTO attribute_keys (source_id, key, label, rank, summary)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?
            .execute(params![self.source_id, key, label, rank, summary])?;
        Ok(())
    }

    /// Adds (or finds) a class in a classification system; returns its id.
    pub fn classification(&mut self, system: &str, code: &str, name: &str) -> Result<i64> {
        self.conn
            .prepare_cached(
                "INSERT OR IGNORE INTO classifications (source_id, system, code, name)
                 VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![self.source_id, system, code, name])?;
        Ok(self
            .conn
            .prepare_cached(
                "SELECT id FROM classifications WHERE source_id = ?1 AND system = ?2 AND code = ?3",
            )?
            .query_row(params![self.source_id, system, code], |r| r.get(0))?)
    }

    /// Records that the source assigns `concept_id` to a class.
    pub fn classify(&mut self, concept_id: i64, classification_id: i64) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT OR IGNORE INTO concept_classifications (concept_id, classification_id, source_id)
                 VALUES (?1, ?2, ?3)",
            )?
            .execute(params![concept_id, classification_id, self.source_id])?;
        Ok(())
    }

    pub fn attribute(&mut self, concept_id: i64, key: &str, value: &str) -> Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO attributes (concept_id, source_id, key, value) VALUES (?1, ?2, ?3, ?4)",
            )?
            .execute(params![concept_id, self.source_id, key, value])?;
        self.counts.attributes += 1;
        Ok(())
    }
}
