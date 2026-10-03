//! A failed update must never damage the installed database.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Result, bail};
use gurd::import::{self, ImportSink, Job};
use gurd::models::{Kind, NameType};
use gurd::sources::{Input, Release, Source, SourceInfo};
use rusqlite::Connection;

#[derive(Clone, Copy)]
enum Behaviour {
    Good,
    FailImport,
    ImportNothing,
    FailValidation,
}

struct Fake(Behaviour);

impl Source for Fake {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            slug: "fake".into(),
            title: "Fake".into(),
            code_system: "fake".into(),
            provider: "Tests".into(),
            license: "n/a".into(),
            attribution: "n/a".into(),
            url: "https://example.invalid/".into(),
            redistributable: false,
            stale_after_days: None,
            download_url: None,
            checksums_url: None,
            notice: None,
        }
    }
    fn release(&self, _: &Input) -> Result<Release> {
        Ok(Release {
            version: "1".into(),
            release_date: None,
        })
    }
    fn import(&self, _: &Input, sink: &mut ImportSink) -> Result<()> {
        match self.0 {
            Behaviour::ImportNothing => return Ok(()),
            Behaviour::FailImport => {
                sink.concept("1", Kind::Ingredient, "X", "half-imported")?;
                bail!("simulated import failure");
            }
            _ => {}
        }
        let id = sink.concept("1", Kind::Ingredient, "X", "testium")?;
        sink.name(id, "testium", NameType::Preferred, "X", None)
    }
    fn validate(&self, _: &Connection, _: i64) -> Result<()> {
        match self.0 {
            Behaviour::FailValidation => bail!("simulated validation failure"),
            _ => Ok(()),
        }
    }
}

fn input(dir: &Path) -> Input {
    let zip = dir.join("in.zip");
    common::make_zip(&zip, &[("x", b"x")]);
    Input::open(&zip).unwrap()
}

fn install(dest: &Path, dir: &Path, b: Behaviour) -> Result<()> {
    let input = input(dir);
    import::install(
        dest,
        &[Job {
            source: &Fake(b),
            input: &input,
            upstream_checksum: None,
            retrieved_at: None,
        }],
    )
    .map(drop)
}

/// Only the database (and possibly its backup) may remain next to it.
fn assert_no_leftovers(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        assert!(!name.contains(".tmp-"), "leftover temporary file {name}");
    }
}

#[test]
fn successful_update_replaces_database_and_keeps_backup() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let before = fs::read(&db).unwrap();

    install(&db, dir.path(), Behaviour::Good).unwrap();

    let sources = gurd::database::Database::open(&db)
        .unwrap()
        .sources()
        .unwrap();
    assert_eq!(sources[0].slug, "fake");
    assert_eq!(fs::read(dir.path().join("gurd.db.bak")).unwrap(), before);
    assert_no_leftovers(dir.path());
}

#[test]
fn failures_leave_installed_database_untouched() {
    for behaviour in [
        Behaviour::FailImport,
        Behaviour::ImportNothing,
        Behaviour::FailValidation,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = common::fixture_db(dir.path());
        let before = fs::read(&db).unwrap();

        assert!(install(&db, dir.path(), behaviour).is_err());

        assert_eq!(fs::read(&db).unwrap(), before);
        assert!(!dir.path().join("gurd.db.bak").exists());
        assert_no_leftovers(dir.path());
        gurd::database::Database::open(&db).unwrap();
    }
}

#[test]
fn failed_first_install_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("gurd.db");
    assert!(install(&db, dir.path(), Behaviour::FailValidation).is_err());
    assert!(!db.exists());
    assert_no_leftovers(dir.path());
}

#[test]
fn cli_update_with_bad_input_keeps_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let before = fs::read(&db).unwrap();

    let junk = dir.path().join("junk.zip");
    fs::write(&junk, b"this is not a zip file").unwrap();
    let wrong = dir.path().join("wrong.zip");
    common::make_zip(&wrong, &[("hello.txt", b"hi")]);

    for file in [&junk, &wrong, &dir.path().join("missing.zip")] {
        let out = Command::new(env!("CARGO_BIN_EXE_gurd"))
            .args([
                "--db",
                db.to_str().unwrap(),
                "update",
                "--from",
                file.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(1),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty());
        assert_eq!(fs::read(&db).unwrap(), before);
    }
    assert_no_leftovers(dir.path());
}

#[test]
fn cli_update_installs_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let zip = common::fixture_zip(dir.path(), "RxNorm_full_prescribe_09082026.zip");
    let db = dir.path().join("sub/dir/gurd.db");
    let out = Command::new(env!("CARGO_BIN_EXE_gurd"))
        .args([
            "--db",
            db.to_str().unwrap(),
            "--json",
            "update",
            "--from",
            zip.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["imported"][0]["source"], "rxnorm");
    assert_eq!(v["imported"][0]["version"], "2026-09-08");
    assert_eq!(v["imported"][0]["concepts"], 24);
    assert!(db.exists());
}

#[test]
fn unknown_source_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_gurd"))
        .args([
            "--db",
            dir.path().join("x.db").to_str().unwrap(),
            "update",
            "--source",
            "nope",
            "--from",
            "x",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown source"));
}

struct BadNavigation;

impl Source for BadNavigation {
    fn info(&self) -> SourceInfo {
        Fake(Behaviour::Good).info()
    }
    fn release(&self, input: &Input) -> Result<Release> {
        Fake(Behaviour::Good).release(input)
    }
    fn import(&self, input: &Input, sink: &mut ImportSink) -> Result<()> {
        Fake(Behaviour::Good).import(input, sink)?;
        sink.navigation(
            Kind::Ingredient,
            gurd::models::Section::Brands,
            &["no_such_predicate"],
            Kind::BrandName,
        )
    }
}

#[test]
fn navigation_with_unknown_predicate_fails_validation() {
    let dir = tempfile::tempdir().unwrap();
    let input = input(dir.path());
    let err = import::install(
        &dir.path().join("gurd.db"),
        &[Job {
            source: &BadNavigation,
            input: &input,
            upstream_checksum: None,
            retrieved_at: None,
        }],
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("unknown predicate"), "{err:#}");
}
