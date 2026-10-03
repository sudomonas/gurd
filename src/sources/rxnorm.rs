//! RxNorm Current Prescribable Content (U.S. National Library of Medicine).
//!
//! Reads RXNCONSO, RXNREL and RXNSAT from the release zip. Only SAB=RXNORM content is
//! imported, plus the UNII codes that the release carries on MTHSPL substance (SU) atoms.
//! Other MTHSPL content (product labels) is not imported.
//!
//! RRF conventions relied on (checked against the 2026-09-08 release):
//! * fields are `|`-separated with a trailing `|`;
//! * every RXCUI has exactly one non-synonym RXNORM term type, which says what it is;
//! * an RXNREL row `RXCUI1|..|RXCUI2|..|RELA` reads "RXCUI2 RELA RXCUI1", e.g.
//!   `6809|..|151827|..|tradename_of` is "Glucophage tradename_of metformin".

use std::collections::HashMap;
use std::io::BufRead;

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use super::{ImportSink, Input, Release, Source, SourceInfo};
use crate::models::{Kind, NameType, Section};

pub const ATTRIBUTION: &str = "This product uses publicly available data courtesy of the U.S. \
National Library of Medicine (NLM), National Institutes of Health, Department of Health and \
Human Services; NLM is not responsible for the product and does not endorse or recommend this \
or any other product.";

/// NLM's RxNorm downloads page, which lists each release with its MD5 checksum.
pub const FILES_PAGE: &str = "https://www.nlm.nih.gov/research/umls/rxnorm/docs/rxnormfiles.html";

/// Always the newest prescribable release. NLM publishes no checksum for this file; to
/// verify a download, pass a dated release URL from FILES_PAGE with its MD5.
pub const CURRENT_URL: &str =
    "https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_current.zip";

pub struct RxNorm;

impl Source for RxNorm {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            slug: "rxnorm".into(),
            title: "RxNorm Current Prescribable Content".into(),
            code_system: "rxcui".into(),
            provider: "U.S. National Library of Medicine".into(),
            license: "Public domain; no UMLS license required. Subject to the RxNorm terms of \
                      service: attribution required; redistributed copies must be kept current \
                      or disclose that they may not reflect the latest NLM data."
                .into(),
            attribution: ATTRIBUTION.into(),
            url: "https://www.nlm.nih.gov/research/umls/rxnorm/docs/prescribe.html".into(),
            redistributable: true,
            // Monthly releases. The RxNorm terms require redistributed copies to be kept
            // current or to disclose that they may not reflect the latest NLM data.
            stale_after_days: Some(45),
            download_url: Some(CURRENT_URL.into()),
            checksums_url: Some(FILES_PAGE.into()),
            notice: None,
        }
    }

    fn release(&self, input: &Input) -> Result<Release> {
        // The readme inside the archive is authoritative; the zip may have been renamed.
        let date = input
            .member_names()
            .iter()
            .find_map(|m| prescribe_date(m))
            .or_else(|| prescribe_date(&input.file_name));
        let Some(date) = date else {
            bail!(
                "{} is not an RxNorm Current Prescribable Content release \
                 (expected Readme_Full_Prescribe_MMDDYYYY.txt inside)",
                input.file_name
            );
        };
        Ok(Release {
            version: date.clone(),
            release_date: Some(date),
        })
    }

    fn import(&self, input: &Input, sink: &mut ImportSink) -> Result<()> {
        let concepts = import_concepts(input, sink)?;
        import_relationships(input, sink, &concepts)?;
        import_attributes(input, sink, &concepts)?;
        for &(from, section, path, to) in NAVIGATION {
            sink.navigation(from, section, path, to)?;
        }
        for (rank, &(key, label, summary)) in ATTRIBUTE_KEYS.iter().enumerate() {
            sink.attribute_key(key, label, rank as i64, summary)?;
        }
        Ok(())
    }

    fn validate(&self, conn: &Connection, source_id: i64) -> Result<()> {
        for kind in [Kind::Ingredient, Kind::ClinicalDrug] {
            let n: i64 = conn.query_row(
                "SELECT count(*) FROM concepts WHERE source_id = ?1 AND kind = ?2",
                (source_id, kind.as_str()),
                |r| r.get(0),
            )?;
            if n == 0 {
                bail!("no {} concepts imported", kind.as_str());
            }
        }
        let without_rxcui: i64 = conn.query_row(
            "SELECT count(*) FROM concepts c WHERE c.source_id = ?1 AND NOT EXISTS (
                 SELECT 1 FROM identifiers i
                 WHERE i.concept_id = c.id AND i.system = 'rxcui' AND i.value = c.source_code)",
            [source_id],
            |r| r.get(0),
        )?;
        if without_rxcui > 0 {
            bail!("{without_rxcui} concepts have no RxCUI identifier");
        }
        let relationships: i64 = conn.query_row(
            "SELECT count(*) FROM relationships WHERE source_id = ?1",
            [source_id],
            |r| r.get(0),
        )?;
        if relationships == 0 {
            bail!("no relationships imported");
        }
        Ok(())
    }
}

