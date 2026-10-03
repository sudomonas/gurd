#![allow(dead_code)]

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

pub const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/rxnorm-mini");

/// Zips the RxNorm fixture into `dir` under `name`, laid out like the NLM release.
pub fn fixture_zip(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let mut zip = ZipWriter::new(File::create(&path).unwrap());
    let root = Path::new(FIXTURE);
    for rel in [
        "rrf/RXNCONSO.RRF",
        "rrf/RXNREL.RRF",
        "rrf/RXNSAT.RRF",
        "Readme_Full_Prescribe_09082026.txt",
    ] {
        zip.start_file(rel, SimpleFileOptions::default()).unwrap();
        zip.write_all(&fs::read(root.join(rel)).unwrap()).unwrap();
    }
    zip.finish().unwrap();
    path
}

/// A zip with arbitrary members.
pub fn make_zip(path: &Path, members: &[(&str, &[u8])]) {
    let mut zip = ZipWriter::new(File::create(path).unwrap());
    for (name, data) in members {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

/// Builds an installed database from the fixture at `dir/drug.db`.
pub fn fixture_db(dir: &Path) -> PathBuf {
    let zip = fixture_zip(dir, "RxNorm_full_prescribe_09082026.zip");
    let db = dir.join("drug.db");
    let input = drug::sources::Input::open(&zip).unwrap();
    drug::import::install(
        &db,
        &[drug::import::Job {
            source: &drug::sources::rxnorm::RxNorm,
            input: &input,
            upstream_checksum: None,
        }],
    )
    .unwrap();
    db
}
