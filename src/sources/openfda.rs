//! openFDA NDC Directory (U.S. Food and Drug Administration): every drug product listed
//! with the FDA, as one JSON document `{"meta": {...}, "results": [product, ...]}` inside
//! `drug-ndc-0001-of-0001.json.zip`. CC0.
//!
//! Each product becomes a concept with its active ingredients as ingredient concepts. The
//! RxCUIs, UNIIs and SPL set ids openFDA attaches to a product are recorded as identifiers,
//! which links products to RxNorm and other sources. Package NDCs are converted to the
//! 11-digit form RxNorm uses, so `gurd id ndc:...` finds both.

use std::collections::HashMap;
use std::fmt;

use anyhow::{Result, anyhow, bail};
use rusqlite::Connection;
use serde::Deserialize;
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};

use super::{ImportSink, Input, Release, Source, SourceInfo, slug};
use crate::models::{Kind, NameType, Section};

pub struct OpenFda;

pub const DOWNLOAD_URL: &str =
    "https://download.open.fda.gov/drug/ndc/drug-ndc-0001-of-0001.json.zip";
const CODE_SYSTEM: &str = "openfda";

impl Source for OpenFda {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            slug: "openfda".into(),
            title: "openFDA NDC Directory".into(),
            code_system: CODE_SYSTEM.into(),
            provider: "U.S. Food and Drug Administration (openFDA)".into(),
            license: "CC0 1.0 Universal (public domain dedication). Do not imply that the FDA \
                      endorses this product."
                .into(),
            attribution: "Data from openFDA, U.S. Food and Drug Administration. The FDA does \
                          not endorse this product."
                .into(),
            url: "https://open.fda.gov/apis/drug/ndc/".into(),
            redistributable: true,
            // The NDC Directory is updated daily.
            stale_after_days: Some(30),
            download_url: Some(DOWNLOAD_URL.into()),
            checksums_url: None,
            notice: Some(
                "openFDA: \"Do not rely on openFDA to make decisions regarding medical care. \
                 You should assume all results are unvalidated.\""
                    .into(),
            ),
        }
    }

    fn release(&self, input: &Input) -> Result<Release> {
        let member = json_member(input)?;
        let mut meta = None;
        input.read(&member, |r| {
            let mut de = serde_json::Deserializer::from_reader(r);
            // Stops after `meta`; it comes first in openFDA's files.
            let _ = (Document {
                on_meta: &mut |m: Meta| {
                    meta = Some(m);
                    Err(anyhow!("stop"))
                },
                on_product: &mut |_| Ok(()),
            })
            .deserialize(&mut de);
            Ok(())
        })?;
        let Some(updated) = meta.and_then(|m| m.last_updated) else {
            bail!(
                "{} is not an openFDA NDC file (no meta.last_updated)",
                input.file_name
            );
        };
        Ok(Release {
            version: updated.clone(),
            release_date: Some(updated),
        })
    }

    fn import(&self, input: &Input, sink: &mut ImportSink) -> Result<()> {
        let member = json_member(input)?;
        let mut ingredients: HashMap<String, i64> = HashMap::new();
        let mut classes: HashMap<(String, String), i64> = HashMap::new();
        input.read(&member, |r| {
            let mut de = serde_json::Deserializer::from_reader(r);
            (Document {
                on_meta: &mut |_| Ok(()),
                on_product: &mut |p: Product| {
                    import_product(&p, sink, &mut ingredients, &mut classes)
                },
            })
            .deserialize(&mut de)
            .map_err(|e| anyhow!("{e}"))
        })?;
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
        let n: i64 = conn.query_row(
            "SELECT count(*) FROM concepts WHERE source_id = ?1 AND kind = 'product'",
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
    ("active_ingredient", "Active ingredients", true),
    ("dosage_form", "Dosage form", true),
    ("labeler_name", "Labeler", true),
    ("generic_name", "Generic name", true),
    ("brand_name", "Brand name", true),
    ("route", "Route", true),
    ("product_type", "Product type", true),
    ("marketing_category", "Marketing category", true),
    ("dea_schedule", "DEA schedule", true),
    ("pharm_class", "Pharmacologic class", true),
    ("product_ndc", "Product NDC", true),
    ("application_number", "Application", false),
    ("manufacturer_name", "Manufacturer", false),
    ("marketing_start_date", "Marketing start", false),
    ("marketing_end_date", "Marketing end", true),
    ("listing_expiration_date", "Listing expires", false),
    ("finished", "Finished product", false),
    ("package", "Packages", false),
];

#[derive(Deserialize)]
struct Meta {
    last_updated: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Product {
    product_id: String,
    product_ndc: String,
    generic_name: Option<String>,
    brand_name: Option<String>,
    brand_name_suffix: Option<String>,
    labeler_name: Option<String>,
    active_ingredients: Vec<Ingredient>,
    dosage_form: Option<String>,
    route: Vec<String>,
    product_type: Option<String>,
    marketing_category: Option<String>,
    application_number: Option<String>,
    dea_schedule: Option<String>,
    pharm_class: Vec<String>,
    marketing_start_date: Option<String>,
    marketing_end_date: Option<String>,
    listing_expiration_date: Option<String>,
    finished: Option<bool>,
    packaging: Vec<Package>,
    openfda: OpenFdaFields,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Ingredient {
    name: String,
    strength: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Package {
    package_ndc: String,
    description: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct OpenFdaFields {
    rxcui: Vec<String>,
    unii: Vec<String>,
    spl_set_id: Vec<String>,
    manufacturer_name: Vec<String>,
}

fn json_member(input: &Input) -> Result<String> {
    match input
        .member_names()
        .into_iter()
        .find(|m| m.to_ascii_lowercase().ends_with(".json"))
    {
        Some(m) => Ok(m),
        None => bail!("{} contains no .json file", input.file_name),
    }
}

fn import_product(
    p: &Product,
    sink: &mut ImportSink,
    ingredients: &mut HashMap<String, i64>,
    classes: &mut HashMap<(String, String), i64>,
) -> Result<()> {
    if p.product_id.is_empty() {
        return Ok(());
    }
    let brand = p.brand_name.as_deref().map(|b| match &p.brand_name_suffix {
        Some(suffix) if !suffix.is_empty() => format!("{b} {suffix}"),
        _ => b.to_owned(),
    });
    let Some(name) = brand.clone().or_else(|| p.generic_name.clone()) else {
        return Ok(());
    };
    let product_type = p.product_type.as_deref().unwrap_or("");
    let id = sink.concept(&p.product_id, Kind::Product, product_type, &name)?;
    sink.name(id, &name, NameType::Preferred, "brand_name", None)?;
    if let Some(generic) = p
        .generic_name
        .as_deref()
        .filter(|g| Some(*g) != brand.as_deref())
    {
        sink.name(id, generic, NameType::Synonym, "generic_name", None)?;
    }
    sink.identifier(id, CODE_SYSTEM, &p.product_id)?;
    for rxcui in &p.openfda.rxcui {
        sink.identifier(id, "rxcui", rxcui)?;
    }
    for unii in &p.openfda.unii {
        sink.identifier(id, "unii", unii)?;
    }
    for set_id in &p.openfda.spl_set_id {
        sink.identifier(id, "spl_set_id", set_id)?;
    }
    for package in &p.packaging {
        if let Some(ndc) = ndc11(&package.package_ndc) {
            sink.identifier(id, "ndc", &ndc)?;
        }
        let text = match &package.description {
            Some(d) => format!("{} {d}", package.package_ndc),
            None => package.package_ndc.clone(),
        };
        sink.attribute(id, "package", &text)?;
    }

    for ing in &p.active_ingredients {
        let code = slug(&ing.name);
        if code.is_empty() {
            continue;
        }
        let ing_id = match ingredients.get(&code) {
            Some(&i) => i,
            None => {
                let i = sink.concept(&code, Kind::Ingredient, "active_ingredient", &ing.name)?;
                sink.name(i, &ing.name, NameType::Preferred, "active_ingredient", None)?;
                sink.identifier(i, CODE_SYSTEM, &code)?;
                ingredients.insert(code, i);
                i
            }
        };
        // A single-ingredient product's UNII is that ingredient's UNII.
        if p.active_ingredients.len() == 1 && p.openfda.unii.len() == 1 {
            sink.identifier(ing_id, "unii", &p.openfda.unii[0])?;
        }
        sink.relationship(id, "has_ingredient", ing_id, "active_ingredients")?;
        sink.relationship(ing_id, "ingredient_of", id, "active_ingredients")?;
        let text = match &ing.strength {
            Some(s) => format!("{} {s}", ing.name),
            None => ing.name.clone(),
        };
        sink.attribute(id, "active_ingredient", &text)?;
    }

    let fields = [
        ("generic_name", p.generic_name.as_deref()),
        ("brand_name", brand.as_deref()),
        ("dosage_form", p.dosage_form.as_deref()),
        ("labeler_name", p.labeler_name.as_deref()),
        ("product_type", p.product_type.as_deref()),
        ("marketing_category", p.marketing_category.as_deref()),
        ("application_number", p.application_number.as_deref()),
        ("dea_schedule", p.dea_schedule.as_deref()),
        ("product_ndc", Some(p.product_ndc.as_str())),
    ];
    for (key, value) in fields {
        if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
            sink.attribute(id, key, v)?;
        }
    }
    for (key, value) in [
        ("marketing_start_date", &p.marketing_start_date),
        ("marketing_end_date", &p.marketing_end_date),
        ("listing_expiration_date", &p.listing_expiration_date),
    ] {
        if let Some(v) = value {
            sink.attribute(id, key, &iso_date(v))?;
        }
    }
    if let Some(f) = p.finished {
        sink.attribute(id, "finished", if f { "yes" } else { "no" })?;
    }
    for route in &p.route {
        sink.attribute(id, "route", route)?;
    }
    for m in &p.openfda.manufacturer_name {
        sink.attribute(id, "manufacturer_name", m)?;
    }
    for class in &p.pharm_class {
        sink.attribute(id, "pharm_class", class)?;
        let (name, system) = split_class(class);
        let key = (system.clone(), slug(name));
        let class_id = match classes.get(&key) {
            Some(&c) => c,
            None => {
                let c = sink.classification(&system, &key.1, name)?;
                classes.insert(key, c);
                c
            }
        };
        sink.classify(id, class_id)?;
    }
    Ok(())
}

/// `"Biguanide [EPC]"` → (`"Biguanide"`, `"fda_epc"`). EPC, MoA, PE and CS are FDA
/// Established Pharmacologic Class, Mechanism of Action, Physiologic Effect and Chemical
/// Structure.
fn split_class(class: &str) -> (&str, String) {
    if let Some(open) = class.rfind(" [") {
        if class.ends_with(']') {
            let kind = &class[open + 2..class.len() - 1];
            return (&class[..open], format!("fda_{}", kind.to_ascii_lowercase()));
        }
    }
    (class, "fda_class".to_owned())
}

/// `20240329` → `2024-03-29`; anything else unchanged.
fn iso_date(s: &str) -> String {
    if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
        format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..])
    } else {
        s.to_owned()
    }
}

/// Converts a 10-digit NDC with dashes (4-4-2, 5-3-2 or 5-4-1) to the 11-digit 5-4-2 form
/// without dashes that RxNorm uses, by zero-padding the short segment.
pub fn ndc11(ndc: &str) -> Option<String> {
    let parts: Vec<&str> = ndc.split('-').collect();
    if parts.len() != 3 || !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let (a, b, c) = (parts[0], parts[1], parts[2]);
    match (a.len(), b.len(), c.len()) {
        (4, 4, 2) => Some(format!("0{a}{b}{c}")),
        (5, 3, 2) => Some(format!("{a}0{b}{c}")),
        (5, 4, 1) => Some(format!("{a}{b}0{c}")),
        (5, 4, 2) => Some(format!("{a}{b}{c}")),
        _ => None,
    }
}

/// Streams the top-level document: `meta` goes to `on_meta`, each element of `results` to
/// `on_product`, without holding the whole array in memory.
struct Document<'a> {
    on_meta: &'a mut dyn FnMut(Meta) -> Result<()>,
    on_product: &'a mut dyn FnMut(Product) -> Result<()>,
}

