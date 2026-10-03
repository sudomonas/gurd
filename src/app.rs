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
                eprintln!("drug: expected SYSTEM:VALUE, e.g. rxcui:6809 or unii:9100L32L2N");
                Ok(ExitCode::from(2))
            }
        },
        Some(Command::Class { ref query }) => class(&cli, &query.join(" ")),
        Some(Command::Sources) => list_sources(&cli),
        Some(Command::Update(ref args)) => update(&cli, args),
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
        None => eprintln!("drug: no match for \"{query}\""),
        // An exact match gets the summary card; anything else gets the list.
        Some(top) if top.matched == Match::Exact => {
            let sections = details::sections(&db, &top.concept)?;
            let shown: HashSet<i64> = sections.values().flatten().map(|c| c.id).collect();
            let others: Vec<Hit> = hits[1..]
                .iter()
                .filter(|h| !shown.contains(&h.concept.id))
                .take(5)
                .cloned()
                .collect();
            let source = sources.iter().find(|s| s.slug == top.concept.source);
            output::emit(&render::card(
                &top.concept,
                &sections,
                &others,
                source,
                style(cli),
            ))?;
        }
        Some(_) => {
            output::emit(&render::hits(&hits, style(cli)))?;
            if hits.len() == args.limit {
                eprintln!(
                    "drug: showing the first {} matches; use --limit to change",
                    args.limit
                );
            }
        }
    }
    Ok(exit(!hits.is_empty()))
}

/// `drug show QUERY`: details of the best match, or of every concept with a given
/// identifier when QUERY looks like `system:value`.
fn show(cli: &Cli, query: &str) -> Result<ExitCode> {
    if let Some((system, value)) = details::parse_identifier(query) {
        return lookup_identifier(cli, query, &system, value);
    }
    let db = open(cli)?;
    let hits = search::search(&db, query, 1)?;
    if let Some(hit) = hits.first() {
        if hit.matched == Match::Fuzzy && !cli.global.json {
            eprintln!("drug: no match for \"{query}\"; showing the closest name");
        }
    } else if !cli.global.json {
        eprintln!("drug: no match for \"{query}\"");
    }
    let concepts = hits.into_iter().map(|h| h.concept).collect();
    print_details(cli, &db, query, concepts)
}

fn lookup_identifier(cli: &Cli, query: &str, system: &str, value: &str) -> Result<ExitCode> {
    let db = open(cli)?;
    let concepts = details::by_identifier(&db, system, value)?;
    if concepts.is_empty() && !cli.global.json {
        eprintln!("drug: nothing has the identifier {system}:{value}");
    }
    print_details(cli, &db, query, concepts)
}

fn print_details(
    cli: &Cli,
    db: &Database,
    query: &str,
    concepts: Vec<ConceptRef>,
) -> Result<ExitCode> {
    let found = !concepts.is_empty();
    let sources = db.sources()?;
    let all: Vec<Details> = concepts
        .into_iter()
        .map(|c| details::details(db, c))
        .collect::<Result<_>>()?;

    if cli.global.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            query: &'a str,
            sources: Vec<SourceVersion>,
            concepts: &'a [Details],
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            query,
            sources: source_versions(&sources),
            concepts: &all,
        })?;
    } else if found {
        let style = style(cli);
        let text: Vec<String> = all
            .iter()
            .map(|d| {
                let source = sources.iter().find(|s| s.slug == d.concept.source);
                render::details(d, source, style)
            })
            .collect();
        output::page(&text.join("\n"), !cli.global.no_pager)?;
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
        eprintln!("drug: importing {} from {}", args.source, file.display());
    }
    let tty = std::io::stderr().is_terminal();
    let mut log = |msg: &str| {
        // Progress lines (\r...) only make sense on a terminal.
        if msg.starts_with('\r') || msg == "\n" {
            if tty {
                eprint!("{msg}");
            }
        } else {
            eprintln!("drug: {msg}");
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

fn available_sources() -> Vec<SourceInfo> {
    sources::builtin().iter().map(|s| s.info()).collect()
}

/// `drug sources`: every source this build can import, and what is installed.
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

/// `drug class QUERY`: drug classes of the best match, from sources that provide them.
fn class(cli: &Cli, query: &str) -> Result<ExitCode> {
    let db = open(cli)?;
    let sources = db.sources()?;
    let concept = match details::parse_identifier(query) {
        Some((system, value)) => details::by_identifier(&db, &system, value)?
            .into_iter()
            .next(),
        None => search::search(&db, query, 1)?
            .into_iter()
            .next()
            .map(|h| h.concept),
    };
    let classes = match &concept {
        Some(c) => details::classes(&db, c)?,
        None => Vec::new(),
    };

    if cli.global.json {
        #[derive(Serialize)]
        struct Doc<'a> {
            json_version: u32,
            query: &'a str,
            sources: Vec<SourceVersion>,
            concept: Option<&'a ConceptRef>,
            classes: &'a [details::Class],
        }
        output::emit_json(&Doc {
            json_version: JSON_VERSION,
            query,
            sources: source_versions(&sources),
            concept: concept.as_ref(),
            classes: &classes,
        })?;
    } else if !details::has_classifications(&db)? {
        eprintln!("drug: no installed source provides drug classes");
    } else if let Some(c) = &concept {
        if classes.is_empty() {
            eprintln!("drug: no drug classes recorded for {}", c.name);
        } else {
            output::emit(&render::classes(c, &classes, style(cli)))?;
        }
    } else {
        eprintln!("drug: no match for \"{query}\"");
    }
    Ok(exit(!classes.is_empty()))
}
