use std::path::Path;
use std::process::{Command, Output};

fn drug(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_drug"))
        .args(args)
        .env("DRUG_DB", db)
        .env_remove("NO_COLOR")
        .output()
        .unwrap()
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap()
}

#[test]
fn version_and_help_succeed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("drug.db");
    let out = drug(&db, &["--version"]);
    assert_eq!(code(&out), 0);
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("drug "));
    assert_eq!(code(&drug(&db, &["--help"])), 0);
}

#[test]
fn usage_errors_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("drug.db");
    assert_eq!(code(&drug(&db, &[])), 2);
    assert_eq!(code(&drug(&db, &["--no-such-flag"])), 2);
    assert_eq!(code(&drug(&db, &["show"])), 2);
    assert_eq!(code(&drug(&db, &["--color", "sometimes", "database"])), 2);
}

#[test]
fn database_not_installed_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("drug.db");
    let out = drug(&db, &["database"]);
    assert_eq!(code(&out), 1);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("not installed"));
    assert!(stdout.contains(&db.display().to_string()));
}

#[test]
fn database_installed_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("drug.db");
    drop(drug::database::create(&db).unwrap());
    let out = drug(&db, &["database"]);
    assert_eq!(code(&out), 0, "{}", String::from_utf8_lossy(&out.stderr));
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("Schema    1")
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn database_json_is_valid_and_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("drug.db");
    drop(drug::database::create(&db).unwrap());
    // The global flag may come before or after the subcommand.
    for args in [["--json", "database"], ["database", "--json"]] {
        let out = drug(&db, &args);
        assert_eq!(code(&out), 0);
        assert!(out.stderr.is_empty());
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["json_version"], 1);
        assert_eq!(v["database"]["installed"], true);
        assert_eq!(v["database"]["schema_version"], 1);
        assert!(v["database"]["sources"].as_array().unwrap().is_empty());
    }
}

#[test]
fn explicit_db_flag_overrides_env() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real.db");
    drop(drug::database::create(&real).unwrap());
    let out = drug(
        &dir.path().join("absent.db"),
        &["--db", real.to_str().unwrap(), "database"],
    );
    assert_eq!(code(&out), 0);
}

#[test]
fn corrupt_database_is_an_error_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("drug.db");
    std::fs::write(&db, b"definitely not a sqlite database file").unwrap();
    let out = drug(&db, &["database"]);
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("drug: "));
}

mod common;

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    (dir, db)
}

#[test]
fn search_prints_results_and_exits_0() {
    let (_d, db) = fixture();
    // A partial name lists matches, one per line.
    let out = drug(&db, &["metfor"]);
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    let first = stdout.lines().next().unwrap();
    assert!(first.starts_with("rxcui:6809"), "{first}");
    assert!(first.ends_with("metformin"), "{first}");
    assert!(!stdout.contains('\x1b'), "no color when piped");

    // An exact name gets the summary card.
    let out = drug(&db, &["metformin"]);
    assert_eq!(code(&out), 0);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("metformin\n"), "{stdout}");
    assert!(stdout.contains("rxcui:6809"));
    assert!(!stdout.contains('\x1b'), "no color when piped");
}

#[test]
fn search_words_need_no_quotes() {
    let (_d, db) = fixture();
    let a = drug(&db, &["amoxicillin", "clavulanate"]);
    let b = drug(&db, &["amoxicillin clavulanate"]);
    assert_eq!(code(&a), 0);
    assert_eq!(a.stdout, b.stdout);
    assert_eq!(
        drug(&db, &["search", "amoxicillin", "clavulanate"]).stdout,
        a.stdout
    );
}

#[test]
fn search_without_match_exits_1() {
    let (_d, db) = fixture();
    let out = drug(&db, &["zzqqxx"]);
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no match"));
}

#[test]
fn search_json() {
    let (_d, db) = fixture();
    let out = drug(&db, &["metformin", "--json"]);
    assert_eq!(code(&out), 0);
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["json_version"], 1);
    assert_eq!(v["query"], "metformin");
    assert_eq!(v["sources"][0]["source"], "rxnorm");
    assert_eq!(v["sources"][0]["version"], "2026-09-08");
    let r = &v["results"][0];
    assert_eq!(r["code_system"], "rxcui");
    assert_eq!(r["code"], "6809");
    assert_eq!(r["name"], "metformin");
    assert_eq!(r["kind"], "ingredient");
    assert_eq!(r["source_type"], "IN");
    assert_eq!(r["match"], "exact");
    assert!(r.get("id").is_none(), "internal ids are not output");
}

#[test]
fn search_json_without_match_is_valid_and_exits_1() {
    let (_d, db) = fixture();
    let out = drug(&db, &["--json", "zzqqxx"]);
    assert_eq!(code(&out), 1);
    assert!(out.stderr.is_empty());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["results"].as_array().unwrap().is_empty());
}

#[test]
fn search_without_database_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let out = drug(&dir.path().join("none.db"), &["metformin"]);
    assert_eq!(code(&out), 1);
    assert!(String::from_utf8_lossy(&out.stderr).contains("drug update"));
}

#[test]
fn color_only_when_forced() {
    let (_d, db) = fixture();
    let out = drug(&db, &["--color", "always", "metformin"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains('\x1b'));
    let out = drug(&db, &["--color", "always", "--no-color", "metformin"]);
    assert!(!String::from_utf8_lossy(&out.stdout).contains('\x1b'));
}

#[test]
fn stale_release_is_disclosed() {
    let (_d, db) = fixture();
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute("UPDATE sources SET release_date = '2000-01-01'", [])
        .unwrap();
    drop(conn);
    for args in [
        &["database"][..],
        &["metformin"],
        &["sources"],
        &["show", "metformin"],
    ] {
        let out = drug(&db, args);
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(
            text.contains("may not reflect the provider's latest data"),
            "{args:?}: {text}"
        );
    }
}

#[test]
fn sources_lists_builtin_sources_even_without_database() {
    let dir = tempfile::tempdir().unwrap();
    let out = drug(&dir.path().join("none.db"), &["sources"]);
    assert_eq!(code(&out), 0);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        text.starts_with("rxnorm  RxNorm Current Prescribable Content"),
        "{text}"
    );
    assert!(text.contains("Installed  no"));
}

#[test]
fn class_without_class_source_exits_1() {
    let (_d, db) = fixture();
    let out = drug(&db, &["class", "metformin"]);
    assert_eq!(code(&out), 1);
    assert!(out.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no installed source provides drug classes")
    );
}
