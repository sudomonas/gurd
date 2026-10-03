//! Adapters other than RxNorm, and databases holding several sources.
//!
//! openFDA uses a real excerpt (CC0). The 1mg, A-Z India and RxTerms inputs are made up in
//! the datasets' formats: those datasets may not be redistributed, so no real rows are kept
//! in the repository.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn gurd(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gurd"))
        .args(args)
        .arg("--no-pager")
        .env("GURD_DB", db)
        .env_remove("NO_COLOR")
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn install(db: &Path, source: &str, from: &Path) {
    let out = gurd(
        db,
        &[
            "update",
            "--source",
            source,
            "--from",
            from.to_str().unwrap(),
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "installing {source}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A database with the RxNorm fixture installed through the CLI, so its release is stored
/// and later updates can rebuild it.
fn rxnorm_db(dir: &Path) -> PathBuf {
    let db = dir.join("gurd.db");
    let rx = common::fixture_zip(dir, "RxNorm_full_prescribe_09082026.zip");
    install(&db, "rxnorm", &rx);
    db
}

fn installed(db: &Path) -> Vec<(String, String)> {
    let v: Value = serde_json::from_str(&stdout(&gurd(db, &["database", "--json"]))).unwrap();
    v["database"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["source"].as_str().unwrap().to_owned(),
                s["version"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

/// A directory of 1mg-style JSON-lines files (made-up products).
fn onemg_dir(dir: &Path) -> PathBuf {
    let root = dir.join("1mg");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("kaggle_medicines.json"),
        concat!(
            r#"{"_id": {"$oid": "000000000000000000000001"}, "tabletname": "Glycofake 500 Tablet", "prescription": "Prescription Required", "marketer": "Example Pharma Ltd", "composition": "Metformin (500mg)", "price": 21.5, "img": "https://example.invalid/a.png", "data": "x", "brief": "Glycofake 500 Tablet is an example record.", "uses": ["Type 2 diabetes mellitus"], "sideeffects": ["Nausea", "Diarrhea"], "sideeffectbrief": "Example note."}"#,
            "\n",
            r#"{"_id": {"$oid": "000000000000000000000002"}, "tabletname": "Amoxyfake CV 625 Tablet", "prescription": "Prescription Required", "marketer": "Example Pharma Ltd", "composition": "Amoxycillin (500mg) + Clavulanic Acid (125mg)", "price": 99, "img": "", "data": "x", "brief": "", "uses": ["Bacterial infections"], "sideeffects": [], "sideeffectbrief": ""}"#,
            "\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("kaggle_injections.json"),
        concat!(
            r#"{"_id": {"$oid": "000000000000000000000003"}, "injectionname": "Vitafake Injection", "prescription": "Prescription Required", "marketer": "Other Labs", "composition": "Vitamin B6 (Pyridoxine) (10mg)", "price": 5.0, "img": "", "data": "x", "brief": "", "uses": [], "sideeffects": [], "sideeffectbrief": ""}"#,
            "\n",
        ),
    )
    .unwrap();
    root
}

/// An A-Z India-style CSV (made-up products), with a quoted comma and `NA`.
fn azindia_csv(dir: &Path) -> PathBuf {
    let path = dir.join("az.csv");
    fs::write(
        &path,
        "id,name,substitute0,substitute1,sideEffect0,sideEffect1,use0,Chemical Class,Habit Forming,Therapeutic Class,Action Class\n\
         1,fakeol 650 tablet,Otherol 650 Tablet,,Nausea,\"Rash, mild\",Treatment of Fever,P-Aminophenol Derivative,No,PAIN ANALGESICS,NA\n\
         2,fakemox 625 tablet,,,,,Treatment of Bacterial infections,NA,No,ANTI INFECTIVES,Penicillins\n",
    )
    .unwrap();
    path
}

/// An RxTerms-style release (made-up display names over the RxNorm fixture's RxCUIs).
fn rxterms_zip(dir: &Path) -> PathBuf {
    let path = dir.join("RxTerms202609.zip");
    common::make_zip(
        &path,
        &[
            (
                "RxTerms202609.txt",
                b"RXCUI|GENERIC_RXCUI|TTY|FULL_NAME|RXN_DOSE_FORM|FULL_GENERIC_NAME|BRAND_NAME|DISPLAY_NAME|ROUTE|NEW_DOSE_FORM|STRENGTH|SUPPRESS_FOR|DISPLAY_NAME_SYNONYM|IS_RETIRED|SXDG_RXCUI|SXDG_TTY|SXDG_NAME|PSN\n\
861007||SCD|metformin hydrochloride 500 MG Oral Tablet|Oral Tablet|metformin hydrochloride 500 MG Oral Tablet||metFORMIN (Oral Pill)|Oral Pill|Tab|500 mg||||||| \n\
999999||SCD|retired drug|Oral Tablet|retired drug||RETIRED (Oral Pill)|Oral Pill|Tab|1 mg|||RETIRED||||\n",
            ),
            (
                "RxTermsIngredients202609.txt",
                b"RXCUI|INGREDIENT|ING_RXCUI\n861007|metformin|6809\n",
            ),
        ],
    );
    path
}

fn openfda_zip(dir: &Path) -> PathBuf {
    let path = dir.join("drug-ndc-0001-of-0001.json.zip");
    let json = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/openfda-mini/drug-ndc-0001-of-0001.json"),
    )
    .unwrap();
    common::make_zip(&path, &[("drug-ndc-0001-of-0001.json", &json)]);
    path
}

#[test]
fn onemg_products_show_every_field() {
    let dir = tempfile::tempdir().unwrap();
    let db = rxnorm_db(dir.path());
    install(&db, "1mg", &onemg_dir(dir.path()));

    let text = stdout(&gurd(&db, &["glycofake 500 tablet"]));
    for part in [
        "1mg medicines (third-party Kaggle scrape)",
        "Unofficial third-party scrape",
        "Composition",
        "Metformin (500mg)",
        "Example Pharma Ltd",
        "Prescription Required",
        "21.5",
        "Type 2 diabetes mellitus",
        "Nausea, Diarrhea",
        "is an example record",
    ] {
        assert!(text.contains(part), "missing {part:?} in:\n{text}");
    }
    // Composition parsing keeps parenthesized names whole.
    let text = stdout(&gurd(&db, &["vitamin b6 (pyridoxine)"]));
    assert!(text.contains("Vitafake Injection"), "{text}");
}

#[test]
fn ingredient_page_combines_sources() {
    let dir = tempfile::tempdir().unwrap();
    let db = rxnorm_db(dir.path());
    install(&db, "1mg", &onemg_dir(dir.path()));
    install(&db, "rxterms", &rxterms_zip(dir.path()));

    let text = stdout(&gurd(&db, &["metformin"]));
    let rxnorm = text.find("RxNorm Current Prescribable Content").unwrap();
    let rxterms = text.find("\nRxTerms\n").expect(&text);
    let onemg = text.find("1mg medicines").expect(&text);
    // Official sources first.
    assert!(rxnorm < rxterms && rxterms < onemg, "{text}");
    assert!(text.contains("metFORMIN (Oral Pill) 500 mg"), "{text}");
    assert!(text.contains("Glycofake 500 Tablet"), "{text}");
    // A retired RxTerms row is not imported.
    assert!(!gurd(&db, &["retired"]).status.success());
}

#[test]
fn azindia_keeps_columns_and_classes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("gurd.db");
    install(&db, "az-india", &azindia_csv(dir.path()));

    let text = stdout(&gurd(&db, &["fakeol 650 tablet"]));
    for part in [
        "Otherol 650 Tablet",
        "Nausea, Rash, mild",
        "Treatment of Fever",
        "PAIN ANALGESICS",
        "P-Aminophenol Derivative",
        "Habit forming",
    ] {
        assert!(text.contains(part), "missing {part:?} in:\n{text}");
    }
    // `NA` means "not available" in this dataset.
    assert!(!text.contains("Action class"), "{text}");

    let text = stdout(&gurd(&db, &["class", "fakemox 625 tablet"]));
    assert!(text.contains("Penicillins"), "{text}");
    assert!(text.contains("ANTI INFECTIVES"), "{text}");
}

#[test]
fn openfda_links_to_rxnorm_by_identifiers() {
    let dir = tempfile::tempdir().unwrap();
    let db = rxnorm_db(dir.path());
    install(&db, "openfda", &openfda_zip(dir.path()));

    // The RxCUI openFDA attaches to a product finds both records.
    let v: Value =
        serde_json::from_str(&stdout(&gurd(&db, &["--json", "id", "rxcui:861007"]))).unwrap();
    let sources: Vec<&str> = v["concepts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["source"].as_str().unwrap())
        .collect();
    assert!(
        sources.contains(&"rxnorm") && sources.contains(&"openfda"),
        "{v}"
    );

    // Package NDCs are stored in RxNorm's 11-digit form.
    let v: Value =
        serde_json::from_str(&stdout(&gurd(&db, &["--json", "id", "ndc:00615857739"]))).unwrap();
    assert_eq!(v["concepts"][0]["source"], "openfda", "{v}");

    // A single-ingredient product's UNII links its ingredient to RxNorm's precise ingredient.
    let text = stdout(&gurd(&db, &["metformin hydrochloride"]));
    assert!(text.contains("openFDA NDC Directory"), "{text}");
    assert!(text.contains("Do not rely on openFDA"), "{text}");

    let text = stdout(&gurd(&db, &["class", "metformin hydrochloride"]));
    assert!(text.contains("Biguanide"), "{text}");
}

#[test]
fn sources_are_rebuilt_together_and_removed() {
    let dir = tempfile::tempdir().unwrap();
    let db = rxnorm_db(dir.path());
    install(&db, "1mg", &onemg_dir(dir.path()));
    let first = installed(&db);
    assert_eq!(
        first.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(),
        ["1mg", "rxnorm"]
    );

    // Updating one source rebuilds the others from their stored releases, unchanged.
    install(&db, "az-india", &azindia_csv(dir.path()));
    let second = installed(&db);
    assert_eq!(second.len(), 3);
    for s in &first {
        assert!(second.contains(s), "{s:?} changed: {second:?}");
    }

    let out = stdout(&gurd(&db, &["remove", "1mg"]));
    assert!(out.contains("Removed 1mg"), "{out}");
    assert_eq!(
        installed(&db)
            .iter()
            .map(|s| s.0.as_str())
            .collect::<Vec<_>>(),
        ["az-india", "rxnorm"]
    );
    assert!(!gurd(&db, &["glycofake 500 tablet"]).status.success());
    assert!(!gurd(&db, &["remove", "1mg"]).status.success());

    // A source whose stored release has gone cannot be rebuilt; nothing changes.
    let stored = PathBuf::from(format!("{}.sources", db.display())).join("az-india");
    fs::remove_dir_all(&stored).unwrap();
    let before = fs::read(&db).unwrap();
    let out = gurd(
        &db,
        &[
            "update",
            "--source",
            "1mg",
            "--from",
            onemg_dir(dir.path()).to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("is not stored"));
    assert_eq!(fs::read(&db).unwrap(), before);

    // Removing the last sources moves the database aside.
    stdout(&gurd(&db, &["remove", "az-india"]));
    stdout(&gurd(&db, &["remove", "rxnorm"]));
    assert!(!db.exists());
    assert!(PathBuf::from(format!("{}.bak", db.display())).exists());
}

#[test]
fn csv_reader_handles_quotes_and_line_breaks() {
    let text = "a,b,c\n1,\"x, y\",\"say \"\"hi\"\"\"\n2,\"multi\nline\",\r\n";
    let mut rows: Vec<Vec<String>> = Vec::new();
    gurd::sources::each_csv_record(&mut text.as_bytes(), |_, r| {
        rows.push(r.to_vec());
        Ok(())
    })
    .unwrap();
    assert_eq!(
        rows,
        [
            vec!["a", "b", "c"],
            vec!["1", "x, y", "say \"hi\""],
            vec!["2", "multi\nline", ""],
        ]
    );
}
