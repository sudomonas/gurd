# Changelog

All notable changes to this project are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

Changes to the JSON output format ([docs/json.md](docs/json.md)) that break consumers
increment `json_version` and are marked **Breaking** here.

## [Unreleased]

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

[Unreleased]: https://github.com/sudomonas/gurd/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/sudomonas/gurd/releases/tag/v0.1.0
