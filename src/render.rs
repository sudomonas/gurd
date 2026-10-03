//! Plain-text rendering. Everything here must stay readable without color and when piped.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::details::{Attribute, Class, Details, Omitted, PageEntry};
use crate::models::{ConceptRef, DatabaseInfo, Section, SourceRecord};
use crate::normalize::normalize;
use crate::output::Style;
use crate::search::Hit;
use crate::sources::SourceInfo;

/// `rxcui:6809`, the same form `gurd id` accepts.
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

/// Items shown per section on the summary page; `gurd show` lists everything.
pub const CARD_ITEMS: usize = 10;

/// Width that text is wrapped to.
const WIDTH: usize = 78;

/// Labelled lines: `  Label     value`, with long values wrapped under the value column.
struct Fields<'a> {
    s: &'a mut String,
    width: usize,
}

impl Fields<'_> {
    fn line(&mut self, label: &str, value: &str) {
        let indent = 2 + self.width + 2;
        let lines = wrap(value, WIDTH.saturating_sub(indent).max(24));
        for (i, text) in lines.iter().enumerate() {
            let label = if i == 0 { label } else { "" };
            let _ = writeln!(self.s, "  {label:w$}  {text}", w = self.width);
        }
        if lines.is_empty() {
            let _ = writeln!(self.s, "  {label}");
        }
    }
}

/// Attributes grouped by key: values of one key are joined into one line.
fn grouped(attributes: &[Attribute]) -> Vec<(&str, String)> {
    let mut out: Vec<(&str, String)> = Vec::new();
    let mut i = 0;
    while i < attributes.len() {
        let key = &attributes[i].key;
        let values: Vec<&str> = attributes[i..]
            .iter()
            .take_while(|a| &a.key == key)
            .map(|a| a.value.as_str())
            .collect();
        out.push((attributes[i].label.as_str(), values.join(", ")));
        i += values.len();
    }
    out
}

/// The heading and identity fields of one record.
fn record_head(
    s: &mut String,
    style: Style,
    entry: &PageEntry,
    source: Option<&SourceRecord>,
    attributes: &[(&str, String)],
) {
    let c = &entry.concept;
    let title = source.map_or(c.source.as_str(), |src| src.title.as_str());
    let _ = writeln!(s, "{}", style.bold(title));
    let width = attributes
        .iter()
        .map(|(l, _)| l.chars().count())
        .chain([7])
        .max()
        .unwrap_or(7);
    let mut f = Fields { s, width };
    f.line("Name", &c.name);
    f.line("Kind", &c.kind_label);
    f.line("ID", &concept_id(c));
    match source {
        Some(src) => f.line("Release", &release_note(src, style)),
        None => f.line("Release", &c.source_version),
    }
    if let Some(via) = &entry.via {
        f.line("Linked", &format!("shares {via} with a record above"));
    }
    if let Some(notice) = source.and_then(|src| src.notice.as_deref()) {
        f.line("Note", notice);
    }
    for (label, value) in attributes {
        f.line(label, value);
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
        "\n  {} {}",
        style.bold(label),
        style.dim(&format!("({})", items.len()))
    );
    for item in items.iter().take(max) {
        let _ = writeln!(s, "    {}", item.name);
    }
    if items.len() > max {
        let _ = writeln!(
            s,
            "    {}",
            style.dim(&format!("… {} more ({more})", items.len() - max))
        );
    }
}

/// One record on the summary page.
pub struct Summary<'a> {
    pub entry: &'a PageEntry,
    pub sections: &'a BTreeMap<Section, Vec<ConceptRef>>,
    pub attributes: &'a [Attribute],
    pub classes: &'a [Class],
}

