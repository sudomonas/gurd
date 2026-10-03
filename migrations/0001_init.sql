-- drug database schema, version 1.
--
-- Design rules:
--   * Every source-derived row carries a source_id. Nothing is stored without provenance.
--   * No identifier system is privileged. A concept is keyed by its own source's code;
--     RxCUI, NDC, UNII, ... are rows in `identifiers`.
--   * Records from different sources are related only through identifiers that a source
--     itself asserted, never by name similarity.
--   * Source-specific vocabulary (e.g. RxNorm TTY/RELA/ATN) is kept verbatim in
--     source_* columns next to the application's own normalized vocabulary.

CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

CREATE TABLE sources (
  id                INTEGER PRIMARY KEY,
  slug              TEXT NOT NULL UNIQUE,     -- 'rxnorm'
  title             TEXT NOT NULL,            -- 'RxNorm Current Prescribable Content'
  code_system       TEXT NOT NULL,            -- identifier system of concepts.source_code, 'rxcui'
  provider          TEXT NOT NULL,            -- 'U.S. National Library of Medicine'
  version           TEXT NOT NULL,            -- as named by the provider
  release_date      TEXT,                     -- ISO 8601 date, if known
  stale_after_days  INTEGER,                  -- age at which a release counts as outdated
  license           TEXT NOT NULL,
  attribution       TEXT NOT NULL,            -- required notice, verbatim
  url               TEXT NOT NULL,
  file_name         TEXT NOT NULL,
  upstream_checksum TEXT,                     -- as published by provider, e.g. 'md5:...'
  sha256            TEXT NOT NULL,            -- computed locally from the input file
  retrieved_at      TEXT NOT NULL,
  imported_at       TEXT NOT NULL,
  importer_version  TEXT NOT NULL,
  origin            TEXT NOT NULL CHECK (origin IN ('builtin', 'bundle')),
  redistributable   INTEGER NOT NULL CHECK (redistributable IN (0, 1))
) STRICT;

-- Application vocabulary for what a concept is. Adapters map source term types onto it.
-- rank orders kinds in search results (lower first).
CREATE TABLE concept_kinds (
  kind  TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  rank  INTEGER NOT NULL
) STRICT;

INSERT INTO concept_kinds (kind, label, rank) VALUES
  ('ingredient',               'Ingredient',                 10),
  ('brand_name',               'Brand name',                 15),
  ('precise_ingredient',       'Precise ingredient',         20),
  ('multiple_ingredients',     'Multiple ingredients',       30),
  ('clinical_drug',            'Clinical drug',              40),
  ('product',                  'Product',                    42),
  ('branded_drug',             'Branded drug',               45),
  ('generic_pack',             'Generic pack',               50),
  ('branded_pack',             'Branded pack',               55),
  ('clinical_component',       'Clinical drug component',    60),
  ('branded_component',        'Branded drug component',     65),
  ('clinical_dose_form',       'Clinical dose form',         70),
  ('clinical_dose_form_precise', 'Clinical dose form (precise)', 72),
  ('branded_dose_form',        'Branded dose form',          75),
  ('branded_dose_form_precise', 'Branded dose form (precise)', 77),
  ('clinical_dose_form_group', 'Clinical dose form group',   80),
  ('clinical_dose_form_group_precise', 'Clinical dose form group (precise)', 82),
  ('branded_dose_form_group',  'Branded dose form group',    85),
  ('dose_form',                'Dose form',                  90),
  ('dose_form_group',          'Dose form group',            95);

CREATE TABLE concepts (
  id          INTEGER PRIMARY KEY,
  source_id   INTEGER NOT NULL REFERENCES sources(id),
  source_code TEXT NOT NULL,                  -- the source's own key (RxCUI for RxNorm)
  kind        TEXT NOT NULL REFERENCES concept_kinds(kind),
  source_type TEXT NOT NULL,                  -- verbatim, e.g. RxNorm TTY 'IN', 'SCD'
  name        TEXT NOT NULL,                  -- the source's preferred name
  UNIQUE (source_id, source_code)
) STRICT;
CREATE INDEX concepts_kind ON concepts(kind);

