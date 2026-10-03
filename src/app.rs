use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::{Result, bail};
use serde::Serialize;

use crate::cli::{Cli, Command, SearchArgs, UpdateArgs};
use crate::config;
use crate::database::{Database, NotInstalled};
use crate::details::{self, Details};
use crate::models::{ConceptRef, DatabaseInfo, SourceRecord};
use crate::output::{self, JSON_VERSION, Style};
use crate::render;
use crate::search::{self, Hit, Match};
use crate::sources::{self, SourceInfo};
use crate::update;

pub fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Some(Command::Database) => database(&cli),
        Some(Command::Search(ref args)) => find(&cli, args),
        None => find(&cli, &cli.search),
        Some(Command::Show { ref query }) => show(&cli, &query.join(" ")),
        Some(Command::Rxcui { ref rxcui }) => {
            lookup_identifier(&cli, &format!("rxcui:{rxcui}"), "rxcui", rxcui.trim())
        }
        Some(Command::Id { ref identifier }) => match details::parse_identifier(identifier) {
            Some((system, value)) => lookup_identifier(&cli, identifier, &system, value),
            None => {
                eprintln!("gurd: expected SYSTEM:VALUE, e.g. rxcui:6809 or unii:9100L32L2N");
                Ok(ExitCode::from(2))
            }
        },
        Some(Command::Class { ref query }) => class(&cli, &query.join(" ")),
        Some(Command::Sources) => list_sources(&cli),
        Some(Command::Update(ref args)) => update(&cli, args),
        Some(Command::Remove { ref source }) => remove(&cli, source),
    }
}

fn open(cli: &Cli) -> Result<Database> {
    Database::open(&config::database_path(cli.global.db.as_deref())?)
}

fn style(cli: &Cli) -> Style {
    Style::new(cli.global.color, cli.global.no_color)
}

fn exit(found: bool) -> ExitCode {
    if found {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[derive(Serialize)]
struct SourceVersion {
    source: String,
    version: String,
}

fn source_versions(sources: &[SourceRecord]) -> Vec<SourceVersion> {
    sources
        .iter()
        .map(|s| SourceVersion {
            source: s.slug.clone(),
            version: s.version.clone(),
        })
        .collect()
}

fn find(cli: &Cli, args: &SearchArgs) -> Result<ExitCode> {
    let query = args.query_string();
    if args.details {
        return show(cli, &query);
    }
    let db = open(cli)?;
    let sources = db.sources()?;
    let hits = search::search(&db, &query, args.limit)?;

    if cli.global.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            query: &'a str,
            sources: Vec<SourceVersion>,
            results: &'a [Hit],
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            query: &query,
            sources: source_versions(&sources),
            results: &hits,
        })?;
        return Ok(exit(!hits.is_empty()));
    }

    match hits.first() {
        None => eprintln!("gurd: no match for \"{query}\""),
        // An exact match gets the summary page; anything else gets the list.
        Some(top) if top.matched == Match::Exact => {
            let mut exact: Vec<ConceptRef> = hits
                .iter()
                .filter(|h| h.matched == Match::Exact)
                .map(|h| h.concept.clone())
                .collect();
            by_source_order(&mut exact);
            let (entries, omitted) = details::page(&db, exact, LINKED_ON_SUMMARY, true)?;
            let mut sections = Vec::new();
            let mut attributes = Vec::new();
            let mut classes = Vec::new();
            // Records shown one per line (several of one source and kind) need no sections.
            let listed: Vec<bool> = entries
                .iter()
                .map(|e| {
                    e.via.is_none()
                        && entries
                            .iter()
                            .filter(|o| {
                                o.via.is_none()
                                    && o.concept.source == e.concept.source
                                    && o.concept.kind == e.concept.kind
                            })
                            .count()
                            > 1
                })
                .collect();
            for (e, &listed) in entries.iter().zip(&listed) {
                if listed {
                    sections.push(Default::default());
                    attributes.push(details::attributes(&db, e.concept.id)?);
                    classes.push(Vec::new());
                    continue;
                }
                sections.push(details::sections(&db, &e.concept)?);
                attributes.push(details::attributes(&db, e.concept.id)?);
                // A record's own classes are among its attributes already; list only
                // those of the records its source relates it to (an ingredient's products).
                let own = details::classes(&db, &e.concept)?;
                let mut related = details::classes_for(&db, &e.concept)?;
                related.retain(|c| !own.iter().any(|o| o.system == c.system && o.code == c.code));
                classes.push(related);
            }
            let shown: HashSet<i64> = entries
                .iter()
                .map(|e| e.concept.id)
                .chain(
                    sections
                        .iter()
                        .flat_map(|m| m.values().flatten().map(|c| c.id)),
                )
                .collect();
            let others: Vec<Hit> = hits
                .iter()
                .filter(|h| !shown.contains(&h.concept.id))
                .take(5)
                .cloned()
                .collect();
            let records: Vec<render::Summary> = entries
                .iter()
                .zip(&sections)
                .zip(&attributes)
                .zip(&classes)
                .map(
                    |(((entry, sections), attributes), classes)| render::Summary {
                        entry,
                        sections,
                        attributes,
                        classes,
                    },
                )
                .collect();
            let text = render::summary_page(
                &top.concept.name,
                &records,
                &omitted,
                &others,
                &sources,
                style(cli),
            );
            output::page(&text, !cli.global.no_pager)?;
        }
        Some(_) => {
            output::emit(&render::hits(&hits, style(cli)))?;
            if hits.len() == args.limit {
                eprintln!(
                    "gurd: showing the first {} matches; use --limit to change",
                    args.limit
                );
            }
            eprintln!("gurd: no exact match; `gurd show {query}` shows the best match in full");
        }
    }
    Ok(exit(!hits.is_empty()))
}