/// The default view of an exact match: every record found, from every source, with its
/// summary attributes and abbreviated sections.
pub fn summary_page(
    title: &str,
    records: &[Summary],
    omitted: &[Omitted],
    others: &[Hit],
    sources: &[SourceRecord],
    style: Style,
) -> String {
    let mut s = String::new();
    page_title(&mut s, style, title);
    let mut i = 0;
    while i < records.len() {
        let r = &records[i];
        let source = sources.iter().find(|x| x.slug == r.entry.concept.source);
        // Several records of one source with the same name and kind (e.g. many products
        // called "Metformin") are listed one per line instead of in full.
        let group: Vec<&Summary> = records[i..]
            .iter()
            .take_while(|o| {
                o.entry.via.is_none()
                    && r.entry.via.is_none()
                    && o.entry.concept.source == r.entry.concept.source
                    && o.entry.concept.kind == r.entry.concept.kind
            })
            .collect();
        s.push('\n');
        if group.len() > 1 {
            record_list(&mut s, style, &group, source, title);
            i += group.len();
            continue;
        }
        let summary: Vec<Attribute> = r.attributes.iter().filter(|a| a.summary).cloned().collect();
        let mut fields = grouped(&summary);
        if !r.classes.is_empty() {
            let names: Vec<String> = r.classes.iter().map(class_label).collect();
            fields.push(("Product classes", names.join(", ")));
        }
        record_head(&mut s, style, r.entry, source, &fields);
        let more = format!("gurd show {}", concept_id(&r.entry.concept));
        for (section, items) in r.sections {
            section_list(&mut s, style, section.label(), items, CARD_ITEMS, &more);
        }
        i += 1;
    }
    omitted_lines(&mut s, style, omitted, sources, title);
    if !others.is_empty() {
        let _ = writeln!(s, "\n{}", style.bold("Other matches"));
        for line in hits(others, style).lines() {
            let _ = writeln!(s, "  {line}");
        }
    }
    let _ = writeln!(
        s,
        "\n{}",
        style.dim(&format!("All details: gurd show {title}"))
    );
    s
}

/// Records of one source listed one per line, each with its first summary attributes.
fn record_list(
    s: &mut String,
    style: Style,
    group: &[&Summary],
    source: Option<&SourceRecord>,
    title: &str,
) {
    let first = &group[0].entry.concept;
    let name = source.map_or(first.source.as_str(), |src| src.title.as_str());
    let _ = writeln!(s, "{}", style.bold(name));
    let mut f = Fields { s, width: 7 };
    match source {
        Some(src) => f.line("Release", &release_note(src, style)),
        None => f.line("Release", &first.source_version),
    }
    if let Some(notice) = source.and_then(|src| src.notice.as_deref()) {
        f.line("Note", notice);
    }
    let _ = writeln!(
        s,
        "\n  {} {}",
        style.bold(&format!("{}s named {}", first.kind_label, first.name)),
        style.dim(&format!("({})", group.len()))
    );
    for r in group.iter().take(CARD_ITEMS) {
        let values: Vec<&str> = r
            .attributes
            .iter()
            .filter(|a| a.summary)
            .map(|a| a.value.as_str())
            .fold(Vec::new(), |mut v, x| {
                if !v.contains(&x) {
                    v.push(x);
                }
                v
            })
            .into_iter()
            .take(3)
            .collect();
        for (n, text) in wrap(&values.join(" · "), WIDTH - 6).iter().enumerate() {
            let indent = if n == 0 { "    " } else { "      " };
            let _ = writeln!(s, "{indent}{text}");
        }
        let _ = writeln!(s, "      {}", style.dim(&concept_id(&r.entry.concept)));
    }
    if group.len() > CARD_ITEMS {
        let _ = writeln!(
            s,
            "    {}",
            style.dim(&format!(
                "… {} more (gurd show {title})",
                group.len() - CARD_ITEMS
            ))
        );
    }
}

fn page_title(s: &mut String, style: Style, title: &str) {
    let _ = writeln!(s, "{}", style.bold(title));
    let _ = writeln!(s, "{}", "═".repeat(title.chars().count().clamp(3, 60)));
}

fn omitted_lines(
    s: &mut String,
    style: Style,
    omitted: &[Omitted],
    sources: &[SourceRecord],
    title: &str,
) {
    if omitted.is_empty() {
        return;
    }
    let parts: Vec<String> = omitted
        .iter()
        .map(|o| {
            let name = sources
                .iter()
                .find(|x| x.slug == o.source)
                .map_or(o.source.as_str(), |x| x.title.as_str());
            format!("{} in {name}", o.count)
        })
        .collect();
    let text = format!(
        "Linked records not shown: {} (gurd show {title})",
        parts.join(", ")
    );
    s.push('\n');
    for line in wrap(&text, WIDTH) {
        let _ = writeln!(s, "{}", style.dim(&line));
    }
}

/// The full view: everything the database holds about each record on the page.
pub fn details_page(
    title: &str,
    records: &[(PageEntry, Details)],
    omitted: &[Omitted],
    sources: &[SourceRecord],
    style: Style,
) -> String {
    let mut s = String::new();
    page_title(&mut s, style, title);
    for (entry, d) in records {
        let source = sources.iter().find(|x| x.slug == d.concept.source);
        s.push('\n');
        s.push_str(&details(entry, d, source, style));
    }
    omitted_lines(&mut s, style, omitted, sources, title);
    s
}

