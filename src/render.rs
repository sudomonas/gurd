//! Plain-text rendering. Everything here must stay readable without color and when piped.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::details::{Class, Details};
use crate::models::{ConceptRef, DatabaseInfo, Section, SourceRecord};
use crate::normalize::normalize;
use crate::output::Style;
use crate::search::Hit;
use crate::sources::SourceInfo;

/// `rxcui:6809`, the same form `drug id` accepts.
pub fn concept_id(c: &ConceptRef) -> String {
    format!("{}:{}", c.code_system, c.code)
}

/// One line per hit: identifier, kind, name, and the matched synonym when it differs.
pub fn hits(hits: &[Hit], style: Style) -> String {
    let id_width = hits
        .iter()
        .map(|h| concept_id(&h.concept).len())
        .max()
        .unwrap_or(0);
    let kind_width = hits
        .iter()
        .map(|h| h.concept.kind_label.len())
        .max()
        .unwrap_or(0);
    let mut s = String::new();
    for h in hits {
        let c = &h.concept;
        let id = format!("{:id_width$}", concept_id(c));
        let kind = format!("{:kind_width$}", c.kind_label);
        let _ = write!(s, "{}  {}  {}", style.dim(&id), kind, style.bold(&c.name));
        if normalize(&h.matched_name) != normalize(&c.name) {
            let _ = write!(s, "  {}", style.dim(&format!("({})", h.matched_name)));
        }
        s.push('\n');
    }
    s
}

/// Items shown per section on the summary card; `drug show` lists everything.
pub const CARD_ITEMS: usize = 10;

fn heading(s: &mut String, style: Style, concept: &ConceptRef, source: Option<&SourceRecord>) {
    let _ = writeln!(s, "{}", style.bold(&concept.name));
    let _ = writeln!(
        s,
        "{}",
        "─".repeat(concept.name.chars().count().clamp(3, 60))
    );
    let _ = writeln!(s, "  Kind      {}", concept.kind_label);
    let _ = writeln!(s, "  ID        {}", concept_id(concept));
    match source {
        Some(src) => {
            let _ = writeln!(s, "  Source    {} {}", src.title, release_note(src, style));
        }
        None => {
            let _ = writeln!(
                s,
                "  Source    {} {}",
                concept.source, concept.source_version
            );
        }
    }
}

fn section_list(
    s: &mut String,
    style: Style,
    label: &str,
    items: &[ConceptRef],
    max: usize,
    more: &str,
) {
    let _ = writeln!(
        s,
        "\n{} {}",
        style.bold(label),
        style.dim(&format!("({})", items.len()))
    );
    for item in items.iter().take(max) {
        let _ = writeln!(s, "  {}", item.name);
    }
    if items.len() > max {
        let _ = writeln!(
            s,
            "  {}",
            style.dim(&format!("… {} more ({more})", items.len() - max))
        );
    }
}

/// The default view of an exact match: identity plus the main sections, abbreviated.
pub fn card(
    concept: &ConceptRef,
    sections: &BTreeMap<Section, Vec<ConceptRef>>,
    others: &[Hit],
    source: Option<&SourceRecord>,
    style: Style,
) -> String {
    let mut s = String::new();
    heading(&mut s, style, concept, source);
    let more = format!("drug show {}", concept_id(concept));
    for (section, items) in sections {
        section_list(&mut s, style, section.label(), items, CARD_ITEMS, &more);
    }
    if !others.is_empty() {
        let _ = writeln!(s, "\n{}", style.bold("Other matches"));
        for line in hits(others, style).lines() {
            let _ = writeln!(s, "  {line}");
        }
    }
    let _ = writeln!(s, "\n{}", style.dim(&format!("All details: {more}")));
    s
}

