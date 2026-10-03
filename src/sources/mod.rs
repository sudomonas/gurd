//! Dataset adapters.
//!
//! Each adapter turns one dataset's native format into rows of the application's
//! source-neutral schema through an [`ImportSink`]. Nothing outside this module knows
//! about any dataset's file format or vocabulary.

pub mod azindia;
pub mod onemg;
pub mod openfda;
pub mod rxnorm;
pub mod rxterms;

use std::cell::RefCell;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
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
    /// Caveat shown wherever this source's data is displayed.
    pub notice: Option<String>,
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
    vec![
        Box::new(rxnorm::RxNorm),
        Box::new(rxterms::RxTerms),
        Box::new(openfda::OpenFda),
        Box::new(onemg::OneMg),
        Box::new(azindia::AzIndia),
    ]
}

/// A code made from a name: lowercase ASCII letters and digits, other runs replaced by
/// `-`. Used as the source code of concepts a dataset names but does not number.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

pub fn find(slug: &str) -> Option<Box<dyn Source>> {
    builtin().into_iter().find(|s| s.info().slug == slug)
}

/// A release as given to `gurd update`: a zip archive, a single file, or a directory of
/// files. Adapters see it as a set of named members either way.
pub struct Input {
    pub path: PathBuf,
    pub file_name: String,
    /// SHA-256 of the file; for a directory, of the sorted list of member names and their
    /// SHA-256 sums.
    pub sha256: String,
    /// Modification time of the file (newest member for a directory), seconds since the
    /// Unix epoch.
    pub modified: Option<u64>,
    kind: InputKind,
}

enum InputKind {
    Zip(RefCell<ZipArchive<File>>),
    /// Member names relative to the directory, sorted.
    Dir(Vec<String>),
    File,
}

impl Input {
    pub fn open(path: &Path) -> Result<Self> {
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            let members = list_dir(path)?;
            if members.is_empty() {
                bail!("{} is an empty directory", path.display());
            }
            let mut listing = String::new();
            let mut modified = None;
            for m in &members {
                let p = path.join(m);
                listing.push_str(&format!("{m}\0{}\n", sha256_file(&p)?));
                modified = modified.max(mtime(&p));
            }
            let sha256 = hex(&Sha256::digest(listing.as_bytes()));
            return Ok(Self {
                path: path.to_owned(),
                file_name,
                sha256,
                modified,
                kind: InputKind::Dir(members),
            });
        }
        let sha256 = sha256_file(path)?;
        let mut file =
            File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        let mut magic = [0u8; 4];
        let is_zip = file.read_exact(&mut magic).is_ok() && magic == *b"PK\x03\x04";
        file.seek(SeekFrom::Start(0))?;
        let kind = if is_zip {
            let archive = ZipArchive::new(file)
                .with_context(|| format!("{} is not a valid zip archive", path.display()))?;
            InputKind::Zip(RefCell::new(archive))
        } else {
            InputKind::File
        };
        Ok(Self {
            path: path.to_owned(),
            file_name,
            sha256,
            modified: mtime(path),
            kind,
        })
    }

    pub fn is_archive(&self) -> bool {
        matches!(self.kind, InputKind::Zip(_))
    }

    pub fn member_names(&self) -> Vec<String> {
        match &self.kind {
            InputKind::Zip(a) => a.borrow().file_names().map(str::to_owned).collect(),
            InputKind::Dir(members) => members.clone(),
            InputKind::File => vec![self.file_name.clone()],
        }
    }

    /// Finds a member by exact path or by trailing path component(s), e.g. `rrf/RXNREL.RRF`.
    pub fn find_member(&self, name: &str) -> Option<String> {
        let suffix = format!("/{name}");
        self.member_names()
            .into_iter()
            .find(|m| m == name || m.ends_with(&suffix))
    }

    /// Streams one member through `f`.
    pub fn read<T>(&self, name: &str, f: impl FnOnce(&mut dyn BufRead) -> Result<T>) -> Result<T> {
        let Some(member) = self.find_member(name) else {
            bail!("{} does not contain {name}", self.file_name);
        };
        let context = || format!("while reading {member}");
        match &self.kind {
            InputKind::Zip(archive) => {
                let mut archive = archive.borrow_mut();
                let entry = archive.by_name(&member)?;
                let mut reader = BufReader::with_capacity(1 << 16, entry);
                f(&mut reader).with_context(context)
            }
            InputKind::Dir(_) | InputKind::File => {
                let path = match self.kind {
                    InputKind::Dir(_) => self.path.join(&member),
                    _ => self.path.clone(),
                };
                let file =
                    File::open(&path).with_context(|| format!("cannot open {}", path.display()))?;
                let mut reader = BufReader::with_capacity(1 << 16, file);
                f(&mut reader).with_context(context)
            }
        }
    }

    /// Modification date of the input as `YYYY-MM-DD`, for datasets that carry no version
    /// of their own.
    pub fn file_date(&self) -> Option<String> {
        self.modified.map(|secs| civil_date(secs as i64))
    }
}

fn list_dir(root: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).with_context(|| format!("cannot read {}", dir.display()))? {
            let path = entry?.path();
            let hidden = path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'));
            if hidden {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn mtime(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// `YYYY-MM-DD` (UTC) for seconds since the Unix epoch.
fn civil_date(secs: i64) -> String {
    // Howard Hinnant's days-to-civil algorithm.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Calls `f` with the fields of each record of a CSV stream (RFC 4180: quoted fields may
/// contain commas, doubled quotes and line breaks). The first record is the header.
pub fn each_csv_record(
    reader: &mut dyn BufRead,
    mut f: impl FnMut(u64, &[String]) -> Result<()>,
) -> Result<()> {
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut line = String::new();
    let mut number = 0u64;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            if quoted {
                bail!("record {}: unterminated quoted field", number + 1);
            }
            if !field.is_empty() || !record.is_empty() {
                record.push(std::mem::take(&mut field));
                number += 1;
                f(number, &record)?;
            }
            return Ok(());
        }
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match (quoted, c) {
                (true, '"') if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                (true, '"') => quoted = false,
                (true, c) => field.push(c),
                (false, '"') if field.is_empty() => quoted = true,
                (false, ',') => record.push(std::mem::take(&mut field)),
                (false, '\n') => {}
                (false, '\r') if chars.peek() == Some(&'\n') => {}
                (false, c) => field.push(c),
            }
        }
        if !quoted {
            record.push(std::mem::take(&mut field));
            number += 1;
            if !(record.len() == 1 && record[0].is_empty()) {
                f(number, &record).with_context(|| format!("record {number}"))?;
            }
            record.clear();
        }
    }
}

/// Calls `f` with each non-empty line of a JSON-lines stream, parsed into `T`.
pub fn each_json_line<T: serde::de::DeserializeOwned>(
    reader: &mut dyn BufRead,
    mut f: impl FnMut(T) -> Result<()>,
) -> Result<()> {
    let mut line = String::new();
    let mut number = 0u64;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        number += 1;
        if line.trim().is_empty() {
            continue;
        }
        let value: T =
            serde_json::from_str(&line).with_context(|| format!("line {number}: invalid JSON"))?;
        f(value).with_context(|| format!("line {number}"))?;
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
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