CREATE TABLE names (
  id          INTEGER PRIMARY KEY,
  concept_id  INTEGER NOT NULL REFERENCES concepts(id),
  source_id   INTEGER NOT NULL REFERENCES sources(id),
  name        TEXT NOT NULL,
  norm        TEXT NOT NULL,                  -- see search normalization
  name_type   TEXT NOT NULL CHECK (name_type IN ('preferred', 'synonym', 'prescribable', 'tall_man')),
  source_type TEXT NOT NULL,                  -- verbatim, e.g. RxNorm TTY 'SY'
  source_ref  TEXT                            -- e.g. RXAUI
) STRICT;
CREATE INDEX names_norm    ON names(norm);
CREATE INDEX names_concept ON names(concept_id);

-- External-content FTS indexes over names.norm. Imports are bulk-only into a fresh
-- database, so they are populated with the FTS 'rebuild' command after loading
-- instead of with triggers.
CREATE VIRTUAL TABLE names_tok USING fts5(
  norm, content='names', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
);
CREATE VIRTUAL TABLE names_tri USING fts5(
  norm, content='names', content_rowid='id', tokenize='trigram'
);

CREATE TABLE identifiers (
  concept_id INTEGER NOT NULL REFERENCES concepts(id),
  source_id  INTEGER NOT NULL REFERENCES sources(id),   -- who asserted this identifier
  system     TEXT NOT NULL,                             -- 'rxcui', 'ndc', 'unii', ...
  value      TEXT NOT NULL,
  PRIMARY KEY (concept_id, system, value)
) STRICT, WITHOUT ROWID;
CREATE INDEX identifiers_lookup ON identifiers(system, value);

CREATE TABLE relationships (
  id               INTEGER PRIMARY KEY,
  source_id        INTEGER NOT NULL REFERENCES sources(id),
  subject_id       INTEGER NOT NULL REFERENCES concepts(id),
  predicate        TEXT NOT NULL,             -- normalized, e.g. 'has_ingredient'
  object_id        INTEGER NOT NULL REFERENCES concepts(id),
  source_predicate TEXT NOT NULL              -- verbatim, e.g. RxNorm RELA
) STRICT;
CREATE INDEX relationships_subject ON relationships(subject_id, predicate);
CREATE INDEX relationships_object  ON relationships(object_id, predicate);

CREATE TABLE attributes (
  id         INTEGER PRIMARY KEY,
  concept_id INTEGER NOT NULL REFERENCES concepts(id),
  source_id  INTEGER NOT NULL REFERENCES sources(id),
  key        TEXT NOT NULL,                   -- verbatim, e.g. RxNorm ATN 'RXN_STRENGTH'
  value      TEXT NOT NULL
) STRICT;
CREATE INDEX attributes_concept ON attributes(concept_id, key);

-- Classification systems (e.g. pharmacologic classes). Empty until a source provides them.
CREATE TABLE classifications (
  id        INTEGER PRIMARY KEY,
  source_id INTEGER NOT NULL REFERENCES sources(id),
  system    TEXT NOT NULL,
  code      TEXT NOT NULL,
  name      TEXT NOT NULL,
  parent_id INTEGER REFERENCES classifications(id),
  UNIQUE (source_id, system, code)
) STRICT;

CREATE TABLE concept_classifications (
  concept_id        INTEGER NOT NULL REFERENCES concepts(id),
  classification_id INTEGER NOT NULL REFERENCES classifications(id),
  source_id         INTEGER NOT NULL REFERENCES sources(id),
  PRIMARY KEY (concept_id, classification_id, source_id)
) STRICT;

-- How to gather a concept's summary sections ("Clinical drugs", "Brands", ...) from the
-- relationship graph. Written by each source's adapter, because only the source knows how
-- its graph is shaped; read by the application, which knows nothing else about the source.
-- These rows are application metadata, not medical facts.
CREATE TABLE navigation (
  source_id INTEGER NOT NULL REFERENCES sources(id),
  from_kind TEXT NOT NULL REFERENCES concept_kinds(kind),
  section   TEXT NOT NULL,                    -- 'ingredients', 'brands', 'clinical_drugs', ...
  path      TEXT NOT NULL,                    -- predicates to follow, space-separated
  to_kind   TEXT NOT NULL REFERENCES concept_kinds(kind),
  PRIMARY KEY (source_id, from_kind, section, path, to_kind)
) STRICT;