/// Everything the database holds about one record.
pub fn details(
    entry: &PageEntry,
    d: &Details,
    source: Option<&SourceRecord>,
    style: Style,
) -> String {
    let mut s = String::new();
    let fields = grouped(&d.attributes);
    record_head(&mut s, style, entry, source, &fields);

    for (section, items) in &d.sections {
        section_list(&mut s, style, section.label(), items, usize::MAX, "");
    }

    if !d.names.is_empty() {
        let _ = writeln!(s, "\n  {}", style.bold("Names"));
        for n in &d.names {
            let label = match n.name_type.as_str() {
                "preferred" => "preferred",
                "prescribable" => "prescribable",
                "tall_man" => "tall man",
                _ => "synonym",
            };
            let _ = writeln!(
                s,
                "    {}  {}",
                n.name,
                style.dim(&format!("({label}, {})", n.source_type))
            );
        }
    }

    if !d.relationships.is_empty() {
        let _ = writeln!(s, "\n  {}", style.bold("Relationships"));
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
                "    {predicate} {}",
                style.dim(&format!("({})", group.len()))
            );
            let id_width = group.iter().map(|c| concept_id(c).len()).max().unwrap_or(0);
            for c in &group {
                let _ = writeln!(
                    s,
                    "      {}  {}",
                    style.dim(&format!("{:id_width$}", concept_id(c))),
                    c.name
                );
            }
            i += group.len();
        }
    }

    if !d.identifiers.is_empty() {
        let _ = writeln!(s, "\n  {}", style.bold("Identifiers"));
        let mut systems: Vec<&str> = d.identifiers.iter().map(|i| i.system.as_str()).collect();
        systems.dedup();
        let width = systems.iter().map(|x| x.len()).max().unwrap_or(0);
        let mut f = Fields {
            s: &mut s,
            width: width + 2,
        };
        for system in systems {
            let values: Vec<&str> = d
                .identifiers
                .iter()
                .filter(|i| i.system == system)
                .map(|i| i.value.as_str())
                .collect();
            f.line(&format!("  {system}"), &values.join(", "));
        }
    }

    if let Some(src) = source {
        let _ = writeln!(s, "\n  {}", style.bold("Source"));
        let _ = writeln!(s, "    {}", src.title);
        let _ = writeln!(s, "    Provider  {}", src.provider);
        let _ = writeln!(s, "    Release   {}", release_note(src, style));
        let _ = writeln!(s, "    Imported  {}", src.imported_at);
        for line in wrap(&src.attribution, 74) {
            let _ = writeln!(s, "    {}", style.dim(&line));
        }
    }
    s
}

/// Greedy word wrap.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
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
                "({age} days old; may not reflect the provider's latest data; run `gurd update`)"
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
        let _ = writeln!(s, "  Status    not installed (run `gurd update`)");
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
    let local: Vec<&str> = info
        .sources
        .iter()
        .filter(|x| !x.redistributable)
        .map(|x| x.slug.as_str())
        .collect();
    if !local.is_empty() {
        let text = format!(
            "This database must not be shared: {} may not be redistributed. \
             See `gurd sources`.",
            local.join(", ")
        );
        s.push('\n');
        for line in wrap(&text, WIDTH) {
            let _ = writeln!(s, "{}", style.bold(&line));
        }
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
                let _ = writeln!(s, "  Installed  no (run `gurd update --source {slug}`)");
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

/// `Biguanide (fda_epc)`
fn class_label(c: &Class) -> String {
    format!("{} ({})", c.name, c.system)
}

/// Classes from one source for the records found, e.g. several products of one name.
pub fn classes(
    title: &str,
    first: &ConceptRef,
    records: usize,
    classes: &[&Class],
    style: Style,
) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{}", style.bold(title));
    let what = if records == 1 {
        format!("{} ({})", first.name, concept_id(first))
    } else {
        format!("{records} records named {}", first.name)
    };
    let _ = writeln!(s, "  {}", style.dim(&what));
    let width = classes.iter().map(|c| c.system.len()).max().unwrap_or(0);
    for c in classes {
        let _ = writeln!(s, "  {:width$}  {}", c.system, c.name);
    }
    s
}
