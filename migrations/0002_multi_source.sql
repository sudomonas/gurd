-- gurd database schema, version 2: several sources per database.

-- A caveat shown wherever a source's data is displayed, e.g. that a dataset is an
-- unofficial third-party scrape. Application metadata written by the adapter.
ALTER TABLE sources ADD COLUMN notice TEXT;

-- How to display a source's attributes: a readable label, the display order, and whether
-- the summary page shows it (otherwise only the detailed view does). Written by each
-- adapter; these rows are application metadata, not medical facts.
CREATE TABLE attribute_keys (
  source_id INTEGER NOT NULL REFERENCES sources(id),
  key       TEXT NOT NULL,                    -- as in attributes.key
  label     TEXT NOT NULL,
  rank      INTEGER NOT NULL,
  summary   INTEGER NOT NULL CHECK (summary IN (0, 1)),
  PRIMARY KEY (source_id, key)
) STRICT;

CREATE INDEX concept_classifications_class ON concept_classifications(classification_id);