/// Everything the database holds about a concept.
pub fn details(d: &Details, source: Option<&SourceRecord>, style: Style) -> String {
    let mut s = String::new();
    heading(&mut s, style, &d.concept, source);

    for (section, items) in &d.sections {
        section_list(&mut s, style, section.label(), items, usize::MAX, "");
    }

    if !d.names.is_empty() {
        let _ = writeln!(s, "\n{}", style.bold("Names"));
        for n in &d.names {
            let label = match n.name_type.as_str() {
                "preferred" => "preferred",
                "prescribable" => "prescribable",
                "tall_man" => "tall man",
                _ => "synonym",
            };
            let _ = writeln!(
                s,
                "  {}  {}",
                n.name,
                style.dim(&format!("({label}, {})", n.source_type))
            );
        }
    }

    if !d.attributes.is_empty() {
        let _ = writeln!(s, "\n{}", style.bold("Attributes"));
        let width = d.attributes.iter().map(|a| a.key.len()).max().unwrap_or(0);
        for a in &d.attributes {
            let _ = writeln!(s, "  {:width$}  {}", a.key, a.value);
        }
    }

    if !d.relationships.is_empty() {
        let _ = writeln!(s, "\n{}", style.bold("Relationships"));
        let mut i = 0;
        while i < d.relationships.len() {
            let predicate = &d.relationships[i].predicate;
            let group: Vec<&ConceptRef> = d.relationships[i..]
                .iter()
                .take_while(|r| &r.predicate == predicate)
                .map(|r| &r.concept)
                .collect();
            let _ = writeln!(
                s,
                "  {predicate} {}",
                style.dim(&format!("({})", group.len()))
            );
            let id_width = group.iter().map(|c| concept_id(c).len()).max().unwrap_or(0);
            for c in &group {
                let _ = writeln!(
                    s,
                    "    {}  {}",
                    style.dim(&format!("{:id_width$}", concept_id(c))),
                    c.name
                );
            }
            i += group.len();
        }
    }

    if !d.identifiers.is_empty() {
        let _ = writeln!(s, "\n{}", style.bold("Identifiers"));
        let mut systems: Vec<&str> = d.identifiers.iter().map(|i| i.system.as_str()).collect();
        systems.dedup();
        let width = systems.iter().map(|x| x.len()).max().unwrap_or(0);
        for system in systems {
            let values: Vec<&str> = d
                .identifiers
                .iter()
                .filter(|i| i.system == system)
                .map(|i| i.value.as_str())
                .collect();
            let indent = 2 + width + 2;
            let text = wrap(&values.join(", "), 78usize.saturating_sub(indent).max(20));
            for (i, line) in text.iter().enumerate() {
                if i == 0 {
                    let _ = writeln!(s, "  {system:width$}  {line}");
                } else {
                    let _ = writeln!(s, "{:indent$}{line}", "");
                }
            }
        }
    }

    if let Some(src) = source {
        let _ = writeln!(s, "\n{}", style.bold("Source"));
        let _ = writeln!(s, "  {}", src.title);
        let _ = writeln!(s, "  Provider  {}", src.provider);
        let _ = writeln!(s, "  Release   {}", release_note(src, style));
        let _ = writeln!(s, "  Imported  {}", src.imported_at);
        for line in wrap(&src.attribution, 76) {
            let _ = writeln!(s, "  {}", style.dim(&line));
        }
    }
    s
}

/// Greedy word wrap.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// "2026-09-08 (25 days old)", with an explicit warning once the release is outdated.
pub fn release_note(src: &SourceRecord, style: Style) -> String {
    match (src.age_days, src.stale) {
        (Some(age), true) => format!(
            "{} {}",
            src.version,
            style.bold(&format!(
                "({age} days old; may not reflect the provider's latest data; run `drug update`)"
            ))
        ),
        (Some(age), false) => format!(
            "{} {}",
            src.version,
            style.dim(&format!("({age} days old)"))
        ),
        (None, _) => src.version.clone(),
    }
}

