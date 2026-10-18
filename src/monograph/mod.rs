pub mod catalog;

use std::collections::{BTreeMap, HashSet};
use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::database::Database;
use crate::details;
use crate::models::ConceptRef;
use crate::search;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrugMonograph {
    pub name: String,
    pub section: u8,
    pub pronunciation: Option<String>,
    pub tagline: Option<String>,
    pub therapeutic_class: Option<String>,
    pub pharmacologic_class: Option<String>,
    pub mechanism_class: Option<String>,
    pub chemical_class: Option<String>,
    pub atc_code: Option<String>,
    pub rxcui: Option<String>,
    pub unii: Option<String>,
    pub cas: Option<String>,
    pub prescription: Option<String>,
    pub habit_forming: Option<String>,
    pub routes: Vec<String>,
    pub dosage_forms: Vec<String>,
    pub common_strengths: Vec<String>,
    pub description: Option<String>,
    pub indications: Vec<String>,
    pub off_label_uses: Vec<String>,
    pub adult_dosing: Vec<String>,
    pub pediatric_dosing: Vec<String>,
    pub renal_adjustment: Vec<String>,
    pub hepatic_adjustment: Vec<String>,
    pub administration_tips: Vec<String>,
    pub boxed_warning: Option<String>,
    pub contraindications: Vec<String>,
    pub warnings_precautions: Vec<String>,
    pub adverse_reactions_common: Vec<String>,
    pub adverse_reactions_serious: Vec<String>,
    pub drug_interactions: Vec<String>,
    pub mechanism_of_action: Option<String>,
    pub pharmacokinetics: Vec<(String, String)>,
    pub pregnancy_lactation: Option<String>,
    pub us_brands: Vec<String>,
    pub india_brands: Vec<String>,
    pub substitutes: Vec<String>,
    pub see_also: Vec<String>,
}

/// Builds a drug monograph for `query`.
/// 1. First consults the curated clinical catalog.
/// 2. Enriches or synthesizes a complete monograph from local database records.
///
/// If `exact_only` is true, requires an exact concept match or catalog alias.
pub fn build(db: &Database, query: &str, exact_only: bool) -> Result<Option<DrugMonograph>> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(None);
    }

    // 1. Check curated catalog
    if let Some(mut monograph) = catalog::find_curated(q) {
        enrich_from_database(db, &mut monograph);
        return Ok(Some(monograph));
    }

    // Search for closest concept in database
    let hits = search::search(db, q, 5)?;
    let Some(top) = hits.first() else {
        return Ok(None);
    };

    if exact_only && top.matched != crate::search::Match::Exact && !top.concept.name.eq_ignore_ascii_case(q) {
        return Ok(None);
    }

    // Check if the top hit matches a curated monograph
    if let Some(mut monograph) = catalog::find_curated(&top.concept.name) {
        enrich_from_database(db, &mut monograph);
        return Ok(Some(monograph));
    }

    // 2. Synthesize from database
    synthesize_from_database(db, &top.concept)
}

/// Enriches an existing monograph with real brand names, formulations, and IDs from `gurd.db`.
fn enrich_from_database(db: &Database, m: &mut DrugMonograph) {
    let conn = db.connection();
    let norm = crate::normalize::normalize(&m.name);

    // Query identifiers if missing (indexed via names_norm)
    if m.rxcui.is_none() {
        if let Ok(mut stmt) = conn.prepare_cached(
            "SELECT i.value FROM names n
             JOIN identifiers i ON i.concept_id = n.concept_id
             WHERE i.system = 'rxcui' AND n.norm = ?1 LIMIT 1"
        ) {
            if let Ok(val) = stmt.query_row(params![norm], |r| r.get::<_, String>(0)) {
                m.rxcui = Some(val);
            }
        }
    }

    // Find linked brands in 1mg & az-india (indexed via names_norm and relationships_subject)
    if let Ok(mut stmt) = conn.prepare_cached(
        "SELECT DISTINCT c.name FROM names n
         JOIN relationships r ON r.subject_id = n.concept_id
         JOIN concepts c ON c.id = r.object_id
         WHERE n.norm = ?1 AND c.source_id IN (2, 3)
         ORDER BY c.name LIMIT 15"
    ) {
        if let Ok(rows) = stmt.query_map(params![norm], |r| r.get::<_, String>(0)) {
            for brand in rows.flatten() {
                let clean = clean_brand_name(&brand);
                if !clean.is_empty() && !m.india_brands.iter().any(|b| b.eq_ignore_ascii_case(&clean)) {
                    m.india_brands.push(clean);
                }
            }
        }
    }

    // Find substitutes from attributes (indexed via names_norm and attributes_concept)
    if let Ok(mut stmt) = conn.prepare_cached(
        "SELECT DISTINCT a.value FROM names n
         CROSS JOIN attributes a ON a.concept_id = n.concept_id
         WHERE n.norm = ?1 AND a.key = 'substitute'
         LIMIT 10"
    ) {
        if let Ok(rows) = stmt.query_map(params![norm], |r| r.get::<_, String>(0)) {
            for sub in rows.flatten() {
                if !m.substitutes.iter().any(|s| s.eq_ignore_ascii_case(&sub)) {
                    m.substitutes.push(sub);
                }
            }
        }
    }
}

