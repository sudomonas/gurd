//! "Extensive A-Z Medicines Dataset of India": a third-party scrape published on Kaggle as
//! one CSV file with a row per product:
//! `id, name, substitute0..4, sideEffect0..41, use0..4, Chemical Class, Habit Forming,
//! Therapeutic Class, Action Class`.
//!
//! Unofficial and not redistributable: the adapter only reads a copy the user supplies.
//! Every non-empty column is kept. `NA` is the dataset's own marker for "not available"
//! and is treated as missing. Columns the adapter does not know are kept under their own
//! names, so nothing in the file is dropped.

use std::collections::HashMap;

use anyhow::{Result, bail};
use rusqlite::Connection;

use super::{ImportSink, Input, Release, Source, SourceInfo, each_csv_record, slug};
use crate::models::{Kind, NameType};

pub struct AzIndia;

const CODE_SYSTEM: &str = "azindia";

impl Source for AzIndia {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            slug: "az-india".into(),
            title: "A-Z Medicines Dataset of India (third-party Kaggle scrape)".into(),
            code_system: CODE_SYSTEM.into(),
            provider: "Unofficial: compiled by a third party from Indian online pharmacy \
                       listings"
                .into(),
            license: "No license. Compiled by a third party from online pharmacy listings; \
                      the original publishers' permission is unknown. Personal reference use \
                      only; do not redistribute."
                .into(),
            attribution: "Extensive A-Z Medicines Dataset of India, published on Kaggle by a \
                          third party."
                .into(),
            url: "https://www.kaggle.com/".into(),
            redistributable: false,
            stale_after_days: None,
            download_url: None,
            checksums_url: None,
            notice: Some(
                "Unofficial third-party dataset. Accuracy, completeness and date are unknown."
                    .into(),
            ),
        }
    }

    fn release(&self, input: &Input) -> Result<Release> {
        let member = csv_member(input)?;
        let header = input.read(&member, |r| {
            let mut header = Vec::new();
            let _ = each_csv_record(r, |_, rec| {
                header = rec.to_vec();
                bail!("stop")
            });
            Ok(header)
        })?;
        if !["id", "name"].iter().all(|c| header.iter().any(|h| h == c)) {
            bail!(
                "{} is not the A-Z Medicines Dataset of India (expected id and name columns)",
                input.file_name
            );
        }
        let Some(date) = input.file_date() else {
            bail!("cannot determine the date of {}", input.file_name);
        };
        Ok(Release {
            version: date,
            release_date: None,
        })
    }

    fn import(&self, input: &Input, sink: &mut ImportSink) -> Result<()> {
        let member = csv_member(input)?;
        let mut columns: Vec<Column> = Vec::new();
        let mut classes: HashMap<(String, String), i64> = HashMap::new();
        input.read(&member, |r| {
            each_csv_record(r, |number, rec| {
                if number == 1 {
                    columns = rec.iter().map(|h| Column::parse(h)).collect();
                    return Ok(());
                }
                import_row(&columns, rec, sink, &mut classes)
            })
        })?;
        for (rank, &(key, label, summary)) in ATTRIBUTE_KEYS.iter().enumerate() {
            sink.attribute_key(key, label, rank as i64, summary)?;
        }
        Ok(())
    }

    fn validate(&self, conn: &Connection, source_id: i64) -> Result<()> {
        let n: i64 = conn.query_row(
            "SELECT count(*) FROM concepts WHERE source_id = ?1",
            [source_id],
            |r| r.get(0),
        )?;
        if n == 0 {
            bail!("no products imported");
        }
        Ok(())
    }
}

#[rustfmt::skip]
const ATTRIBUTE_KEYS: &[(&str, &str, bool)] = &[
    ("use", "Uses", true),
    ("side_effect", "Side effects", true),
    ("substitute", "Substitutes", true),
    ("therapeutic_class", "Therapeutic class", true),
    ("action_class", "Action class", true),
    ("chemical_class", "Chemical class", true),
    ("habit_forming", "Habit forming", true),
];

/// Columns that are also classification systems, for `gurd class`.
const CLASS_KEYS: &[&str] = &["therapeutic_class", "action_class", "chemical_class"];

enum Column {
    Id,
    Name,
    /// An attribute key; numbered columns (`sideEffect0`, `sideEffect1`, ...) share one.
    Attribute(String),
}

impl Column {
    fn parse(header: &str) -> Column {
        let base = header.trim_end_matches(|c: char| c.is_ascii_digit());
        match (header, base) {
            ("id", _) => Column::Id,
            ("name", _) => Column::Name,
            (_, "substitute") => Column::Attribute("substitute".into()),
            (_, "sideEffect") => Column::Attribute("side_effect".into()),
            (_, "use") => Column::Attribute("use".into()),
            ("Chemical Class", _) => Column::Attribute("chemical_class".into()),
            ("Habit Forming", _) => Column::Attribute("habit_forming".into()),
            ("Therapeutic Class", _) => Column::Attribute("therapeutic_class".into()),
            ("Action Class", _) => Column::Attribute("action_class".into()),
            (other, _) => Column::Attribute(other.to_owned()),
        }
    }
}

fn csv_member(input: &Input) -> Result<String> {
    match input
        .member_names()
        .into_iter()
        .find(|m| m.to_ascii_lowercase().ends_with(".csv"))
    {
        Some(m) => Ok(m),
        None => bail!("{} contains no .csv file", input.file_name),
    }
}

fn import_row(
    columns: &[Column],
    rec: &[String],
    sink: &mut ImportSink,
    classes: &mut HashMap<(String, String), i64>,
) -> Result<()> {
    let field = |want: fn(&Column) -> bool| {
        columns
            .iter()
            .zip(rec)
            .find(|(c, _)| want(c))
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    };
    let (Some(code), Some(name)) = (
        field(|c| matches!(c, Column::Id)),
        field(|c| matches!(c, Column::Name)),
    ) else {
        return Ok(());
    };
    let id = sink.concept(code, Kind::Product, "medicine", name)?;
    sink.name(id, name, NameType::Preferred, "medicine", None)?;
    sink.identifier(id, CODE_SYSTEM, code)?;
    for (column, value) in columns.iter().zip(rec) {
        let Column::Attribute(key) = column else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() || value == "NA" {
            continue;
        }
        sink.attribute(id, key, value)?;
        if CLASS_KEYS.contains(&key.as_str()) {
            let class_key = (key.clone(), slug(value));
            let class = match classes.get(&class_key) {
                Some(&c) => c,
                None => {
                    let c = sink.classification(key, &class_key.1, value)?;
                    classes.insert(class_key, c);
                    c
                }
            };
            sink.classify(id, class)?;
        }
    }
    Ok(())
}
