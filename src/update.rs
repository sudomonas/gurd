//! `gurd update`: obtain a release file, optionally verify it, and install a database
//! built from it. The same steps apply to every source:
//!
//! 1. take the release file: a local file (`--from`), or a download from `--url` or the
//!    source's default location into `<cache>/<file>.part`;
//! 2. verify it against the MD5 the user supplies (`--md5`), or else print its MD5 and
//!    SHA-256 so the user can compare them with what the provider publishes;
//! 3. read the release version from the file; stop if it is already installed (unless
//!    forced);
//! 4. build and validate a new database, then swap it in (`import::install`).
//!
//! A failure at any step leaves the installed database untouched.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};

use crate::database::Database;
use crate::import::{self, Imported, Job};
use crate::sources::{Fetch, Input, Source, md5_file};

pub struct Options<'a> {
    pub dest: &'a Path,
    /// Use this file instead of downloading.
    pub from: Option<&'a Path>,
    /// Download from here instead of the source's default location.
    pub url: Option<&'a str>,
    /// Expected MD5 of the release file, as published by the provider.
    pub md5: Option<String>,
    /// Install even if the same release is already installed.
    pub force: bool,
    /// Keep the downloaded file in the cache directory.
    pub keep_download: bool,
    pub cache_dir: PathBuf,
}

#[derive(Debug)]
pub enum Outcome {
    Installed(Vec<Imported>),
    UpToDate { source: String, version: String },
}

/// `fetch` is `None` in builds without network support.
pub fn update(
    source: &dyn Source,
    opts: &Options,
    fetch: Option<&dyn Fetch>,
    log: &mut dyn FnMut(&str),
) -> Result<Outcome> {
    let md5 = opts.md5.as_deref().map(parse_md5).transpose()?;

    if let Some(file) = opts.from {
        return verify_and_install(source, opts, file, md5.as_deref(), log);
    }
    let Some(url) = opts
        .url
        .map(str::to_owned)
        .or_else(|| source.info().download_url)
    else {
        bail!(
            "{} has no download location; use --url URL or --from FILE",
            source.info().slug
        );
    };
    let Some(fetch) = fetch else {
        bail!(
            "this build of gurd has no network support; download {url} yourself \
             and run `gurd update --from FILE`"
        );
    };

    fs::create_dir_all(&opts.cache_dir)
        .with_context(|| format!("cannot create {}", opts.cache_dir.display()))?;
    let file = opts.cache_dir.join(file_name(&url));
    let part = opts.cache_dir.join(format!("{}.part", file_name(&url)));
    log(&format!("downloading {url}"));
    let mut progress = Progress::default();
    let downloaded = fetch.download(&url, &part, &mut |done, total| {
        progress.report(done, total, log)
    });
    progress.finish(log);
    if let Err(err) = downloaded {
        let _ = fs::remove_file(&part);
        return Err(err);
    }
    if let Err(err) = fs::rename(&part, &file) {
        let _ = fs::remove_file(&part);
        return Err(err.into());
    }

    let result = verify_and_install(source, opts, &file, md5.as_deref(), log);
    if !opts.keep_download {
        let _ = fs::remove_file(&file);
    }
    result
}

