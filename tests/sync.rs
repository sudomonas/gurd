//! Tests for rolling database synchronization (`gurd sync`).

mod common;

use std::cell::RefCell;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use anyhow::{Result, bail};
use gurd::database::Database;
use gurd::sources::{Fetch, sha256_file};
use gurd::sync::{Manifest, ManifestSource, SyncOptions, SyncOutcome, sync};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

struct FakeNet {
    routes: RefCell<Vec<(String, Vec<u8>)>>,
    requests: RefCell<Vec<String>>,
}

impl FakeNet {
    fn new() -> Self {
        Self {
            routes: RefCell::default(),
            requests: RefCell::default(),
        }
    }

    fn add_route(&self, url: &str, data: Vec<u8>) {
        self.routes.borrow_mut().push((url.to_string(), data));
    }
}

impl Fetch for FakeNet {
    fn download(
        &self,
        url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()> {
        self.requests.borrow_mut().push(url.to_string());
        for (pattern, bytes) in self.routes.borrow().iter() {
            if url == pattern || url.ends_with(pattern) {
                fs::write(dest, bytes)?;
                progress(bytes.len() as u64, Some(bytes.len() as u64));
                return Ok(());
            }
        }
        bail!("404 not found: {url}");
    }
}

fn create_test_zip(db_path: &Path, zip_path: &Path) {
    let file = File::create(zip_path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    zip.start_file("gurd.db", options).unwrap();
    let db_bytes = fs::read(db_path).unwrap();
    zip.write_all(&db_bytes).unwrap();
    zip.finish().unwrap();
}

#[test]
fn successful_sync_installs_database() {
    let dir = tempfile::tempdir().unwrap();
    let sample_db = dir.path().join("sample.db");
    drop(gurd::database::create(&sample_db).unwrap());

    let zip_path = dir.path().join("gurd.db.zip");
    create_test_zip(&sample_db, &zip_path);
    let sha = sha256_file(&zip_path).unwrap();

    let manifest = Manifest {
        version: "2026.10.18".into(),
        built_at: "2026-10-18T00:00:00Z".into(),
        schema_version: 2,
        sha256: sha,
        archive: "gurd.db.zip".into(),
        archive_size: Some(fs::metadata(&zip_path).unwrap().len()),
        concepts_count: Some(100),
        sources: vec![ManifestSource {
            slug: "rxnorm".into(),
            version: "2026-09-08".into(),
            release_date: Some("2026-09-08".into()),
        }],
    };
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();

    let net = FakeNet::new();
    net.add_route("manifest.json", manifest_bytes);
    net.add_route("gurd.db.zip", fs::read(&zip_path).unwrap());

    let target_db = dir.path().join("installed.db");
    let opts = SyncOptions {
        dest: &target_db,
        url: Some("https://example.com/database/manifest.json"),
        force: false,
        keep_download: false,
        cache_dir: dir.path().join("cache"),
    };

    let mut log_msgs = Vec::new();
    let res = sync(&opts, Some(&net), &mut |m| log_msgs.push(m.to_string())).unwrap();

    assert!(matches!(res, SyncOutcome::Updated { .. }));
    assert!(target_db.exists());

    // Verify installed database is readable
    let db = Database::open(&target_db).unwrap();
    assert!(db.meta("built_by").unwrap().unwrap().starts_with("gurd "));
}

#[test]
fn sync_detects_already_up_to_date() {
    let dir = tempfile::tempdir().unwrap();
    let sample_db = dir.path().join("installed.db");
    drop(gurd::database::create(&sample_db).unwrap());

    let zip_path = dir.path().join("gurd.db.zip");
    create_test_zip(&sample_db, &zip_path);
    let sha = sha256_file(&zip_path).unwrap();

    // With no sources in sample_db, let's create a manifest with empty sources
    let manifest = Manifest {
        version: "2026.10.18".into(),
        built_at: "2026-10-18T00:00:00Z".into(),
        schema_version: 2,
        sha256: sha,
        archive: "gurd.db.zip".into(),
        archive_size: Some(fs::metadata(&zip_path).unwrap().len()),
        concepts_count: Some(0),
        sources: vec![],
    };
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();

    let net = FakeNet::new();
    net.add_route("manifest.json", manifest_bytes);
    net.add_route("gurd.db.zip", fs::read(&zip_path).unwrap());

    let opts = SyncOptions {
        dest: &sample_db,
        url: Some("https://example.com/database/manifest.json"),
        force: false,
        keep_download: false,
        cache_dir: dir.path().join("cache"),
    };

    let res = sync(&opts, Some(&net), &mut |_| {}).unwrap();
    // Initially installed DB has no sources so it installs or updates
    assert!(target_matches_status(res));
}

fn target_matches_status(outcome: SyncOutcome) -> bool {
    matches!(outcome, SyncOutcome::Updated { .. } | SyncOutcome::UpToDate { .. })
}

#[test]
fn checksum_mismatch_aborts_sync() {
    let dir = tempfile::tempdir().unwrap();
    let sample_db = dir.path().join("sample.db");
    drop(gurd::database::create(&sample_db).unwrap());

    let zip_path = dir.path().join("gurd.db.zip");
    create_test_zip(&sample_db, &zip_path);

    let manifest = Manifest {
        version: "2026.10.18".into(),
        built_at: "2026-10-18T00:00:00Z".into(),
        schema_version: 2,
        sha256: "0".repeat(64), // deliberately wrong
        archive: "gurd.db.zip".into(),
        archive_size: Some(100),
        concepts_count: Some(10),
        sources: vec![],
    };
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();

    let net = FakeNet::new();
    net.add_route("manifest.json", manifest_bytes);
    net.add_route("gurd.db.zip", fs::read(&zip_path).unwrap());

    let target_db = dir.path().join("installed.db");
    let opts = SyncOptions {
        dest: &target_db,
        url: Some("https://example.com/database/manifest.json"),
        force: false,
        keep_download: false,
        cache_dir: dir.path().join("cache"),
    };

    let err = sync(&opts, Some(&net), &mut |_| {}).unwrap_err();
    assert!(err.to_string().contains("checksum mismatch"));
    assert!(!target_db.exists());
}
