//! `drug update`: obtain a release file, optionally verify it, and install a database
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
            "this build of drug has no network support; download {url} yourself \
             and run `drug update --from FILE`"
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
        let place = info
            .checksums_url
            .map(|u| format!(" with the checksum published at {u}"))
            .unwrap_or_default();
        log(&format!(
            "not verified (no --md5): MD5 {}, SHA-256 {}; compare{place}",
            md5_file(file)?,
            input.sha256
        ));
    }

    let release = source.release(&input)?;
    let slug = source.info().slug;
    if !opts.force && installed_version(opts.dest, &slug).as_ref() == Some(&release.version) {
        return Ok(Outcome::UpToDate {
            source: slug,
            version: release.version,
        });
    }

    let imported = import::install(
        opts.dest,
        &[Job {
            source,
            input: &input,
            upstream_checksum: checksum,
        }],
    )?;
    Ok(Outcome::Installed(imported))
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