/// Orders records by source, in the order of `sources::builtin()` (official sources
/// first), keeping the search ranking within a source.
fn by_source_order(concepts: &mut [ConceptRef]) {
    let order: Vec<String> = sources::builtin().iter().map(|s| s.info().slug).collect();
    concepts.sort_by_key(|c| {
        order
            .iter()
            .position(|s| *s == c.source)
            .unwrap_or(usize::MAX)
    });
}

/// Linked records shown per source and identifier: on the summary page, and in full.
const LINKED_ON_SUMMARY: usize = 3;
const LINKED_IN_DETAILS: usize = 20;

/// `gurd show QUERY`: the full page for the best match and every other record with the
/// same name, plus linked records; or for every record with an identifier when QUERY
/// looks like `system:value`.
fn show(cli: &Cli, query: &str) -> Result<ExitCode> {
    if let Some((system, value)) = details::parse_identifier(query) {
        return lookup_identifier(cli, query, &system, value);
    }
    let db = open(cli)?;
    let best = search::search(&db, query, 1)?;
    let mut concepts = Vec::new();
    if let Some(hit) = best.first() {
        if hit.matched == Match::Fuzzy && !cli.global.json {
            eprintln!("gurd: no match for \"{query}\"; showing the closest name");
        }
        // Every record named exactly like the best match, from any source.
        concepts = search::search(&db, &hit.concept.name, 0)?
            .into_iter()
            .filter(|h| h.matched == Match::Exact)
            .map(|h| h.concept)
            .collect();
        if !concepts.iter().any(|c| c.id == hit.concept.id) {
            concepts.insert(0, hit.concept.clone());
        }
    } else if !cli.global.json {
        eprintln!("gurd: no match for \"{query}\"");
    }
    let title = concepts.first().map(|c| c.name.clone()).unwrap_or_default();
    by_source_order(&mut concepts);
    print_details(cli, &db, query, &title, concepts)
}

