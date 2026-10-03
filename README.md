# drug

A small, local-first command-line tool for looking up drug names, ingredients, clinical
drugs and brands in a local SQLite database built from openly published datasets.

```console
$ drug metformin
metformin
─────────
  Kind      Ingredient
  ID        rxcui:6809
  Source    RxNorm Current Prescribable Content 2026-09-08 (25 days old)

Precise ingredients (1)
  metformin hydrochloride

Brands (14)
  Glucophage
  Glumetza
  ...
```

- **Offline.** Lookups read a local database and never touch the network. Only
  `drug update` downloads anything.
- **No telemetry, no account, no API key.**
- **Fast.** A lookup against the full RxNorm prescribable release takes 2–20 ms,
  including process startup.
- **Source-aware.** Every fact in the database records which dataset and which release
  it came from, and the output says so.
- **Not tied to one dataset.** RxNorm is the first source; the schema, search and CLI are
  source-neutral, so other datasets can be added as adapters.

## What it is not

`drug` is a reference and lookup tool. It does **not**:

- diagnose, recommend treatment, or choose between drugs;
- calculate doses or check interactions or contraindications;
- generate or infer any medical information. It shows only what the installed datasets
  contain, and shows missing information as missing.

It does not replace professional prescribing references. Data can be out of date or
incomplete; check the release date that every result displays.

## Installation

### Prebuilt binary (Linux x86_64)

