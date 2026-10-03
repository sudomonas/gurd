# gurd

A small, local-first command-line tool that works like a man page for drugs: look up a
drug name and read everything the installed datasets say about it, from several sources
at once, each clearly labelled. Everything is read from a local SQLite database.

```console
$ gurd metformin
Metformin
═════════

RxNorm Current Prescribable Content
  Name     metformin
  Kind     Ingredient
  ID       rxcui:6809
  Release  2026-09-08 (25 days old)

  Brands (14)
    Glucophage
    ...

RxTerms
  ...
  Clinical drugs (71)
    metFORMIN (Oral Pill) 500 mg
    ...

openFDA NDC Directory
  Products named Metformin (17)
    METFORMIN HYDROCHLORIDE 500 mg/1 · TABLET, EXTENDED RELEASE · Coupler LLC
    ...

1mg medicines (third-party Kaggle scrape)
  Note     Unofficial third-party scrape of 1mg.com. ...
  Products (5618)
    Glycomet 500 SR Tablet
    ...
```

- **Offline.** Lookups read a local database and never touch the network. Only
  `gurd update` downloads anything.
- **No telemetry, no account, no API key.**
- **Fast.** Lookups take 5–50 ms, including process startup, on a 1.25 GB database
  holding five sources.
- **Source-aware.** Every fact records which dataset and which release it came from, and
  the output says so. Unofficial datasets carry a visible note.
- **Many sources, one page.** RxNorm, RxTerms, openFDA and Indian datasets can be
  installed side by side. Records from different sources are linked only through
  identifiers the sources themselves publish (RxCUI, UNII, NDC), never by guessing from
  names.

## What it is not

`gurd` is a reference and lookup tool. It does **not**:

- diagnose, recommend treatment, or choose between drugs;
- calculate doses or check interactions or contraindications;
- generate or infer any medical information. It shows only what the installed datasets
  contain, and shows missing information as missing.

It does not replace professional prescribing references. Data can be out of date or
incomplete; check the release date that every result displays.

## Installation

### Prebuilt binary (Linux x86_64)

