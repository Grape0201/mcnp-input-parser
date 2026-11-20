use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Severity level of a validation diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Severity {
    /// Fatal MCNP input error that will cause MCNP to reject the input or behave erroneously.
    Error,
    /// Potential issue, unused definition, or non-fatal inconsistency.
    Warning,
}

/// High-level category of a validation diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValidationCategory {
    /// Block structure and file-level organization.
    Structure,
    /// Out-of-range ID numbers.
    IdRange,
    /// Duplicate IDs or duplicate singleton cards.
    DuplicateId,
    /// Surface and macrobody parameter validation.
    Surface,
    /// Cross-card reference integrity and circular reference checks.
    Reference,
    /// Cell definition and parameter validation.
    Cell,
    /// Source definition (`SDEF`, `KCODE`, `KSRC`, `SI`, `SP`, `SB`, `DS`) validation.
    Source,
    /// Tally and mesh tally (`F`, `E`, `T`, `C`, `DE`, `DF`, `FM`, `FS`, `SD`, `FMESH`) validation.
    Tally,
    /// Material (`M`, `MT`) validation.
    Material,
    /// Particle transport mode (`MODE`) consistency.
    Mode,
    /// Other data cards (`TR`, `NPS`, `KCODE`, unrecognized cards).
    DataCard,
}

/// Specific machine-readable error/warning code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    // Structure (E0xx)
    EmptyCellsBlock,
    EmptySurfacesBlock,
    EmptyDataBlock,
    UnexpectedBlankLine,

    // ID Range & Duplicates (E1xx)
    CellIdOutOfRange,
    SurfaceIdOutOfRange,
    DataCardIdOutOfRange,
    DuplicateCellId,
    DuplicateSurfaceId,
    DuplicateDataCard,

    // Surface & Macrobody (E2xx)
    UnknownSurfaceMnemonic,
    InvalidSurfaceParamCount,
    InvalidSurfaceGeometry,
    InvalidPeriodicBoundary,

    // Cross-References & Cycles (E3xx / W3xx)
    UndefinedSurface,
    InvalidMacrobodyFacet,
    UndefinedCell,
    CircularCellReference,
    UndefinedMaterial,
    UndefinedTransform,
    UndefinedUniverse,
    CircularUniverseFill,
    UnusedSurface,
    UnusedMaterial,

    // Cell Definition & Parameters (E4xx)
    ZeroDensityNonVoidCell,
    DuplicateCellParameter,
    InvalidCellParameterValue,
    MissingCellImportance,
    ImportanceCardLengthMismatch,
    UnrecognizedCellParameter,
    VoidCellWithDensity,
    FilledCellWithMaterial,
    LatticeCellMissingFill,
    InvalidCellComplement,

    // Source Definition (E5xx / W5xx)
    MissingSourceDefinition,
    ConflictingSourceDefinitions,
    MissingKsrcForKcode,
    UndefinedSourceDistribution,
    IncompleteSourceDistribution,
    SourceDistributionLengthMismatch,
    NonMonotonicSourceBins,
    InvalidSourceProbability,
    InvalidSdefParameter,
    InvalidSdefDependency,
    InconsistentSdefOptions,

    // Tally & Mesh (E6xx)
    OrphanTallyModifierCard,
    TallyTargetKindMismatch,
    IncompleteDoseCards,
    NonMonotonicTallyBins,
    InvalidCosineBinRange,
    InvalidDetectorRadius,
    InvalidFmeshConfiguration,

    // Material (E7xx)
    EmptyMaterialCard,
    MixedMaterialFractionSigns,
    InvalidZaidFormat,

    // Mode (E8xx / W8xx)
    EmptyModeCard,
    ParticleNotInMode,

    // DataCard / Physics / Control (E9xx / W9xx)
    InvalidTransformCardLength,
    InvalidNpsCount,
    InvalidKcodeParameters,
    UnrecognizedDataCard,
}

impl ErrorCode {
    /// Returns the short string identifier (e.g. `"E301"`, `"W309"`).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EmptyCellsBlock => "E001",
            Self::EmptySurfacesBlock => "E002",
            Self::EmptyDataBlock => "E003",
            Self::UnexpectedBlankLine => "E004",

            Self::CellIdOutOfRange => "E101",
            Self::SurfaceIdOutOfRange => "E102",
            Self::DataCardIdOutOfRange => "E103",
            Self::DuplicateCellId => "E104",
            Self::DuplicateSurfaceId => "E105",
            Self::DuplicateDataCard => "E106",

            Self::UnknownSurfaceMnemonic => "E201",
            Self::InvalidSurfaceParamCount => "E202",
            Self::InvalidSurfaceGeometry => "E203",
            Self::InvalidPeriodicBoundary => "E204",

            Self::UndefinedSurface => "E301",
            Self::InvalidMacrobodyFacet => "E302",
            Self::UndefinedCell => "E303",
            Self::CircularCellReference => "E304",
            Self::UndefinedMaterial => "E305",
            Self::UndefinedTransform => "E306",
            Self::UndefinedUniverse => "E307",
            Self::CircularUniverseFill => "E308",
            Self::UnusedSurface => "W309",
            Self::UnusedMaterial => "W310",

