# JSON output

Every command accepts `--json`. The output is a single JSON document on stdout. Nothing
else is written to stdout. In JSON mode, lookups print nothing to stderr either, so a
script only needs to check the exit status and parse stdout.

```sh
gurd metformin --json | jq -r '.results[0].code'
```

## Stability

Every document has a top-level `json_version` (currently **1**). Within one version:

- keys are not renamed, removed or retyped;
- new keys may be added, so consumers should ignore keys they do not know;
- new values may appear in open-ended fields such as `kind`, `match`, `predicate`,
  `code_system` and section names.

Anything else is a breaking change: it increments `json_version` and is listed in
`CHANGELOG.md`. `tests/json.rs` checks the key set of every document type.

## Exit status

| Status | Meaning |
|---|---|
| 0 | Something was found, or the command succeeded |
| 1 | Nothing was found, or an error occurred. A lookup that finds nothing still prints a complete document with empty `results`, `concepts` or `classes`; an error prints nothing on stdout and a message on stderr |
| 2 | Invalid command-line usage |

## Common objects

### Concept

A concept is one entry of one source: an ingredient, a brand name, a clinical drug, and
so on.

| Key | Type | Description |
|---|---|---|
| `source` | string | Source that defines the concept, e.g. `rxnorm` |
| `source_version` | string | Release of that source in this database, e.g. `2026-09-08` |
| `code_system` | string | Identifier system of `code`, e.g. `rxcui` |
| `code` | string | The source's identifier for the concept, e.g. `6809` |
| `name` | string | The source's preferred name |
| `kind` | string | What the concept is, in source-neutral terms (see below) |
| `source_type` | string | The source's own term type, verbatim (RxNorm TTY, e.g. `IN`) |

`kind` is one of `ingredient`, `precise_ingredient`, `multiple_ingredients`,
`brand_name`, `clinical_drug`, `branded_drug`, `product`, `generic_pack`,
`branded_pack`, `clinical_component`, `branded_component`, `clinical_dose_form`,
`clinical_dose_form_precise`, `branded_dose_form`, `branded_dose_form_precise`,
`clinical_dose_form_group`, `clinical_dose_form_group_precise`,
`branded_dose_form_group`, `dose_form` or `dose_form_group`.

`code_system:code` (e.g. `rxcui:6809`) is accepted by `gurd show` and `gurd id`.

### Source versions

Lookup documents list the installed sources so the reader knows what was queried:

```json
"sources": [{ "source": "rxnorm", "version": "2026-09-08" }]
```

## `gurd <query>` / `gurd search <query>`

```json
{
  "json_version": 1,
  "query": "metformin",
  "sources": [{ "source": "rxnorm", "version": "2026-09-08" }],
  "results": [
    {
      "source": "rxnorm",
      "source_version": "2026-09-08",
      "code_system": "rxcui",
      "code": "6809",
      "name": "metformin",
      "kind": "ingredient",
      "source_type": "IN",
      "match": "exact",
      "matched_name": "metformin"
    }
  ]
}
```

Each result is a concept plus:

| Key | Description |
|---|---|
| `match` | How it matched: `exact`, `prefix`, `token`, `substring` or `fuzzy`, strongest first. Substring matching is skipped when there is an exact match |
| `matched_name` | The name that matched. This may be a synonym, e.g. `metFORMIN HCl 500 MG Oral Tablet` |

Results are ordered by match strength, then kind, then name length. `--limit N` (default
20; 0 means no limit) caps the number of results.

## `gurd show <query>`, `gurd <query> --details`, `gurd rxcui <RXCUI>`, `gurd id <SYSTEM:VALUE>`

```json
{
  "json_version": 1,
  "query": "rxcui:861007",
  "sources": [{ "source": "rxnorm", "version": "2026-09-08" }],
  "concepts": [
    {
      "source": "rxnorm", "source_version": "2026-09-08", "code_system": "rxcui",
      "code": "861007", "name": "metformin hydrochloride 500 MG Oral Tablet",
      "kind": "clinical_drug", "source_type": "SCD",
      "sections": {
        "ingredients": [ { "...concept...": "" } ],
        "dose_forms": [ { "...concept...": "" } ]
      },
      "names": [
        { "name": "metFORMIN HCl 500 MG Oral Tablet", "name_type": "prescribable", "source_type": "PSN" }
      ],
      "identifiers": [ { "system": "ndc", "value": "00093104801" } ],
      "attributes": [ { "key": "RXN_HUMAN_DRUG", "label": "Human drug", "value": "US" } ],
      "relationships": [
        { "predicate": "has_dose_form", "concept": { "...concept...": "" } }
      ]
    }
  ]
}
```

`show` returns the best match for the query and every other concept, from any source,
with exactly the same (normalized) name; or every concept with the identifier if the
query looks like `system:value`. `rxcui` and `id` return every concept carrying the
identifier, which can come from more than one source. In each case the concepts of other
sources that share an `rxcui` or `unii` identifier with them follow, at most 20 per source
and identifier. Concepts are never linked by name.

