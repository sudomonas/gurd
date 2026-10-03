//! Name search.
//!
//! Matching runs in tiers, strongest first; a concept appears once, under the best tier
//! that found it:
//!
//! 1. `exact`     the normalized name equals the normalized query
//! 2. `prefix`    a name starts with the query
//! 3. `token`     every query word occurs as a word of a name (the last one as a prefix)
//! 4. `substring` a name contains the query (three characters or more); skipped when
//!    there is an exact match
//! 5. `fuzzy`     the closest ingredient or brand names by edit distance; only tried
//!    when nothing else matched
//!
//! Within a tier, results are ordered by kind (ingredients and brands before drugs and
//! components), then by length of the matched name, then alphabetically.

use std::collections::HashSet;

use anyhow::Result;
use rusqlite::params;
use serde::Serialize;

use crate::database::Database;
use crate::models::ConceptRef;
use crate::normalize::normalize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Match {
    Exact,
    Prefix,
    Token,
    Substring,
    Fuzzy,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    #[serde(flatten)]
    pub concept: ConceptRef,
    #[serde(rename = "match")]
    pub matched: Match,
    /// The name that matched, which may be a synonym rather than the preferred name.
    pub matched_name: String,
}

/// Kinds considered for fuzzy matching: the names people actually type.
const FUZZY_KINDS: &str =
    "'ingredient', 'precise_ingredient', 'multiple_ingredients', 'brand_name'";

/// Searches all installed sources. `limit` of 0 means no limit.
pub fn search(db: &Database, query: &str, limit: usize) -> Result<Vec<Hit>> {
    let q = normalize(query);
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let limit = if limit == 0 { usize::MAX } else { limit };
    let mut hits: Vec<Hit> = Vec::new();
    let mut seen = HashSet::new();
    let upper = format!("{q}\u{10FFFF}");

    let tiers = [
        (Match::Exact, "n.norm = ?1", Some(q.clone())),
        // Every string starting with q sorts between q and q + U+10FFFF. The bound is
        // computed here: an expression in SQL would keep SQLite from using the index.
        (
            Match::Prefix,
            "n.norm > ?1 AND n.norm < ?3",
            Some(q.clone()),
        ),
        (
            Match::Token,
            "n.id IN (SELECT rowid FROM names_tok WHERE names_tok MATCH ?1)",
            token_query(&q),
        ),
        (
            Match::Substring,
            "n.id IN (SELECT rowid FROM names_tri WHERE names_tri MATCH ?1)",
            (q.chars().count() >= 3).then(|| phrase(&q)),
        ),
    ];
    for (tier, condition, arg) in tiers {
        let Some(arg) = arg else { continue };
        if hits.len() >= limit {
            break;
        }
        let want = limit.saturating_sub(hits.len()).saturating_add(seen.len());
        // Substring matching finds partial words; with an exact match it adds little and
        // is the slowest tier on large databases.
        if tier == Match::Substring && hits.first().is_some_and(|h| h.matched == Match::Exact) {
            break;
        }
        for hit in tier_hits(db, tier, condition, &arg, &upper, want)? {
            if hits.len() >= limit {
                break;
            }
            if seen.insert(hit.concept.id) {
                hits.push(hit);
            }
        }
    }

    if hits.is_empty() {
        hits = fuzzy(db, &q, limit)?;
    }
    Ok(hits)
}

fn tier_hits(
    db: &Database,
    tier: Match,
    condition: &str,
    arg: &str,
    upper: &str,
    limit: usize,
) -> Result<Vec<Hit>> {
    // For each concept, one matching name stands for it: the preferred name if it
    // matches, else the shortest. (SQLite returns the bare column `n.name` from the row
    // holding min().)
    let sql = format!(
        "WITH m AS (
             SELECT n.concept_id, n.name,
                    min((n.name_type <> 'preferred') * 1000000 + length(n.name)) AS len
             FROM names n WHERE {condition}
             GROUP BY n.concept_id
         )
         SELECT {CONCEPT_COLUMNS}, m.name
         FROM m JOIN concepts c ON c.id = m.concept_id {CONCEPT_JOINS}
         ORDER BY k.rank, m.len, c.name
         LIMIT ?2"
    );
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let mut stmt = db.connection().prepare_cached(&sql)?;
    let row = |r: &rusqlite::Row| {
        Ok(Hit {
            concept: concept_from_row(r)?,
            matched: tier,
            matched_name: r.get(CONCEPT_COLUMN_COUNT)?,
        })
    };
    // ?3 is only referenced by the prefix condition.
    let rows = if condition.contains("?3") {
        stmt.query_map(params![arg, limit, upper], row)?
            .collect::<rusqlite::Result<_>>()?
    } else {
        stmt.query_map(params![arg, limit], row)?
            .collect::<rusqlite::Result<_>>()?
    };
    Ok(rows)
}