            Self::ZeroDensityNonVoidCell => "E401",
            Self::DuplicateCellParameter => "E402",
            Self::InvalidCellParameterValue => "E403",
            Self::MissingCellImportance => "E404",
            Self::ImportanceCardLengthMismatch => "E405",
            Self::UnrecognizedCellParameter => "E406",
            Self::VoidCellWithDensity => "E407",
            Self::FilledCellWithMaterial => "E408",
            Self::LatticeCellMissingFill => "E409",
            Self::InvalidCellComplement => "E410",

            Self::MissingSourceDefinition => "E501",
            Self::ConflictingSourceDefinitions => "E502",
            Self::MissingKsrcForKcode => "W503",
            Self::UndefinedSourceDistribution => "E504",
            Self::IncompleteSourceDistribution => "E505",
            Self::SourceDistributionLengthMismatch => "E506",
            Self::NonMonotonicSourceBins => "E507",
            Self::InvalidSourceProbability => "E508",
            Self::InvalidSdefParameter => "E509",
            Self::InvalidSdefDependency => "E510",
            Self::InconsistentSdefOptions => "W511",

            Self::OrphanTallyModifierCard => "E601",
            Self::TallyTargetKindMismatch => "E602",
            Self::IncompleteDoseCards => "E603",
            Self::NonMonotonicTallyBins => "E604",
            Self::InvalidCosineBinRange => "E605",
            Self::InvalidDetectorRadius => "E606",
            Self::InvalidFmeshConfiguration => "E607",

            Self::EmptyMaterialCard => "E701",
            Self::MixedMaterialFractionSigns => "E702",
            Self::InvalidZaidFormat => "E703",

            Self::EmptyModeCard => "E801",
            Self::ParticleNotInMode => "W802",

            Self::InvalidTransformCardLength => "E901",
            Self::InvalidNpsCount => "E902",
            Self::InvalidKcodeParameters => "E903",
            Self::UnrecognizedDataCard => "W904",
        }
    }
}

/// A validation diagnostic (error or warning) detected in an [`McnpInput`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
#[error("[{}] {}{}: {}", code.as_str(), Self::severity_label(*severity), Self::line_label(*line_number), message)]
pub struct ValidationError {
    pub code: ErrorCode,
    pub severity: Severity,
    pub category: ValidationCategory,
    /// 1-based starting line number of the offending card (if known from [`CardLineSpans`]).
    pub line_number: Option<usize>,
    pub message: String,
}

impl ValidationError {
    fn severity_label(s: Severity) -> &'static str {
        match s {
            Severity::Error => "Error",
            Severity::Warning => "Warning",
        }
    }

    fn line_label(line: Option<usize>) -> String {
        match line {
            Some(n) => format!(" at line {}", n),
            None => String::new(),
        }
    }
}

pub(super) const MAX_MCNP_ID: u32 = 99_999_999;

/// Known standard MCNP data card prefixes (used to distinguish supported/known MCNP cards from typos).
pub(super) const KNOWN_MCNP_DATA_PREFIXES: &[&str] = &[
    "m", "mt", "mx", "mpn", "otfdb", "totnu", "nonu", "awtab", "xs", "void", "mgopt", "drxs",
    "mode", "phys", "act", "fmult", "tropt", "unc", "cosy", "bfld", "bflcl", "sdef", "si", "sp",
    "sb", "ds", "sc", "ssw", "ssr", "kcode", "ksrc", "kopts", "f", "fx", "fy", "fz", "fc", "e",
    "t", "c", "fq", "fm", "de", "df", "em", "tm", "cm", "cf", "sf", "fs", "sd", "fu", "ft", "tf",
    "fmesh", "tmesh", "rmesh", "cmesh", "smesh", "imp", "vov", "esplt", "tsplt", "wwp", "wwn",
    "wwe", "wwg", "wwge", "mesh", "ext", "vect", "fcl", "dxt", "dd", "pd", "dxc", "bbrem", "pikmt",
    "pwt", "lca", "lcb", "lcc", "lae", "lea", "elpt", "mphys", "var", "cut", "thtme", "mph",
    "tmp", "tr", "uran", "dm", "embed", "embee", "embeb", "embem", "embtb", "embdb", "embdf",
    "notrn", "pert", "ksen", "ksental", "nps", "ctme", "prdmp", "lost", "idum", "rdum", "za",
    "zb", "zc", "zd", "print", "talnp", "files", "dbcn", "rand", "dawwg", "splt",
];

/// Known standard MCNP cell parameter keywords.
pub(super) const KNOWN_MCNP_CELL_PARAMETERS: &[&str] = &[
    "imp", "vol", "u", "trcl", "lat", "fill", "elpt", "tmp", "rho", "mat", "nonu", "pwt", "ext",
    "fcl", "wwn", "dxc", "pd", "bbrm", "bbrem", "unc", "cosy", "bfld", "bflcl", "embed", "embee",
    "embeb", "embem", "embtb", "embdb", "embdf", "vec", "vect", "dir", "seg", "rad", "tag", "smx",
    "corr", "var", "trn", "cut", "geom", "wwge", "wwp", "wwe", "esplt", "tsplt", "splt", "vov",
    "mgopt", "mesh", "bcat", "e", "t",
];