Each [GitHub release](https://github.com/sudomonas/gurd/releases) has a
`gurd-VERSION-x86_64-linux-gnu.tar.gz` archive and its SHA-256 checksum. The binary
needs glibc 2.39 or newer (e.g. Arch, Fedora 40+, Ubuntu 24.04+, Debian 13+).

```sh
curl -LO https://github.com/sudomonas/gurd/releases/download/v0.2.0/gurd-0.2.0-x86_64-linux-gnu.tar.gz
curl -LO https://github.com/sudomonas/gurd/releases/download/v0.2.0/gurd-0.2.0-x86_64-linux-gnu.tar.gz.sha256
sha256sum -c gurd-0.2.0-x86_64-linux-gnu.tar.gz.sha256
tar xzf gurd-0.2.0-x86_64-linux-gnu.tar.gz
install -Dm755 gurd-0.2.0-x86_64-linux-gnu/gurd ~/.local/bin/gurd
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
./target/release/gurd --version
```

The binary is a single file (about 5 MB) with no runtime dependencies.

**Build without network code.** `cargo install --path . --no-default-features` produces
a binary with no HTTP client compiled in at all. It can still build its database from a
file you downloaded yourself (`gurd update --from FILE`).

**Arch Linux.** A draft `PKGBUILD` is in [`packaging/arch/`](packaging/arch/PKGBUILD).
It is not on the AUR yet.

**crates.io.** Not published yet; `cargo install gurd` will work once it is.

## Getting a database

`gurd` ships without data. Install one or more sources with `gurd update`:

```sh
gurd update                                    # RxNorm, downloaded from NLM
gurd update --source openfda                   # openFDA NDC Directory, downloaded from FDA
gurd update --source rxterms --url https://data.lhncbc.nlm.nih.gov/public/rxterms/release/RxTerms202609.zip
gurd update --source 1mg --from ~/Downloads/gurd-data/1mg
gurd update --source az-india --from ~/Downloads/gurd-data/all-medicines-data
gurd remove az-india                           # drop a source again
```

`gurd sources` lists every supported source with its license and where to get it.
`--from` takes a zip file, a single file, or a directory of files.

Each update builds a new database containing **every installed source**, so their
versions are always recorded side by side. The release file of each installed source is
kept in `gurd.db.sources/` next to the database, so the others are rebuilt without
downloading again. With all five sources the database is about 1.25 GB, plus about
300 MB of stored releases and the previous database kept as `gurd.db.bak`.

The database lives at `$XDG_DATA_HOME/gurd/gurd.db` (normally
`~/.local/share/gurd/gurd.db`). Use `--db PATH` or `GURD_DB=PATH` to use another file.

### Verifying the download

`gurd` does not check checksums by itself, because it doesn't scrape providers' web
pages. Pass the checksum the provider publishes and `gurd` will refuse a file that
doesn't match:

```sh
gurd update \
  --url https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_09082026.zip \
  --md5 88bbe4cefabd8e71f58651c1c3188646
```

Dated release files and their MD5 checksums are listed on NLM's
[RxNorm Files](https://www.nlm.nih.gov/research/umls/rxnorm/docs/rxnormfiles.html) page.
NLM publishes no checksum for the `…_current.zip` file that plain `gurd update` fetches;
without `--md5`, `gurd` installs the file but prints its MD5 and SHA-256 so you can
compare them yourself, and records no verified checksum.

### Using a file you downloaded yourself

```sh
gurd update --from ~/Downloads/RxNorm_full_prescribe_09082026.zip --md5 88bbe4ce…
```

`--from` never uses the network.

### How updates are kept safe

1. Download to `~/.cache/gurd/` (or use the `--from` file).
2. Verify the MD5 if `--md5` was given.
3. Read the release version from the file. If that release is already installed, stop
   (`--force` reinstalls).
4. Import into a temporary database next to the real one.
5. Validate: SQLite integrity and foreign-key checks, required tables, and the source's
   own sanity checks.
6. Atomically rename the new database over the old one. The previous database is kept
   as `gurd.db.bak`, and the release file is stored in `gurd.db.sources/<source>/`.

If any step fails, the temporary file is deleted and the installed database is left
byte-for-byte unchanged.

## Usage

```sh
gurd metformin                    # an exact match prints a page with every source
gurd "dolo 650 tablet"            # a product page: composition, uses, side effects, ...
gurd aspirin
gurd augmentin                    # a brand shows its ingredients and products
gurd "amoxicillin clavulanate"    # quotes are optional: gurd amoxicillin clavulanate
gurd metfor                       # partial names give a ranked list
gurd metformin --details          # full view: every field, name, relationship, identifier
gurd show metformin               # same as --details
gurd show dolo 650                # full view of the best match, even if not exact
gurd rxcui 6809                   # look up by RxCUI
gurd id unii:9100L32L2N           # look up by any identifier: rxcui, unii, ndc
gurd class metformin hydrochloride   # drug classes, from sources that provide them
gurd metformin --json | jq        # machine-readable output
gurd database                     # where the database is and which releases it holds
gurd sources                      # supported sources, licenses, download locations
gurd update                       # download and install the current release
gurd remove 1mg                   # remove a source
```

**The page.** When a name matches exactly, `gurd` prints one page with a block per
source, official sources first. Each block shows the record's fields with readable
labels (composition, marketer, uses, side effects, strength, route, ...), its release, any
note about the source, and related records (ingredients, products, brands). When one
source has many records of the same name (say 17 openFDA products called "Metformin"),
they are listed one per line. Records of other sources that share an RxCUI or UNII are
added to the page. `gurd show` prints the same page with every field and relationship.
Long pages open in your pager.

Search tries, in order: exact name, prefix, whole words in any order, substring (skipped
when there is an exact match), and finally a fuzzy match (so `metfromin` finds metformin)
only if nothing else matched.
Results are ranked by how they matched, then by kind (ingredients and brands first).

Names are matched only against what the datasets contain. RxNorm, for example, has no
`paracetamol` synonym, so `gurd paracetamol` finds only the Indian and openFDA records that
use that name; use `acetaminophen` for RxNorm. `gurd` does not add its own aliases.

If a drug name collides with a subcommand, use `gurd search NAME`.

### Options

| Option | Meaning |
|---|---|
| `--json` | Print one JSON document on stdout; see [docs/json.md](docs/json.md) |
| `-d`, `--details` | Show the detailed view of the best match |
| `--limit N` | Maximum number of search results (default 20) |
| `--db PATH` | Database file (also `GURD_DB`) |
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

| Source (`--source`) | What it adds | How to get it | Terms |
|---|---|---|---|
| `rxnorm`: RxNorm Current Prescribable Content (NLM) | US ingredients, clinical and branded drugs, brands, relationships | `gurd update` downloads it | Public domain; attribution required |
| `rxterms`: RxTerms (NLM) | Prescriber display names, routes, dose forms, strengths | `--url` with a monthly file from NLM | "Free to use"; keep local |
| `openfda`: openFDA NDC Directory (FDA) | Every US-listed product: labeler, dosage form, route, active ingredients, packages, FDA pharmacologic classes | `gurd update --source openfda` downloads it | CC0 |
| `1mg`: 1mg medicines (third-party Kaggle scrape) | Indian products: composition, marketer, prescription status, price, uses, side effects, description | `--from` the JSON files you downloaded | Unofficial; personal use only |
| `az-india`: A-Z Medicines Dataset of India (third-party Kaggle scrape) | Indian products: substitutes, uses, side effects, therapeutic/action/chemical class, habit forming | `--from` the CSV you downloaded | Unofficial; personal use only |

Details, attribution and redistribution conditions for each are in
[DATA-LICENSES.md](DATA-LICENSES.md). The two Indian datasets were scraped from online
pharmacies by third parties: their accuracy and date are unknown, `gurd` shows a note on
every record from them, and a database containing them must not be shared (`gurd
database` says so).

`gurd database` and every result show the release date of the data. When a release is
older than the source's update cycle (45 days for RxNorm, which releases monthly), `gurd`
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

Only `gurd update` (without `--from`) uses the network. This is enforced in the code, not
just promised:

- The HTTP client is an optional dependency behind the `net` cargo feature, and only
  `src/net.rs` uses it. A `--no-default-features` build contains no HTTP code.
- The HTTP client is only created inside the `update` command.
- Lookups open the database read-only.
- `tests/architecture.rs` fails if any other module uses the HTTP crate or creates the
  client.

## Reproducing a database

A database is fully determined by the release file it was built from and the version of
`gurd` that built it, both of which it records (`gurd database --json` shows the release
version, file name, SHA-256, verified checksum, import time and `gurd` version).

To rebuild a specific release:

```sh
curl -O https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_09082026.zip
md5sum RxNorm_full_prescribe_09082026.zip   # compare with NLM's RxNorm Files page
gurd --db ./gurd.db update --from RxNorm_full_prescribe_09082026.zip \
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

`scripts/bench.sh path/to/gurd.db` times lookups against a real database.

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
  update.rs, net.rs   `gurd update` and `gurd remove`; net.rs is the only HTTP code
  sources/            one adapter per dataset; the only code that knows file formats
migrations/           SQLite schema
docs/json.md          JSON output format
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).