pub fn database(info: &DatabaseInfo, available: &[SourceInfo], style: Style) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{}", style.bold("Database"));
    let _ = writeln!(s, "  Path      {}", info.path);
    if !info.installed {
        let _ = writeln!(s, "  Status    not installed (run `drug update`)");
        return s;
    }
    if let Some(v) = info.schema_version {
        let _ = writeln!(s, "  Schema    {v}");
    }
    if let Some(at) = &info.built_at {
        let _ = writeln!(s, "  Built     {at}");
    }
    if let Some(by) = &info.built_by {
        let _ = writeln!(s, "  Built by  {by}");
    }
    let _ = writeln!(s, "\n{}", style.bold("Sources"));
    let width = info
        .sources
        .iter()
        .map(|x| x.slug.len())
        .chain(available.iter().map(|a| a.slug.len()))
        .max()
        .unwrap_or(0);
    for src in &info.sources {
        let _ = writeln!(s, "  {:width$}  {}", src.slug, release_note(src, style));
        let _ = writeln!(s, "  {:width$}  {} ({})", "", src.title, src.provider);
    }
    for a in available
        .iter()
        .filter(|a| !info.sources.iter().any(|i| i.slug == a.slug))
    {
        let _ = writeln!(s, "  {:width$}  {}", a.slug, style.dim("not installed"));
    }
    s
}

pub fn sources(available: &[SourceInfo], installed: &[SourceRecord], style: Style) -> String {
    let mut s = String::new();
    let mut first = true;
    let mut entry = |s: &mut String,
                     slug: &str,
                     title: &str,
                     provider: &str,
                     license: &str,
                     url: &str,
                     download: (Option<&str>, Option<&str>),
                     redistributable: bool,
                     inst: Option<&SourceRecord>| {
        if !first {
            s.push('\n');
        }
        first = false;
        let _ = writeln!(s, "{}  {}", style.bold(slug), title);
        let _ = writeln!(s, "  Provider   {provider}");
        match inst {
            Some(i) => {
                let _ = writeln!(s, "  Installed  {}", release_note(i, style));
            }
            None => {
                let _ = writeln!(s, "  Installed  no (run `drug update --source {slug}`)");
            }
        }
        for (n, line) in wrap(license, 64).iter().enumerate() {
            let label = if n == 0 { "License" } else { "" };
            let _ = writeln!(s, "  {label:9}  {line}");
        }
        let _ = writeln!(s, "  Website    {url}");
        if let Some(d) = download.0 {
            let _ = writeln!(s, "  Download   {d}");
        }
        if let Some(c) = download.1 {
            let _ = writeln!(s, "  Checksums  {c}");
        }
        let _ = writeln!(
            s,
            "  Shareable  {}",
            if redistributable {
                "yes, under the terms above"
            } else {
                "no; keep it local"
            }
        );
    };
    for a in available {
        let inst = installed.iter().find(|i| i.slug == a.slug);
        entry(
            &mut s,
            &a.slug,
            &a.title,
            &a.provider,
            &a.license,
            &a.url,
            (a.download_url.as_deref(), a.checksums_url.as_deref()),
            a.redistributable,
            inst,
        );
    }
    for i in installed
        .iter()
        .filter(|i| !available.iter().any(|a| a.slug == i.slug))
    {
        entry(
            &mut s,
            &i.slug,
            &i.title,
            &i.provider,
            &i.license,
            &i.url,
            (None, None),
            i.redistributable,
            Some(i),
        );
    }
    s
}

pub fn classes(concept: &ConceptRef, classes: &[Class], style: Style) -> String {
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{}  {}",
        style.bold(&concept.name),
        style.dim(&concept_id(concept))
    );
    let width = classes
        .iter()
        .map(|c| c.system.len() + c.code.len() + 1)
        .max()
        .unwrap_or(0);
    for c in classes {
        let id = format!("{}:{}", c.system, c.code);
        let _ = writeln!(
            s,
            "  {id:width$}  {}  {}",
            c.name,
            style.dim(&format!("({} {})", c.source, c.source_version))
        );
    }
    s
}
