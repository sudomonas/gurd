//! The JSON output format is a public interface (docs/json.md). These tests pin the set
//! of keys in every document type; changing them requires bumping `json_version` and
//! updating the documentation.

mod common;

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const CONCEPT: &[&str] = &[
    "code",
    "code_system",
    "kind",
    "name",
    "source",
    "source_type",
    "source_version",
];
const SOURCE_RECORD: &[&str] = &[
    "age_days",
    "attribution",
    "code_system",
    "file_name",
    "imported_at",
    "importer_version",
    "license",
    "origin",
    "provider",
    "redistributable",
    "release_date",
    "retrieved_at",
    "sha256",
    "source",
    "stale",
    "stale_after_days",
    "title",
    "upstream_checksum",
    "url",
    "version",
];

fn json(db: &Path, args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_drug"))
        .arg("--json")
        .args(args)
        .env("DRUG_DB", db)
        .output()
        .unwrap();
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

/// Every key path in a document, with array elements as `[]` and the (data-dependent)
/// names of section maps as `*`.
fn paths(v: &Value) -> BTreeSet<String> {
    fn walk(v: &Value, path: String, parent: &str, out: &mut BTreeSet<String>) {
        if !path.is_empty() {
            out.insert(path.clone());
        }
        match v {
            Value::Object(map) => {
                for (k, child) in map {
                    let k = if parent == "sections" {
                        "*"
                    } else {
                        k.as_str()
                    };
                    let p = if path.is_empty() {
                        k.to_owned()
                    } else {
                        format!("{path}.{k}")
                    };
                    walk(child, p, k, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, format!("{path}[]"), parent, out);
                }
            }
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(v, String::new(), "", &mut out);
    out
}

fn expect(fixed: &[&str], nested: &[(&str, &[&str])]) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = fixed.iter().map(|s| (*s).to_owned()).collect();
    for (prefix, keys) in nested {
        out.insert((*prefix).to_owned());
        for k in *keys {
            out.insert(format!("{prefix}.{k}"));
        }
    }
    out
}

const SOURCES: (&str, &[&str]) = ("sources[]", &["source", "version"]);

#[test]
fn search_document() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let mut expected = expect(
        &["json_version", "query", "results", "sources"],
        &[
            SOURCES,
            ("results[]", CONCEPT),
            ("results[]", &["match", "matched_name"]),
        ],
    );
    expected.insert("results[]".into());
    assert_eq!(paths(&json(&db, &["metformin"])), expected);
}

#[test]
fn details_document() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let expected = expect(
        &[
            "json_version",
            "query",
            "sources",
            "concepts",
            "concepts[].sections",
            "concepts[].sections.*",
            "concepts[].names",
            "concepts[].identifiers",
            "concepts[].attributes",
            "concepts[].relationships",
        ],
        &[
            SOURCES,
            ("concepts[]", CONCEPT),
            ("concepts[].sections.*[]", CONCEPT),
            ("concepts[].names[]", &["name", "name_type", "source_type"]),
            ("concepts[].identifiers[]", &["system", "value"]),
            ("concepts[].attributes[]", &["key", "value"]),
            ("concepts[].relationships[]", &["predicate", "concept"]),
            ("concepts[].relationships[].concept", CONCEPT),
        ],
    );
    // A clinical drug has every part: sections, names, identifiers, attributes, relationships.
    for args in [
        &["show", "rxcui:861007"][..],
        &["rxcui", "861007"],
        &["id", "rxcui:861007"],
    ] {
        assert_eq!(paths(&json(&db, args)), expected, "{args:?}");
    }
}

#[test]
fn database_document() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let expected = expect(
        &["json_version", "database"],
        &[
            (
                "database",
                &[
                    "path",
                    "installed",
                    "schema_version",
                    "built_at",
                    "built_by",
                    "sources",
                ],
            ),
            ("database.sources[]", SOURCE_RECORD),
        ],
    );
    assert_eq!(paths(&json(&db, &["database"])), expected);
}

#[test]
fn sources_document() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let expected = expect(
        &["json_version", "sources"],
        &[
            (
                "sources[]",
                &[
                    "source",
                    "title",
                    "provider",
                    "license",
                    "attribution",
                    "url",
                    "download_url",
                    "checksums_url",
                    "redistributable",
                    "builtin",
                    "installed",
                ],
            ),
            ("sources[].installed", SOURCE_RECORD),
        ],
    );
    assert_eq!(paths(&json(&db, &["sources"])), expected);
}

#[test]
fn class_document() {
    let dir = tempfile::tempdir().unwrap();
    let db = common::fixture_db(dir.path());
    let expected = expect(
        &["json_version", "query", "sources", "classes"],
        &[SOURCES, ("concept", CONCEPT)],
    );
    assert_eq!(paths(&json(&db, &["class", "metformin"])), expected);
}

#[test]
fn update_document() {
    let dir = tempfile::tempdir().unwrap();
    let zip = common::fixture_zip(dir.path(), "RxNorm_full_prescribe_09082026.zip");
    let out = Command::new(env!("CARGO_BIN_EXE_drug"))
        .args(["--json", "update", "--from", zip.to_str().unwrap()])
        .env("DRUG_DB", dir.path().join("drug.db"))
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let expected = expect(
        &["json_version", "database", "imported", "up_to_date"],
        &[(
            "imported[]",
            &[
                "source",
                "version",
                "concepts",
                "names",
                "identifiers",
                "relationships",
                "attributes",
            ],
        )],
    );
    assert_eq!(paths(&v), expected);
}