/// How RxNorm's graph answers "which clinical drugs contain this ingredient?" and similar.
/// Each path was checked against the relationship patterns of the 2026-09-08 release.
/// Paths go through components rather than brand names, because a brand name can cover
/// products with different ingredients.
#[rustfmt::skip]
const NAVIGATION: &[(Kind, Section, &[&str], Kind)] = {
    use Kind::*;
    use Section::*;
    &[
        (Ingredient, PreciseIngredients, &["has_form"], PreciseIngredient),
        (Ingredient, Brands, &["has_tradename"], BrandName),
        (Ingredient, ClinicalDrugs, &["ingredient_of", "constitutes"], ClinicalDrug),
        (Ingredient, BrandedDrugs, &["ingredient_of", "constitutes"], BrandedDrug),
        (Ingredient, Combinations, &["part_of"], MultipleIngredients),

        (PreciseIngredient, Ingredients, &["form_of"], Ingredient),
        (PreciseIngredient, Brands, &["precise_ingredient_of"], BrandName),
        (PreciseIngredient, ClinicalDrugs, &["precise_ingredient_of", "constitutes"], ClinicalDrug),
        (PreciseIngredient, BrandedDrugs, &["precise_ingredient_of", "constitutes"], BrandedDrug),
        (PreciseIngredient, Combinations, &["part_of"], MultipleIngredients),

        (MultipleIngredients, Ingredients, &["has_part"], Ingredient),
        (MultipleIngredients, Ingredients, &["has_part"], PreciseIngredient),
        (MultipleIngredients, ClinicalDrugs, &["ingredients_of"], ClinicalDrug),
        (MultipleIngredients, BrandedDrugs, &["ingredients_of", "has_tradename"], BrandedDrug),

        (BrandName, Ingredients, &["tradename_of"], Ingredient),
        (BrandName, PreciseIngredients, &["has_precise_ingredient"], PreciseIngredient),
        (BrandName, BrandedDrugs, &["ingredient_of"], BrandedDrug),
        (BrandName, Packs, &["ingredient_of", "contained_in"], BrandedPack),

        (ClinicalDrug, Ingredients, &["consists_of", "has_ingredient"], Ingredient),
        (ClinicalDrug, PreciseIngredients, &["consists_of", "has_precise_ingredient"], PreciseIngredient),
        (ClinicalDrug, BrandedDrugs, &["has_tradename"], BrandedDrug),
        (ClinicalDrug, DoseForms, &["has_dose_form"], DoseForm),
        (ClinicalDrug, Packs, &["contained_in"], GenericPack),
        (ClinicalDrug, Packs, &["contained_in"], BrandedPack),

        (BrandedDrug, Ingredients, &["consists_of", "has_ingredient"], Ingredient),
        (BrandedDrug, PreciseIngredients, &["consists_of", "has_precise_ingredient"], PreciseIngredient),
        (BrandedDrug, Brands, &["has_ingredient"], BrandName),
        (BrandedDrug, ClinicalDrugs, &["tradename_of"], ClinicalDrug),
        (BrandedDrug, DoseForms, &["has_dose_form"], DoseForm),
        (BrandedDrug, Packs, &["contained_in"], BrandedPack),

        (GenericPack, Contents, &["contains"], ClinicalDrug),
        (GenericPack, Packs, &["has_tradename"], BrandedPack),
        (GenericPack, DoseForms, &["has_dose_form"], DoseForm),
        (BrandedPack, Contents, &["contains"], ClinicalDrug),
        (BrandedPack, Contents, &["contains"], BrandedDrug),
        (BrandedPack, Packs, &["tradename_of"], GenericPack),
        (BrandedPack, DoseForms, &["has_dose_form"], DoseForm),

        (ClinicalComponent, Ingredients, &["has_ingredient"], Ingredient),
        (ClinicalComponent, PreciseIngredients, &["has_precise_ingredient"], PreciseIngredient),
        (ClinicalComponent, ClinicalDrugs, &["constitutes"], ClinicalDrug),
        (ClinicalComponent, BrandedDrugs, &["constitutes"], BrandedDrug),
        (BrandedComponent, Brands, &["has_ingredient"], BrandName),
        (BrandedComponent, BrandedDrugs, &["constitutes"], BrandedDrug),
    ]
};