/// Dynamically synthesizes a clinical monograph from the rich attributes in `gurd.db`.
fn synthesize_from_database(db: &Database, concept: &ConceptRef) -> Result<Option<DrugMonograph>> {
    let conn = db.connection();

    // 1. Identifiers
    let mut rxcui = None;
    let mut unii = None;
    if let Ok(mut stmt) = conn.prepare_cached(
        "SELECT system, value FROM identifiers WHERE concept_id = ?1"
    ) {
        if let Ok(rows) = stmt.query_map([concept.id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) {
            for (sys, val) in rows.flatten() {
                match sys.as_str() {
                    "rxcui" => rxcui = Some(val),
                    "unii" => unii = Some(val),
                    _ => {}
                }
            }
        }
    }

    // 2. Classifications
    let mut therapeutic_class = None;
    let mut pharmacologic_class = None;
    let mut mechanism_class = None;
    let mut chemical_class = None;

    if let Ok(classes) = details::classes_for(db, concept) {
        for cl in classes {
            match cl.system.as_str() {
                "fda_epc" => pharmacologic_class = Some(format!("{} [EPC]", cl.name)),
                "fda_moa" => mechanism_class = Some(format!("{} [MOA]", cl.name)),
                "fda_cs" => therapeutic_class = Some(cl.name),
                "atc" => therapeutic_class = Some(format!("{} ({})", cl.name, cl.code)),
                _ => {
                    if therapeutic_class.is_none() {
                        therapeutic_class = Some(cl.name);
                    }
                }
            }
        }
    }

    // 3. Query aggregated attributes across all related concepts and products
    let mut uses_map: BTreeMap<String, usize> = BTreeMap::new();
    let mut side_effects_map: BTreeMap<String, usize> = BTreeMap::new();
    let mut substitutes_set = HashSet::new();
    let mut routes_set = HashSet::new();
    let mut forms_set = HashSet::new();
    let mut strengths_set = HashSet::new();
    let mut brands_set = HashSet::new();
    let mut prescription = None;
    let mut habit_forming = None;
    let mut brief_desc = None;

    let norm = crate::normalize::normalize(&concept.name);
    let attr_sql = "WITH matched_concepts AS (
                        SELECT DISTINCT c.id FROM concepts c 
                        JOIN names n ON n.concept_id = c.id 
                        WHERE n.norm = ?1
                    ),
                    all_related AS (
                        SELECT id FROM matched_concepts
                        UNION
                        SELECT r.subject_id FROM matched_concepts m
                        CROSS JOIN relationships r ON r.object_id = m.id
                    )
                    SELECT a.key, a.value, count(*) as cnt 
                    FROM all_related ar
                    CROSS JOIN attributes a ON a.concept_id = ar.id
                    GROUP BY a.key, a.value";

    if let Ok(mut stmt) = conn.prepare_cached(attr_sql) {
        if let Ok(rows) = stmt.query_map(params![norm], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)? as usize))
        }) {
            for (k, val, count) in rows.flatten() {
                match k.as_str() {
                    "use" => {
                        *uses_map.entry(val).or_default() += count;
                    }
                    "side_effect" => {
                        *side_effects_map.entry(val).or_default() += count;
                    }
                    "therapeutic_class" => {
                        if therapeutic_class.is_none() {
                            therapeutic_class = Some(val);
                        }
                    }
                    "action_class" => {
                        if mechanism_class.is_none() {
                            mechanism_class = Some(val);
                        }
                    }
                    "chemical_class" => {
                        if chemical_class.is_none() {
                            chemical_class = Some(val);
                        }
                    }
                    "substitute" => {
                        substitutes_set.insert(val);
                    }
                    "prescription" => {
                        if prescription.is_none() {
                            prescription = Some(val);
                        }
                    }
                    "habit_forming" => {
                        if habit_forming.is_none() {
                            habit_forming = Some(val);
                        }
                    }
                    "route" => {
                        routes_set.insert(val);
                    }
                    "dose_form" | "dosage_form" => {
                        forms_set.insert(val);
                    }
                    "strength" | "RXN_STRENGTH" => {
                        strengths_set.insert(val);
                    }
                    "brand_name" => {
                        brands_set.insert(val);
                    }
                    "brief" if brief_desc.is_none() && val.len() > 30 => {
                        brief_desc = Some(val);
                    }
                    _ => {}
                }
            }
        }
    }

    // Sort uses by frequency
    let mut sorted_uses: Vec<(String, usize)> = uses_map.into_iter().collect();
    sorted_uses.sort_by_key(|a| std::cmp::Reverse(a.1));
    let indications: Vec<String> = sorted_uses
        .into_iter()
        .take(8)
        .map(|(u, _)| format!("{u}."))
        .collect();

    // Sort side effects by frequency
    let mut sorted_se: Vec<(String, usize)> = side_effects_map.into_iter().collect();
    sorted_se.sort_by_key(|a| std::cmp::Reverse(a.1));
    let adverse_reactions_common: Vec<String> = sorted_se
        .into_iter()
        .take(12)
        .map(|(s, _)| s)
        .collect();

    let mut common_strengths: Vec<String> = strengths_set.into_iter().collect();
    common_strengths.sort_by(|a, b| crate::details::natural_cmp(a, b));
    common_strengths.truncate(10);

    let mut dosage_forms: Vec<String> = forms_set.into_iter().take(6).collect();
    dosage_forms.sort();

    let mut routes: Vec<String> = routes_set.into_iter().take(4).collect();
    routes.sort();

    let mut india_brands: Vec<String> = brands_set.into_iter().take(12).collect();
    india_brands.sort();

    let mut substitutes: Vec<String> = substitutes_set.into_iter().take(8).collect();
    substitutes.sort();

    let display_name = title_case(&concept.name);

    let description = brief_desc.unwrap_or_else(|| {
        format!(
            "{display_name} is a pharmaceutical agent indexed in clinical and pharmacy formularies. It is utilized primarily for {}.",
            if !indications.is_empty() {
                indications[0].trim_end_matches('.')
            } else {
                "targeted clinical indications"
            }
        )
    });

    let tagline = therapeutic_class
        .as_ref()
        .map(|tc| format!("Therapeutic agent: {tc}"))
        .or_else(|| Some(format!("Clinical drug: {display_name}")));

    Ok(Some(DrugMonograph {
        name: display_name,
        section: 7,
        pronunciation: None,
        tagline,
        therapeutic_class,
        pharmacologic_class,
        mechanism_class,
        chemical_class,
        atc_code: None,
        rxcui,
        unii,
        cas: None,
        prescription: prescription.or_else(|| Some("Prescription Required".into())),
        habit_forming: habit_forming.or_else(|| Some("No".into())),
        routes: if routes.is_empty() { vec!["Oral".into()] } else { routes },
        dosage_forms: if dosage_forms.is_empty() { vec!["Tablet / Capsule".into()] } else { dosage_forms },
        common_strengths,
        description: Some(description),
        indications: if indications.is_empty() {
            vec!["Refer to official clinical package insert for detailed approved indications.".into()]
        } else {
            indications
        },
        off_label_uses: Vec::new(),
        adult_dosing: vec![
            "Dosage must be individualized based on disease indication, patient severity, and clinical response.".into(),
            "Consult institutional guidelines or prescribing label for definitive dosing intervals.".into(),
        ],
        pediatric_dosing: vec![
            "Consult dedicated pediatric dosing guidelines or weight-based references.".into(),
        ],
        renal_adjustment: vec![
            "Assess renal function prior to initiation; adjust dosage in renal impairment if cleared via kidneys.".into(),
        ],
        hepatic_adjustment: vec![
            "Exercise caution in moderate to severe hepatic impairment.".into(),
        ],
        administration_tips: vec![
            "Take consistently with respect to meals as directed on packaging.".into(),
        ],
        boxed_warning: None,
        contraindications: vec![
            "Known hypersensitivity to the active drug or any inactive excipients.".into(),
        ],
        warnings_precautions: vec![
            "Monitor patient response, clinical lab parameters, and adverse symptoms during active therapy.".into(),
        ],
        adverse_reactions_common,
        adverse_reactions_serious: vec![
            "Severe hypersensitivity, anaphylactoid reactions, or systemic organ toxicity (report immediately).".into(),
        ],
        drug_interactions: vec![
            "Review concurrent medications for hepatic cytochrome P450 or renal clearance interactions.".into(),
        ],
        mechanism_of_action: None,
        pharmacokinetics: Vec::new(),
        pregnancy_lactation: Some("Consult pregnancy safety registries and obstetric prescribing guidelines prior to administration in pregnant or lactating individuals.".into()),
        us_brands: Vec::new(),
        india_brands,
        substitutes,
        see_also: vec![
            format!("Database search: gurd search {}", concept.name),
            format!("Full records: gurd raw {}", concept.name),
        ],
    }))
}

fn clean_brand_name(s: &str) -> String {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.is_empty() {
        return String::new();
    }
    // Take first 2-3 words (brand name before strength/form)
    let brand = if parts.len() > 1 && parts[1].chars().all(|c| c.is_ascii_digit() || c == 'm' || c == 'g') {
        parts[0].to_string()
    } else if parts.len() > 2 {
        format!("{} {}", parts[0], parts[1])
    } else {
        parts[0].to_string()
    };
    title_case(&brand)
}

fn title_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize = true;
    for c in s.chars() {
        if c.is_whitespace() || c == '-' || c == '/' {
            capitalize = true;
            result.push(c);
        } else if capitalize {
            result.extend(c.to_uppercase());
            capitalize = false;
        } else {
            result.extend(c.to_lowercase());
        }
    }
    result
}
