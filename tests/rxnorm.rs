mod common;

use gurd::database::Database;
use gurd::sources::rxnorm::{ATTRIBUTION, RxNorm};
use gurd::sources::{Input, Source};
use rusqlite::Connection;

fn fixture() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let conn = Connection::open(db).unwrap();
    (dir, conn)
}

fn one<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str) -> T {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn records_source_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let sources = Database::open(&db).unwrap().sources().unwrap();
    assert_eq!(sources.len(), 1);
    let s = &sources[0];
    assert_eq!(s.slug, "rxnorm");
    assert_eq!(s.version, "2026-09-08");
    assert_eq!(s.release_date.as_deref(), Some("2026-09-08"));
    assert_eq!(s.attribution, ATTRIBUTION);
    assert_eq!(s.file_name, "RxNorm_full_prescribe_09082026.zip");
    assert_eq!(s.sha256.len(), 64);
    assert_eq!(s.origin, "builtin");
    assert!(s.redistributable);
}

#[test]
fn imports_every_fixture_concept_with_its_kind() {
    let (_dir, conn) = fixture();
    assert_eq!(one::<i64>(&conn, "SELECT count(*) FROM concepts"), 24);
    for (rxcui, kind, name) in [
        ("6809", "ingredient", "metformin"),
        ("235743", "precise_ingredient", "metformin hydrochloride"),
        ("19711", "multiple_ingredients", "amoxicillin / clavulanate"),
        ("151392", "brand_name", "Augmentin"),
        (
            "861007",
            "clinical_drug",
            "metformin hydrochloride 500 MG Oral Tablet",
        ),
        (
            "824194",
            "branded_drug",
            "amoxicillin 875 MG / clavulanate 125 MG Oral Tablet [Augmentin]",
        ),
        (
            "860974",
            "clinical_component",
            "metformin hydrochloride 500 MG",
        ),
        ("317541", "dose_form", "Oral Tablet"),
        (
            "2646570",
            "clinical_dose_form_precise",
            "metformin hydrochloride Oral Tablet",
        ),
    ] {
        let (k, n): (String, String) = conn
            .query_row(
                "SELECT kind, name FROM concepts WHERE source_code = ?1",
                [rxcui],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((k.as_str(), n.as_str()), (kind, name), "RxCUI {rxcui}");
    }
}

#[test]
fn imports_names_with_types() {
    let (_dir, conn) = fixture();
    let tall_man: String = one(
        &conn,
        "SELECT n.name FROM names n JOIN concepts c ON c.id = n.concept_id
         WHERE c.source_code = '6809' AND n.name_type = 'tall_man'",
    );
    assert_eq!(tall_man, "metFORMIN");
    let norm: String = one(&conn, "SELECT norm FROM names WHERE name = 'metFORMIN'");
    assert_eq!(norm, "metformin");
    let prescribable: i64 = one(
        &conn,
        "SELECT count(*) FROM names WHERE name_type = 'prescribable'",
    );
    assert!(prescribable > 0);
    // Every concept has exactly one preferred name.
    let bad: i64 = one(
        &conn,
        "SELECT count(*) FROM concepts c
         WHERE (SELECT count(*) FROM names n WHERE n.concept_id = c.id AND n.name_type = 'preferred') != 1",
    );
    assert_eq!(bad, 0);
}

#[test]
fn ignores_mthspl_product_content() {
    let (_dir, conn) = fixture();
    assert_eq!(
        one::<i64>(
            &conn,
            "SELECT count(*) FROM names WHERE source_type IN ('DP', 'SU')"
        ),
        0
    );
    assert_eq!(
        one::<i64>(
            &conn,
            "SELECT count(*) FROM attributes WHERE key IN ('SPL_SET_ID', 'LABELER', 'DM_SPL_ID')"
        ),
        0
    );
}

#[test]
fn imports_identifiers() {
    let (_dir, conn) = fixture();
    let unii: String = one(
        &conn,
        "SELECT i.value FROM identifiers i JOIN concepts c ON c.id = i.concept_id
         WHERE c.source_code = '6809' AND i.system = 'unii'",
    );
    assert_eq!(unii, "9100L32L2N");
    let ndcs: i64 = one(
        &conn,
        "SELECT count(*) FROM identifiers i JOIN concepts c ON c.id = i.concept_id
         WHERE c.source_code = '861007' AND i.system = 'ndc'",
    );
    assert!(ndcs > 0);
}

#[test]
fn relationships_read_subject_predicate_object() {
    let (_dir, conn) = fixture();
    let rel = |subject: &str, predicate: &str, object: &str| -> i64 {
        conn.query_row(
            "SELECT count(*) FROM relationships r
             JOIN concepts s ON s.id = r.subject_id
             JOIN concepts o ON o.id = r.object_id
             WHERE s.source_code = ?1 AND r.predicate = ?2 AND o.source_code = ?3",
            [subject, predicate, object],
            |r| r.get(0),
        )
        .unwrap()
    };
    // Glucophage is a tradename of metformin, not the other way round.
    assert_eq!(rel("151827", "tradename_of", "6809"), 1);
    assert_eq!(rel("6809", "has_tradename", "151827"), 1);
    assert_eq!(rel("6809", "tradename_of", "151827"), 0);
    // Augmentin's ingredients.
    assert_eq!(rel("151392", "tradename_of", "723"), 1);
    assert_eq!(rel("151392", "tradename_of", "48203"), 1);
    // Clinical drug → component → ingredient.
    assert_eq!(rel("861007", "consists_of", "860974"), 1);
    assert_eq!(rel("860974", "has_precise_ingredient", "235743"), 1);
}

#[test]
fn imports_strength_attribute() {
    let (_dir, conn) = fixture();
    let strength: String = one(
        &conn,
        "SELECT a.value FROM attributes a JOIN concepts c ON c.id = a.concept_id
         WHERE c.source_code = '860974' AND a.key = 'RXN_STRENGTH'",
    );
    assert_eq!(strength, "500 MG");
}

#[test]
fn version_comes_from_readme_even_if_zip_is_renamed() {
    let dir = tempfile::tempdir().unwrap();
    let zip = common::fixture_zip(dir.path(), "renamed.zip");
    let release = RxNorm.release(&Input::open(&zip).unwrap()).unwrap();
    assert_eq!(release.version, "2026-09-08");
}

#[test]
fn rejects_archives_that_are_not_prescribable_releases() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("something.zip");
    common::make_zip(&zip, &[("rrf/RXNCONSO.RRF", b"")]);
    let err = RxNorm.release(&Input::open(&zip).unwrap()).unwrap_err();
    assert!(
        err.to_string()
            .contains("not an RxNorm Current Prescribable"),
        "{err}"
    );
}

