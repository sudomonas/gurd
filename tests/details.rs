mod common;

use std::path::Path;
use std::process::{Command, Output};

use gurd::database::Database;
use gurd::details;
use gurd::models::Section;
use gurd::search::search;

fn names(items: &[gurd::models::ConceptRef]) -> Vec<&str> {
    items.iter().map(|c| c.name.as_str()).collect()
}

fn concept(db: &Database, rxcui: &str) -> gurd::models::ConceptRef {
    details::by_identifier(db, "rxcui", rxcui)
        .unwrap()
        .remove(0)
}

fn run(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gurd"))
        .args(args)
        .env("GURD_DB", db)
        .env("PAGER", "false") // would swallow output if (wrongly) used when piped
        .output()
        .unwrap()
}

#[test]
fn ingredient_sections() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&common::fixture_db(dir.path())).unwrap();
    let s = details::sections(&db, &concept(&db, "6809")).unwrap();
    assert_eq!(names(&s[&Section::Brands]), ["Glucophage"]);
    assert_eq!(
        names(&s[&Section::PreciseIngredients]),
        ["metformin hydrochloride"]
    );
    assert_eq!(
        names(&s[&Section::ClinicalDrugs]),
        ["metformin hydrochloride 500 MG Oral Tablet"]
    );
}

#[test]
fn brand_shows_ingredients_and_products() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&common::fixture_db(dir.path())).unwrap();
    let s = details::sections(&db, &concept(&db, "151392")).unwrap();
    assert_eq!(
        names(&s[&Section::Ingredients]),
        ["amoxicillin", "clavulanate"]
    );
    assert_eq!(
        names(&s[&Section::BrandedDrugs]),
        ["amoxicillin 875 MG / clavulanate 125 MG Oral Tablet [Augmentin]"]
    );
}

#[test]
fn branded_drug_links_back_to_brand_ingredients_and_generic() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&common::fixture_db(dir.path())).unwrap();
    let s = details::sections(&db, &concept(&db, "824194")).unwrap();
    assert_eq!(names(&s[&Section::Brands]), ["Augmentin"]);
    assert_eq!(
        names(&s[&Section::Ingredients]),
        ["amoxicillin", "clavulanate"]
    );
    assert_eq!(
        names(&s[&Section::ClinicalDrugs]),
        ["amoxicillin 875 MG / clavulanate 125 MG Oral Tablet"]
    );
    assert_eq!(names(&s[&Section::DoseForms]), ["Oral Tablet"]);
}

#[test]
fn sections_only_contain_what_the_graph_says() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&common::fixture_db(dir.path())).unwrap();
    // aspirin is in the fixture with no related concepts: no sections at all.
    let s = details::sections(&db, &concept(&db, "1191")).unwrap();
    assert!(s.is_empty(), "{s:?}");
}

#[test]
fn full_details() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&common::fixture_db(dir.path())).unwrap();
    let d = details::details(&db, concept(&db, "6809")).unwrap();
    assert!(
        d.names
            .iter()
            .any(|n| n.name == "metFORMIN" && n.name_type == "tall_man")
    );
    assert!(
        d.identifiers
            .iter()
            .any(|i| i.system == "unii" && i.value == "9100L32L2N")
    );
    assert!(
        d.relationships
            .iter()
            .any(|r| r.predicate == "has_tradename" && r.concept.name == "Glucophage")
    );
}

#[test]
fn exact_match_prints_card() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let out = run(&db, &["augmentin"]);
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("Augmentin\n"), "{text}");
    assert!(text.contains("  ID       rxcui:151392"), "{text}");
    assert!(
        text.contains("Ingredients (2)\n    amoxicillin\n    clavulanate\n"),
        "{text}"
    );
    assert!(
        text.contains("RxNorm Current Prescribable Content\n"),
        "{text}"
    );
    assert!(text.contains("  Release  2026-09-08"), "{text}");
}

#[test]
fn non_exact_match_prints_list() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let text = String::from_utf8(run(&db, &["metfor"]).stdout).unwrap();
    assert!(text.starts_with("rxcui:6809"), "{text}");
}

#[test]
fn show_details_and_flag_agree() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let a = run(&db, &["show", "metformin"]);
    let b = run(&db, &["metformin", "--details"]);
    let c = run(&db, &["show", "rxcui:6809"]);
    assert_eq!(a.status.code(), Some(0));
    assert_eq!(a.stdout, b.stdout);
    assert_eq!(a.stdout, c.stdout);
    let text = String::from_utf8(a.stdout).unwrap();
    for part in [
        "Names",
        "Identifiers",
        "Relationships",
        "Source",
        "metFORMIN",
        "9100L32L2N",
        "National Library of Medicine",
    ] {
        assert!(text.contains(part), "missing {part}");
    }
}

#[test]
fn rxcui_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let out = run(&db, &["rxcui", "6809"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .starts_with("metformin\n")
    );

    for bad in ["99999999", "abc", ""] {
        let out = run(&db, &["rxcui", bad]);
        assert_eq!(out.status.code(), Some(1), "{bad:?}");
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn identifier_lookup() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let out = run(&db, &["--json", "id", "UNII:9100L32L2N"]);
    assert_eq!(out.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["concepts"][0]["code"], "6809");
    assert_eq!(
        run(&db, &["id", "not-an-identifier"]).status.code(),
        Some(2)
    );
    assert_eq!(run(&db, &["id", "unii:NOPE"]).status.code(), Some(1));
}

#[test]
fn details_json() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let out = run(&db, &["--json", "show", "augmentin"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let c = &v["concepts"][0];
    assert_eq!(c["code"], "151392");
    assert_eq!(c["kind"], "brand_name");
    let ingredients: Vec<&str> = c["sections"]["ingredients"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(ingredients, ["amoxicillin", "clavulanate"]);
    assert!(
        c["relationships"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["predicate"] == "tradename_of")
    );
}

#[test]
fn show_unknown_exits_1_with_empty_json() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    assert_eq!(run(&db, &["show", "zzqqxx"]).status.code(), Some(1));
    let out = run(&db, &["--json", "show", "zzqqxx"]);
    assert_eq!(out.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["concepts"].as_array().unwrap().is_empty());
}

#[test]
fn search_and_details_agree_on_top_concept() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&common::fixture_db(dir.path())).unwrap();
    let hit = &search(&db, "glucophage", 1).unwrap()[0];
    assert_eq!(hit.concept.code, "151827");
}