fn lookup_identifier(cli: &Cli, query: &str, system: &str, value: &str) -> Result<ExitCode> {
    let db = open(cli)?;
    let concepts = details::by_identifier(&db, system, value)?;
    if concepts.is_empty() && !cli.global.json {
        eprintln!("gurd: nothing has the identifier {system}:{value}");
    }
    let title = concepts.first().map(|c| c.name.clone()).unwrap_or_default();
    print_details(cli, &db, query, &title, concepts)
}

fn print_details(
    cli: &Cli,
    db: &Database,
    query: &str,
    title: &str,
    concepts: Vec<ConceptRef>,
) -> Result<ExitCode> {
    let found = !concepts.is_empty();
    let sources = db.sources()?;
    let (entries, omitted) = details::page(db, concepts, LINKED_IN_DETAILS, false)?;
    let all: Vec<(details::PageEntry, Details)> = entries
        .into_iter()
        .map(|e| Ok((e.clone(), details::details(db, e.concept)?)))
        .collect::<Result<_>>()?;

    if cli.global.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            query: &'a str,
            sources: Vec<SourceVersion>,
            concepts: Vec<&'a Details>,
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            query,
            sources: source_versions(&sources),
            concepts: all.iter().map(|(_, d)| d).collect(),
        })?;
    } else if found {
        let text = render::details_page(title, &all, &omitted, &sources, style(cli));
        output::page(&text, !cli.global.no_pager)?;
    }
    Ok(exit(found))
}

