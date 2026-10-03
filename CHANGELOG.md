# Changelog

All notable changes to this project are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

Changes to the JSON output format ([docs/json.md](docs/json.md)) that break consumers
increment `json_version` and are marked **Breaking** here.

## [Unreleased]

### Added

- New sources: **openFDA NDC Directory** (`openfda`), **RxTerms** (`rxterms`), and two
  unofficial Indian datasets for personal use: **1mg medicines** (`1mg`) and the
  **A-Z Medicines Dataset of India** (`az-india`).
- A database can hold several sources. Every `gurd update` rebuilds all installed sources
  together from their stored release files (`gurd.db.sources/`), so versions are always
  recorded side by side.
- `gurd remove SOURCE` removes a source and rebuilds without it.
- The lookup page: an exact match shows one page with a block per source (official sources
  first), every record's fields with readable labels, related records, and records of
  other sources linked by a shared RxCUI or UNII. Many same-name records of one source are
  listed one per line. `gurd show` gives the full page for the best match.
- `gurd class` gathers classes from all matching records, including the single-ingredient
  products of an ingredient; openFDA and A-Z India provide classes.
- `--from` accepts a zip, a single file, or a directory.
- Sources can carry a notice (e.g. "unofficial scrape") shown with their data; `gurd
  database` says when a database must not be shared.
- JSON: `notice` on source records, `label` on attributes, a `gurd remove` document.

### Changed

- Database schema 2. Databases built by 0.2.0 are rebuilt by the next `gurd update`;
  their sources other than the one being updated must be reinstalled, because 0.2.0 did
  not store release files.
- Substring matching is skipped when there is an exact match, and prefix matching uses the
  index again; lookups on large databases take 5–50 ms.

## [0.2.0] - 2026-10-03

### Changed

- **Breaking:** renamed the program from `drug` to `gurd`, to match the repository and
  because the `drug` crate name is taken on crates.io. The binary, crate, environment
  variable (`DRUG_DB` → `GURD_DB`), database file (`gurd.db`), data directory
  (`~/.local/share/gurd`) and cache directory (`~/.cache/gurd`) all changed. A database
  built by 0.1.0 can be moved to the new path, or rebuilt with `gurd update`. The JSON
  format is unchanged.

## [0.1.0] - 2026-10-03

### Added

- First version.
- `drug <query>`: offline tiered search (exact, prefix, word, substring, fuzzy) over a
  local SQLite database, with a summary card for exact matches.
- `drug show`, `--details`, `drug rxcui`, `drug id SYSTEM:VALUE`: detailed view with
  names, sections, relationships, attributes, identifiers and source attribution.
- `drug database` (alias `info`) and `drug sources`: installed and supported sources,
  release versions, licenses, and a warning when a release may be out of date.
- `drug class`: reports that no installed source provides drug classes.
- `drug update`: downloads a release (or uses `--from FILE`), verifies it against
  `--md5` if given, imports it into a temporary database, validates it and swaps it in
  atomically, keeping the previous database as `drug.db.bak`.
- RxNorm Current Prescribable Content adapter.
- `--json` output for every command, `json_version` 1.
- `net` cargo feature (on by default); without it the binary contains no network code.

[Unreleased]: https://github.com/sudomonas/gurd/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/sudomonas/gurd/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/sudomonas/gurd/releases/tag/v0.1.0