Each [GitHub release](https://github.com/sudomonas/gurd/releases) has a
`drug-VERSION-x86_64-linux-gnu.tar.gz` archive and its SHA-256 checksum. The binary
needs glibc 2.39 or newer (e.g. Arch, Fedora 40+, Ubuntu 24.04+, Debian 13+).

```sh
curl -LO https://github.com/sudomonas/gurd/releases/download/v0.1.0/drug-0.1.0-x86_64-linux-gnu.tar.gz
curl -LO https://github.com/sudomonas/gurd/releases/download/v0.1.0/drug-0.1.0-x86_64-linux-gnu.tar.gz.sha256
sha256sum -c drug-0.1.0-x86_64-linux-gnu.tar.gz.sha256
tar xzf drug-0.1.0-x86_64-linux-gnu.tar.gz
install -Dm755 drug-0.1.0-x86_64-linux-gnu/drug ~/.local/bin/drug
```

### From source

Building needs Rust 1.85 or newer. SQLite is compiled in, so no system library is needed.

```sh
git clone https://github.com/sudomonas/gurd.git
cd gurd
cargo install --path .
```

or build without installing:

```sh
cargo build --release
./target/release/drug --version
```

The binary is a single file (about 5 MB) with no runtime dependencies.

**Build without network code.** `cargo install --path . --no-default-features` produces
a binary with no HTTP client compiled in at all. It can still build its database from a
file you downloaded yourself (`drug update --from FILE`).

**Arch Linux.** A draft `PKGBUILD` is in [`packaging/arch/`](packaging/arch/PKGBUILD).
It is not on the AUR yet.

**crates.io.** Not published yet. The crate name `drug` is already taken there by an
unrelated project, so a published package will need a different crate name; the binary
will still be called `drug`.

## Getting a database

`drug` ships without data. Build the database once:

```sh
drug update
```

This downloads NLM's RxNorm Current Prescribable Content (about 75 MB), imports it into a
new database, validates it, and only then moves it into place. It takes a few minutes
to download and about 10 seconds to import. The 2026-09-08 release gives a 143 MB database.

The database lives at `$XDG_DATA_HOME/drug/drug.db` (normally
`~/.local/share/drug/drug.db`). Use `--db PATH` or `DRUG_DB=PATH` to use another file.

### Verifying the download

`drug` does not check checksums by itself, because it doesn't scrape providers' web
pages. Pass the checksum the provider publishes and `drug` will refuse a file that
doesn't match:

```sh
drug update \
  --url https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_09082026.zip \
  --md5 88bbe4cefabd8e71f58651c1c3188646
```

Dated release files and their MD5 checksums are listed on NLM's
[RxNorm Files](https://www.nlm.nih.gov/research/umls/rxnorm/docs/rxnormfiles.html) page.
NLM publishes no checksum for the `…_current.zip` file that plain `drug update` fetches;
without `--md5`, `drug` installs the file but prints its MD5 and SHA-256 so you can
compare them yourself, and records no verified checksum.

### Using a file you downloaded yourself

```sh
drug update --from ~/Downloads/RxNorm_full_prescribe_09082026.zip --md5 88bbe4ce…
```

`--from` never uses the network.

### How updates are kept safe

1. Download to `~/.cache/drug/` (or use the `--from` file).
2. Verify the MD5 if `--md5` was given.
3. Read the release version from the file. If that release is already installed, stop
   (`--force` reinstalls).
4. Import into a temporary database next to the real one.
5. Validate: SQLite integrity and foreign-key checks, required tables, and the source's
   own sanity checks.
6. Atomically rename the new database over the old one. The previous database is kept
   as `drug.db.bak`.

If any step fails, the temporary file is deleted and the installed database is left
byte-for-byte unchanged.

## Usage

```sh
drug metformin                    # search; an exact match prints a summary card
drug aspirin
drug augmentin                    # a brand shows its ingredients and products
drug "amoxicillin clavulanate"    # quotes are optional: drug amoxicillin clavulanate
drug metfor                       # partial names give a ranked list
drug metformin --details          # full view: all names, relationships, identifiers
drug show metformin               # same as --details
drug rxcui 6809                   # look up by RxCUI
drug id unii:9100L32L2N           # look up by any identifier: rxcui, unii, ndc
drug metformin --json | jq        # machine-readable output
drug database                     # where the database is and which releases it holds
drug sources                      # supported sources, licenses, download locations
drug update                       # download and install the current release
```

Search tries, in order: exact name, prefix, whole words in any order, substring, and
finally a fuzzy match (so `metfromin` finds metformin) only if nothing else matched.
Results are ranked by how they matched, then by kind (ingredients and brands first).

Names are matched only against what the dataset contains. RxNorm, for example, has no
`paracetamol` synonym, so `drug paracetamol` finds nothing; use `acetaminophen`. `drug`
does not add its own aliases.

If a drug name collides with a subcommand, use `drug search NAME`.

### Options

| Option | Meaning |
|---|---|
| `--json` | Print one JSON document on stdout; see [docs/json.md](docs/json.md) |
| `-d`, `--details` | Show the detailed view of the best match |
| `--limit N` | Maximum number of search results (default 20) |
| `--db PATH` | Database file (also `DRUG_DB`) |
| `--color auto\|always\|never`, `--no-color` | Color is used only on a terminal and respects `NO_COLOR` |
| `--no-pager` | Long detailed views use `$PAGER` (default `less -FRX`) only when stdout is a terminal |

There is no configuration file; the defaults work without one.

### Exit status

| Status | Meaning |
|---|---|
| 0 | Something was found, or the command succeeded |
| 1 | Nothing was found, or an error occurred |
| 2 | Invalid command-line usage |

Diagnostics go to stderr. With `--json`, lookups write nothing to stderr.

## Data sources

| Source | Status | Terms |
|---|---|---|
| RxNorm Current Prescribable Content (U.S. National Library of Medicine) | Supported | Public domain; attribution required; see [DATA-LICENSES.md](DATA-LICENSES.md) |
| RxTerms, ATC, DailyMed, openFDA | Not supported yet | Terms are being checked; see [DATA-LICENSES.md](DATA-LICENSES.md) |

The full RxNorm release (which needs a UMLS license and contains proprietary sources) is
not supported.

`drug database` and every result show the release date of the data. When a release is
older than the source's update cycle (45 days for RxNorm, which releases monthly), `drug`
says it may not reflect the provider's latest data.

This product uses publicly available data courtesy of the U.S. National Library of
Medicine (NLM), National Institutes of Health, Department of Health and Human Services;
NLM is not responsible for the product and does not endorse or recommend this or any
other product.

## Licensing

The software is released under the [MIT License](LICENSE).

**The software license does not apply to third-party datasets downloaded or imported by
this project.** Each dataset is governed by its provider's own terms, listed in
[DATA-LICENSES.md](DATA-LICENSES.md).

## Offline behavior

Only `drug update` (without `--from`) uses the network. This is enforced in the code, not
just promised:

- The HTTP client is an optional dependency behind the `net` cargo feature, and only
  `src/net.rs` uses it. A `--no-default-features` build contains no HTTP code.
- The HTTP client is only created inside the `update` command.
- Lookups open the database read-only.
- `tests/architecture.rs` fails if any other module uses the HTTP crate or creates the
  client.

## Reproducing a database

A database is fully determined by the release file it was built from and the version of
`drug` that built it, both of which it records (`drug database --json` shows the release
version, file name, SHA-256, verified checksum, import time and `drug` version).

To rebuild a specific release:

```sh
curl -O https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_09082026.zip
md5sum RxNorm_full_prescribe_09082026.zip   # compare with NLM's RxNorm Files page
drug --db ./drug.db update --from RxNorm_full_prescribe_09082026.zip \
     --md5 88bbe4cefabd8e71f58651c1c3188646
```

Generated databases are never committed to the repository. If prebuilt databases are
offered later, they will be attached to GitHub Releases along with the release file's
checksum and the attribution and currency notice the RxNorm terms require.

## Development

```sh
cargo build
cargo test                          # default build (with network code)
cargo test --no-default-features    # build without network code
cargo clippy --all-targets
cargo clippy --all-targets --no-default-features
```

Tests use a small fixture of real RxNorm rows copied verbatim (`tests/fixtures/rxnorm-mini`,
regenerated with `scripts/make-fixture.sh`) and never use the network; downloads are
tested through a fake.

`scripts/bench.sh path/to/drug.db` times lookups against a real database.

Layout:

```text
src/
  main.rs, cli.rs     argument parsing (clap)
  app.rs              commands
  search.rs           tiered search
  details.rs          summary card and detailed view queries
  render.rs, output.rs  text output, color and pager
  database.rs         opening and schema checks
  import.rs           building, validating and swapping in a new database
  update.rs, net.rs   `drug update`; net.rs is the only HTTP code
  sources/            one adapter per dataset; the only code that knows file formats
migrations/           SQLite schema
docs/json.md          JSON output format
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
