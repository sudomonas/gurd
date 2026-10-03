# Data licenses

The MIT license in [LICENSE](LICENSE) covers the `drug` software only. **It does not
apply to any dataset** that `drug` downloads, imports or stores. Each dataset remains
under its provider's own terms, summarized below. Where this summary and the provider's
official terms differ, the provider's terms apply.

A dataset being free to download does not make it "open source". The status of each
dataset below is taken from the provider's own documentation, not from third-party
summaries.

`drug sources` prints the same information for the sources compiled into your binary,
and `drug database --json` records, for each installed source, the exact release file,
its SHA-256, any checksum it was verified against, and when it was retrieved and
imported.

## RxNorm Current Prescribable Content

| | |
|---|---|
| Dataset | RxNorm Current Prescribable Content (a subset of RxNorm) |
| Provider | U.S. National Library of Medicine (NLM), National Institutes of Health |
| Source | <https://www.nlm.nih.gov/research/umls/rxnorm/docs/prescribe.html> |
| Version | Monthly releases, identified by date (e.g. `2026-09-08`). `drug` reads the version from the readme inside the release file |
| License/terms | Public domain. NLM: "The National Library of Medicine provides this subset without any licensing restrictions" and "No license required; public domain". No UMLS license is needed. Use is subject to the [RxNorm terms of service](https://www.nlm.nih.gov/research/umls/rxnorm/docs/termsofservice.html) below |
| Attribution | Required, verbatim (below) |
| Redistribution conditions | Allowed, provided the attribution is shown, NLM endorsement is not implied, and redistributed data is kept current or clearly marked as possibly not current |
| Download URL | Current release: <https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_current.zip>. Dated releases: `https://download.nlm.nih.gov/rxnorm/RxNorm_full_prescribe_MMDDYYYY.zip`, listed on the [RxNorm Files page](https://www.nlm.nih.gov/research/umls/rxnorm/docs/rxnormfiles.html) |
| Checksum | NLM publishes an MD5 for each dated release on the RxNorm Files page; none for the `current` file. Example: `RxNorm_full_prescribe_09082026.zip` has MD5 `88bbe4cefabd8e71f58651c1c3188646` |
| Content | SAB=RXNORM and SAB=MTHSPL only; no First DataBank, Micromedex or VA content; no obsolete or suppressed data. `drug` imports the RXNORM content and the UNII codes on MTHSPL substance atoms |

**Required attribution:**

> This product uses publicly available data courtesy of the U.S. National Library of
> Medicine (NLM), National Institutes of Health, Department of Health and Human Services;
> NLM is not responsible for the product and does not endorse or recommend this or any
> other product.

`drug` shows this statement in the detailed view's Source block, in `drug sources`, and
in JSON output.

**Other terms of service:**

- Users must "not indicate or imply that NLM has endorsed its products/services/applications."
- Anyone redistributing the data must "maintain the most current version of all distributed
  data, or make known in a clear and conspicuous manner that the products/services/applications
  do not reflect the most current/accurate data available from NLM."

`drug` meets the second condition by showing the release date on every result, and by
marking a release as possibly out of date once it is more than 45 days old (RxNorm is
released monthly). Anyone redistributing a database built by `drug` must meet these
conditions too.

### Full RxNorm (not supported)

The full RxNorm release requires a UMLS license and includes proprietary sources with
restrictions under section 12 of the UMLS license agreement. `drug` does not support it,
and a database built from it must not be redistributed.

## Datasets not supported yet

These are candidates for future adapters. None is downloaded, bundled or redistributed by
`drug` today. Terms were last checked on 2026-10-03 against the providers' own pages.

| Dataset | Provider | Status of terms | Plan |
|---|---|---|---|
| RxTerms | NLM Lister Hill Center | [Project page](https://lhncbc.nlm.nih.gov/MOR/RxTerms/) says it is free to use; no explicit redistribution terms found | User download only; no redistribution until terms are confirmed |
| ATC/DDD | WHO Collaborating Centre for Drug Statistics Methodology | [Copyright notice](https://atcddd.fhi.no/copyright_disclaimer/): "Copying and distribution for commercial purposes is not allowed. Changing or manipulating the material is not allowed." | Never bundled. Even a local import may count as manipulation; needs a separate legal review |
| openFDA (e.g. NDC directory) | U.S. Food and Drug Administration | Provider states CC0, with some third-party content excluded and marked | To be re-checked when work starts |
| DailyMed | NLM / FDA | Not verified | To be checked before any work starts |

If a dataset's terms are ambiguous, `drug` will not redistribute it; at most it will let
users download it from the official source themselves, and this file will state the
limitation.