impl<'de> DeserializeSeed<'de> for Document<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for Document<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an openFDA document")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "meta" => {
                    let meta: Meta = map.next_value()?;
                    (self.on_meta)(meta).map_err(de::Error::custom)?;
                }
                "results" => map.next_value_seed(Results(&mut *self.on_product))?,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

struct Results<'a>(&'a mut dyn FnMut(Product) -> Result<()>);

impl<'de> DeserializeSeed<'de> for Results<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for Results<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("an array of products")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let mut n = 0u64;
        while let Some(p) = seq.next_element::<Product>()? {
            n += 1;
            let id = p.product_id.clone();
            (self.0)(p).map_err(|e| de::Error::custom(format!("product {n} ({id}): {e:#}")))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_ndcs() {
        assert_eq!(ndc11("0093-1048-01").as_deref(), Some("00093104801"));
        assert_eq!(ndc11("71610-941-60").as_deref(), Some("71610094160"));
        assert_eq!(ndc11("12345-6789-1").as_deref(), Some("12345678901"));
        assert_eq!(ndc11("12-34-5"), None);
    }

    #[test]
    fn splits_classes() {
        assert_eq!(
            split_class("Biguanide [EPC]"),
            ("Biguanide", "fda_epc".to_owned())
        );
        assert_eq!(
            split_class("Biguanides [CS]"),
            ("Biguanides", "fda_cs".to_owned())
        );
        assert_eq!(split_class("Other"), ("Other", "fda_class".to_owned()));
    }
}
