//! Dataset adapters.
//!
//! Each adapter turns one dataset's native format into rows of the application's
//! source-neutral schema through an [`ImportSink`]. Nothing outside this module knows
//! about any dataset's file format or vocabulary.

pub mod rxnorm;

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use zip::ZipArchive;

pub use crate::import::ImportSink;

/// Static description of a dataset.
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub slug: String,
    pub title: String,
    /// Identifier system of the source's own concept codes, e.g. `rxcui`.
    pub code_system: String,
    pub provider: String,
    pub license: String,
    pub attribution: String,
    pub url: String,
    pub redistributable: bool,
    /// Age in days after which an installed release is reported as outdated.
    pub stale_after_days: Option<u32>,
    /// Where `gurd update` downloads the release from by default, if anywhere.
    pub download_url: Option<String>,
    /// Where the provider publishes checksums, for the user to compare against.
    pub checksums_url: Option<String>,
}

/// What a particular input file says about its release.
#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub release_date: Option<String>,
}

pub trait Source {
    fn info(&self) -> SourceInfo;

    /// Reads release metadata from the input without importing it.
    fn release(&self, input: &Input) -> Result<Release>;

    /// Writes the dataset into the sink.
    fn import(&self, input: &Input, sink: &mut ImportSink) -> Result<()>;

    /// Source-specific sanity checks on the imported data. Failing here aborts the
    /// update and leaves the installed database untouched.
    fn validate(&self, _conn: &Connection, _source_id: i64) -> Result<()> {
        Ok(())
    }
}

/// Network access for `gurd update`. Implemented by `net::Http` in builds with the
/// `net` feature, and by fakes in tests. Adapters never see it.
pub trait Fetch {
    /// GETs `url` into `dest`, reporting (bytes so far, total if known).
    fn download(
        &self,
        url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()>;
}

/// Adapters compiled into this binary.
pub fn builtin() -> Vec<Box<dyn Source>> {
    vec![Box::new(rxnorm::RxNorm)]
}

pub fn find(slug: &str) -> Option<Box<dyn Source>> {
    builtin().into_iter().find(|s| s.info().slug == slug)
}

/// A downloaded release archive.
pub struct Input {
    pub path: PathBuf,
    pub file_name: String,
    pub sha256: String,
    /// Modification time of the file, seconds since the Unix epoch.
    pub modified: Option<u64>,
    archive: RefCell<ZipArchive<File>>,
}

impl Input {
    pub fn open(path: &Path) -> Result<Self> {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let sha256 = sha256_file(path)?;
        let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let modified = file
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let archive = ZipArchive::new(file)
            .with_context(|| format!("{} is not a zip archive", path.display()))?;
        Ok(Self {
            path: path.to_owned(),
            file_name,
            sha256,
            modified,
            archive: RefCell::new(archive),
        })
    }

    pub fn member_names(&self) -> Vec<String> {
        self.archive
            .borrow()
            .file_names()
            .map(str::to_owned)
            .collect()
    }

    /// Finds a member by exact path or by trailing path component(s), e.g. `rrf/RXNREL.RRF`.
    pub fn find_member(&self, name: &str) -> Option<String> {
        let suffix = format!("/{name}");
        self.member_names()
            .into_iter()
            .find(|m| m == name || m.ends_with(&suffix))
    }

    /// Streams one archive member through `f`.
    pub fn read<T>(&self, name: &str, f: impl FnOnce(&mut dyn BufRead) -> Result<T>) -> Result<T> {
        let Some(member) = self.find_member(name) else {
            bail!("{} does not contain {name}", self.file_name);
        };
        let mut archive = self.archive.borrow_mut();
        let entry = archive.by_name(&member)?;
        let mut reader = BufReader::with_capacity(1 << 16, entry);
        f(&mut reader).with_context(|| format!("while reading {member}"))
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    hash_file::<Sha256>(path)
}

pub fn md5_file(path: &Path) -> Result<String> {
    hash_file::<md5::Md5>(path)
}

fn hash_file<D: Digest>(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut hasher = D::new();
    let mut buf = vec![0; 1 << 16];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
