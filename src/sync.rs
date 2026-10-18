//! Rolling database sync: downloads prebuilt `gurd.db.zip` and verifies it against `manifest.json`.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use crate::database::Database;
use crate::sources::Fetch;

pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/sudomonas/gurd/releases/download/database/manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub built_at: String,
    pub schema_version: u32,
    pub sha256: String,
    pub archive: String,
    #[serde(default)]
    pub archive_size: Option<u64>,
    #[serde(default)]
    pub concepts_count: Option<u64>,
    pub sources: Vec<ManifestSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestSource {
    pub slug: String,
    pub version: String,
    #[serde(default)]
    pub release_date: Option<String>,
}

#[derive(Debug)]
pub enum SyncOutcome {
    UpToDate {
        version: String,
        sources: Vec<ManifestSource>,
    },
    Updated {
        version: String,
        previous_version: Option<String>,
        concepts_count: Option<u64>,
        sources: Vec<ManifestSource>,
    },
}

pub struct SyncOptions<'a> {
    pub dest: &'a Path,
    pub url: Option<&'a str>,
    pub force: bool,
    pub keep_download: bool,
    pub cache_dir: PathBuf,
}

pub fn sync(
    opts: &SyncOptions,
    fetch: Option<&dyn Fetch>,
    log: &mut dyn FnMut(&str),
) -> Result<SyncOutcome> {
    let Some(fetch) = fetch else {
        bail!("this build of gurd was compiled without network support (net feature disabled)");
    };

    let manifest_url = opts.url.unwrap_or(DEFAULT_MANIFEST_URL);
    fs::create_dir_all(&opts.cache_dir)
        .with_context(|| format!("cannot create cache directory {}", opts.cache_dir.display()))?;

    let manifest_file = opts.cache_dir.join("manifest.json");
    log("checking for database updates...");
    fetch
        .download(manifest_url, &manifest_file, &mut |_, _| {})
        .with_context(|| format!("cannot download database manifest from {manifest_url}"))?;

    let manifest_bytes = fs::read(&manifest_file)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| "invalid manifest JSON returned by release server")?;

    // Check if installed database is already up to date
    if opts.dest.exists() && !opts.force {
        if let Ok(db) = Database::open(opts.dest) {
            if let Ok(sources) = db.sources() {
                let all_match = !sources.is_empty()
                    && manifest.sources.iter().all(|ms| {
                        sources
                            .iter()
                            .any(|s| s.slug == ms.slug && s.version == ms.version)
                    });
                if all_match {
                    return Ok(SyncOutcome::UpToDate {
                        version: manifest.version,
                        sources: manifest.sources,
                    });
                }
            }
        }
    }

    // Determine archive URL (relative to manifest or absolute)
    let archive_url = if manifest.archive.starts_with("http://")
        || manifest.archive.starts_with("https://")
    {
        manifest.archive.clone()
    } else if let Some(slash_idx) = manifest_url.rfind('/') {
        format!("{}/{}", &manifest_url[..slash_idx], manifest.archive)
    } else {
        manifest.archive.clone()
    };

    let archive_dest = opts.cache_dir.join(&manifest.archive);
    let part_dest = opts.cache_dir.join(format!("{}.part", manifest.archive));

    let size_desc = manifest
        .archive_size
        .map(|s| format!(" ({:.1} MB)", s as f64 / 1_000_000.0))
        .unwrap_or_default();
    log(&format!(
        "downloading database release {}{size_desc}...",
        manifest.version
    ));

    let mut last_mb = 0;
    fetch.download(
        &archive_url,
        &part_dest,
        &mut |downloaded, total| {
            let cur_mb = downloaded / 1_000_000;
            if cur_mb != last_mb {
                last_mb = cur_mb;
                if let Some(tot) = total {
                    let pct = (downloaded as f64 / tot as f64 * 100.0) as u32;
                    log(&format!(
                        "\r  downloaded {:.1} MB / {:.1} MB ({pct}%)",
                        downloaded as f64 / 1_000_000.0,
                        tot as f64 / 1_000_000.0
                    ));
                } else {
                    log(&format!(
                        "\r  downloaded {:.1} MB",
                        downloaded as f64 / 1_000_000.0
                    ));
                }
            }
        },
    )?;
    log("\n");

    // Verify SHA-256 of the downloaded archive
    log("verifying checksum...");
    let hex_sha = crate::sources::sha256_file(&part_dest)?;
    if !manifest.sha256.is_empty() && hex_sha != manifest.sha256 {
        let _ = fs::remove_file(&part_dest);
        bail!(
            "checksum mismatch for {archive_url}:\n  expected: {}\n  actual:   {}",
            manifest.sha256,
            hex_sha
        );
    }

    fs::rename(&part_dest, &archive_dest)?;

    // Extract gurd.db from archive
    log("extracting database...");
    let tmp_dest = sibling(opts.dest, &format!("tmp-sync-{}", std::process::id()));
    let _ = fs::remove_file(&tmp_dest);

    let zip_file = File::open(&archive_dest)?;
    let mut zip = ZipArchive::new(zip_file).context("corrupt zip archive")?;
    let mut db_entry = zip
        .by_name("gurd.db")
        .context("archive does not contain 'gurd.db'")?;

    if let Some(parent) = tmp_dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out_file = File::create(&tmp_dest)?;
    io::copy(&mut db_entry, &mut out_file)?;
    drop(out_file);
    drop(db_entry);
    drop(zip);

    // Validate extracted database
    log("validating database...");
    let opened = Database::open(&tmp_dest)
        .context("extracted database failed schema/integrity validation")?;
    let previous_version = if opts.dest.exists() {
        Database::open(opts.dest).ok().and_then(|db| {
            db.sources()
                .ok()
                .and_then(|s| s.first().map(|src| src.version.clone()))
        })
    } else {
        None
    };
    drop(opened);

    // Atomically swap into destination
    replace_file(&tmp_dest, opts.dest)?;

    if !opts.keep_download {
        let _ = fs::remove_file(&archive_dest);
    }
    let _ = fs::remove_file(&manifest_file);

    Ok(SyncOutcome::Updated {
        version: manifest.version,
        previous_version,
        concepts_count: manifest.concepts_count,
        sources: manifest.sources,
    })
}

fn replace_file(tmp: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        let bak = sibling(dest, "bak");
        let _ = fs::remove_file(&bak);
        if fs::hard_link(dest, &bak).is_err() {
            let _ = fs::copy(dest, &bak);
        }
    }
    fs::rename(tmp, dest)
        .with_context(|| format!("cannot move new database to {}", dest.display()))?;
    Ok(())
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}