fn database(cli: &Cli) -> Result<ExitCode> {
    let path = config::database_path(cli.global.db.as_deref())?;
    let info = match Database::open(&path) {
        Ok(db) => DatabaseInfo {
            path: path.display().to_string(),
            installed: true,
            schema_version: Some(crate::database::SCHEMA_VERSION),
            built_at: db.meta("built_at")?,
            built_by: db.meta("built_by")?,
            sources: db.sources()?,
        },
        Err(err) if err.is::<NotInstalled>() => DatabaseInfo {
            path: path.display().to_string(),
            installed: false,
            schema_version: None,
            built_at: None,
            built_by: None,
            sources: Vec::new(),
        },
        Err(err) => return Err(err),
    };

    if cli.global.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            database: &'a DatabaseInfo,
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            database: &info,
        })?;
    } else {
        output::emit(&render::database(&info, &available_sources(), style(cli)))?;
    }

    Ok(if info.installed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn update(cli: &Cli, args: &UpdateArgs) -> Result<ExitCode> {
    let path = config::database_path(cli.global.db.as_deref())?;
    let Some(source) = sources::find(&args.source) else {
        let known: Vec<_> = sources::builtin().iter().map(|s| s.info().slug).collect();
        bail!(
            "unknown source `{}` (available: {})",
            args.source,
            known.join(", ")
        );
    };

    #[cfg(feature = "net")]
    let http = crate::net::Http::new();
    #[cfg(feature = "net")]
    let fetch: Option<&dyn sources::Fetch> = Some(&http);
    #[cfg(not(feature = "net"))]
    let fetch: Option<&dyn sources::Fetch> = None;

    let options = update::Options {
        dest: &path,
        from: args.from.as_deref(),
        url: args.url.as_deref(),
        md5: args.md5.clone(),
        force: args.force,
        keep_download: args.keep_download,
        cache_dir: config::cache_dir()?,
    };
    if let Some(file) = &args.from {
        eprintln!("gurd: importing {} from {}", args.source, file.display());
    }
    let tty = std::io::stderr().is_terminal();
    let mut log = |msg: &str| {
        // Progress lines (\r...) only make sense on a terminal.
        if msg.starts_with('\r') || msg == "\n" {
            if tty {
                eprint!("{msg}");
            }
        } else {
            eprintln!("gurd: {msg}");
        }
    };
    let (imported, up_to_date) = match update::update(source.as_ref(), &options, fetch, &mut log)? {
        update::Outcome::Installed(imported) => (imported, Vec::new()),
        update::Outcome::UpToDate { source, version } => {
            (Vec::new(), vec![SourceVersion { source, version }])
        }
    };

    if cli.global.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            database: String,
            imported: Vec<Summary<'a>>,
            up_to_date: &'a [SourceVersion],
        }
        #[derive(Serialize)]
        struct Summary<'a> {
            source: &'a str,
            version: &'a str,
            concepts: u64,
            names: u64,
            identifiers: u64,
            relationships: u64,
            attributes: u64,
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            database: path.display().to_string(),
            imported: imported
                .iter()
                .map(|i| Summary {
                    source: &i.slug,
                    version: &i.version,
                    concepts: i.counts.concepts,
                    names: i.counts.names,
                    identifiers: i.counts.identifiers,
                    relationships: i.counts.relationships,
                    attributes: i.counts.attributes,
                })
                .collect(),
            up_to_date: &up_to_date,
        })?;
    } else {
        let mut s = String::new();
        for u in &up_to_date {
            let _ = writeln!(
                s,
                "{} {} is already installed; nothing changed (use --force to reinstall)",
                u.source, u.version
            );
        }
        if !imported.is_empty() {
            let _ = writeln!(s, "Installed {}", path.display());
        }
        for i in &imported {
            let c = i.counts;
            let _ = writeln!(
                s,
                "  {}  {}  {} concepts, {} names, {} identifiers, {} relationships, {} attributes",
                i.slug,
                i.version,
                c.concepts,
                c.names,
                c.identifiers,
                c.relationships,
                c.attributes
            );
        }
        output::emit(&s)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn remove(cli: &Cli, slug: &str) -> Result<ExitCode> {
    let path = config::database_path(cli.global.db.as_deref())?;
    let mut log = |msg: &str| eprintln!("gurd: {msg}");
    let imported = update::remove(&path, slug, &mut log)?;
    if cli.global.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            database: String,
            removed: &'a str,
            sources: Vec<SourceVersion>,
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            database: path.display().to_string(),
            removed: slug,
            sources: imported
                .iter()
                .map(|i| SourceVersion {
                    source: i.slug.clone(),
                    version: i.version.clone(),
                })
                .collect(),
        })?;
    } else if imported.is_empty() {
        output::emit(&format!(
            "Removed {slug}; no sources remain (the old database is kept as {}.bak)\n",
            path.display()
        ))?;
    } else {
        let rest: Vec<String> = imported
            .iter()
            .map(|i| format!("{} {}", i.slug, i.version))
            .collect();
        output::emit(&format!(
            "Removed {slug}; the database now holds {}\n",
            rest.join(", ")
        ))?;
    }
    Ok(ExitCode::SUCCESS)
}

fn available_sources() -> Vec<SourceInfo> {
    sources::builtin().iter().map(|s| s.info()).collect()
}

/// `gurd sources`: every source this build can import, and what is installed.
fn list_sources(cli: &Cli) -> Result<ExitCode> {
    let installed = match open(cli) {
        Ok(db) => db.sources()?,
        Err(err) if err.is::<NotInstalled>() => Vec::new(),
        Err(err) => return Err(err),
    };
    let available = available_sources();

    if cli.global.json {
        #[derive(Serialize)]
        struct Entry<'a> {
            source: &'a str,
            title: &'a str,
            provider: &'a str,
            license: &'a str,
            attribution: &'a str,
            url: &'a str,
            download_url: Option<&'a str>,
            checksums_url: Option<&'a str>,
            redistributable: bool,
            builtin: bool,
            installed: Option<&'a SourceRecord>,
        }
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            sources: Vec<Entry<'a>>,
        }
        let mut entries: Vec<Entry> = available
            .iter()
            .map(|a| Entry {
                source: &a.slug,
                title: &a.title,
                provider: &a.provider,
                license: &a.license,
                attribution: &a.attribution,
                url: &a.url,
                download_url: a.download_url.as_deref(),
                checksums_url: a.checksums_url.as_deref(),
                redistributable: a.redistributable,
                builtin: true,
                installed: installed.iter().find(|i| i.slug == a.slug),
            })
            .collect();
        for i in installed
            .iter()
            .filter(|i| !available.iter().any(|a| a.slug == i.slug))
        {
            entries.push(Entry {
                source: &i.slug,
                title: &i.title,
                provider: &i.provider,
                license: &i.license,
                attribution: &i.attribution,
                url: &i.url,
                download_url: None,
                checksums_url: None,
                redistributable: i.redistributable,
                builtin: false,
                installed: Some(i),
            });
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            sources: entries,
        })?;
    } else {
        output::emit(&render::sources(&available, &installed, style(cli)))?;
    }
    Ok(ExitCode::SUCCESS)
}

