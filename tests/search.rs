mod common;

use gurd::database::Database;
use gurd::search::{Hit, Match, search};

fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let path = common::fixture_db(dir.path());
    let db = Database::open(&path).unwrap();
    (dir, db)
}

fn codes(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|h| h.concept.code.as_str()).collect()
}

#[test]
fn exact_ingredient_comes_first() {
    let (_d, db) = db();
    let hits = search(&db, "metformin", 20).unwrap();
    assert_eq!(hits[0].concept.code, "6809");
    assert_eq!(hits[0].concept.kind, "ingredient");
    assert_eq!(hits[0].matched, Match::Exact);
    assert_eq!(hits[0].concept.code_system, "rxcui");
    assert_eq!(hits[0].concept.source, "rxnorm");
    assert_eq!(hits[0].concept.source_version, "2026-09-08");
}

#[test]
fn case_and_whitespace_do_not_matter() {
    let (_d, db) = db();
    for q in ["METFORMIN", "  Metformin ", "metFORMIN"] {
        let hits = search(&db, q, 20).unwrap();
        assert_eq!(hits[0].concept.code, "6809", "{q}");
        assert_eq!(hits[0].matched, Match::Exact, "{q}");
    }
}

#[test]
fn multi_word_exact_match() {
    let (_d, db) = db();
    let hits = search(&db, "metformin hydrochloride", 20).unwrap();
    assert_eq!(hits[0].concept.code, "235743");
    assert_eq!(hits[0].concept.kind, "precise_ingredient");
    assert_eq!(hits[0].matched, Match::Exact);
}

#[test]
fn each_concept_appears_once_under_its_best_tier() {
    let (_d, db) = db();
    let hits = search(&db, "metformin", 0).unwrap();
    let mut seen = codes(&hits);
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), hits.len());
    assert!(hits.windows(2).all(|w| w[0].matched <= w[1].matched));
}

#[test]
fn prefix_match() {
    let (_d, db) = db();
    let hits = search(&db, "metfor", 20).unwrap();
    assert_eq!(hits[0].concept.code, "6809");
    assert_eq!(hits[0].matched, Match::Prefix);
}

#[test]
fn token_match_in_any_order() {
    let (_d, db) = db();
    let hits = search(&db, "amoxicillin clavulanate", 20).unwrap();
    assert_eq!(hits[0].concept.code, "19711");
    assert_eq!(hits[0].concept.kind, "multiple_ingredients");
    assert_eq!(hits[0].matched, Match::Token);

    let hits = search(&db, "clavulanate amoxicillin", 20).unwrap();
    assert!(codes(&hits).contains(&"19711"));
}

#[test]
fn substring_match() {
    let (_d, db) = db();
    let hits = search(&db, "tformi", 20).unwrap();
    assert_eq!(hits[0].concept.code, "6809");
    assert_eq!(hits[0].matched, Match::Substring);
}

#[test]
fn fuzzy_match_only_when_nothing_else() {
    let (_d, db) = db();
    let hits = search(&db, "metfromin", 20).unwrap();
    assert_eq!(codes(&hits), ["6809"]);
    assert_eq!(hits[0].matched, Match::Fuzzy);
}

#[test]
fn brand_lookup() {
    let (_d, db) = db();
    let hits = search(&db, "augmentin", 20).unwrap();
    assert_eq!(hits[0].concept.code, "151392");
    assert_eq!(hits[0].concept.kind, "brand_name");
    // The branded drug is found too, through its name.
    assert!(codes(&hits).contains(&"824194"));
}

#[test]
fn synonym_match_reports_matched_name() {
    let (_d, db) = db();
    let hits = search(&db, "metformin hcl 500", 20).unwrap();
    assert_eq!(hits[0].concept.code, "861007");
    assert_ne!(hits[0].matched_name, hits[0].concept.name);
}

#[test]
fn unknown_and_empty_queries_find_nothing() {
    let (_d, db) = db();
    for q in ["zzqqxx", "paracetamol", "", "   ", "\"", "*", "a\"b OR c"] {
        assert!(search(&db, q, 20).unwrap().is_empty(), "{q:?}");
    }
}

#[test]
fn limit_is_respected() {
    let (_d, db) = db();
    assert_eq!(search(&db, "metformin", 2).unwrap().len(), 2);
    assert!(search(&db, "metformin", 0).unwrap().len() > 2);
}