fn fuzzy(db: &Database, q: &str, limit: usize) -> Result<Vec<Hit>> {
    let len = q.chars().count();
    let max = match len {
        0..=3 => return Ok(Vec::new()),
        4..=5 => 1,
        6..=10 => 2,
        _ => 3,
    };
    let sql = format!(
        "SELECT {CONCEPT_COLUMNS}, n.name, n.norm, k.rank
         FROM names n JOIN concepts c ON c.id = n.concept_id {CONCEPT_JOINS}
         WHERE c.kind IN ({FUZZY_KINDS}) AND length(n.norm) BETWEEN ?1 AND ?2"
    );
    let mut stmt = db.connection().prepare_cached(&sql)?;
    let rows = stmt.query_map(params![(len - max) as i64, (len + max) as i64], |r| {
        Ok((
            concept_from_row(r)?,
            r.get::<_, String>(CONCEPT_COLUMN_COUNT)?,
            r.get::<_, String>(CONCEPT_COLUMN_COUNT + 1)?,
            r.get::<_, i64>(CONCEPT_COLUMN_COUNT + 2)?,
        ))
    })?;

    let query: Vec<char> = q.chars().collect();
    let mut found: Vec<(usize, i64, Hit)> = Vec::new();
    for row in rows {
        let (concept, name, norm, rank) = row?;
        let distance = edit_distance(&query, &norm);
        if distance > max {
            continue;
        }
        match found
            .iter_mut()
            .find(|(_, _, h)| h.concept.id == concept.id)
        {
            Some(existing) if existing.0 <= distance => {}
            Some(existing) => existing.0 = distance,
            None => found.push((
                distance,
                rank,
                Hit {
                    concept,
                    matched: Match::Fuzzy,
                    matched_name: name,
                },
            )),
        }
    }
    found.sort_by(|a, b| (a.0, a.1, &a.2.concept.name).cmp(&(b.0, b.1, &b.2.concept.name)));
    // Only the closest matches: a "did you mean", not a list of everything similar.
    let best = found.first().map_or(0, |f| f.0);
    Ok(found
        .into_iter()
        .take_while(|f| f.0 == best)
        .take(limit)
        .map(|(_, _, h)| h)
        .collect())
}

pub(crate) const CONCEPT_COLUMNS: &str =
    "c.id, s.slug, s.version, s.code_system, c.source_code, c.kind, k.label, c.source_type, c.name";
const CONCEPT_COLUMN_COUNT: usize = 9;
pub(crate) const CONCEPT_JOINS: &str =
    "JOIN concept_kinds k ON k.kind = c.kind JOIN sources s ON s.id = c.source_id";

pub(crate) fn concept_from_row(r: &rusqlite::Row) -> rusqlite::Result<ConceptRef> {
    Ok(ConceptRef {
        id: r.get(0)?,
        source: r.get(1)?,
        source_version: r.get(2)?,
        code_system: r.get(3)?,
        code: r.get(4)?,
        kind: r.get(5)?,
        kind_label: r.get(6)?,
        source_type: r.get(7)?,
        name: r.get(8)?,
    })
}

/// FTS5 query requiring every word; the last word may be incomplete.
fn token_query(q: &str) -> Option<String> {
    let words: Vec<&str> = q
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let (last, rest) = words.split_last()?;
    let mut out: Vec<String> = rest.iter().map(|w| phrase(w)).collect();
    out.push(format!("{}*", phrase(last)));
    Some(out.join(" "))
}

/// Quotes text as a single FTS5 phrase.
fn phrase(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Optimal string alignment distance: Levenshtein plus adjacent transpositions, so
/// "metfromin" is one edit from "metformin".
fn edit_distance(a: &[char], b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut before: Vec<usize> = vec![0; b.len() + 1];
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for i in 0..a.len() {
        cur[0] = i + 1;
        for j in 0..b.len() {
            let cost = usize::from(a[i] != b[j]);
            let mut d = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
            if i > 0 && j > 0 && a[i] == b[j - 1] && a[i - 1] == b[j] {
                d = d.min(before[j - 1] + 1);
            }
            cur[j + 1] = d;
        }
        std::mem::swap(&mut before, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_token_queries() {
        assert_eq!(
            token_query("metformin hydro").unwrap(),
            "\"metformin\" \"hydro\"*"
        );
        assert_eq!(
            token_query("amoxicillin / clavulanate").unwrap(),
            "\"amoxicillin\" \"clavulanate\"*"
        );
        assert_eq!(token_query("// -"), None);
    }

    #[test]
    fn edit_distances() {
        let d = |a: &str, b: &str| edit_distance(&a.chars().collect::<Vec<_>>(), b);
        assert_eq!(d("metfromin", "metformin"), 1);
        assert_eq!(d("metfromin", "merbromin"), 2);
        assert_eq!(d("ab", "ba"), 1);
        assert_eq!(d("aspirin", "aspirin"), 0);
        assert_eq!(d("asprin", "aspirin"), 1);
        assert_eq!(d("", "abc"), 3);
    }
}
