use serde::Serialize;

/// Provenance of one installed dataset.
#[derive(Debug, Clone, Serialize)]
pub struct SourceRecord {
    #[serde(rename = "source")]
    pub slug: String,
    pub title: String,
    pub code_system: String,
    pub provider: String,
    pub version: String,
    pub release_date: Option<String>,
    pub license: String,
    pub attribution: String,
    pub url: String,
    pub file_name: String,
    pub upstream_checksum: Option<String>,
    pub sha256: String,
    pub retrieved_at: String,
    pub imported_at: String,
    pub importer_version: String,
    pub origin: String,
    pub redistributable: bool,
    pub stale_after_days: Option<i64>,
    /// Days since the release date, if known.
    pub age_days: Option<i64>,
    /// True when the release is older than `stale_after_days`: it may not reflect the
    /// provider's latest data.
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DatabaseInfo {
    pub path: String,
    pub installed: bool,
    pub schema_version: Option<i64>,
    pub built_at: Option<String>,
    pub built_by: Option<String>,
    pub sources: Vec<SourceRecord>,
}

/// What a concept is, in the application's source-neutral vocabulary.
/// Mirrors the `concept_kinds` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Ingredient,
    PreciseIngredient,
    MultipleIngredients,
    BrandName,
    ClinicalDrug,
    Product,
    BrandedDrug,
    GenericPack,
    BrandedPack,
    ClinicalComponent,
    BrandedComponent,
    ClinicalDoseForm,
    ClinicalDoseFormPrecise,
    BrandedDoseForm,
    BrandedDoseFormPrecise,
    ClinicalDoseFormGroup,
    ClinicalDoseFormGroupPrecise,
    BrandedDoseFormGroup,
    DoseForm,
    DoseFormGroup,
}

impl Kind {
    pub const ALL: [Kind; 20] = [
        Kind::Ingredient,
        Kind::PreciseIngredient,
        Kind::MultipleIngredients,
        Kind::BrandName,
        Kind::ClinicalDrug,
        Kind::Product,
        Kind::BrandedDrug,
        Kind::GenericPack,
        Kind::BrandedPack,
        Kind::ClinicalComponent,
        Kind::BrandedComponent,
        Kind::ClinicalDoseForm,
        Kind::ClinicalDoseFormPrecise,
        Kind::BrandedDoseForm,
        Kind::BrandedDoseFormPrecise,
        Kind::ClinicalDoseFormGroup,
        Kind::ClinicalDoseFormGroupPrecise,
        Kind::BrandedDoseFormGroup,
        Kind::DoseForm,
        Kind::DoseFormGroup,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Ingredient => "ingredient",
            Kind::PreciseIngredient => "precise_ingredient",
            Kind::MultipleIngredients => "multiple_ingredients",
            Kind::BrandName => "brand_name",
            Kind::ClinicalDrug => "clinical_drug",
            Kind::Product => "product",
            Kind::BrandedDrug => "branded_drug",
            Kind::GenericPack => "generic_pack",
            Kind::BrandedPack => "branded_pack",
            Kind::ClinicalComponent => "clinical_component",
            Kind::BrandedComponent => "branded_component",
            Kind::ClinicalDoseForm => "clinical_dose_form",
            Kind::ClinicalDoseFormPrecise => "clinical_dose_form_precise",
            Kind::BrandedDoseForm => "branded_dose_form",
            Kind::BrandedDoseFormPrecise => "branded_dose_form_precise",
            Kind::ClinicalDoseFormGroupPrecise => "clinical_dose_form_group_precise",
            Kind::ClinicalDoseFormGroup => "clinical_dose_form_group",
            Kind::BrandedDoseFormGroup => "branded_dose_form_group",
            Kind::DoseForm => "dose_form",
            Kind::DoseFormGroup => "dose_form_group",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NameType {
    Preferred,
    Synonym,
    Prescribable,
    TallMan,
}

impl NameType {
    pub fn as_str(self) -> &'static str {
        match self {
            NameType::Preferred => "preferred",
            NameType::Synonym => "synonym",
            NameType::Prescribable => "prescribable",
            NameType::TallMan => "tall_man",
        }
    }
}

/// A concept as shown in results. `id` is internal to one database file and never output.
#[derive(Debug, Clone, Serialize)]
pub struct ConceptRef {
    #[serde(skip)]
    pub id: i64,
    pub source: String,
    pub source_version: String,
    /// Identifier system of `code`, e.g. `rxcui`.
    pub code_system: String,
    pub code: String,
    pub name: String,
    pub kind: String,
    #[serde(skip)]
    pub kind_label: String,
    /// The source's own term type, e.g. RxNorm TTY `IN`.
    pub source_type: String,
}

/// Summary sections of a concept, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Ingredients,
    PreciseIngredients,
    Brands,
    ClinicalDrugs,
    BrandedDrugs,
    Combinations,
    DoseForms,
    Packs,
    Contents,
}

impl Section {
    pub const ALL: [Section; 9] = [
        Section::Ingredients,
        Section::PreciseIngredients,
        Section::Brands,
        Section::ClinicalDrugs,
        Section::BrandedDrugs,
        Section::Combinations,
        Section::DoseForms,
        Section::Packs,
        Section::Contents,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Section::Ingredients => "ingredients",
            Section::PreciseIngredients => "precise_ingredients",
            Section::Brands => "brands",
            Section::ClinicalDrugs => "clinical_drugs",
            Section::BrandedDrugs => "branded_drugs",
            Section::Combinations => "combinations",
            Section::DoseForms => "dose_forms",
            Section::Packs => "packs",
            Section::Contents => "contents",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Section::Ingredients => "Ingredients",
            Section::PreciseIngredients => "Precise ingredients",
            Section::Brands => "Brands",
            Section::ClinicalDrugs => "Clinical drugs",
            Section::BrandedDrugs => "Branded drugs",
            Section::Combinations => "Combinations",
            Section::DoseForms => "Dose forms",
            Section::Packs => "Packs",
            Section::Contents => "Contents",
        }
    }

    pub fn parse(s: &str) -> Option<Section> {
        Section::ALL.into_iter().find(|x| x.as_str() == s)
    }
}
