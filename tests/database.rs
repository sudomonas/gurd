use gurd::database::{self, Database, NotInstalled, SCHEMA_VERSION};
use rusqlite::Connection;

fn names_of(conn: &Connection, kind: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type = ?1")
        .unwrap();
    stmt.query_map([kind], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn create_then_open() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gurd.db");
    drop(database::create(&path).unwrap());

    let db = Database::open(&path).unwrap();
    assert_eq!(db.path(), path);
    assert!(db.meta("built_at").unwrap().is_some());
    assert!(db.meta("built_by").unwrap().unwrap().starts_with("gurd "));
    assert!(db.sources().unwrap().is_empty());
}

#[test]
fn required_tables_exist() {
    let dir = tempfile::tempdir().unwrap();
    let conn = database::create(&dir.path().join("gurd.db")).unwrap();
    let tables = names_of(&conn, "table");
    for t in [
        "meta",
        "sources",
        "concept_kinds",
        "concepts",
        "names",
        "names_tok",
        "names_tri",
        "identifiers",
        "relationships",
        "attributes",
        "classifications",
        "concept_classifications",
    ] {
        assert!(tables.iter().any(|x| x == t), "missing table {t}");
    }
}

#[test]
fn required_indexes_exist() {
    let dir = tempfile::tempdir().unwrap();
    let conn = database::create(&dir.path().join("gurd.db")).unwrap();
    let indexes = names_of(&conn, "index");
    for i in [
        "names_norm",
        "names_concept",
        "concepts_kind",
        "identifiers_lookup",
        "relationships_subject",
        "relationships_object",
        "attributes_concept",
    ] {
        assert!(indexes.iter().any(|x| x == i), "missing index {i}");
    }
}

#[test]
fn concept_kinds_are_seeded() {
    let dir = tempfile::tempdir().unwrap();
    let conn = database::create(&dir.path().join("gurd.db")).unwrap();
    for kind in [
        "ingredient",
        "precise_ingredient",
        "clinical_drug",
        "branded_drug",
        "brand_name",
        "generic_pack",
        "branded_pack",
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM concept_kinds WHERE kind = ?1",
                [kind],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "kind {kind}");
    }
}

#[test]
fn source_metadata_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gurd.db");
    let conn = database::create(&path).unwrap();
    conn.execute(
        "INSERT INTO sources (slug, title, code_system, provider, version, release_date, license, attribution,
                              url, file_name, upstream_checksum, sha256, retrieved_at, imported_at,
                              importer_version, origin, redistributable)
         VALUES ('test', 'Test Source', 'test', 'Tester', '2026-09-08', '2026-09-08', 'Public domain',
                 'Courtesy of Tester', 'https://example.org/t.zip', 't.zip', NULL, 'abc',
                 '2026-10-03T00:00:00Z', '2026-10-03T00:00:00Z', '1', 'builtin', 1)",
        [],
    )
    .unwrap();
    drop(conn);

    let sources = Database::open(&path).unwrap().sources().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].slug, "test");
    assert_eq!(sources[0].version, "2026-09-08");
    assert!(sources[0].redistributable);
}

#[test]
fn every_source_derived_table_requires_a_source() {
    let dir = tempfile::tempdir().unwrap();
    let conn = database::create(&dir.path().join("gurd.db")).unwrap();
    for table in [
        "concepts",
        "names",
        "identifiers",
        "relationships",
        "attributes",
        "classifications",
        "concept_classifications",
    ] {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT \"notnull\" FROM pragma_table_info('{table}') WHERE name = 'source_id'"
            ))
            .unwrap();
        let not_null: i64 = stmt
            .query_row([], |r| r.get(0))
            .unwrap_or_else(|_| panic!("{table} has no source_id"));
        assert_eq!(not_null, 1, "{table}.source_id must be NOT NULL");
    }
}

