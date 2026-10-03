# gurd developer guide

This guide explains how `gurd` was built, how each part works, which programming
concepts it uses, and how to maintain it. Read it from top to bottom once. After that,
use the [maintenance recipes](#14-maintenance-recipes) as a reference.

Contents:

1. [What the program does](#1-what-the-program-does)
2. [How it was built, step by step](#2-how-it-was-built-step-by-step)
3. [The big picture: layers](#3-the-big-picture-layers)
4. [Following one command through the code](#4-following-one-command-through-the-code)
5. [The database schema](#5-the-database-schema)
6. [Data sources and the RxNorm adapter](#6-data-sources-and-the-rxnorm-adapter)
7. [Building and installing a database safely](#7-building-and-installing-a-database-safely)
8. [Search](#8-search)
9. [Summary cards and details: the navigation table](#9-summary-cards-and-details-the-navigation-table)
10. [`gurd update` and the network](#10-gurd-update-and-the-network)
11. [Output: text, JSON, color, pager](#11-output-text-json-color-pager)
12. [Programming concepts used](#12-programming-concepts-used)
13. [Tests](#13-tests)
14. [Maintenance recipes](#14-maintenance-recipes)
15. [Rules you should not break](#15-rules-you-should-not-break)
16. [Several sources in one database (schema 2)](#16-several-sources-in-one-database-schema-2)
17. [Known limitations and future work](#17-known-limitations-and-future-work)

---

## 1. What the program does

`gurd` is a command-line drug lookup tool that works offline:

```sh
gurd metformin            # search by name
gurd rxcui 6809           # look up by identifier
gurd metformin --json     # the same, for scripts
gurd update               # the only command that uses the network
```

It has two halves:

- **The importer** (`gurd update`) takes a dataset release file (today: NLM's RxNorm
  Current Prescribable Content, a zip of `|`-separated text files) and turns it into a
  SQLite database at `~/.local/share/gurd/gurd.db`.
- **The lookup side** (every other command) only reads that database. It never writes to
  it and never uses the network.

Four ideas guided every design decision:

1. **Offline by construction.** Lookups cannot reach the network because the network
   code is not reachable from them, and in some builds isn't compiled at all.
2. **Provenance.** Every fact in the database records which dataset and which release
   it came from.
3. **Source-neutral core.** RxNorm is the first dataset, not the architecture. Only one
   file (`src/sources/rxnorm.rs`) knows RxNorm's format and vocabulary.
4. **No invented data.** The program shows what the dataset says. Missing data stays
   missing. There are no hand-made aliases and no guesses.

---

## 2. How it was built, step by step

The project was built in eight phases. Each phase ended with `cargo build`,
`cargo test` and `cargo clippy` passing before the next one started.

| Phase | What was built | Key files |
|---|---|---|
| 0. Design | Architecture, crate choices, schema, license research | (design discussion) |
| 1. Skeleton | Cargo project, clap CLI, XDG paths, embedded schema, read-only open, `gurd database` | `cli.rs`, `config.rs`, `database.rs`, `migrations/` |
| 2. Importer | Streaming RRF parser, test fixture of real rows, build-validate-swap install | `sources/`, `import.rs` |
| 3. Search | Five-tier ranked search, list output, benchmark | `search.rs`, `render.rs`, `output.rs` |
| 4. Details | Summary card, `show`, `rxcui`, `id`, pager, navigation table | `details.rs`, `render.rs` |
| 5. JSON | Documented, versioned JSON format and a test that pins it | `docs/json.md`, `tests/json.rs` |
| 6. Versioning | Release age, outdated warning, `gurd sources`, `gurd class` | `database.rs`, `render.rs`, `app.rs` |
| 7. Network update | Download, user-supplied MD5, fake-network tests, `net` feature | `update.rs`, `net.rs` |
| 8. Docs, packaging | README, licenses, changelog, PKGBUILD, release | `*.md`, `packaging/` |

Some decisions changed along the way, and the reasons still matter:

- **"Don't rely on RxNorm."** Early on you asked that users be able to plug in any drug
  database. So no identifier is global. A concept is keyed by its own source's code,
  and RxCUI, NDC and UNII are plain rows in an `identifiers` table. This is why the CLI
  has `gurd id SYSTEM:VALUE`, and why `gurd rxcui 6809` is only a shortcut for
  `gurd id rxcui:6809`.
- **Relationship direction was checked against real data** before any query relied on
  it (see [section 6](#6-data-sources-and-the-rxnorm-adapter)).
- **Navigation became data, not code.** Phase 4 needed "which clinical drugs contain
  this ingredient?". Hard-coding RxNorm relationship names in the app would have broken
  the source-neutral rule, so the adapter writes those paths into a `navigation` table
  instead (see [section 9](#9-summary-cards-and-details-the-navigation-table)).
- **Checksums are manual.** Phase 7 first read NLM's web page to find the MD5. You asked
  for this to be generic rather than RxNorm-specific, so now the user passes `--md5`.
  The tool never scrapes a provider's page.
- **Renamed from `drug` to `gurd`** (v0.2.0), because the `drug` crate name was taken on
  crates.io.

---

## 3. The big picture: layers

```text
             ┌───────────────────────────────┐
  argv  ───► │ main.rs  cli.rs               │  parse arguments (clap)
             └──────────────┬────────────────┘
                            ▼
             ┌───────────────────────────────┐
             │ app.rs                        │  one function per command
             └──┬───────────┬────────────┬───┘
                │           │            │
     lookups    ▼           ▼            ▼   update
   ┌──────────────────┐ ┌──────────┐ ┌──────────────────────┐
   │ search.rs        │ │render.rs │ │ update.rs            │
   │ details.rs       │ │output.rs │ │   ├── net.rs (HTTP)  │
   └────────┬─────────┘ └──────────┘ │   └── import.rs      │
            ▼                        │         ▲            │
   ┌──────────────────┐              │         │ ImportSink │
   │ database.rs      │ read-only    │  sources/mod.rs      │
   │ (SQLite)         │              │  sources/rxnorm.rs   │
   └──────────────────┘              └──────────────────────┘
```

Every file in `src/`, and what it is responsible for:

| File | Lines | Responsibility |
|---|---|---|
| `main.rs` | 23 | Entry point: parse args, run, turn errors into exit codes |
| `lib.rs` | 21 | Declares the modules. The binary is a thin wrapper around this library, so tests can call the library directly |
| `cli.rs` | 151 | Command-line definition (clap derive structs) |
| `app.rs` | 483 | One function per command; wires search, details, rendering, update together |
| `config.rs` | 39 | XDG paths for the database and the cache |
| `database.rs` | 144 | Opening (read-only, schema-version check), creating, reading source metadata |
| `models.rs` | 220 | Shared data types: `Kind`, `NameType`, `Section`, `ConceptRef`, `SourceRecord` |
| `normalize.rs` | 28 | The one function that normalizes names for matching |
| `search.rs` | 280 | Tiered name search and fuzzy matching |
| `details.rs` | 352 | Summary sections, full details, identifier lookup, classes, natural sort |
| `render.rs` | 399 | Plain-text output for every command |
| `output.rs` | 96 | Writing to stdout, JSON, color, pager |
| `import.rs` | 358 | Building, validating and atomically installing a database; `ImportSink` |
| `update.rs` | 232 | The `gurd update` workflow: obtain file → verify → install |
| `net.rs` | 68 | The only HTTP code (only in builds with the `net` feature) |
| `sources/mod.rs` | 174 | The `Source` and `Fetch` traits, the registry of adapters, the zip reader `Input`, hashing |
| `sources/rxnorm.rs` | 401 | Everything RxNorm-specific |

Arrows point downward only: `search.rs` never calls `app.rs`, `database.rs` never
renders text, and adapters never see the CLI. When you add code, keep it that way.

---

## 4. Following one command through the code

Here is what happens when you type `gurd metformin`.

**1. `main.rs`**

```rust
let cli = gurd::cli::Cli::parse();
```

clap reads `argv` and fills in the `Cli` struct from `cli.rs`. If the arguments are
invalid, clap prints an error and exits with status 2 on its own. The `main` function
also handles one special case: options with no query and no subcommand also exit 2.
`GURD_DB` in the environment counts as an "argument" to clap, which is why this check is
done by hand.

**2. `cli.rs`**

`Cli` has three parts: `global` (`--db`, `--json`, `--color`, ...), `search` (the bare
query words, `--details`, `--limit`) and an optional `command` (`show`, `rxcui`, ...).
`gurd metformin` has no subcommand, so `command` is `None` and `search.query` is
`["metformin"]`. Global flags have `global = true`, so they work before or after a
subcommand.

**3. `app::run`**

`run` is a `match` on the command. `None` goes to `find(&cli, &cli.search)`.

**4. `app::find`**

- `open(cli)` resolves the path (`--db`, else `GURD_DB`, else XDG) and calls
  `Database::open`, which opens SQLite **read-only** and checks the schema version.
- `search::search(&db, "metformin", 20)` returns a ranked `Vec<Hit>`.
- In JSON mode it serializes a document and stops.
- Otherwise, if the top hit is an `Exact` match, it fetches `details::sections` for it
  and renders a **summary card**. If not, it renders a **list**.

**5. `search::search`** normalizes the query and runs the tiers (see
[section 8](#8-search)).

**6. `details::sections`** reads the `navigation` rows for the concept's kind and follows
the relationship graph (see [section 9](#9-summary-cards-and-details-the-navigation-table)).

**7. `render::card`** builds a `String`. **`output::emit`** writes it to stdout and
ignores a closed pipe.

**8. Exit code.** `find` returns `ExitCode::SUCCESS` if anything was found, and 1
otherwise. If any `?` returned an error, `main` prints `gurd: <error chain>` to stderr and
exits 1.

Every lookup command follows the same pattern: open → query → JSON or render → exit code.

---

## 5. The database schema

The schema lives in `migrations/0001_init.sql`. It is compiled into the binary with
`include_str!` and applied by `database::create`.

### Tables

```text
sources ─┬─< concepts ─┬─< names            (every spelling of a concept)
         │             ├─< identifiers      (rxcui, ndc, unii, ...)
         │             ├─< attributes       (key/value, e.g. RXN_STRENGTH)
         │             └─< relationships >─ concepts   (subject → predicate → object)
         ├─< classifications ─< concept_classifications   (empty today)
         └─< navigation                     (how to build card sections)
concept_kinds   (application vocabulary: ingredient, brand_name, clinical_drug, ...)
meta            (built_by, built_at)
names_tok, names_tri   (FTS5 full-text indexes over names.norm)
```

| Table | One row is... | Notes |
|---|---|---|
| `sources` | one installed dataset release | Version, release date, license, attribution, file name, SHA-256, verified checksum, retrieval and import times, `origin`, `redistributable`, `stale_after_days` |
| `concept_kinds` | one kind of concept | Seeded by the migration. `rank` orders search results |
| `concepts` | one thing in a dataset (an ingredient, a brand, a clinical drug...) | `source_code` is the source's own key (RxCUI). `source_type` keeps the verbatim term type (`IN`, `SCD`) |
| `names` | one name of a concept | `norm` is the normalized form used for matching. `name_type` is preferred / synonym / prescribable / tall_man |
| `identifiers` | one external code on a concept | `(system, value)` is indexed so `gurd id` is fast |
| `relationships` | one edge: subject → predicate → object | `predicate` is what the app reads; `source_predicate` keeps the original label |
| `attributes` | one key/value fact | Keys kept verbatim (`RXN_STRENGTH`, `RXN_HUMAN_DRUG`, ...) |
| `navigation` | one rule for building a card section | Written by the adapter. See section 9 |
| `meta` | build metadata | `built_by` (`gurd 0.2.0`), `built_at` |

### Design rules (also written at the top of the SQL file)

- **Every source-derived row has a `source_id`.** `tests/database.rs` checks this for
  every table, so a new table without it fails the tests.
- **No global identifier.** RxCUI is an identifier like any other.
- **Records from different sources are linked only by identifiers that a source itself
  asserted, never by similar names.** Linking by name would be inventing data.
- **Source vocabulary is kept verbatim** in `source_*` columns, next to the app's own
  normalized vocabulary.

### SQLite features used

- **`STRICT` tables.** SQLite normally accepts any type in any column. `STRICT` makes it
  reject, for example, text in an `INTEGER` column.
- **`WITHOUT ROWID`** on `identifiers`. The primary key *is* the storage order, which
  saves space for a table with 300k+ small rows.
- **`CHECK` constraints** on `origin`, `redistributable` and `name_type`.
- **Foreign keys** (`REFERENCES`). They are checked once after import with
  `pragma_foreign_key_check` rather than on every insert.
- **FTS5 external-content tables.** `names_tok` and `names_tri` index `names.norm`
  without storing a second copy of the text (`content='names'`). `names_tok` uses the
  `unicode61` tokenizer (whole words). `names_tri` uses the `trigram` tokenizer, which
  indexes every three-character slice and so allows fast substring search. Because
  imports are bulk-only into a fresh file, the indexes are filled once with the FTS
  `'rebuild'` command instead of with triggers.
- **`PRAGMA user_version`** stores the schema version (currently 1). `Database::open`
  refuses a newer or older version with a clear message.

### Views of the data you might want

Every row is traceable. For example, to see every name of metformin and where each came
from:

```sql
SELECT n.name, n.name_type, n.source_type, s.slug, s.version
FROM names n JOIN concepts c ON c.id = n.concept_id JOIN sources s ON s.id = n.source_id
WHERE c.source_code = '6809';
```

`sqlite3 ~/.local/share/gurd/gurd.db` is the fastest way to explore.

---

## 6. Data sources and the RxNorm adapter

### The `Source` trait (`src/sources/mod.rs`)

```rust
pub trait Source {
    fn info(&self) -> SourceInfo;                                  // static description
    fn release(&self, input: &Input) -> Result<Release>;           // version of this file
    fn import(&self, input: &Input, sink: &mut ImportSink) -> Result<()>;
    fn validate(&self, conn: &Connection, source_id: i64) -> Result<()> { Ok(()) }
}

pub fn builtin() -> Vec<Box<dyn Source>> {
    vec![Box::new(rxnorm::RxNorm), Box::new(rxterms::RxTerms), Box::new(openfda::OpenFda),
         Box::new(onemg::OneMg), Box::new(azindia::AzIndia)]
}
```

- `info()` returns the slug (`rxnorm`), title, provider, license text, required
  attribution, website, whether it may be redistributed, how old a release can get
  before it is "outdated" (45 days), the default download URL, and where the provider
  publishes checksums.
- `release()` reads the version **from inside the file**, so renaming the zip doesn't
  matter.
- `import()` writes rows. It can only write through `ImportSink`, which stamps every
  row with the source's id. That is how provenance is guaranteed: an adapter has no way
  to write a row without a `source_id`.
- `validate()` runs source-specific sanity checks after the import. If it fails, the
  update is aborted.

`Input` wraps the zip file. It computes the SHA-256 when opened, finds members by
trailing path (`rrf/RXNCONSO.RRF` matches even if the zip has a top folder), and
`read()` streams one member through a closure. The file is never unpacked to disk.

### The RxNorm release format

The prescribable release is a zip with a readme and three files in `rrf/`. Each line is
a row of fields separated by `|`, with a trailing `|`:

| File | Contains | Fields used |
|---|---|---|
| `RXNCONSO.RRF` | Names ("atoms"). Each RXCUI has one main term type and possibly synonyms | RXCUI(0), RXAUI(7), SAB(11), TTY(12), CODE(13), STR(14), SUPPRESS(16) |
| `RXNREL.RRF` | Relationships between concepts | RXCUI1(0), STYPE1(2), RXCUI2(4), STYPE2(6), RELA(7), SAB(10), SUPPRESS(14) |
| `RXNSAT.RRF` | Attributes, including NDC codes | RXCUI(0), ATN(8), SAB(9), ATV(10), SUPPRESS(11) |

### What the adapter does (`src/sources/rxnorm.rs`)

1. **`import_concepts`** reads RXNCONSO:
   - It keeps unsuppressed rows (`SUPPRESS = N`) with `SAB = RXNORM`. It also keeps the
     UNII codes from `SAB = MTHSPL, TTY = SU` rows, but no other MTHSPL content.
   - For each RXCUI, the atom whose term type is a *concept* term type (`IN`, `PIN`,
     `MIN`, `BN`, `SCD`, `SBD`, `GPCK`, `BPCK`, `SCDC`, ...) creates the concept.
     `kind_for()` maps each term type to an app `Kind`. If an RXCUI has two such atoms,
     the import fails, because that would mean the data doesn't match our assumptions.
   - Every atom becomes a `names` row: `PSN` → prescribable, `TMSY` → tall man, others →
     synonym, and the concept-defining atom → preferred.
   - Each concept gets an `rxcui` identifier. UNIIs become `unii` identifiers.
   - It returns a `HashMap<RXCUI, concept id>` that the next two steps use.
2. **`import_relationships`** reads RXNREL. It keeps RXNORM, concept-to-concept,
   unsuppressed rows with a RELA label.
   **Direction matters:** a row `RXCUI1|..|RXCUI2|..|RELA` means "RXCUI2 RELA RXCUI1".
   For example, `6809|..|151827|..|tradename_of` means "Glucophage (151827) tradename_of
   metformin (6809)". So `subject = RXCUI2` and `object = RXCUI1`. This was checked
   against the real release before relying on it, and `tests/rxnorm.rs` pins it.
3. **`import_attributes`** reads RXNSAT. `NDC` values become `ndc` identifiers.
   Everything else is stored as an attribute under its original ATN name.
4. **Navigation rows** (the `NAVIGATION` constant) are written. See section 9.

`each_row()` is the tiny RRF parser. It reads one line at a time into a reused
`String`, splits on `|`, checks the field count (`fields + 1` because of the trailing
`|`), and calls your closure with `&[&str]` slices that borrow from the line, so nothing
is copied. Errors carry the line number.

`prescribe_date()` turns `Readme_Full_Prescribe_09082026.txt` into `2026-09-08`.

`validate()` fails if there are no ingredients or clinical drugs, no relationships, or a
concept without its RxCUI identifier.

### Test fixture

`tests/fixtures/rxnorm-mini/` holds 982 real rows covering metformin, Glucophage,
amoxicillin/clavulanate, Augmentin, acetaminophen and aspirin, copied verbatim from the
2026-09-08 release. RxNorm is public domain, so shipping them is allowed.
`scripts/make-fixture.sh` regenerates them from an unpacked release. Tests zip the
fixture on the fly (`tests/common/mod.rs::fixture_zip`).

---

## 7. Building and installing a database safely

`import::install(dest, jobs)` is the heart of update safety. The rule is that the
installed database is never modified. A new one is built next to it and swapped in only
when it is complete and valid.

```text
gurd.db (installed, untouched)
   │
   ├── build gurd.db.tmp-<pid>
   │      create schema → for each source: insert `sources` row, adapter imports
   │      in one transaction → rebuild FTS → validate → ANALYZE, VACUUM → fsync
   │      → reopen through Database::open (the same path lookups use)
   │
   ├── any error ──► delete tmp file, return error. gurd.db is byte-for-byte unchanged
   │
   └── success ──► hard-link gurd.db → gurd.db.bak (copy if links unsupported)
                   rename(tmp, gurd.db)   ← atomic on POSIX
                   fsync the directory
```

Why each step is there:

- **Same directory for the temp file.** `rename()` is atomic only within one filesystem.
  A reader sees either the whole old file or the whole new one, never a half-written
  file.
- **`journal_mode = OFF`, `synchronous = OFF` during the build.** On failure the file is
  thrown away anyway, so crash safety during the build doesn't matter. This makes the
  import much faster (about 10 s for the full release).
- **One transaction per source.** Thousands of inserts in one transaction are far faster
  than autocommit.
- **`prepare_cached`** in `ImportSink` compiles each `INSERT` once and reuses it for
  hundreds of thousands of rows.
- **Validation** (`import::validate`): `PRAGMA integrity_check`, no foreign-key
  violations, every source has concepts, and at least one of each source's navigation
  paths matches real relationships. Then the adapter's own `validate`.
- **`ANALYZE`** gives SQLite's query planner statistics. **`VACUUM`** compacts the file.
- **The hard-link backup** keeps `gurd.db` in place until the rename, so there is never a
  moment without an installed database.
- **`database::create` refuses to overwrite an existing file**, so a bug can't build on
  top of the real database.

`tests/update.rs` and `tests/network_update.rs` check every failure path: the installed
file is compared byte-for-byte before and after, and no temp files may remain.

---

## 8. Search

`search::search(db, query, limit)` in `src/search.rs`.

**Normalization** (`normalize.rs`): lowercase, collapse whitespace, trim. The same
function is applied to names at import time (stored in `names.norm`) and to the query,
so both sides always agree.

**Tiers**, tried in order. A concept appears once, under the best tier that found it:

| Tier | SQL condition | Index used | Example |
|---|---|---|---|
| exact | `n.norm = ?1` | B-tree `names_norm` | `metformin` |
| prefix | `n.norm > q AND n.norm < q || char(1114111)` | B-tree range scan | `metfor` |
| token | `names_tok MATCH '"amoxicillin" "clav"*'` | FTS5 words | `amoxicillin clavulanate`, any order |
| substring | `names_tri MATCH '"tformi"'` (3+ chars) | FTS5 trigram | `tformi` |
| fuzzy | edit distance in Rust | length filter in SQL | `metfromin` → metformin |

Notes:

- The **prefix** trick: every string that starts with `q` sorts between `q` and
  `q` followed by the highest Unicode code point (`char(1114111)`). This lets the B-tree
  index answer "starts with" without `LIKE`.
- **`token_query`** splits the query on non-alphanumerics, quotes each word as an FTS5
  phrase, and adds `*` to the last one so a half-typed final word still matches.
  **`phrase`** escapes `"`, so user input can never inject FTS5 syntax (`OR`, `NEAR`,
  ...). A test covers this.
- **One name per concept** (`tier_hits`): a concept can have many matching names. The SQL
  groups by concept and picks the preferred name if it matched, otherwise the shortest
  (`min((name_type <> 'preferred') * 1000000 + length(name))`). SQLite returns the bare
  column `n.name` from the row that holds the `min()`.
- **Ranking** inside a tier: `concept_kinds.rank` (ingredients 10, brands 15, precise
  ingredients 20, ... dose form groups 95), then the matched name's length, then the
  name.
- **Fuzzy** runs only when every other tier found nothing. It considers only ingredient,
  precise-ingredient, multiple-ingredient and brand names (what people actually type).
  SQL pre-filters by length. Rust computes the **optimal string alignment distance**
  (Levenshtein plus swapping two adjacent letters as one edit), allowing 1 edit for 4–5
  characters, 2 for 6–10 and 3 for longer queries. **Only the closest distance is
  returned**, so it behaves like "did you mean", not "everything similar".

Speed on the real 143 MB database: 2–20 ms per command including process start.
`scripts/bench.sh` measures it.

---

## 9. Summary cards and details: the navigation table

The card for `metformin` shows sections such as "Brands" and "Clinical drugs". Those
are not stored anywhere; they are found by walking the relationship graph. But the walk
depends on how a dataset shapes its graph. In RxNorm:

```text
metformin (IN) ──ingredient_of──► metformin 500 MG (SCDC) ──constitutes──► metformin 500 MG Oral Tablet (SCD)
```

If `details.rs` contained `"ingredient_of"`, the core would know RxNorm vocabulary. So
the adapter **declares** the walks as data:

```rust
// (from kind, section, predicates to follow, keep concepts of this kind)
(Ingredient, ClinicalDrugs, &["ingredient_of", "constitutes"], ClinicalDrug),
(Ingredient, Brands,        &["has_tradename"],               BrandName),
(BrandName,  Ingredients,   &["tradename_of"],                Ingredient),
```

At import, these rows go into the `navigation` table. At lookup, `details::sections`:

1. selects the navigation rows for this concept's source and kind;
2. for each, `follow()` builds a SQL query with one self-join of `relationships` per
   predicate (`r0 JOIN r1 ON r1.subject_id = r0.object_id ...`), and keeps the end
   concepts of the target kind;
3. de-duplicates, drops empty sections, and sorts: names starting with the concept's own
   name first, then fewer words (single-ingredient products before combinations), then
   **natural order** (`natural_cmp`: "500 MG" before "1000 MG").

Brand → product paths go through components, not brand names, because one brand name
can cover products with different ingredients.

`details::details` adds everything else for the full view: all names, identifiers,
attributes, and every direct relationship.

A new data source just writes its own navigation rows, and cards work for it without any
change to the core. Navigation rows are application metadata, not medical facts.

---

## 10. `gurd update` and the network

`update::update` in `src/update.rs`:

1. **Get the file.** Use `--from FILE` if given (no network at all). Otherwise download
   from `--url`, or from the source's `download_url`, into
   `~/.cache/gurd/<name>.part`, then rename it to `<name>`. A failed download deletes the
   `.part` file.
2. **Verify.** With `--md5`, compute the file's MD5 and refuse a mismatch. Without it,
   print the MD5 and SHA-256 and where the provider publishes checksums, and record no
   checksum.
3. **Already installed?** Read the version from the file. If the installed database has
   the same version for this source, stop (`--force` overrides).
4. **Install** with `import::install` (section 7).
5. Delete the download unless `--keep-download`.

### How "lookups never use the network" is enforced

This is architecture, not a promise:

- **Cargo feature.** `ureq` (the HTTP client) is an optional dependency behind the `net`
  feature (on by default). `lib.rs` has `#[cfg(feature = "net")] pub mod net;`. With
  `--no-default-features`, `net.rs` isn't compiled and the binary contains no HTTP code.
- **One HTTP module.** Only `net.rs` mentions `ureq`.
- **One place creates the client.** `net::Http::new()` is called only inside
  `app::update`.
- **Adapters never get network access.** Code reaches the network only through the
  `Fetch` trait, and only `update.rs` receives one.
- **Read-only database** for lookups (`SQLITE_OPEN_READ_ONLY`).
- **`tests/architecture.rs`** scans the source files and fails if `ureq` appears outside
  `net.rs`, if `Http::new(` appears outside `app::update`, or if RxNorm file or field
  names appear outside the adapter.

### Dependency injection for testing

`update()` takes `fetch: Option<&dyn Fetch>` instead of creating an HTTP client itself.
The real program passes `net::Http`. `tests/network_update.rs` passes a `FakeNet` that
serves a fixture zip, or fails mid-download, and records which URLs were requested. This
is how every failure path is tested without touching the Internet.

---

## 11. Output: text, JSON, color, pager

- **stdout vs stderr.** Results go to stdout. Diagnostics ("no match", "showing the
  first 20") go to stderr with a `gurd:` prefix. In JSON mode lookups print nothing on
  stderr. This is what makes `gurd x | grep` and `gurd x --json | jq` work.
- **Exit codes.** 0 found/success, 1 not found or error, 2 usage error.
- **`output::emit`** ignores `BrokenPipe`, so `gurd metformin | head -1` doesn't print an
  error when `head` closes the pipe early.
- **Color** (`output::Style`) is used only for bold/dim emphasis, never to carry
  meaning. It's on only if stdout is a terminal, `NO_COLOR` is unset, and `TERM` isn't
  `dumb`, unless `--color always|never` or `--no-color` says otherwise. It uses raw ANSI
  codes, not a crate.
- **Pager** (`output::page`). Only the detailed view uses it, and only when stdout is a
  terminal. It runs `$PAGER` through `sh -c`, or `less -FRX` by default (`-F` quits if the
  text fits on one screen). If the pager can't start, it prints normally.
- **JSON** (`docs/json.md`). Every document has `json_version: 1`. Each command defines
  a small `#[derive(Serialize)] struct Doc` inline and calls `output::emit_json`. The
  stability rules: within one version, keys are never renamed, removed or retyped, and
  new keys may be added. `tests/json.rs` pins the exact key set of every document.
- **Release age.** `database::sources()` computes `age_days` in SQL with `julianday()`.
  `render::release_note` adds "(N days old; may not reflect the provider's latest data;
  run `gurd update`)" once a release is older than `stale_after_days`. This is the
  disclosure the RxNorm terms require for redistributed data.

---

## 12. Programming concepts used

This section maps general concepts to the places in the code where you'll see them.

### Rust language

| Concept | What it means | Where |
|---|---|---|
| **Modules and a library + binary crate** | `lib.rs` exposes modules, `main.rs` is a thin wrapper. Tests use `gurd::search::search` directly | `lib.rs`, `main.rs`, `tests/*` |
| **Structs and enums** | Data types. Enums with `match` model closed sets such as `Kind`, `Section`, `Match` and `Command` | `models.rs`, `search.rs`, `cli.rs` |
| **Pattern matching** | `match` on enums and tuples. `let ... else` for early returns | `app::run`, `rxnorm::import_concepts` (`match (f[11], f[12])`) |
| **Traits** | Interfaces. `Source` (a dataset adapter), `Fetch` (something that downloads) | `sources/mod.rs` |
| **Trait objects (`dyn Trait`)** | Values whose concrete type is decided at runtime. `Vec<Box<dyn Source>>` is the adapter registry, `&dyn Fetch` is real-or-fake network | `sources::builtin`, `update::update` |
| **Generics** | Code over any type that meets a bound. `hash_file::<D: Digest>` works for both SHA-256 and MD5 | `sources/mod.rs` |
| **Ownership and borrowing** | `&str` slices borrow from a line buffer instead of copying. `each_row` passes `&[&str]` that live only for one line | `rxnorm::each_row` |
| **Lifetimes** | `Job<'a>` holds references to a source and an input. `ImportSink<'c>` borrows the open transaction, so it cannot outlive it | `import.rs` |
| **Closures** | Functions passed as values: the per-row callback, the `log` callback, the download `progress` callback | `each_row`, `update::update`, `Fetch::download` |
| **Iterators** | `map`, `filter`, `find`, `take_while`, `collect`. For example "keep only the closest fuzzy matches" | `search::fuzzy` |
| **Error handling with `Result` and `?`** | Every fallible function returns `anyhow::Result<T>`. `?` propagates errors. `.with_context(...)` adds "while reading rrf/RXNREL.RRF, line 42" | everywhere |
| **Typed errors** | `NotInstalled` is its own error type so `app` can tell "no database yet" (`err.is::<NotInstalled>()`) apart from real failures | `database.rs`, `app::database` |
| **Interior mutability (`RefCell`)** | `Input::read` takes `&self` but the zip reader needs mutation. `RefCell` checks borrowing at runtime | `sources::Input` |
| **Derive macros** | `#[derive(Serialize)]` generates JSON code. `#[derive(Parser)]` generates the CLI parser. Attributes like `#[serde(rename = "source")]` and `#[serde(skip)]` adjust the output | `models.rs`, `cli.rs` |
| **Conditional compilation** | `#[cfg(feature = "net")]` includes or removes code at compile time | `lib.rs`, `app::update` |
| **Compile-time constants and macros** | `include_str!` embeds the SQL schema. `env!("CARGO_PKG_VERSION")` embeds the version. `concat!` joins literals | `database.rs`, `net.rs` |
| **`const` data tables** | `NAVIGATION` is a static table, with `#[rustfmt::skip]` to keep it aligned | `rxnorm.rs` |
| **Unit tests vs integration tests** | `#[cfg(test)] mod tests` inside a file tests private functions. `tests/*.rs` test the public API and the real binary | `search.rs`, `tests/` |

### Databases

| Concept | Where |
|---|---|
| Normalized relational schema, foreign keys, constraints | `migrations/0001_init.sql` |
| Indexes for exact match and range ("starts with") queries | `names_norm`, prefix tier |
| Full-text search (FTS5) with word and trigram tokenizers | `names_tok`, `names_tri` |
| Prepared and cached statements, parameter binding (`?1`), never string-concatenating user input into SQL | `prepare_cached` everywhere |
| Transactions for bulk inserts | `import::build` |
| Graph traversal with self-joins, built dynamically from data | `details::follow` |
| Schema versioning with `PRAGMA user_version` | `database.rs` |
| Read-only connections | `Database::open` |

### Algorithms

| Algorithm | Where |
|---|---|
| Tiered ranking with de-duplication (`HashSet` of seen concepts) | `search::search` |
| Dynamic programming: optimal string alignment edit distance with three rolling rows | `search::edit_distance` |
| Natural sort (digit runs compared as numbers) | `details::natural_cmp` |
| Streaming file hashing in 64 KB chunks | `sources::hash_file` |
| Streaming line parser with a reused buffer | `rxnorm::each_row` |

### Systems and Unix

| Concept | Where |
|---|---|
| Atomic file replacement (temp file + `rename`), `fsync` of file and directory | `import::replace`, `import::build` |
| Hard links for zero-copy backups | `import::replace` |
| XDG Base Directory spec (relative values ignored) | `config.rs` |
| Exit codes, stdout/stderr separation, `isatty` checks | `main.rs`, `output.rs`, `app.rs` |
| `NO_COLOR`, `PAGER`, broken pipes | `output.rs` |
| Spawning a child process and piping to it | `output::page` |

### Software design

| Principle | How it shows up |
|---|---|
| **Layered architecture** | cli → app → search/details → database. Dependencies point one way |
| **Adapter pattern** | Each dataset is an adapter behind `Source`. The core never sees dataset formats |
| **Dependency injection** | `update()` receives a `Fetch` instead of creating one, so tests can fake the network |
| **Single write path** | Adapters write only through `ImportSink`, which enforces provenance |
| **Data over code** | Navigation paths are data written by adapters, not `if source == rxnorm` branches |
| **Fail safe** | Any error during update leaves the old database untouched |
| **Architecture tests** | Rules (offline lookups, formats in adapters) are checked by tests, not just documented |
| **Contract tests** | The JSON key set is pinned by tests |
| **Small dependency tree** | No `dirs`, color, pager or fuzzy-matching crates. Each is a few lines of std code |

### Dependencies (`Cargo.toml`)

| Crate | Why |
|---|---|
| `clap` (derive, env) | Argument parsing, help text, usage errors (exit 2), `GURD_DB` |
| `rusqlite` (bundled) | SQLite. `bundled` compiles SQLite in, which guarantees FTS5 and the trigram tokenizer on every system |
| `serde`, `serde_json` | JSON output |
| `anyhow` | Error type with context chains |
| `zip` (deflate only) | Reading the release archive without unpacking it |
| `sha2`, `md-5` | SHA-256 for provenance, MD5 because providers publish MD5 |
| `ureq` (optional, rustls) | Blocking HTTPS. Much smaller than `reqwest` and needs no async runtime |
| dev: `tempfile` | Temporary directories in tests |

The release profile uses `lto = true`, `codegen-units = 1` and `strip = true` for a
smaller, faster binary (about 5 MB).

---

## 13. Tests

Run everything:

```sh
cargo test                                   # default build
cargo test --no-default-features             # build without network code
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo fmt --check
```

There are 99 tests. None use the network.

| File | Tests | What it checks |
|---|---|---|
| `tests/architecture.rs` | 3 | `ureq` only in `net.rs`. HTTP client created only in `app::update`. RxNorm names only in the adapter |
| `tests/cli.rs` | 17 | The real binary (`CARGO_BIN_EXE_gurd`): exit codes, `--version`, usage errors, missing database, corrupt database, JSON on stdout only, color flags, outdated-release notice, `sources` without a database |
| `tests/database.rs` | 13 | Schema creation, read-only open rejects writes, version checks, required tables and indexes, every data table has `source_id`, Rust `Kind` list matches `concept_kinds` |
| `tests/details.rs` | 13 | Card sections, brand → ingredients, details view, identifier lookup, pager not used when piped (`PAGER=false`) |
| `tests/json.rs` | 6 | Exact key sets of every JSON document |
| `tests/network_update.rs` | 9 | `FakeNet`: failed download, checksum mismatch, a file that isn't a release, up-to-date, `--force`, `--url`, unverified install, `--from` never fetches |
| `tests/rxnorm.rs` | 11 | Importer: kinds, names, identifiers, relationship direction, strengths, version detection, malformed files, navigation predicates exist in the fixture |
| `tests/search.rs` | 12 | Every tier, case-insensitivity, multi-word, unknown terms, FTS syntax escaping, fuzzy only-closest |
| `tests/update.rs` | 7 | Failed import, empty import, failed validation, corrupt or wrong zip, failed first install: all leave the database byte-for-byte unchanged with no temp files |
| unit tests in `src/` | 8 | Normalization, date parsing, term-type mapping, token queries, edit distance, natural sort, identifier parsing, URL file names |

`tests/common/mod.rs` has helpers: `fixture_zip` (zips the fixture like an NLM release),
`make_zip` (arbitrary zip), and `fixture_db` (builds an installed database from the
fixture).

When you fix a bug, add a test that fails without the fix first.

---

## 14. Maintenance recipes

### Update your own database to a new RxNorm release

NLM publishes a new release at the beginning of each month. On the
[RxNorm Files page](https://www.nlm.nih.gov/research/umls/rxnorm/docs/rxnormfiles.html),
copy the dated `RxNorm_full_prescribe_MMDDYYYY.zip` link and its MD5:

```sh
gurd update --url https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_MMDDYYYY.zip --md5 <md5>
```

If validation fails, NLM may have changed something. Your old database is untouched.
Read the error, check the release notes, and adjust the adapter.

### Try a change against real data without touching your installed database

```sh
cargo run --release -- --db /tmp/test.db update --from RxNorm_full_prescribe_MMDDYYYY.zip
cargo run --release -- --db /tmp/test.db metformin
sqlite3 /tmp/test.db 'SELECT kind, count(*) FROM concepts GROUP BY kind'
scripts/bench.sh /tmp/test.db
```

### NLM adds a new term type

Symptoms: concepts missing from search, or an import error. Steps:

1. Add a `Kind` variant in `models.rs`, in `Kind::ALL`, `as_str`.
2. Add a row to `concept_kinds` in the migration (with a `rank`).
3. Map the TTY in `rxnorm::kind_for`.
4. `tests/database.rs` checks the Rust list and the SQL list match.
5. Because the schema changed, existing databases are rebuilt with `gurd update --force`.
   Before the first public release with real users, consider bumping `SCHEMA_VERSION`
   (see below).

### Change the schema

1. Edit `migrations/0001_init.sql` only if no released database must stay readable.
   Otherwise add `migrations/0002_....sql`, append it to `MIGRATIONS` in `database.rs`,
   and bump `SCHEMA_VERSION`. Old databases will then say "run `gurd update` to rebuild
   it", which is correct, because databases are always rebuilt rather than migrated in
   place.
2. Any new table holding source data needs a `source_id` column (a test enforces this).
3. Update `ImportSink` if adapters need to write the new table.

### Change JSON output

1. Make the change, then run `cargo test --test json`. It will fail and show the new key
   set.
2. Update `tests/json.rs` and `docs/json.md`.
3. Adding a key is fine within `json_version` 1. Renaming, removing or changing a type is
   breaking: bump `JSON_VERSION` in `output.rs` and mark it **Breaking** in
   `CHANGELOG.md`.

### Add a new data source

1. **Check the license first**, on the provider's official pages. Record it in
   `DATA-LICENSES.md`. If redistribution is unclear, set `redistributable: false`.
2. Create `src/sources/<name>.rs` with a unit struct implementing `Source`.
3. In `import`, write concepts, names, identifiers, relationships and attributes through
   the sink. Use the source's own code as `source_code`. Use shared identifier systems
   (`unii`, `ndc`, `rxcui`) so concepts link across sources.
4. Write navigation rows for its relationship graph so pages show sections, and
   `attribute_key` rows (label, order, summary or not) for its attributes.
5. Register it in `sources::builtin()`. Its position there is its position on the page,
   so put official sources before unofficial ones. `gurd sources` and
   `gurd update --source <name>` then work automatically.
6. Add a small fixture and tests like `tests/sources.rs`. Use real rows only if the
   license allows redistribution; otherwise make up rows in the same format.
7. Extend `tests/architecture.rs` with the new format's file/field names so they can't
   leak out of the adapter.

`src/sources/onemg.rs` (JSON lines), `azindia.rs` (CSV), `openfda.rs` (one large JSON
document, streamed) and `rxterms.rs` (`|`-separated text) are short examples of each
format.

### Regenerate the test fixture

```sh
unzip RxNorm_full_prescribe_MMDDYYYY.zip -d /tmp/rx
scripts/make-fixture.sh /tmp/rx
cargo test
```

### Release a new version

1. Update `version` in `Cargo.toml`, move CHANGELOG entries from **Unreleased** to the
   new version, and update the version in the README install commands and the PKGBUILD.
2. Run all checks (section 13).
3. Build and package:

   ```sh
   cargo build --release --locked
   V=0.2.1; N=gurd-$V-x86_64-linux-gnu
   mkdir -p dist/$N && cp target/release/gurd README.md LICENSE DATA-LICENSES.md CHANGELOG.md docs/json.md dist/$N/
   (cd dist && tar czf $N.tar.gz $N && sha256sum $N.tar.gz > $N.tar.gz.sha256)
   ```

4. Commit, tag and publish:

   ```sh
   git commit -am "Release $V" && git tag -a v$V -m "gurd $V" && git push origin main v$V
   gh release create v$V --title "gurd $V" --notes-file notes.md dist/$N.tar.gz dist/$N.tar.gz.sha256
   ```

5. Download the asset from GitHub, verify the checksum, and run it once.

The binary built on Arch needs glibc 2.39 or newer. A static musl build or a GitHub
Actions build on an older distro would make it run on more systems.

### Publish to the AUR

In `packaging/arch/`, run `updpkgsums` to fill in the source tarball checksum, test with
`makepkg -si`, generate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO`, and push
both files to `ssh://aur@aur.archlinux.org/gurd.git`.

### Publish to crates.io

The name `gurd` was free as of 2026-10-03. Run `cargo publish --dry-run`, then
`cargo publish`. `Cargo.toml` already has the metadata, and `exclude` keeps `packaging/`
and `scripts/` out of the package.

---

## 15. Rules you should not break

1. **Never add medical content by hand** (aliases, doses, interactions) or generate it.
   If data is missing, show it as missing.
2. **Never let lookups touch the network.** Network code stays in `net.rs`, reached only
   from `gurd update`.
3. **Never modify the installed database in place.** Build new, validate, rename.
4. **Every source-derived row has a `source_id`.** Write only through `ImportSink`.
5. **Dataset formats stay in their adapter.** No RxNorm vocabulary in the core.
6. **Never link records from different sources by name.** Only by identifiers.
7. **Don't break the JSON contract** without bumping `json_version`.
8. **Diagnostics go to stderr**, results to stdout. Color never carries meaning.
9. **Check a dataset's license on the provider's official pages** before supporting it,
   and never call a dataset "open source" just because it can be downloaded.
10. **Keep dependencies few.** Prefer a few lines of std code over a new crate.

---

## 16. Several sources in one database (schema 2)

This section describes what changed when `gurd` went from one source to many. It's the
part to read before touching updates or the page view.

### Updates rebuild everything

`update::verify_and_install` builds a new database from **all** installed sources: the
one being updated (from its new file) plus every other installed source, reopened from
its stored release in `<db>.sources/<slug>/<file>`. That keeps the rule "never silently
mix versions": every source's version is written into the same fresh database.

1. Read the installed sources straight from the old database's `sources` table
   (`stored_records`). This deliberately skips the schema-version check, so an old
   database can still be rebuilt.
2. Copy (hard-link where possible) the new release into `<slug>.new/` (`stage_release`).
3. `import::install` with all the jobs. Each job keeps the old `retrieved_at` and checksum.
4. On success, swap `<slug>.new/` into place (`commit_release`). On failure, delete it.

If a stored release is missing (for example, a database built by 0.2.0), the update
stops with a message telling you to reinstall or remove that source. `gurd remove`
rebuilds from the remaining stored releases; removing the last source moves the
database to `.bak`.

`fs::copy` doesn't keep file times, so `copy_tree` restores them. The 1mg and A-Z
adapters use the file date as their version, and a rebuild must not change it.

### Inputs: zip, file or directory

`sources::Input` hides the difference. `member_names()` lists zip members, the files of
a directory (recursively, hidden files skipped), or the single file. `read(name, f)`
streams a member. A directory's SHA-256 is the hash of its sorted `name\0sha256` list.
`each_csv_record` (RFC 4180, with quoted commas and line breaks) and `each_json_line`
are shared helpers.

### Attribute labels

`migrations/0002_multi_source.sql` adds `attribute_keys (source_id, key, label, rank,
summary)` and `sources.notice`. Adapters declare, for each attribute key, a label
("Composition"), an order, and whether the summary page shows it. Unlisted keys still
appear in the detailed view under their raw names. `details::attributes` returns
attributes in that order with their labels.

### The page

`app::find` (exact match) and `app::show` build a page:

1. **Primary records:** every concept whose normalized name equals the query (or, for
   `show`, the best match's name), sorted by `by_source_order` (the order of
   `sources::builtin()`).
2. **Linked records** (`details::page`, `details::linked`): concepts of *other* sources
   that share an `rxcui` or `unii` identifier with a primary record. On the summary page
   only links between records of the same kind are shown, at most 3 per source and
   identifier; the rest are counted in a "Linked records not shown" line. `show`
   shows up to 20 per source and identifier, of any kind.
3. **Rendering** (`render::summary_page`, `render::details_page`): a block per record.
   Several primary records of the same source and kind become one compact list
   (`record_list`), and the app skips fetching their sections.

Linking by identifier is the only cross-source link. Two records with the same name in
different sources appear on the same page because both match the query, which is what
search does anyway; nothing is stored that connects them.

### Classes

`details::classes_for` returns a concept's own classes plus those of the concepts its
source relates it to directly that relate to no other concept of its kind. For an
ingredient, that means its single-ingredient products. This prevents a combination
product's classes (glipizide/metformin → sulfonylurea) from being attributed to
metformin. Classes are never carried across sources. It returns immediately for sources
without classes (1mg, RxNorm, RxTerms).

### Adapters added

| Adapter | Input | Concepts | Links to others by |
|---|---|---|---|
| `rxterms` | zip of `|` files | clinical/branded drugs and packs named "DISPLAY_NAME STRENGTH", ingredients | `rxcui` (same codes as RxNorm) |
| `openfda` | zip with one JSON document, streamed with a serde `Visitor` so 245 MB is never held in memory | products, active ingredients, FDA classes | `rxcui`, `unii`, 11-digit `ndc`, `spl_set_id` |
| `1mg` | directory of JSON-lines files | products, ingredients parsed from the composition text | none (Indian products have no shared codes) |
| `az-india` | one CSV | products with every column as attributes, classes | none |

### Performance notes

On the 1.25 GB database with all five sources, warm lookups take 5–50 ms. Two fixes
were needed:

- The prefix tier compared against `?1 || char(1114111)`. An expression on the right
  stops SQLite from using the index (111 ms). The bound is now computed in Rust and
  passed as `?3` (0.4 ms).
- The trigram substring tier is slow for long queries full of common trigrams
  ("tablet"), so it is skipped when there is an exact match.

## 17. Known limitations and future work

- **Sources not built yet:** India Drug Registry (needs a sample portal export),
  NLEM, CDSCO lists, Jan Aushadhi (PDFs), ICMR/MoHFW guidelines (needs a document store),
  WHO EML, Wikidata, DailyMed and ChEMBL. See `DATA-LICENSES.md`.
- **User bundle format not built.** The planned format is a `source.json` manifest plus
  TSV files, to import any drug database without writing Rust, marked
  `origin = 'bundle'`, `redistributable = 0`. The schema already has these columns.
- **`gurd class`** works but no installed source provides classes yet.
- **`paracetamol`** finds nothing because RxNorm has no such synonym. This is by design
  (no invented aliases). A future source with international names would fix it.
- **Disk use.** With all five sources: a 1.25 GB database, the previous one as `.bak`,
  and about 300 MB of stored releases. Each update rebuilds every source (about
  1.5 minutes). A `--no-backup` flag would be easy.
- **Release binaries** are Linux x86_64 with glibc 2.39+ only.
- **A full real `gurd update` download** (about 75 MB from NLM, which can be slow) has
  not been run end to end. The same code paths are covered by the fake-network tests,
  and the real file was imported via `--from`.
