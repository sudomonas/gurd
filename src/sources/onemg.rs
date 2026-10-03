//! 1mg medicines: a third-party scrape of 1mg.com (Tata 1mg), published on Kaggle as
//! JSON-lines files (`kaggle_medicines.json`, `kaggle_capsules.json`, ...).
//!
//! Unofficial and not redistributable: the adapter only reads a copy the user supplies.
//!
//! Each line is one product:
//! `{"_id": {"$oid": ...}, "tabletname": ..., "composition": "A (500mg) + B (125mg)",
//!   "marketer", "prescription", "price", "img", "brief", "uses": [...],
//!   "sideeffects": [...], "sideeffectbrief"}`.
//! The name key depends on the file (`tabletname`, `capsulename`, `injectionname`).
//! Ingredients are taken from the composition text exactly as written.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde_json::{Map, Value};

use super::{ImportSink, Input, Release, Source, SourceInfo, each_json_line, slug};
use crate::models::{Kind, NameType, Section};

pub struct OneMg;

const CODE_SYSTEM: &str = "onemg";

impl Source for OneMg {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            slug: "1mg".into(),
            title: "1mg medicines (third-party Kaggle scrape)".into(),
            code_system: CODE_SYSTEM.into(),
            provider: "Unofficial: scraped from 1mg.com (Tata 1mg) by a third party".into(),
            license: "No license. A third party scraped this data from 1mg.com; whether 1mg \
                      permits that is unknown and its terms may forbid it. Personal reference \
                      use only; do not redistribute."
                .into(),
            attribution: "Data originally published on 1mg.com by Tata 1mg Healthcare \
                          Solutions; collected and published on Kaggle by a third party."
                .into(),
            url: "https://www.1mg.com/".into(),
            redistributable: false,
            stale_after_days: None,
            download_url: None,
            checksums_url: None,
            notice: Some(
                "Unofficial third-party scrape of 1mg.com. Accuracy, completeness and date \
                 are unknown; prices are as listed when scraped."
                    .into(),
            ),
        }
    }

    fn release(&self, input: &Input) -> Result<Release> {
        let members = json_members(input);
        let Some(first) = members.first() else {
            bail!("{} contains no .json files", input.file_name);
        };
        // Check the shape on the first record rather than failing halfway through import.
        input.read(first, |r| {
            let mut checked = false;
            each_json_line(r, |o: Map<String, Value>| {
                if !checked {
                    if name_key(&o).is_none() || !o.contains_key("composition") {
                        bail!("not a 1mg medicines file (no *name or composition field)");
                    }
                    checked = true;
                }
                Ok(())
            })
        })?;
        // The dataset carries no version or date; the files' date stands in for one.
        let Some(date) = input.file_date() else {
            bail!("cannot determine the date of {}", input.file_name);
        };
        Ok(Release {
            version: date,
            release_date: None,
        })
    }

    fn import(&self, input: &Input, sink: &mut ImportSink) -> Result<()> {
        let mut ingredients: HashMap<String, i64> = HashMap::new();
        for member in json_members(input) {
            input.read(&member, |r| {
                each_json_line(r, |o: Map<String, Value>| {
                    import_record(&o, sink, &mut ingredients)
                })
            })?;
        }
        sink.navigation(
            Kind::Ingredient,
            Section::Products,
            &["ingredient_of"],
            Kind::Product,
        )?;
        sink.navigation(
            Kind::Product,
            Section::Ingredients,
            &["has_ingredient"],
            Kind::Ingredient,
        )?;
        for (rank, &(key, label, summary)) in ATTRIBUTE_KEYS.iter().enumerate() {
            sink.attribute_key(key, label, rank as i64, summary)?;
        }
        Ok(())
    }

    fn validate(&self, conn: &Connection, source_id: i64) -> Result<()> {
        for kind in [Kind::Product, Kind::Ingredient] {
            let n: i64 = conn.query_row(
                "SELECT count(*) FROM concepts WHERE source_id = ?1 AND kind = ?2",
                (source_id, kind.as_str()),
                |r| r.get(0),
            )?;
            if n == 0 {
                bail!("no {} records imported", kind.as_str());
            }
        }
        Ok(())
    }
}

#[rustfmt::skip]
const ATTRIBUTE_KEYS: &[(&str, &str, bool)] = &[
    ("composition", "Composition", true),
    ("marketer", "Marketer", true),
    ("prescription", "Prescription", true),
    ("price", "Price (₹, when scraped)", true),
    ("use", "Uses", true),
    ("side_effect", "Side effects", true),
    ("brief", "About", true),
    ("side_effects_note", "About side effects", false),
    ("listing", "Listed under", false),
    ("image", "Image", false),
];

