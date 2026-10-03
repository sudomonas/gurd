# Contributing

Thanks for helping. `gurd` aims to stay a small Unix-style lookup tool, so please read
the ground rules before starting on something large; opening an issue first is welcome.

## Ground rules

- **No invented medical information.** Everything shown must come from an installed
  dataset. Don't add hand-written aliases, doses, interactions or other data to the code;
  missing data is shown as missing. Generated (e.g. LLM-written) drug content will not be
  accepted.
- **No network during lookups.** Only `gurd update` may use the network, and only
  through `src/net.rs`. `tests/architecture.rs` enforces this.
- **Keep dataset formats in their adapters.** File formats and vocabulary (RRF files, TTY
  codes, RELA labels, ...) belong in `src/sources/<source>.rs` only. The CLI, search and
  schema stay source-neutral.
- **Every fact keeps its source.** Rows that hold source data carry a `source_id`.
- **Out of scope:** GUI, TUI, daemon, web server, accounts, patient records, clinical
  decision support, dosing. See the README.
- **Few dependencies.** Explain why a new crate is needed and why the standard library
  isn't enough.

## Development

```sh
cargo build
cargo test
cargo test --no-default-features
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo fmt --check
```

All of these must pass. Tests must not use the network; use the fake `Fetch`
implementation in `tests/network_update.rs` for download paths.

To try changes against real data, build a database from an RxNorm release into a
scratch file rather than your installed one:

```sh
cargo run --release -- --db /tmp/gurd.db update --from RxNorm_full_prescribe_MMDDYYYY.zip
cargo run --release -- --db /tmp/gurd.db metformin
scripts/bench.sh /tmp/gurd.db
```

## Changing output

- **Text output** may change between releases, but keep it readable in monochrome; color
  must never be the only way something is shown.
- **JSON output** is a contract. See the stability rules in [docs/json.md](docs/json.md).
  `tests/json.rs` fails when keys change; update the docs, and bump `json_version` if the
  change can break existing consumers. Note the change in [CHANGELOG.md](CHANGELOG.md).
- **The schema** (`migrations/`) is versioned by `PRAGMA user_version`. A change that
  older binaries can't read needs a new schema version.

## Adding a data source

1. **Check the license first**, on the provider's own official pages. If the terms are
   unclear, the source can at most be user-downloaded, never redistributed. Add it to
   [DATA-LICENSES.md](DATA-LICENSES.md) with the provider's wording and links.
2. Add `src/sources/<source>.rs` implementing the `Source` trait (`info`, `release`,
   `import`, `validate`), and register it in `sources::builtin()`.
3. Write rows only through `ImportSink`. Use the source's own codes as concept keys, and
   record shared identifiers (UNII, NDC, ...) in `identifiers`; never link records from
   different sources by name.
4. Add a small fixture of real rows, copied verbatim, and only if the source's terms allow
   it, with a script to regenerate it.
5. Add tests for the import, the release version detection and validation failures.

## Commits and pull requests

- Keep changes focused; one topic per pull request.
- Add a CHANGELOG entry under **Unreleased** for user-visible changes.
- By contributing you agree that your code is released under the [MIT License](LICENSE).