| Key | Description |
|---|---|
| `sections` | Related concepts grouped for reading: `ingredients`, `precise_ingredients`, `brands`, `products`, `clinical_drugs`, `branded_drugs`, `combinations`, `dose_forms`, `packs`, `contents`. Empty sections are omitted. Sections are gathered from the source's own relationships; nothing is inferred |
| `names` | Every name of the concept. `name_type` is `preferred`, `synonym`, `prescribable` or `tall_man`; `source_type` is the source's own term type |
| `identifiers` | Identifiers the source assigns: `rxcui`, `ndc`, `unii`, ... |
| `attributes` | Source attributes in display order. `key` is the source's own name (RxNorm `ATN`, e.g. `RXN_STRENGTH`; for other sources the adapter's name for a column, e.g. `composition`); `label` is a readable name declared by the source's adapter, or the key itself. A key with several values (e.g. `side_effect`) appears once per value, in the source's order |
| `relationships` | Direct relationships as the source records them: "*this concept* `predicate` *concept*". Predicates are verbatim (RxNorm `RELA`) |

## `gurd class <query>`

```json
{
  "json_version": 1,
  "query": "metformin",
  "sources": [{ "source": "rxnorm", "version": "2026-09-08" }],
  "concept": { "...concept...": "" },
  "classes": [
    { "system": "...", "code": "...", "name": "...", "source": "...", "source_version": "..." }
  ]
}
```

The classes come from every concept that matches the query exactly (or the best match if
none does), across sources. For each concept they are its own classes plus those of the
concepts its source relates it to directly that relate to no other concept of its kind,
such as an ingredient's single-ingredient products; a combination product's classes are
never attributed to one of its ingredients. Classes are never carried across sources;
`source` says which one assigned each. `concept` is the first concept with classes (or the
best match), or `null` if nothing matched. `classes` is empty unless an installed source
provides classes: openFDA (`fda_epc`, `fda_moa`, `fda_pe`, `fda_cs`) and A-Z India
(`therapeutic_class`, `action_class`, `chemical_class`) do; RxNorm does not.

## `gurd database`

```json
{
  "json_version": 1,
  "database": {
    "path": "/home/user/.local/share/gurd/gurd.db",
    "installed": true,
    "schema_version": 2,
    "built_at": "2026-10-03T12:44:02Z",
    "built_by": "gurd 0.1.0",
    "sources": [ { "...source record...": "" } ]
  }
}
```

If no database is installed, `installed` is `false`, the other fields are `null` or empty,
and the exit status is 1.

### Source record

| Key | Description |
|---|---|
| `source` | Short name, e.g. `rxnorm` |
| `title`, `provider` | e.g. `RxNorm Current Prescribable Content`, `U.S. National Library of Medicine` |
| `version`, `release_date` | Release as named by the provider, and its date if known |
| `code_system` | Identifier system of the source's concept codes |
| `license`, `attribution`, `url` | Terms, required attribution text, official page |
| `file_name`, `sha256`, `upstream_checksum` | Input file, its SHA-256, and the provider's published checksum if one was checked |
| `retrieved_at`, `imported_at`, `importer_version` | When the file was obtained and imported, and by which version of gurd |
| `origin` | `builtin` (an adapter in gurd) or `bundle` (a user-supplied dataset) |
| `redistributable` | Whether the dataset's terms allow sharing a database built from it |
| `age_days`, `stale_after_days`, `stale` | Days since the release, the age at which it counts as outdated, and whether it is |
| `notice` | A caveat to show with the source's data (e.g. that it is an unofficial scrape), or `null` |

## `gurd sources`

```json
{
  "json_version": 1,
  "sources": [
    {
      "source": "rxnorm", "title": "...", "provider": "...", "license": "...",
      "attribution": "...", "url": "...",
      "download_url": "...", "checksums_url": "...", "redistributable": true,
      "builtin": true,
      "installed": { "...source record...": "" }
    }
  ]
}
```

`download_url` is where `gurd update` downloads from by default, and `checksums_url` is
where the provider publishes checksums for you to check against. Either may be `null`.
`installed` is `null` for sources that are not in the database.

## `gurd update`

```json
{
  "json_version": 1,
  "database": "/home/user/.local/share/gurd/gurd.db",
  "imported": [
    { "source": "rxnorm", "version": "2026-09-08", "concepts": 81468, "names": 140841,
      "identifiers": 327760, "relationships": 552956, "attributes": 230799 }
  ],
  "up_to_date": []
}
```

`imported` lists every source in the new database: the one being updated and the
installed sources rebuilt with it from their stored releases. If the release in the file
is already installed, the database is left as it is: `imported` is empty and
`up_to_date` lists `{ "source", "version" }` for that release.
Progress messages go to stderr. Without `--md5`, stderr also shows the file's MD5 and
SHA-256 so you can compare them with the provider's published checksum.

## `gurd remove <source>`

```json
{
  "json_version": 1,
  "database": "/home/user/.local/share/gurd/gurd.db",
  "removed": "1mg",
  "sources": [{ "source": "rxnorm", "version": "2026-09-08" }]
}
```

`sources` lists the sources the rebuilt database holds; it is empty when the removed
source was the last one (the old database is then kept as `<db>.bak`).
