//! `drug update` against a fake network: every failure leaves the installed database
//! exactly as it was, and checksums are only what the user supplies.

mod common;

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use drug::database::Database;
use drug::sources::rxnorm::{CURRENT_URL, RxNorm};
use drug::sources::{Fetch, md5_file};
use drug::update::{Options, Outcome, update};

/// Serves one file for any URL (or fails); records requested URLs.
struct FakeNet {
    file: Option<PathBuf>,
    requests: RefCell<Vec<String>>,
}

impl FakeNet {
    fn serving(file: &Path) -> Self {
        Self {
            file: Some(file.to_owned()),
            requests: RefCell::default(),
        }
    }
    fn failing() -> Self {
        Self {
            file: None,
            requests: RefCell::default(),
        }
    }
}

impl Fetch for FakeNet {
    fn download(
        &self,
        url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()> {
        self.requests.borrow_mut().push(url.to_owned());
        let Some(file) = &self.file else {
            fs::write(dest, b"partial")?;
            bail!("connection reset");
        };
        let data = fs::read(file)?;
        fs::write(dest, &data)?;
        progress(data.len() as u64, Some(data.len() as u64));
        Ok(())
    }
}

struct Setup {
    dir: tempfile::TempDir,
    zip: PathBuf,
    db: PathBuf,
}

fn setup(with_db: bool) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    let zip = common::fixture_zip(&src, "RxNorm_full_prescribe_09082026.zip");
    let db = if with_db {
        common::fixture_db(dir.path())
    } else {
        dir.path().join("drug.db")
    };
    Setup { dir, zip, db }
}

fn options(s: &Setup) -> Options<'_> {
    Options {
        dest: &s.db,
        from: None,
        url: None,
        md5: None,
        force: false,
        keep_download: false,
        cache_dir: s.dir.path().join("cache"),
    }
}

fn run(opts: &Options, net: &FakeNet) -> (Result<Outcome>, Vec<String>) {
    let mut log = Vec::new();
    let r = update(&RxNorm, opts, Some(net), &mut |m| log.push(m.to_owned()));
    (r, log)
}

fn cache_is_empty(s: &Setup) -> bool {
    fs::read_dir(s.dir.path().join("cache"))
        .map(|mut d| d.next().is_none())
        .unwrap_or(true)
}

fn installed_checksum(s: &Setup) -> Option<String> {
    Database::open(&s.db).unwrap().sources().unwrap()[0]
        .upstream_checksum
        .clone()
}

#[test]
fn default_download_location_is_declared_by_the_source() {
    let s = setup(false);
    let net = FakeNet::serving(&s.zip);
    let (r, log) = run(&options(&s), &net);
    assert!(matches!(r.unwrap(), Outcome::Installed(_)));
    assert_eq!(*net.requests.borrow(), [CURRENT_URL]);
    assert!(cache_is_empty(&s), "download removed after install");
    // Unverified: the file's hashes are shown for the user to check, nothing is recorded.
    let md5 = md5_file(&s.zip).unwrap();
    assert!(
        log.iter()
            .any(|l| l.contains("not verified") && l.contains(&md5)),
        "{log:?}"
    );
    assert_eq!(installed_checksum(&s), None);
}

#[test]
fn user_supplied_url_and_md5_are_used() {
    let s = setup(false);
    let net = FakeNet::serving(&s.zip);
    let md5 = md5_file(&s.zip).unwrap();
    let url = "https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_09082026.zip";
    let opts = Options {
        url: Some(url),
        md5: Some(md5.to_uppercase()),
        ..options(&s)
    };
    let (r, log) = run(&opts, &net);
    assert!(matches!(r.unwrap(), Outcome::Installed(_)));
    assert_eq!(*net.requests.borrow(), [url]);
    assert!(log.iter().any(|l| l.contains("MD5 verified")));
    assert_eq!(installed_checksum(&s), Some(format!("md5:{md5}")));
}

#[test]
fn keep_download_keeps_file() {
    let s = setup(false);
    let opts = Options {
        keep_download: true,
        ..options(&s)
    };
    run(&opts, &FakeNet::serving(&s.zip)).0.unwrap();
    assert!(
        s.dir
            .path()
            .join("cache/RxNorm_full_prescribe_current.zip")
            .exists()
    );
}

#[test]
fn same_release_is_not_reinstalled_unless_forced() {
    let s = setup(true); // fixture release 2026-09-08 installed
    let before = fs::read(&s.db).unwrap();
    let (r, _) = run(&options(&s), &FakeNet::serving(&s.zip));
    assert!(matches!(r.unwrap(), Outcome::UpToDate { ref version, .. } if version == "2026-09-08"));
    assert_eq!(fs::read(&s.db).unwrap(), before);
    assert!(cache_is_empty(&s));

    let opts = Options {
        force: true,
        ..options(&s)
    };
    assert!(matches!(
        run(&opts, &FakeNet::serving(&s.zip)).0.unwrap(),
        Outcome::Installed(_)
    ));
}

#[test]
fn failed_download_keeps_database() {
    let s = setup(true);
    let before = fs::read(&s.db).unwrap();
    let (r, _) = run(&options(&s), &FakeNet::failing());
    assert!(format!("{:#}", r.unwrap_err()).contains("connection reset"));
    assert_eq!(fs::read(&s.db).unwrap(), before);
    assert!(cache_is_empty(&s), "partial download removed");
}

#[test]
fn checksum_mismatch_keeps_database() {
    let s = setup(true);
    let before = fs::read(&s.db).unwrap();
    let opts = Options {
        md5: Some("0".repeat(32)),
        force: true,
        ..options(&s)
    };
    let (r, _) = run(&opts, &FakeNet::serving(&s.zip));
    assert!(format!("{:#}", r.unwrap_err()).contains("checksum mismatch"));
    assert_eq!(fs::read(&s.db).unwrap(), before);
    assert!(cache_is_empty(&s));
}

#[test]
fn failed_import_keeps_database() {
    let s = setup(true);
    let before = fs::read(&s.db).unwrap();
    let bad = s.dir.path().join("src/bad.zip");
    common::make_zip(&bad, &[("hello.txt", b"not a release")]);
    assert!(run(&options(&s), &FakeNet::serving(&bad)).0.is_err());
    assert_eq!(fs::read(&s.db).unwrap(), before);
    assert!(cache_is_empty(&s));
}

#[test]
fn local_files_never_touch_the_network() {
    let s = setup(true);
    let before = fs::read(&s.db).unwrap();
    let net = FakeNet::failing();

    let wrong = Options {
        from: Some(&s.zip),
        md5: Some("0".repeat(32)),
        ..options(&s)
    };
    assert!(run(&wrong, &net).0.is_err());
    assert_eq!(fs::read(&s.db).unwrap(), before);

    let bad_format = Options {
        from: Some(&s.zip),
        md5: Some("xyz".into()),
        ..options(&s)
    };
    assert!(format!("{:#}", run(&bad_format, &net).0.unwrap_err()).contains("32 hex"));

    let md5 = md5_file(&s.zip).unwrap();
    let right = Options {
        from: Some(&s.zip),
        md5: Some(md5),
        force: true,
        ..options(&s)
    };
    assert!(matches!(
        run(&right, &net).0.unwrap(),
        Outcome::Installed(_)
    ));
    assert!(net.requests.borrow().is_empty());
}

#[test]
fn without_network_support_explains_manual_route() {
    let s = setup(false);
    let err = update(&RxNorm, &options(&s), None, &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("--from"), "{err}");
}