#[test]
fn fts_tokenizers_are_available() {
    let dir = tempfile::tempdir().unwrap();
    let conn = database::create(&dir.path().join("gurd.db")).unwrap();
    conn.execute_batch(
        "INSERT INTO sources (id, slug, title, code_system, provider, version, license, attribution, url, file_name,
                              sha256, retrieved_at, imported_at, importer_version, origin, redistributable)
         VALUES (1, 't', 't', 't', 't', '1', 't', 't', 't', 't', 't', 't', 't', '1', 'builtin', 1);
         INSERT INTO concepts (id, source_id, source_code, kind, source_type, name)
         VALUES (1, 1, '1', 'ingredient', 'IN', 'metformin');
         INSERT INTO names (concept_id, source_id, name, norm, name_type, source_type)
         VALUES (1, 1, 'metformin hydrochloride', 'metformin hydrochloride', 'preferred', 'PIN');
         INSERT INTO names_tok (names_tok) VALUES ('rebuild');
         INSERT INTO names_tri (names_tri) VALUES ('rebuild');",
    )
    .unwrap();
    let tok: i64 = conn
        .query_row(
            "SELECT count(*) FROM names_tok WHERE names_tok MATCH 'hydrochloride'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let tri: i64 = conn
        .query_row(
            "SELECT count(*) FROM names_tri WHERE names_tri MATCH 'tfor'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!((tok, tri), (1, 1));
}

#[test]
fn opened_database_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gurd.db");
    drop(database::create(&path).unwrap());
    let db = Database::open(&path).unwrap();
    let err = db
        .connection()
        .execute("INSERT INTO meta (key, value) VALUES ('x', 'y')", [])
        .unwrap_err();
    assert!(err.to_string().contains("readonly"), "{err}");
}

#[test]
fn missing_database_is_reported_as_not_installed() {
    let dir = tempfile::tempdir().unwrap();
    let err = Database::open(&dir.path().join("absent.db")).err().unwrap();
    assert!(err.is::<NotInstalled>());
}

#[test]
fn create_refuses_to_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gurd.db");
    std::fs::write(&path, b"precious").unwrap();
    assert!(database::create(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"precious");
}

#[test]
fn rejects_other_files_and_schema_versions() {
    let dir = tempfile::tempdir().unwrap();

    let junk = dir.path().join("junk.db");
    std::fs::write(&junk, b"not sqlite at all, just some bytes......").unwrap();
    assert!(Database::open(&junk).is_err());

    let plain = dir.path().join("plain.db");
    Connection::open(&plain)
        .unwrap()
        .execute_batch("CREATE TABLE t (x);")
        .unwrap();
    assert!(
        Database::open(&plain)
            .unwrap_err()
            .to_string()
            .contains("not a gurd database")
    );

    let newer = dir.path().join("newer.db");
    let conn = database::create(&newer).unwrap();
    conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    drop(conn);
    assert!(
        Database::open(&newer)
            .unwrap_err()
            .to_string()
            .contains("newer")
    );
}

#[test]
fn kind_enum_matches_schema() {
    let dir = tempfile::tempdir().unwrap();
    let conn = database::create(&dir.path().join("gurd.db")).unwrap();
    let mut stmt = conn
        .prepare("SELECT kind FROM concept_kinds ORDER BY kind")
        .unwrap();
    let mut in_db: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let mut in_code: Vec<String> = gurd::models::Kind::ALL
        .iter()
        .map(|k| k.as_str().to_owned())
        .collect();
    in_db.sort();
    in_code.sort();
    assert_eq!(in_db, in_code);
    for k in gurd::models::Kind::ALL {
        assert_eq!(gurd::models::Kind::parse(k.as_str()), Some(k));
    }
}

#[test]
fn staleness_is_computed_from_release_date() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gurd.db");
    let conn = database::create(&path).unwrap();
    conn.execute_batch(
        "INSERT INTO sources (slug, title, code_system, provider, version, release_date,
                              stale_after_days, license, attribution, url, file_name, sha256,
                              retrieved_at, imported_at, importer_version, origin, redistributable)
         VALUES
           ('old',   't', 'x', 'p', '1', '2000-01-01', 45, 'l', 'a', 'u', 'f', 's', 'r', 'i', '1', 'builtin', 1),
           ('fresh', 't', 'x', 'p', '1', date('now'),  45, 'l', 'a', 'u', 'f', 's', 'r', 'i', '1', 'builtin', 1),
           ('never', 't', 'x', 'p', '1', '2000-01-01', NULL, 'l', 'a', 'u', 'f', 's', 'r', 'i', '1', 'builtin', 1),
           ('nodate','t', 'x', 'p', '1', NULL,         45, 'l', 'a', 'u', 'f', 's', 'r', 'i', '1', 'builtin', 1);",
    )
    .unwrap();
    drop(conn);
    let sources = Database::open(&path).unwrap().sources().unwrap();
    let get = |slug: &str| sources.iter().find(|s| s.slug == slug).unwrap();
    assert!(get("old").stale);
    assert!(get("old").age_days.unwrap() > 9000);
    assert!(!get("fresh").stale);
    assert_eq!(get("fresh").age_days, Some(0));
    assert!(!get("never").stale, "no threshold, never stale");
    assert!(!get("nodate").stale);
    assert_eq!(get("nodate").age_days, None);
}