#[test]
fn rejects_malformed_rows() {
    let dir = tempfile::tempdir().unwrap();
    let zip = dir.path().join("RxNorm_full_prescribe_09082026.zip");
    common::make_zip(
        &zip,
        &[
            ("rrf/RXNCONSO.RRF", b"6809|ENG|too|few|fields|\n"),
            ("rrf/RXNREL.RRF", b""),
            ("rrf/RXNSAT.RRF", b""),
        ],
    );
    let input = Input::open(&zip).unwrap();
    let err = gurd::import::build(
        &dir.path().join("out.db"),
        &[gurd::import::Job {
            source: &RxNorm,
            input: &input,
            upstream_checksum: None,
            retrieved_at: None,
        }],
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("expected 18 fields"), "{err:#}");
}

#[test]
fn navigation_predicates_exist_in_the_data() {
    let (_dir, conn) = fixture();
    let mut stmt = conn
        .prepare("SELECT DISTINCT path FROM navigation")
        .unwrap();
    let paths: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!paths.is_empty());
    let mut missing: Vec<String> = Vec::new();
    for path in &paths {
        for p in path.split(' ') {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM relationships WHERE predicate = ?1",
                    [p],
                    |r| r.get(0),
                )
                .unwrap();
            if n == 0 && !missing.contains(&p.to_owned()) {
                missing.push(p.to_owned());
            }
        }
    }
    missing.sort();
    // The fixture contains no packs; every other predicate must be real.
    assert_eq!(missing, ["contained_in", "contains"]);
}
