# Data licenses

The MIT license in [LICENSE](LICENSE) covers the `gurd` software only. **It does not
apply to any dataset** that `gurd` downloads, imports or stores. Each dataset remains
under its provider's own terms, summarized below. Where this summary and the provider's
official terms differ, the provider's terms apply.

A dataset being free to download does not make it "open source". The status of each
dataset below is taken from the provider's own documentation, not from third-party
summaries.

`gurd sources` prints the same information for the sources compiled into your binary,
and `gurd database --json` records, for each installed source, the exact release file,
its SHA-256, any checksum it was verified against, and when it was retrieved and
imported.

## RxNorm Current Prescribable Content

| | |
|---|---|
| Dataset | RxNorm Current Prescribable Content (a subset of RxNorm) |
| Provider | U.S. National Library of Medicine (NLM), National Institutes of Health |
| Source | <https://www.nlm.nih.gov/research/umls/rxnorm/docs/prescribe.html> |
| Version | Monthly releases, identified by date (e.g. `2026-09-08`). `gurd` reads the version from the readme inside the release file |
| License/terms | Public domain. NLM: "The National Library of Medicine provides this subset without any licensing restrictions" and "No license required; public domain". No UMLS license is needed. Use is subject to the [RxNorm terms of service](https://www.nlm.nih.gov/research/umls/rxnorm/docs/termsofservice.html) below |
| Attribution | Required, verbatim (below) |
| Redistribution conditions | Allowed, provided the attribution is shown, NLM endorsement is not implied, and redistributed data is kept current or clearly marked as possibly not current |
| Download URL | Current release: <https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_current.zip>. Dated releases: `https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_MMDDYYYY.zip`, listed on the [RxNorm Files page](https://www.nlm.nih.gov/research/umls/rxnorm/docs/rxnormfiles.html) |
| Checksum | NLM publishes an MD5 for each dated release on the RxNorm Files page; none for the `current` file. Example: `RxNorm_full_prescribe_09082026.zip` has MD5 `88bbe4cefabd8e71f58651c1c3188646` |
| Content | SAB=RXNORM and SAB=MTHSPL only; no First DataBank, Micromedex or VA content; no obsolete or suppressed data. `gurd` imports the RXNORM content and the UNII codes on MTHSPL substance atoms |

**Required attribution:**

> This product uses publicly available data courtesy of the U.S. National Library of
> Medicine (NLM), National Institutes of Health, Department of Health and Human Services;
> NLM is not responsible for the product and does not endorse or recommend this or any
> other product.

`gurd` shows this statement in the detailed view's Source block, in `gurd sources`, and
in JSON output.

**Other terms of service:**

- Users must "not indicate or imply that NLM has endorsed its products/services/applications."
- Anyone redistributing the data must "maintain the most current version of all distributed
  data, or make known in a clear and conspicuous manner that the products/services/applications
  do not reflect the most current/accurate data available from NLM."

`gurd` meets the second condition by showing the release date on every result, and by
marking a release as possibly out of date once it is more than 45 days old (RxNorm is
released monthly). Anyone redistributing a database built by `gurd` must meet these
conditions too.

### Full RxNorm (not supported)

The full RxNorm release requires a UMLS license and includes proprietary sources with
restrictions under section 12 of the UMLS license agreement. `gurd` does not support it,
and a database built from it must not be redistributed.

## openFDA NDC Directory

| | |
|---|---|
| Dataset | NDC Directory, as exported by openFDA (`drug-ndc-0001-of-0001.json.zip`) |
| Provider | U.S. Food and Drug Administration |
| Source | <https://open.fda.gov/apis/drug/ndc/> |
| Version | The export's `meta.last_updated` date (e.g. `2026-10-02`); updated daily |
| License/terms | [CC0 1.0 Universal](https://open.fda.gov/license/). openFDA asks users not to imply endorsement. openFDA excludes GMDN medical-device terminology from CC0; that concerns device data, not the drug NDC export |
| Attribution | Not required; `gurd` credits openFDA and states that the FDA does not endorse it |
| Redistribution conditions | Allowed |
| Download URL | <https://download.open.fda.gov/drug/ndc/drug-ndc-0001-of-0001.json.zip> (listed in <https://api.fda.gov/download.json>) |
| Checksum | None published; `gurd` records the file's SHA-256 |

openFDA's own disclaimer, shown by `gurd` on every openFDA record: "Do not rely on openFDA
to make decisions regarding medical care. [...] you should assume all results are
unvalidated."

## RxTerms

| | |
|---|---|
| Dataset | RxTerms |
| Provider | U.S. National Library of Medicine, Lister Hill National Center for Biomedical Communications |
| Source | <https://lhncbc.nlm.nih.gov/MOR/RxTerms/> |
| Version | Monthly, from the file name (`RxTerms202609.txt` → `2026-09`); `gurd` dates it to the first of that month |
| License/terms | NLM states RxTerms is "free to use". No explicit redistribution terms are published, so `gurd` treats it as **local-only** |
| Attribution | `gurd` credits NLM |
| Redistribution conditions | Unclear; do not redistribute databases containing it |
| Download URL | `https://data.lhncbc.nlm.nih.gov/public/rxterms/release/RxTerms<YYYYMM>.zip` |
| Checksum | None published |

## 1mg medicines (third-party Kaggle scrape)

| | |
|---|---|
| Dataset | JSON-lines files `kaggle_medicines.json`, `kaggle_capsules.json`, `kaggle_injections.json` (about 117,000 products) |
| Provider | **Unofficial.** Scraped from 1mg.com (Tata 1mg) by a third party and published on Kaggle |
| Version | None in the data; `gurd` uses the files' modification date |
| License/terms | **None.** Whether 1mg permits the scraping is unknown and its terms may forbid it. Use for personal reference only |
| Attribution | `gurd` names 1mg.com as the original publisher |
| Redistribution conditions | **Do not redistribute** the files or any database built from them. `gurd` never downloads them; you supply your own copy with `--from` |
| Download URL | None (Kaggle, with your own account) |
| Checksum | None; `gurd` records the SHA-256 of the directory's files |

Prices, uses, side effects and descriptions are shown exactly as they appear in the
files. They were written by an online pharmacy, are not a prescribing reference, and may
be out of date.

## A-Z Medicines Dataset of India (third-party Kaggle scrape)

| | |
|---|---|
| Dataset | One CSV file, a row per product (about 248,000), with substitutes, side effects, uses, and chemical, therapeutic and action classes |
| Provider | **Unofficial.** Compiled by a third party from Indian online pharmacy listings and published on Kaggle |
| Version | None in the data; `gurd` uses the file's modification date |
| License/terms | **None stated by the original publishers.** Use for personal reference only |
| Attribution | `gurd` names the dataset |
| Redistribution conditions | **Do not redistribute.** You supply your own copy with `--from` |
| Download URL | None (Kaggle, with your own account) |
| Checksum | None |

## Test fixtures

The repository contains small test fixtures. RxNorm and openFDA fixtures are real rows
copied unedited (public domain and CC0). Fixtures for RxTerms, 1mg and the A-Z India
dataset are **made up** in those datasets' formats, because their terms do not allow
redistribution.

## Datasets not supported yet

None of these is downloaded, bundled or redistributed by `gurd` today. Terms were last
checked on 2026-10-03 against the providers' own pages, where those could be reached.

| Dataset | Provider | Status of terms | Plan |
|---|---|---|---|
| India Drug Registry | ABDM / CDSCO / NRCeS | Not stated on the portal. Bulk export is behind a CAPTCHA, meaning it is intended for people using the portal | Import a file you export from the portal; or an API connector using your own ABDM integrator credentials |
| NLEM 2022 (National List of Essential Medicines) | Ministry of Health, India | Government publication; terms not checked (the CDSCO site refuses automated access) | Import the PDF you download |
| CDSCO approved drug lists | CDSCO | Government publication; terms not checked | Import the PDFs you download |
| Jan Aushadhi product list | PMBI | "All Rights Reserved" | Personal use only, from the PDF you export |
| ICMR Standard Treatment Workflows, MoHFW Standard Treatment Guidelines | ICMR, MoHFW | Government publications; terms not checked | A document store with `gurd guide`, quoting pages verbatim |
| WHO Model List of Essential Medicines (eEML) | WHO | CC BY 3.0 IGO, attribution required | Download format still to be confirmed |
| DailyMed (label text) | NLM / FDA | Not verified; labels are written by manufacturers | Opt-in, from a file you download (several GB) |
| ChEMBL | EMBL-EBI | CC BY-SA 3.0 (share-alike) | Opt-in, from the SQLite dump (5.4 GB) |
| Wikidata | Wikimedia | CC0 | International generic names; to be added |
| ATC/DDD | WHO Collaborating Centre for Drug Statistics Methodology | "Copying and distribution for commercial purposes is not allowed. Changing or manipulating the material is not allowed." | Never bundled; needs legal review even for local import |
| National Formulary of India, MIMS/CIMS | IPC; commercial publishers | Sold or by subscription; no free copy exists | Only through a licensed export you own, imported locally |

If a dataset's terms are ambiguous, `gurd` will not redistribute it; at most it will let
users import their own copy from the official source, and this file will state the
limitation.