/// Display labels for RxNorm attribute names (ATN), in display order. Unlisted
/// attributes are shown in the detailed view under their own names.
#[rustfmt::skip]
const ATTRIBUTE_KEYS: &[(&str, &str, bool)] = &[
    ("RXN_STRENGTH", "Strength", true),
    ("RXN_AVAILABLE_STRENGTH", "Available strength", true),
    ("RXN_QUANTITY", "Quantity", true),
    ("RXN_QUALITATIVE_DISTINCTION", "Distinction", true),
    ("RXTERM_FORM", "RxTerms form", false),
    ("RXN_HUMAN_DRUG", "Human drug", false),
    ("RXN_VET_DRUG", "Veterinary drug", false),
    ("RXN_BN_CARDINALITY", "Brand cardinality", false),
    ("RXN_IN_EXPRESSED_FLAG", "Ingredient expressed", false),
    ("RXN_BOSS_FROM", "Strength basis", false),
    ("RXN_AI", "Active ingredient", false),
    ("RXN_AM", "Active moiety", false),
    ("RXN_BOSS_STRENGTH_NUM_VALUE", "Strength numerator", false),
    ("RXN_BOSS_STRENGTH_NUM_UNIT", "Strength numerator unit", false),
    ("RXN_BOSS_STRENGTH_DENOM_VALUE", "Strength denominator", false),
    ("RXN_BOSS_STRENGTH_DENOM_UNIT", "Strength denominator unit", false),
    ("RXN_ACTIVATED", "Activated", false),
    ("RXN_OBSOLETED", "Obsoleted", false),
];

/// RxNorm term type → application kind, for the term types that define a concept.
fn kind_for(tty: &str) -> Option<Kind> {
    Some(match tty {
        "IN" => Kind::Ingredient,
        "PIN" => Kind::PreciseIngredient,
        "MIN" => Kind::MultipleIngredients,
        "BN" => Kind::BrandName,
        "SCD" => Kind::ClinicalDrug,
        "SBD" => Kind::BrandedDrug,
        "GPCK" => Kind::GenericPack,
        "BPCK" => Kind::BrandedPack,
        "SCDC" => Kind::ClinicalComponent,
        "SBDC" => Kind::BrandedComponent,
        "SCDF" => Kind::ClinicalDoseForm,
        "SCDFP" => Kind::ClinicalDoseFormPrecise,
        "SBDF" => Kind::BrandedDoseForm,
        "SBDFP" => Kind::BrandedDoseFormPrecise,
        "SCDG" => Kind::ClinicalDoseFormGroup,
        "SCDGP" => Kind::ClinicalDoseFormGroupPrecise,
        "SBDG" => Kind::BrandedDoseFormGroup,
        "DF" => Kind::DoseForm,
        "DFG" => Kind::DoseFormGroup,
        _ => return None,
    })
}

fn name_type_for(tty: &str) -> NameType {
    match tty {
        "TMSY" => NameType::TallMan,
        "PSN" => NameType::Prescribable,
        _ => NameType::Synonym,
    }
}

struct Atom {
    rxcui: String,
    rxaui: String,
    tty: String,
    name: String,
}

/// RXCUI → concept id.
type ConceptMap = HashMap<String, i64>;

fn import_concepts(input: &Input, sink: &mut ImportSink) -> Result<ConceptMap> {
    // RXNCONSO: RXCUI LAT TS LUI STT SUI ISPREF RXAUI SAUI SCUI SDUI SAB TTY CODE STR SRL SUPPRESS CVF
    let mut atoms = Vec::new();
    let mut uniis = Vec::new();
    input.read("rrf/RXNCONSO.RRF", |r| {
        each_row(r, 18, |f| {
            if f[16] != "N" {
                return Ok(());
            }
            match (f[11], f[12]) {
                ("RXNORM", tty) => atoms.push(Atom {
                    rxcui: f[0].to_owned(),
                    rxaui: f[7].to_owned(),
                    tty: tty.to_owned(),
                    name: f[14].to_owned(),
                }),
                ("MTHSPL", "SU") if !f[13].is_empty() => {
                    uniis.push((f[0].to_owned(), f[13].to_owned()));
                }
                _ => {}
            }
            Ok(())
        })
    })?;

    let mut concepts = ConceptMap::new();
    let mut preferred = Vec::new();
    for (i, atom) in atoms.iter().enumerate() {
        let Some(kind) = kind_for(&atom.tty) else {
            continue;
        };
        if concepts.contains_key(&atom.rxcui) {
            bail!("RXCUI {} has more than one concept term type", atom.rxcui);
        }
        let id = sink.concept(&atom.rxcui, kind, &atom.tty, &atom.name)?;
        sink.identifier(id, "rxcui", &atom.rxcui)?;
        concepts.insert(atom.rxcui.clone(), id);
        preferred.push(i);
    }

    let mut is_preferred = vec![false; atoms.len()];
    for i in preferred {
        is_preferred[i] = true;
    }
    for (atom, preferred) in atoms.iter().zip(is_preferred) {
        // Atoms of RXCUIs with an unrecognized term type are skipped.
        let Some(&id) = concepts.get(&atom.rxcui) else {
            continue;
        };
        let name_type = if preferred {
            NameType::Preferred
        } else {
            name_type_for(&atom.tty)
        };
        sink.name(id, &atom.name, name_type, &atom.tty, Some(&atom.rxaui))?;
    }

    for (rxcui, unii) in &uniis {
        if let Some(&id) = concepts.get(rxcui) {
            sink.identifier(id, "unii", unii)?;
        }
    }
    Ok(concepts)
}

