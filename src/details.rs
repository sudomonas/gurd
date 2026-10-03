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

#[derive(Debug, Serialize)]
pub struct Attribute {
    pub key: String,
    pub value: String,
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
            continue; // written by a newer version of drug
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
    let key = |c: &ConceptRef| {
        let leads = normalize(&c.name).starts_with(&own);
        (!leads, c.name.split_whitespace().count())
    };
    for items in sections.values_mut() {
        items.sort_by(|a, b| {
            key(a)
                .cmp(&key(b))
                .then_with(|| natural_cmp(&a.name, &b.name))
        });
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

    let attributes = conn
        .prepare_cached(
            "SELECT key, value FROM attributes WHERE concept_id = ?1 ORDER BY key, value",
        )?
        .query_map([id], |r| {
            Ok(Attribute {
                key: r.get(0)?,
                value: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

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