fn verify_and_install(
    source: &dyn Source,
    opts: &Options,
    file: &Path,
    md5: Option<&str>,
    log: &mut dyn FnMut(&str),
) -> Result<Outcome> {
    if md5.is_some() && file.is_dir() {
        bail!(
            "--md5 checks a single file; {} is a directory",
            file.display()
        );
    }
    let checksum = match md5 {
        Some(expected) => {
            let actual = md5_file(file)?;
            if actual != expected {
                bail!(
                    "checksum mismatch for {}: expected MD5 {expected}, got {actual}",
                    file.display()
                );
            }
            log(&format!("MD5 verified: {actual}"));
            Some(format!("md5:{actual}"))
        }
        None => None,
    };

    let input = Input::open(file)?;
    if checksum.is_none() {
        let info = source.info();
        let hashes = if file.is_dir() {
            format!("SHA-256 of the directory listing {}", input.sha256)
        } else {
            format!("MD5 {}, SHA-256 {}", md5_file(file)?, input.sha256)
        };
        let place = match info.checksums_url {
            Some(u) => format!("; compare with the checksum published at {u}"),
            None if info.redistributable => "; the provider publishes no checksum".to_owned(),
            None => String::new(),
        };
        log(&format!("not verified (no --md5): {hashes}{place}"));
    }

    let release = source.release(&input)?;
    let slug = source.info().slug;
    if !opts.force && installed_version(opts.dest, &slug).as_ref() == Some(&release.version) {
        return Ok(Outcome::UpToDate {
            source: slug,
            version: release.version,
        });
    }

    // Every installed source is rebuilt together, so their versions are recorded side by
    // side in one database. The other sources are rebuilt from their stored releases.
    let others = stored_sources(opts.dest, Some(&slug), log)?;
    let staged = stage_release(opts.dest, &slug, file)?;
    let mut jobs = vec![Job {
        source,
        input: &input,
        upstream_checksum: checksum,
        retrieved_at: None,
    }];
    jobs.extend(others.iter().map(StoredSource::job));
    match import::install(opts.dest, &jobs) {
        Ok(imported) => {
            commit_release(opts.dest, &slug, &staged)?;
            Ok(Outcome::Installed(imported))
        }
        Err(err) => {
            let _ = remove_path(&staged);
            Err(err)
        }
    }
}

/// Removes one source: rebuilds the database from the other installed sources. Removing
/// the last source moves the database aside to `<db>.bak`.
pub fn remove(dest: &Path, slug: &str, log: &mut dyn FnMut(&str)) -> Result<Vec<Imported>> {
    if !stored_records(dest)?.iter().any(|r| r.slug == slug) {
        bail!("{slug} is not installed");
    }
    let others = stored_sources(dest, Some(slug), log)?;
    let imported = if others.is_empty() {
        let mut bak = dest.as_os_str().to_owned();
        bak.push(".bak");
        fs::rename(dest, PathBuf::from(bak))
            .with_context(|| format!("cannot move {} aside", dest.display()))?;
        Vec::new()
    } else {
        let jobs: Vec<Job> = others.iter().map(StoredSource::job).collect();
        import::install(dest, &jobs)?
    };
    let _ = remove_path(&releases_dir(dest).join(slug));
    Ok(imported)
}

/// Where the release files of installed sources are kept, so the database can be rebuilt
/// without downloading them again: `<db>.sources/<source>/<file>`.
pub fn releases_dir(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".sources");
    PathBuf::from(name)
}

/// An installed source, reopened from its stored release.
struct StoredSource {
    source: Box<dyn Source>,
    input: Input,
    record: StoredRecord,
}

impl StoredSource {
    fn job(&self) -> Job<'_> {
        Job {
            source: self.source.as_ref(),
            input: &self.input,
            upstream_checksum: self.record.upstream_checksum.clone(),
            retrieved_at: Some(self.record.retrieved_at.clone()),
        }
    }
}

struct StoredRecord {
    slug: String,
    file_name: String,
    upstream_checksum: Option<String>,
    retrieved_at: String,
}