fn import_relationships(input: &Input, sink: &mut ImportSink, concepts: &ConceptMap) -> Result<()> {
    // RXNREL: RXCUI1 RXAUI1 STYPE1 REL RXCUI2 RXAUI2 STYPE2 RELA RUI SRUI SAB SL DIR RG SUPPRESS CVF
    input.read("rrf/RXNREL.RRF", |r| {
        each_row(r, 16, |f| {
            if f[10] != "RXNORM" || f[2] != "CUI" || f[6] != "CUI" || f[7].is_empty() {
                return Ok(());
            }
            if !matches!(f[14], "" | "N") {
                return Ok(());
            }
            if let (Some(&subject), Some(&object)) = (concepts.get(f[4]), concepts.get(f[0])) {
                sink.relationship(subject, f[7], object, f[7])?;
            }
            Ok(())
        })
    })
}

fn import_attributes(input: &Input, sink: &mut ImportSink, concepts: &ConceptMap) -> Result<()> {
    // RXNSAT: RXCUI LUI SUI RXAUI STYPE CODE ATUI SATUI ATN SAB ATV SUPPRESS CVF
    input.read("rrf/RXNSAT.RRF", |r| {
        each_row(r, 13, |f| {
            if f[9] != "RXNORM" || f[11] != "N" {
                return Ok(());
            }
            let Some(&id) = concepts.get(f[0]) else {
                return Ok(());
            };
            match f[8] {
                "NDC" => sink.identifier(id, "ndc", f[10]),
                atn => sink.attribute(id, atn, f[10]),
            }
        })
    })
}

/// Calls `f` with the fields of each line, checking the field count.
fn each_row(
    reader: &mut dyn BufRead,
    fields: usize,
    mut f: impl FnMut(&[&str]) -> Result<()>,
) -> Result<()> {
    let mut line = String::new();
    let mut number = 0u64;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(());
        }
        number += 1;
        let text = line.trim_end_matches(['\n', '\r']);
        if text.is_empty() {
            continue;
        }
        let row: Vec<&str> = text.split('|').collect();
        // Lines end with '|', which yields one extra empty field.
        if row.len() != fields + 1 {
            bail!(
                "line {number}: expected {fields} fields, found {}",
                row.len() - 1
            );
        }
        f(&row).with_context(|| format!("line {number}"))?;
    }
}

/// Extracts `YYYY-MM-DD` from names like `Readme_Full_Prescribe_09082026.txt`
/// or `RxNorm_full_prescribe_09082026.zip`.
fn prescribe_date(name: &str) -> Option<String> {
    let base = name.rsplit('/').next()?.to_ascii_lowercase();
    let rest = &base[base.find("prescribe_")? + "prescribe_".len()..];
    let digits = rest.get(..8)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (mm, dd, yyyy) = (&digits[0..2], &digits[2..4], &digits[4..8]);
    let (m, d): (u32, u32) = (mm.parse().ok()?, dd.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(format!("{yyyy}-{mm}-{dd}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_dates() {
        assert_eq!(
            prescribe_date("Readme_Full_Prescribe_09082026.txt").as_deref(),
            Some("2026-09-08")
        );
        assert_eq!(
            prescribe_date("dl/RxNorm_full_prescribe_09082026.zip").as_deref(),
            Some("2026-09-08")
        );
        assert_eq!(prescribe_date("RxNorm_full_09082026.zip"), None);
        assert_eq!(prescribe_date("Readme_Full_Prescribe_13082026.txt"), None);
    }

    #[test]
    fn maps_concept_term_types() {
        assert_eq!(kind_for("IN"), Some(Kind::Ingredient));
        assert_eq!(kind_for("SBD"), Some(Kind::BrandedDrug));
        assert_eq!(kind_for("SY"), None);
        assert_eq!(kind_for("PSN"), None);
    }
}
