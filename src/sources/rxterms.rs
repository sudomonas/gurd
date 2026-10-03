//! RxTerms (NLM Lister Hill Center): RxNorm clinical drugs with the display names, routes,
//! dose forms and strength lists prescribers see, as a zip of `|`-separated files:
//! `RxTerms<YYYYMM>.txt` (one row per drug) and `RxTermsIngredients<YYYYMM>.txt`.
//!
//! Concepts are keyed by RxCUI, so they link to RxNorm's. NLM describes RxTerms as "free to
//! use" without stating redistribution terms, so it is treated as local-only.

use std::collections::HashMap;
use std::io::BufRead;

use anyhow::{Result, bail};
use rusqlite::Connection;

use super::{ImportSink, Input, Release, Source, SourceInfo};
use crate::models::{Kind, NameType, Section};

pub struct RxTerms;

impl Source for RxTerms {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            slug: "rxterms".into(),
            title: "RxTerms".into(),
            code_system: "rxcui".into(),
            provider: "U.S. National Library of Medicine, Lister Hill National Center for \
                       Biomedical Communications"
                .into(),
            license: "\"Free to use\" (NLM). No explicit redistribution terms are published, \
                      so keep it local."
                .into(),
            attribution: "RxTerms, U.S. National Library of Medicine.".into(),
            url: "https://lhncbc.nlm.nih.gov/MOR/RxTerms/".into(),
            redistributable: false,
            stale_after_days: Some(45),
            download_url: None,
            checksums_url: None,
            notice: None,
        }
    }

    fn release(&self, input: &Input) -> Result<Release> {
        let Some(version) = main_member(input).and_then(|m| month(&m)) else {
            bail!(
                "{} is not an RxTerms release (expected RxTerms<YYYYMM>.txt inside)",
                input.file_name
            );
        };
        Ok(Release {
            release_date: Some(format!("{version}-01")),
            version,
        })
    }

    fn import(&self, input: &Input, sink: &mut ImportSink) -> Result<()> {
        let Some(main) = main_member(input) else {
            bail!("{} contains no RxTerms<YYYYMM>.txt", input.file_name);
        };
        let mut drugs: HashMap<String, i64> = HashMap::new();
        input.read(&main, |r| {
            each_row(r, |row| {
                let get = |k: &str| row.get(k).copied().unwrap_or("").trim();
                if !get("IS_RETIRED").is_empty() {
                    return Ok(());
                }
                let rxcui = get("RXCUI");
                let kind = match get("TTY") {
                    "SCD" => Kind::ClinicalDrug,
                    "SBD" => Kind::BrandedDrug,
                    "GPCK" => Kind::GenericPack,
                    "BPCK" => Kind::BrandedPack,
                    _ => return Ok(()),
                };
                if rxcui.is_empty() || drugs.contains_key(rxcui) {
                    return Ok(());
                }
                // RxTerms' own presentation: display name plus strength, as on a
                // prescriber's pick list, e.g. "metFORMIN XR (Oral Pill) 500 mg".
                let full = get("FULL_NAME");
                let display = match (get("DISPLAY_NAME"), get("STRENGTH")) {
                    ("", _) => full.to_owned(),
                    (d, "") => d.to_owned(),
                    (d, s) => format!("{d} {s}"),
                };
                let name = display.as_str();
                let id = sink.concept(rxcui, kind, get("TTY"), name)?;
                drugs.insert(rxcui.to_owned(), id);
                sink.identifier(id, "rxcui", rxcui)?;
                sink.name(id, name, NameType::Preferred, "DISPLAY_NAME STRENGTH", None)?;
                if full != name {
                    sink.name(id, full, NameType::Synonym, "FULL_NAME", None)?;
                }
                for (col, ty) in [
                    ("DISPLAY_NAME", NameType::Synonym),
                    ("DISPLAY_NAME_SYNONYM", NameType::Synonym),
                    ("PSN", NameType::Prescribable),
                ] {
                    let v = get(col);
                    if !v.is_empty() && v != name {
                        sink.name(id, v, ty, col, None)?;
                    }
                }
                for &(col, key, _, _) in COLUMNS {
                    let v = get(col);
                    if !v.is_empty() {
                        sink.attribute(id, key, v)?;
                    }
                }
                Ok(())
            })
        })?;

        if let Some(member) = ingredients_member(input) {
            let mut ingredients: HashMap<String, i64> = HashMap::new();
            input.read(&member, |r| {
                each_row(r, |row| {
                    let get = |k: &str| row.get(k).copied().unwrap_or("").trim();
                    let (Some(&drug), ing_rxcui, name) =
                        (drugs.get(get("RXCUI")), get("ING_RXCUI"), get("INGREDIENT"))
                    else {
                        return Ok(());
                    };
                    if ing_rxcui.is_empty() || name.is_empty() {
                        return Ok(());
                    }
                    let ing = match ingredients.get(ing_rxcui) {
                        Some(&i) => i,
                        None => {
                            let i = sink.concept(ing_rxcui, Kind::Ingredient, "IN", name)?;
                            sink.identifier(i, "rxcui", ing_rxcui)?;
                            sink.name(i, name, NameType::Preferred, "INGREDIENT", None)?;
                            ingredients.insert(ing_rxcui.to_owned(), i);
                            i
                        }
                    };
                    sink.relationship(drug, "has_ingredient", ing, "RxTermsIngredients")?;
                    sink.relationship(ing, "ingredient_of", drug, "RxTermsIngredients")?;
                    Ok(())
                })
            })?;
        }

        use Kind::*;
        for (from, section, path, to) in [
            (
                Ingredient,
                Section::ClinicalDrugs,
                "ingredient_of",
                ClinicalDrug,
            ),
            (
                Ingredient,
                Section::BrandedDrugs,
                "ingredient_of",
                BrandedDrug,
            ),
            (Ingredient, Section::Packs, "ingredient_of", GenericPack),
            (Ingredient, Section::Packs, "ingredient_of", BrandedPack),
            (
                ClinicalDrug,
                Section::Ingredients,
                "has_ingredient",
                Ingredient,
            ),
            (
                BrandedDrug,
                Section::Ingredients,
                "has_ingredient",
                Ingredient,
            ),
            (
                GenericPack,
                Section::Ingredients,
                "has_ingredient",
                Ingredient,
            ),
            (
                BrandedPack,
                Section::Ingredients,
                "has_ingredient",
                Ingredient,
            ),
        ] {
            sink.navigation(from, section, &[path], to)?;
        }
        for (rank, &(_, key, label, summary)) in COLUMNS.iter().enumerate() {
            sink.attribute_key(key, label, rank as i64, summary)?;
        }
        Ok(())
    }

    fn validate(&self, conn: &Connection, source_id: i64) -> Result<()> {
        let n: i64 = conn.query_row(
            "SELECT count(*) FROM concepts WHERE source_id = ?1 AND kind = 'clinical_drug'",
            [source_id],
            |r| r.get(0),
        )?;
        if n == 0 {
            bail!("no clinical drugs imported");
        }
        Ok(())
    }
}

