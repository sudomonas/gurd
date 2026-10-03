//! Everything the database holds about one concept.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use anyhow::{Result, bail};
use rusqlite::params;
use serde::Serialize;

use crate::database::Database;
use crate::models::{ConceptRef, Section};
use crate::normalize::normalize;
use crate::search::{CONCEPT_COLUMNS, CONCEPT_JOINS, concept_from_row};

#[derive(Debug, Serialize)]
pub struct Details {
    #[serde(flatten)]
    pub concept: ConceptRef,
    /// Summary sections; empty sections are omitted.
    pub sections: BTreeMap<Section, Vec<ConceptRef>>,
    pub names: Vec<Name>,
    pub identifiers: Vec<Identifier>,
    pub attributes: Vec<Attribute>,
    /// Direct relationships, as recorded by the source: `<this concept> predicate <concept>`.
    pub relationships: Vec<Relationship>,
}

#[derive(Debug, Serialize)]
pub struct Name {
    pub name: String,
    pub name_type: String,
    pub source_type: String,
}

#[derive(Debug, Serialize)]
pub struct Identifier {
    pub system: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Attribute {
    pub key: String,
    /// Display label declared by the source, or the key itself.
    pub label: String,
    pub value: String,
    /// Shown on the summary page as well as in the detailed view.
    #[serde(skip)]
    pub summary: bool,
}

#[derive(Debug, Serialize)]
pub struct Relationship {
    pub predicate: String,
    pub concept: ConceptRef,
}

#[derive(Debug, Serialize)]
pub struct Class {
    pub system: String,
    pub code: String,
    pub name: String,
    pub source: String,
    pub source_version: String,
}

/// True if any installed source provides classifications at all.
pub fn has_classifications(db: &Database) -> Result<bool> {
    Ok(db
        .connection()
        .query_row("SELECT EXISTS (SELECT 1 FROM classifications)", [], |r| {
            r.get(0)
        })?)
}

/// Classes assigned to a concept, by the sources that assigned them.
pub fn classes(db: &Database, concept: &ConceptRef) -> Result<Vec<Class>> {
    let mut stmt = db.connection().prepare_cached(
        "SELECT cl.system, cl.code, cl.name, s.slug, s.version
         FROM concept_classifications cc
         JOIN classifications cl ON cl.id = cc.classification_id
         JOIN sources s ON s.id = cc.source_id
         WHERE cc.concept_id = ?1
         ORDER BY cl.system, cl.code",
    )?;
    let rows = stmt.query_map([concept.id], |r| {
        Ok(Class {
            system: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
            source: r.get(3)?,
            source_version: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Classes of a concept and of the concepts its source relates it to directly that relate
/// to no other concept of its kind (an ingredient's single-ingredient products, say), so
/// a combination product's classes are never attributed to one of its ingredients. Only
/// the concept's own source is consulted: classes are never carried across sources.
pub fn classes_for(db: &Database, concept: &ConceptRef) -> Result<Vec<Class>> {
    let any: bool = db
        .connection()
        .prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM classifications
                        WHERE source_id = (SELECT source_id FROM concepts WHERE id = ?1))",
        )?
        .query_row([concept.id], |r| r.get(0))?;
    if !any {
        return Ok(Vec::new());
    }
    let mut stmt = db.connection().prepare_cached(
        "SELECT DISTINCT cl.system, cl.code, cl.name, s.slug, s.version
         FROM (SELECT ?1 AS id
               UNION SELECT r.object_id FROM relationships r
                     WHERE r.subject_id = ?1
                       AND r.source_id = (SELECT source_id FROM concepts WHERE id = ?1)
                       AND (SELECT count(DISTINCT r2.object_id)
                            FROM relationships r2 JOIN concepts c2 ON c2.id = r2.object_id
                            WHERE r2.subject_id = r.object_id
                              AND c2.kind = (SELECT kind FROM concepts WHERE id = ?1)) = 1) x
         JOIN concept_classifications cc ON cc.concept_id = x.id
         JOIN classifications cl ON cl.id = cc.classification_id
         JOIN sources s ON s.id = cc.source_id
         ORDER BY cl.system, cl.name",
    )?;
    let rows = stmt.query_map([concept.id], |r| {
        Ok(Class {
            system: r.get(0)?,
            code: r.get(1)?,
            name: r.get(2)?,
            source: r.get(3)?,
            source_version: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Concepts carrying the identifier `system:value`, from any source.
pub fn by_identifier(db: &Database, system: &str, value: &str) -> Result<Vec<ConceptRef>> {
    let sql = format!(
        "SELECT DISTINCT {CONCEPT_COLUMNS}
         FROM identifiers i JOIN concepts c ON c.id = i.concept_id {CONCEPT_JOINS}
         WHERE i.system = ?1 AND i.value = ?2
         ORDER BY k.rank, c.name"
    );
    let mut stmt = db.connection().prepare_cached(&sql)?;
    let rows = stmt.query_map(params![system, value], concept_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Splits `system:value`. The system is lowercased; the value is kept as given.
pub fn parse_identifier(s: &str) -> Option<(String, &str)> {
    let (system, value) = s.split_once(':')?;
    let ok = !system.is_empty()
        && !value.is_empty()
        && system
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    ok.then(|| (system.to_ascii_lowercase(), value.trim()))
}

pub fn concept(db: &Database, id: i64) -> Result<ConceptRef> {
    let sql = format!("SELECT {CONCEPT_COLUMNS} FROM concepts c {CONCEPT_JOINS} WHERE c.id = ?1");
    match db.connection().query_row(&sql, [id], concept_from_row) {
        Ok(c) => Ok(c),
        Err(rusqlite::Error::QueryReturnedNoRows) => bail!("no concept with id {id}"),
        Err(e) => Err(e.into()),
    }
}

/// Summary sections only; cheap enough for the default search view.
pub fn sections(db: &Database, concept: &ConceptRef) -> Result<BTreeMap<Section, Vec<ConceptRef>>> {
    let conn = db.connection();
    let mut stmt = conn.prepare_cached(
        "SELECT n.section, n.path, n.to_kind
         FROM navigation n JOIN concepts c ON c.source_id = n.source_id AND c.kind = n.from_kind
         WHERE c.id = ?1",
    )?;
    let rules: Vec<(String, String, String)> = stmt
        .query_map([concept.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;

    let mut sections: BTreeMap<Section, Vec<ConceptRef>> = BTreeMap::new();
    for (section, path, to_kind) in rules {
        let Some(section) = Section::parse(&section) else {
            continue; // written by a newer version of gurd
        };
        let predicates: Vec<&str> = path.split(' ').collect();
        let items = follow(db, concept.id, &predicates, &to_kind)?;
        let entry = sections.entry(section).or_default();
        for item in items {
            if item.id != concept.id && !entry.iter().any(|e| e.id == item.id) {
                entry.push(item);
            }
        }
    }
    sections.retain(|_, v| !v.is_empty());
    // Items named after the concept itself first, shorter names (usually fewer
    // ingredients) before longer ones, then natural order: "metformin hydrochloride 500 MG
    // Oral Tablet" comes before the combination products.
    let own = normalize(&concept.name);
    for items in sections.values_mut() {
        let mut keyed: Vec<((bool, usize), ConceptRef)> = std::mem::take(items)
            .into_iter()
            .map(|c| {
                let leads = normalize(&c.name).starts_with(&own);
                ((!leads, c.name.split_whitespace().count()), c)
            })
            .collect();
        keyed.sort_by(|(ka, a), (kb, b)| ka.cmp(kb).then_with(|| natural_cmp(&a.name, &b.name)));
        *items = keyed.into_iter().map(|(_, c)| c).collect();
    }
    Ok(sections)
}

/// Concepts of kind `to_kind` reached from `start` along `predicates`.
fn follow(
    db: &Database,
    start: i64,
    predicates: &[&str],
    to_kind: &str,
) -> Result<Vec<ConceptRef>> {
    let mut joins = String::new();
    for i in 1..predicates.len() {
        joins.push_str(&format!(
            " JOIN relationships r{i} ON r{i}.subject_id = r{}.object_id AND r{i}.predicate = ?{}",
            i - 1,
            i + 3
        ));
    }
    let last = predicates.len() - 1;
    let sql = format!(
        "SELECT DISTINCT {CONCEPT_COLUMNS}
         FROM relationships r0{joins}
         JOIN concepts c ON c.id = r{last}.object_id {CONCEPT_JOINS}
         WHERE r0.subject_id = ?1 AND r0.predicate = ?2 AND c.kind = ?3"
    );
    let mut values: Vec<rusqlite::types::Value> = vec![
        start.into(),
        predicates[0].to_owned().into(),
        to_kind.to_owned().into(),
    ];
    values.extend(predicates[1..].iter().map(|p| (*p).to_owned().into()));
    let mut stmt = db.connection().prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values), concept_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub fn details(db: &Database, concept: ConceptRef) -> Result<Details> {
    let conn = db.connection();
    let id = concept.id;

    let names = conn
        .prepare_cached(
            "SELECT name, name_type, source_type FROM names WHERE concept_id = ?1
             ORDER BY name_type = 'preferred' DESC, name_type, name",
        )?
        .query_map([id], |r| {
            Ok(Name {
                name: r.get(0)?,
                name_type: r.get(1)?,
                source_type: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let identifiers = conn
        .prepare_cached(
            "SELECT system, value FROM identifiers WHERE concept_id = ?1 ORDER BY system, value",
        )?
        .query_map([id], |r| {
            Ok(Identifier {
                system: r.get(0)?,
                value: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let attributes = attributes(db, id)?;

    let sql = format!(
        "SELECT {CONCEPT_COLUMNS}, r.predicate
         FROM relationships r JOIN concepts c ON c.id = r.object_id {CONCEPT_JOINS}
         WHERE r.subject_id = ?1"
    );
    let mut relationships: Vec<Relationship> = conn
        .prepare_cached(&sql)?
        .query_map([id], |r| {
            Ok(Relationship {
                concept: concept_from_row(r)?,
                predicate: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    relationships.sort_by(|a, b| {
        a.predicate
            .cmp(&b.predicate)
            .then_with(|| natural_cmp(&a.concept.name, &b.concept.name))
    });

    Ok(Details {
        sections: sections(db, &concept)?,
        concept,
        names,
        identifiers,
        attributes,
        relationships,
    })
}

/// One record on a page, and the identifier that linked it to the records found by name.
#[derive(Debug, Clone)]
pub struct PageEntry {
    pub concept: ConceptRef,
    pub via: Option<String>,
}

/// Linked records of one source left off a page.
#[derive(Debug, Clone)]
pub struct Omitted {
    pub source: String,
    pub count: usize,
}

/// Everything a page shows for a lookup: the records found (`primary`) and the records of
/// other sources linked to them by a shared identifier, at most `per_source` per source
/// and identifier. With `same_kind_only`, only linked records of the same kind as the
/// record they link to are shown (e.g. ingredient to ingredient); the rest are counted
/// as omitted.
pub fn page(
    db: &Database,
    primary: Vec<ConceptRef>,
    per_source: usize,
    same_kind_only: bool,
) -> Result<(Vec<PageEntry>, Vec<Omitted>)> {
    let mut entries: Vec<PageEntry> = Vec::new();
    for c in primary {
        if !entries.iter().any(|e| e.concept.id == c.id) {
            entries.push(PageEntry {
                concept: c,
                via: None,
            });
        }
    }
    let mut omitted: Vec<Omitted> = Vec::new();
    let mut omitted_ids = std::collections::HashSet::new();
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for i in 0..entries.len() {
        if entries[i].via.is_some() {
            continue;
        }
        for (c, via) in linked(db, &entries[i].concept.clone())? {
            if entries.iter().any(|e| e.concept.id == c.id) {
                continue;
            }
            let wanted = !same_kind_only || c.kind == entries[i].concept.kind;
            let n = counts.entry((c.source.clone(), via.clone())).or_default();
            if wanted && *n < per_source {
                *n += 1;
                omitted_ids.remove(&c.id);
                entries.push(PageEntry {
                    concept: c,
                    via: Some(via),
                });
            } else if omitted_ids.insert(c.id) {
                match omitted.iter_mut().find(|o| o.source == c.source) {
                    Some(o) => o.count += 1,
                    None => omitted.push(Omitted {
                        source: c.source.clone(),
                        count: 1,
                    }),
                }
            }
        }
    }
    Ok((entries, omitted))
}

/// A concept's attributes in display order, with the labels its source declared.
/// Values of one key keep the order the source listed them in.
pub fn attributes(db: &Database, concept_id: i64) -> Result<Vec<Attribute>> {
    let rows = db
        .connection()
        .prepare_cached(
            "SELECT a.key, COALESCE(k.label, a.key), a.value, COALESCE(k.summary, 0)
             FROM attributes a
             LEFT JOIN attribute_keys k ON k.source_id = a.source_id AND k.key = a.key
             WHERE a.concept_id = ?1
             ORDER BY COALESCE(k.rank, 1000000), a.key, a.id",
        )?
        .query_map([concept_id], |r| {
            Ok(Attribute {
                key: r.get(0)?,
                label: r.get(1)?,
                value: r.get(2)?,
                summary: r.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Identifier systems through which records of different sources are linked. Each is an
/// identifier the sources themselves assert; nothing is linked by name.
pub const LINKING_SYSTEMS: &str = "'rxcui', 'unii'";

/// Concepts of other sources that share a linking identifier with `concept`, each with the
/// identifier (`system:value`) that links it.
pub fn linked(db: &Database, concept: &ConceptRef) -> Result<Vec<(ConceptRef, String)>> {
    let sql = format!(
        "SELECT {CONCEPT_COLUMNS}, i2.system || ':' || i2.value
         FROM identifiers i1
         JOIN identifiers i2 ON i2.system = i1.system AND i2.value = i1.value
         JOIN concepts c ON c.id = i2.concept_id {CONCEPT_JOINS}
         WHERE i1.concept_id = ?1 AND i1.system IN ({LINKING_SYSTEMS})
           AND c.source_id <> (SELECT source_id FROM concepts WHERE id = ?1)
         ORDER BY s.slug, k.rank, c.name"
    );
    let mut out: Vec<(ConceptRef, String)> = Vec::new();
    let mut stmt = db.connection().prepare_cached(&sql)?;
    let rows = stmt.query_map([concept.id], |r| {
        Ok((concept_from_row(r)?, r.get::<_, String>(9)?))
    })?;
    for row in rows {
        let (c, via) = row?;
        if !out.iter().any(|(o, _)| o.id == c.id) {
            out.push((c, via));
        }
    }
    Ok(out)
}

/// Case-insensitive ordering that compares digit runs as numbers, so
/// "500 MG" sorts before "1000 MG".
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let da = a.iter().take_while(|c| c.is_ascii_digit()).count();
                let db = b.iter().take_while(|c| c.is_ascii_digit()).count();
                let (na, nb) = (trim_zeros(&a[..da]), trim_zeros(&b[..db]));
                let ord = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
                if ord != Ordering::Equal {
                    return ord;
                }
                a = &a[da..];
                b = &b[db..];
            }
            (Some(x), Some(y)) => {
                let ord = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

fn trim_zeros(digits: &[u8]) -> &[u8] {
    let n = digits.iter().take_while(|&&d| d == b'0').count();
    &digits[n..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v = vec![
            "metformin 1000 MG",
            "metformin 500 MG",
            "Metformin 850 MG",
            "metformin 50 MG",
        ];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            [
                "metformin 50 MG",
                "metformin 500 MG",
                "Metformin 850 MG",
                "metformin 1000 MG"
            ]
        );
        assert_eq!(natural_cmp("a 007", "a 7"), Ordering::Equal);
    }

    #[test]
    fn identifiers() {
        assert_eq!(
            parse_identifier("rxcui:6809"),
            Some(("rxcui".into(), "6809"))
        );
        assert_eq!(
            parse_identifier("RXCUI:6809"),
            Some(("rxcui".into(), "6809"))
        );
        assert_eq!(parse_identifier("metformin"), None);
        assert_eq!(parse_identifier("rxcui:"), None);
        assert_eq!(parse_identifier("a b:c"), None);
    }
}