/// `gurd class QUERY`: drug classes of every record matching QUERY exactly (or of the best
/// match), from the sources that provide classes.
fn class(cli: &Cli, query: &str) -> Result<ExitCode> {
    let db = open(cli)?;
    let sources = db.sources()?;
    let mut concepts: Vec<ConceptRef> = match details::parse_identifier(query) {
        Some((system, value)) => details::by_identifier(&db, &system, value)?,
        None => {
            let hits = search::search(&db, query, 0)?;
            let exact: Vec<ConceptRef> = hits
                .iter()
                .filter(|h| h.matched == Match::Exact)
                .map(|h| h.concept.clone())
                .collect();
            if exact.is_empty() {
                hits.into_iter().take(1).map(|h| h.concept).collect()
            } else {
                exact
            }
        }
    };
    by_source_order(&mut concepts);
    let mut found: Vec<(ConceptRef, Vec<details::Class>)> = Vec::new();
    for c in &concepts {
        let classes = details::classes_for(&db, c)?;
        if !classes.is_empty() {
            found.push((c.clone(), classes));
        }
    }

    if cli.global.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            query: &'a str,
            sources: Vec<SourceVersion>,
            concept: Option<&'a ConceptRef>,
            classes: Vec<&'a details::Class>,
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            query,
            sources: source_versions(&sources),
            concept: found.first().map(|(c, _)| c).or(concepts.first()),
            classes: found.iter().flat_map(|(_, cl)| cl).collect(),
        })?;
    } else if !details::has_classifications(&db)? {
        eprintln!("gurd: no installed source provides drug classes");
    } else if concepts.is_empty() {
        eprintln!("gurd: no match for \"{query}\"");
    } else if found.is_empty() {
        eprintln!("gurd: no drug classes recorded for {}", concepts[0].name);
    } else {
        // One list per source: many records of a source (products with the same name)
        // usually share their classes.
        let mut text = Vec::new();
        let mut i = 0;
        while i < found.len() {
            let source = found[i].0.source.clone();
            let group: Vec<&(ConceptRef, Vec<details::Class>)> = found[i..]
                .iter()
                .take_while(|(c, _)| c.source == source)
                .collect();
            let mut classes: Vec<&details::Class> = Vec::new();
            for (_, cl) in &group {
                for c in cl {
                    if !classes
                        .iter()
                        .any(|x| x.system == c.system && x.code == c.code)
                    {
                        classes.push(c);
                    }
                }
            }
            classes.sort_by(|a, b| (&a.system, &a.name).cmp(&(&b.system, &b.name)));
            let title = sources
                .iter()
                .find(|s| s.slug == source)
                .map_or(source.as_str(), |s| s.title.as_str());
            text.push(render::classes(
                title,
                &group[0].0,
                group.len(),
                &classes,
                style(cli),
            ));
            i += group.len();
        }
        output::emit(&text.join("\n"))?;
    }
    Ok(exit(!found.is_empty()))
}