fn json_members(input: &Input) -> Vec<String> {
    input
        .member_names()
        .into_iter()
        .filter(|m| {
            let m = m.to_ascii_lowercase();
            m.ends_with(".json") || m.ends_with(".jsonl")
        })
        .collect()
}

/// The product-name key: `tabletname`, `capsulename`, ...
fn name_key(o: &Map<String, Value>) -> Option<&str> {
    o.keys()
        .map(String::as_str)
        .find(|k| k.ends_with("name") && o[*k].is_string())
}

fn text<'a>(o: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    o.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn import_record(
    o: &Map<String, Value>,
    sink: &mut ImportSink,
    ingredients: &mut HashMap<String, i64>,
) -> Result<()> {
    let Some(key) = name_key(o) else {
        bail!("record has no name field");
    };
    let Some(name) = text(o, key) else {
        return Ok(());
    };
    let Some(oid) = o
        .get("_id")
        .and_then(|v| v.get("$oid"))
        .and_then(Value::as_str)
    else {
        bail!("record {name:?} has no _id");
    };
    // `tabletname` → "tablet"
    let listing = key.trim_end_matches("name");

    let id = sink.concept(oid, Kind::Product, listing, name)?;
    sink.name(id, name, NameType::Preferred, listing, None)?;
    sink.identifier(id, CODE_SYSTEM, oid)?;
    sink.attribute(id, "listing", listing)?;

    if let Some(composition) = text(o, "composition") {
        sink.attribute(id, "composition", composition)?;
        for (ingredient, _strength) in parse_composition(composition) {
            let code = slug(&ingredient);
            if code.is_empty() {
                continue;
            }
            let ing = match ingredients.get(&code) {
                Some(&ing) => ing,
                None => {
                    let ing = sink.concept(&code, Kind::Ingredient, "composition", &ingredient)?;
                    sink.name(ing, &ingredient, NameType::Preferred, "composition", None)?;
                    sink.identifier(ing, CODE_SYSTEM, &code)?;
                    ingredients.insert(code, ing);
                    ing
                }
            };
            sink.relationship(id, "has_ingredient", ing, "composition")?;
            sink.relationship(ing, "ingredient_of", id, "composition")?;
        }
    }
    for (field, attr) in [
        ("marketer", "marketer"),
        ("prescription", "prescription"),
        ("brief", "brief"),
        ("sideeffectbrief", "side_effects_note"),
        ("img", "image"),
    ] {
        if let Some(v) = text(o, field) {
            sink.attribute(id, attr, v)?;
        }
    }
    if let Some(price) = o.get("price").filter(|v| v.is_number()) {
        sink.attribute(id, "price", &price.to_string())?;
    }
    for (field, attr) in [("uses", "use"), ("sideeffects", "side_effect")] {
        for v in o.get(field).and_then(Value::as_array).into_iter().flatten() {
            if let Some(v) = v.as_str().map(str::trim).filter(|s| !s.is_empty()) {
                sink.attribute(id, attr, v)
                    .with_context(|| format!("{name}: {field}"))?;
            }
        }
    }
    Ok(())
}

/// Splits `"Amoxycillin (500mg) + Clavulanic Acid (125mg)"` into ingredient names and
/// strengths. The strength is the last parenthesized group of each part, so names that
/// contain parentheses themselves (`"Vitamin B6 (Pyridoxine) (10mg)"`) stay whole.
pub fn parse_composition(composition: &str) -> Vec<(String, Option<String>)> {
    composition
        .split(" + ")
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            if part.ends_with(')') {
                if let Some(open) = part.rfind('(') {
                    let name = part[..open].trim();
                    let strength = part[open + 1..part.len() - 1].trim();
                    if !name.is_empty() {
                        return Some((name.to_owned(), Some(strength.to_owned())));
                    }
                }
            }
            Some((part.to_owned(), None))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_compositions() {
        assert_eq!(
            parse_composition("Amoxycillin (500mg) + Clavulanic Acid (125mg)"),
            [
                ("Amoxycillin".to_owned(), Some("500mg".to_owned())),
                ("Clavulanic Acid".to_owned(), Some("125mg".to_owned())),
            ]
        );
        assert_eq!(
            parse_composition("Vitamin B6 (Pyridoxine) (10mg)"),
            [(
                "Vitamin B6 (Pyridoxine)".to_owned(),
                Some("10mg".to_owned())
            )]
        );
        assert_eq!(
            parse_composition("Lactobacillus"),
            [("Lactobacillus".to_owned(), None)]
        );
    }
}