/// Source records of the installed database, read without the schema-version check so an
/// older database can still be rebuilt. No database means no sources.
fn stored_records(dest: &Path) -> Result<Vec<StoredRecord>> {
    if !dest.exists() {
        return Ok(Vec::new());
    }
    let conn = Connection::open_with_flags(dest, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut stmt = conn.prepare(
        "SELECT slug, file_name, upstream_checksum, retrieved_at FROM sources ORDER BY slug",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(StoredRecord {
            slug: r.get(0)?,
            file_name: r.get(1)?,
            upstream_checksum: r.get(2)?,
            retrieved_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn stored_sources(
    dest: &Path,
    except: Option<&str>,
    log: &mut dyn FnMut(&str),
) -> Result<Vec<StoredSource>> {
    let records = match stored_records(dest) {
        Ok(r) => r,
        Err(err) => {
            log(&format!(
                "cannot read the installed database ({err}); it will be replaced"
            ));
            return Ok(Vec::new());
        }
    };
    let mut out = Vec::new();
    for record in records
        .into_iter()
        .filter(|r| Some(r.slug.as_str()) != except)
    {
        let Some(source) = crate::sources::find(&record.slug) else {
            bail!(
                "installed source {} is not supported by this build; remove it with `gurd remove {}`",
                record.slug,
                record.slug
            );
        };
        let path = releases_dir(dest)
            .join(&record.slug)
            .join(&record.file_name);
        if !path.exists() {
            bail!(
                "cannot rebuild {}: its release file is not stored at {}; \
                 reinstall it with `gurd update --source {} --from FILE` or remove it with \
                 `gurd remove {}`",
                record.slug,
                path.display(),
                record.slug,
                record.slug
            );
        }
        log(&format!(
            "rebuilding {} from {}",
            record.slug,
            path.display()
        ));
        let input = Input::open(&path)?;
        out.push(StoredSource {
            source,
            input,
            record,
        });
    }
    Ok(out)
}

/// Copies a release into `<releases>/<slug>.new/` ahead of the build.
fn stage_release(dest: &Path, slug: &str, file: &Path) -> Result<PathBuf> {
    let staged = releases_dir(dest).join(format!("{slug}.new"));
    let _ = remove_path(&staged);
    fs::create_dir_all(&staged).with_context(|| format!("cannot create {}", staged.display()))?;
    let name = file.file_name().unwrap_or_default();
    copy_tree(file, &staged.join(name))
        .with_context(|| format!("cannot store a copy of {}", file.display()))?;
    Ok(staged)
}

/// Makes a staged release the stored release of `slug`.
fn commit_release(dest: &Path, slug: &str, staged: &Path) -> Result<()> {
    let target = releases_dir(dest).join(slug);
    let _ = remove_path(&target);
    fs::rename(staged, &target).with_context(|| format!("cannot store {}", target.display()))
}

/// Copies a file or directory, hard-linking files where possible.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else if fs::hard_link(from, to).is_err() {
        fs::copy(from, to)?;
        // Keep the original time: some sources take their version from it.
        if let Ok(modified) = fs::metadata(from).and_then(|m| m.modified()) {
            let _ = fs::File::options()
                .write(true)
                .open(to)
                .and_then(|f| f.set_modified(modified));
        }
    }
    Ok(())
}

fn remove_path(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Installed release of `slug`, if a readable database exists.
fn installed_version(dest: &Path, slug: &str) -> Option<String> {
    let db = Database::open(dest).ok()?;
    let sources = db.sources().ok()?;
    sources
        .into_iter()
        .find(|s| s.slug == slug)
        .map(|s| s.version)
}

/// Last path segment of a URL, without query or fragment; safe as a file name.
fn file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let name = path.rsplit('/').next().unwrap_or("");
    let name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .collect();
    if name.is_empty() || name.starts_with('.') {
        "download".to_owned()
    } else {
        name
    }
}

fn parse_md5(s: &str) -> Result<String> {
    let s = s.trim().to_ascii_lowercase();
    if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("not an MD5 checksum (32 hex digits): {s}");
    }
    Ok(s)
}

/// Percent progress, at most one message per percent.
#[derive(Default)]
struct Progress {
    last: Option<u64>,
    shown: bool,
}

impl Progress {
    fn report(&mut self, done: u64, total: Option<u64>, log: &mut dyn FnMut(&str)) {
        let Some(total) = total.filter(|t| *t > 0) else {
            return;
        };
        let pct = done * 100 / total;
        if self.last != Some(pct) {
            self.last = Some(pct);
            self.shown = true;
            log(&format!(
                "\r{pct:3}% of {:.1} MB",
                total as f64 / 1_000_000.0
            ));
        }
    }

    fn finish(&mut self, log: &mut dyn FnMut(&str)) {
        if self.shown {
            log("\n");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_from_urls() {
        assert_eq!(
            file_name("https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_09082026.zip"),
            "RxNorm_full_prescribe_09082026.zip"
        );
        assert_eq!(file_name("https://x.org/a/b.zip?token=1#f"), "b.zip");
        assert_eq!(file_name("https://x.org/"), "download");
        assert_eq!(file_name("https://x.org/../.."), "download");
    }
}