/// RxTerms column → (attribute key, label, shown on the summary page).
#[rustfmt::skip]
const COLUMNS: &[(&str, &str, &str, bool)] = &[
    ("FULL_NAME", "full_name", "Full name", true),
    ("DISPLAY_NAME", "display_name", "Display name", false),
    ("BRAND_NAME", "brand_name", "Brand", true),
    ("ROUTE", "route", "Route", true),
    ("NEW_DOSE_FORM", "dose_form", "Dose form", true),
    ("STRENGTH", "strength", "Strength", true),
    ("FULL_GENERIC_NAME", "generic_name", "Generic", false),
    ("RXN_DOSE_FORM", "rxnorm_dose_form", "RxNorm dose form", false),
    ("SXDG_NAME", "dose_form_group", "Dose form group", false),
    ("GENERIC_RXCUI", "generic_rxcui", "Generic RxCUI", false),
    ("SUPPRESS_FOR", "suppress_for", "Suppress for", false),
];

fn main_member(input: &Input) -> Option<String> {
    input.member_names().into_iter().find(|m| {
        let base = m.rsplit('/').next().unwrap_or(m);
        base.starts_with("RxTerms") && month(base).is_some()
    })
}

fn ingredients_member(input: &Input) -> Option<String> {
    input.member_names().into_iter().find(|m| {
        m.rsplit('/')
            .next()
            .is_some_and(|b| b.starts_with("RxTermsIngredients") && b.ends_with(".txt"))
    })
}

/// `RxTerms202609.txt` → `2026-09`.
fn month(name: &str) -> Option<String> {
    let base = name.rsplit('/').next()?;
    let digits = base.strip_prefix("RxTerms")?.strip_suffix(".txt")?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let m: u32 = digits[4..].parse().ok()?;
    (1..=12)
        .contains(&m)
        .then(|| format!("{}-{}", &digits[..4], &digits[4..]))
}

/// Calls `f` with each data row of a `|`-separated file with a header row, as a map from
/// column name to value.
fn each_row(
    reader: &mut dyn BufRead,
    mut f: impl FnMut(&HashMap<&str, &str>) -> Result<()>,
) -> Result<()> {
    let mut header: Vec<String> = Vec::new();
    let mut line = String::new();
    let mut number = 0u64;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        number += 1;
        let text = line.trim_end_matches(['\n', '\r']);
        if number == 1 {
            header = text.split('|').map(str::to_owned).collect();
            continue;
        }
        if text.is_empty() {
            continue;
        }
        let row: HashMap<&str, &str> = header
            .iter()
            .map(String::as_str)
            .zip(text.split('|'))
            .collect();
        f(&row).map_err(|e| e.context(format!("line {number}")))?;
    }
}

#[cfg(test)]
mod tests {
    use super::month;

    #[test]
    fn release_months() {
        assert_eq!(month("RxTerms202609.txt").as_deref(), Some("2026-09"));
        assert_eq!(month("RxTermsIngredients202609.txt"), None);
        assert_eq!(month("RxTerms202613.txt"), None);
    }
}
