use crate::ast::{
    BoundaryCondition, CellCard, CellDefinition, CellParam, DataCard, DetectorEntry, Entry, FmBin,
    GeometryExpr, McnpInput, Particle, SdefValue, SdefVariable, SiEntry, SiOption, SpOption,
    SpSbContent, SurfaceKind, TallyKind, TallyTarget,
};
use crate::CardLineSpans;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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

    // Source Definition (E5xx / W5xx)
    MissingSourceDefinition,
    ConflictingSourceDefinitions,
    MissingKsrcForKcode,
    UndefinedSourceDistribution,
    IncompleteSourceDistribution,
    SourceDistributionLengthMismatch,
    NonMonotonicSourceBins,
    InvalidSourceProbability,

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

            Self::MissingSourceDefinition => "E501",
            Self::ConflictingSourceDefinitions => "E502",
            Self::MissingKsrcForKcode => "W503",
            Self::UndefinedSourceDistribution => "E504",
            Self::IncompleteSourceDistribution => "E505",
            Self::SourceDistributionLengthMismatch => "E506",
            Self::NonMonotonicSourceBins => "E507",
            Self::InvalidSourceProbability => "E508",

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

const MAX_MCNP_ID: u32 = 99_999_999;

/// Known standard MCNP data card prefixes (used to distinguish supported/known MCNP cards from typos).
const KNOWN_MCNP_DATA_PREFIXES: &[&str] = &[
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

/// Validates a parsed [`McnpInput`] AST and returns all detected errors and warnings.
pub fn validate_mcnp_input(input: &McnpInput, spans: &CardLineSpans) -> Vec<ValidationError> {
    let mut diagnostics = Vec::new();

    let cell_line = |idx: usize, id: u32| -> Option<usize> {
        spans
            .cell_card_lines
            .get(idx)
            .copied()
            .or_else(|| spans.cell_lines.get(&id).copied())
    };

    let surf_line = |idx: usize, id: u32| -> Option<usize> {
        spans
            .surface_card_lines
            .get(idx)
            .copied()
            .or_else(|| spans.surface_lines.get(&id).copied())
    };

    let data_line = |idx: usize| -> Option<usize> { spans.data_card_lines.get(idx).copied() };

    // ========================================================================
    // 1. Block Structure Checks
    // ========================================================================
    if input.cells.is_empty() {
        diagnostics.push(ValidationError {
            code: ErrorCode::EmptyCellsBlock,
            severity: Severity::Error,
            category: ValidationCategory::Structure,
            line_number: None,
            message: "Cell cards block (Block 1) is empty".to_string(),
        });
    }
    if input.surfaces.is_empty() {
        diagnostics.push(ValidationError {
            code: ErrorCode::EmptySurfacesBlock,
            severity: Severity::Error,
            category: ValidationCategory::Structure,
            line_number: None,
            message: "Surface cards block (Block 2) is empty".to_string(),
        });
    }
    if input.data_cards.is_empty() {
        diagnostics.push(ValidationError {
            code: ErrorCode::EmptyDataBlock,
            severity: Severity::Error,
            category: ValidationCategory::Structure,
            line_number: None,
            message: "Data cards block (Block 3) is empty".to_string(),
        });
    }

    // ========================================================================
    // 2. Build Lookup Maps & Check ID Ranges / Duplicates
    // ========================================================================
    let mut cell_map: HashMap<u32, (usize, &CellCard)> = HashMap::new();
    for (idx, cell) in input.cells.iter().enumerate() {
        let line = cell_line(idx, cell.id);
        if cell.id == 0 || cell.id > MAX_MCNP_ID {
            diagnostics.push(ValidationError {
                code: ErrorCode::CellIdOutOfRange,
                severity: Severity::Error,
                category: ValidationCategory::IdRange,
                line_number: line,
                message: format!(
                    "Cell ID {} is out of valid MCNP range (1..={})",
                    cell.id, MAX_MCNP_ID
                ),
            });
        }
        if let Some((first_idx, _)) = cell_map.insert(cell.id, (idx, cell)) {
            let first_line = cell_line(first_idx, cell.id);
            diagnostics.push(ValidationError {
                code: ErrorCode::DuplicateCellId,
                severity: Severity::Error,
                category: ValidationCategory::DuplicateId,
                line_number: line,
                message: match first_line {
                    Some(fl) => format!("Duplicate Cell ID {} (first defined at line {})", cell.id, fl),
                    None => format!("Duplicate Cell ID {}", cell.id),
                },
            });
        }
    }

    let mut surface_map: HashMap<u32, (usize, &crate::ast::SurfaceCard)> = HashMap::new();
    for (idx, surf) in input.surfaces.iter().enumerate() {
        let line = surf_line(idx, surf.id);
        if surf.id == 0 || surf.id > MAX_MCNP_ID {
            diagnostics.push(ValidationError {
                code: ErrorCode::SurfaceIdOutOfRange,
                severity: Severity::Error,
                category: ValidationCategory::IdRange,
                line_number: line,
                message: format!(
                    "Surface ID {} is out of valid MCNP range (1..={})",
                    surf.id, MAX_MCNP_ID
                ),
            });
        }
        if let Some((first_idx, _)) = surface_map.insert(surf.id, (idx, surf)) {
            let first_line = surf_line(first_idx, surf.id);
            diagnostics.push(ValidationError {
                code: ErrorCode::DuplicateSurfaceId,
                severity: Severity::Error,
                category: ValidationCategory::DuplicateId,
                line_number: line,
                message: match first_line {
                    Some(fl) => format!(
                        "Duplicate Surface ID {} (first defined at line {})",
                        surf.id, fl
                    ),
                    None => format!("Duplicate Surface ID {}", surf.id),
                },
            });
        }
    }

    // Track Data Card definitions
    let mut materials: HashMap<u32, usize> = HashMap::new();
    let mut mt_cards: HashMap<u32, usize> = HashMap::new();
    let mut transforms: HashMap<u32, usize> = HashMap::new();
    let mut tallies: HashMap<u32, (usize, TallyKind)> = HashMap::new();
    let mut fmeshes: HashMap<u32, usize> = HashMap::new();
    let mut si_cards: HashMap<u32, (usize, &crate::ast::SiCard)> = HashMap::new();
    let mut sp_cards: HashMap<u32, (usize, &crate::ast::SpSbCard)> = HashMap::new();
    let mut sb_cards: HashMap<u32, (usize, &crate::ast::SpSbCard)> = HashMap::new();
    let mut ds_cards: HashMap<u32, (usize, &crate::ast::DsCard)> = HashMap::new();
    let mut sc_cards: HashMap<u32, usize> = HashMap::new();
    let mut de_cards: HashMap<u32, (usize, &crate::ast::DoseCard)> = HashMap::new();
    let mut df_cards: HashMap<u32, (usize, &crate::ast::DoseCard)> = HashMap::new();
    let mut fm_cards: HashMap<u32, usize> = HashMap::new();
    let mut fs_cards: HashMap<u32, usize> = HashMap::new();
    let mut sd_cards: HashMap<u32, usize> = HashMap::new();
    let mut e_cards: HashMap<u32, usize> = HashMap::new();
    let mut t_cards: HashMap<u32, usize> = HashMap::new();
    let mut c_cards: HashMap<u32, usize> = HashMap::new();
    let mut fc_cards: HashMap<u32, usize> = HashMap::new();

    let mut mode_indices = Vec::new();
    let mut sdef_indices = Vec::new();
    let mut kcode_indices = Vec::new();
    let mut ksrc_indices = Vec::new();
    let mut nps_indices = Vec::new();

    let mut used_surfaces: HashSet<u32> = HashSet::new();
    let mut used_materials: HashSet<u32> = HashSet::new();

    for (idx, card) in input.data_cards.iter().enumerate() {
        let line = data_line(idx);
        let check_dup = |map: &mut HashMap<u32, usize>,
                         id: u32,
                         name: &str,
                         diag: &mut Vec<ValidationError>| {
            if let Some(prev_idx) = map.insert(id, idx) {
                let prev_line = data_line(prev_idx);
                diag.push(ValidationError {
                    code: ErrorCode::DuplicateDataCard,
                    severity: Severity::Error,
                    category: ValidationCategory::DuplicateId,
                    line_number: line,
                    message: match prev_line {
                        Some(pl) => format!(
                            "Duplicate {}{} card (first defined at line {})",
                            name, id, pl
                        ),
                        None => format!("Duplicate {}{} card", name, id),
                    },
                });
            }
        };

        match card {
            DataCard::Material { id, .. } => {
                if *id == 0 || *id > MAX_MCNP_ID {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!("Material ID M{} is out of valid range", id),
                    });
                }
                check_dup(&mut materials, *id, "M", &mut diagnostics);
            }
            DataCard::ThermalScattering { id, .. } => {
                check_dup(&mut mt_cards, *id, "MT", &mut diagnostics);
            }
            DataCard::Transform { id, .. } => {
                if *id == 0 || *id > 999 {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!("Transform ID TR{} is out of valid range (1..=999)", id),
                    });
                }
                check_dup(&mut transforms, *id, "TR", &mut diagnostics);
            }
            DataCard::Tally { id, kind, .. } => {
                if *id == 0 || *id > MAX_MCNP_ID {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!("Tally ID F{} is out of valid range", id),
                    });
                }
                if let Some((prev_idx, _)) = tallies.insert(*id, (idx, *kind)) {
                    let prev_line = data_line(prev_idx);
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => {
                                format!("Duplicate F{} card (first defined at line {})", id, pl)
                            }
                            None => format!("Duplicate F{} card", id),
                        },
                    });
                }
            }
            DataCard::DetectorTally { id, .. } => {
                if let Some((prev_idx, _)) = tallies.insert(*id, (idx, TallyKind::F5DetectorFlux)) {
                    let prev_line = data_line(prev_idx);
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => {
                                format!("Duplicate F{} card (first defined at line {})", id, pl)
                            }
                            None => format!("Duplicate F{} card", id),
                        },
                    });
                }
            }
            DataCard::Fmesh(fm) => {
                check_dup(&mut fmeshes, fm.id, "FMESH", &mut diagnostics);
            }
            DataCard::Si(si) => {
                if si.id == 0 || si.id > 999 {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!(
                            "Source distribution ID SI{} is out of valid range (1..=999)",
                            si.id
                        ),
                    });
                }
                if let Some((prev_idx, _)) = si_cards.insert(si.id, (idx, si)) {
                    let prev_line = data_line(prev_idx);
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => {
                                format!("Duplicate SI{} card (first defined at line {})", si.id, pl)
                            }
                            None => format!("Duplicate SI{} card", si.id),
                        },
                    });
                }
            }
            DataCard::SpSb(sp) => {
                if sp.id == 0 || sp.id > 999 {
                    let prefix = if sp.is_bias { "SB" } else { "SP" };
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!(
                            "Source distribution ID {}{} is out of valid range (1..=999)",
                            prefix, sp.id
                        ),
                    });
                }
                let target_map = if sp.is_bias {
                    &mut sb_cards
                } else {
                    &mut sp_cards
                };
                let prefix = if sp.is_bias { "SB" } else { "SP" };
                if let Some((prev_idx, _)) = target_map.insert(sp.id, (idx, sp)) {
                    let prev_line = data_line(prev_idx);
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => format!(
                                "Duplicate {}{} card (first defined at line {})",
                                prefix, sp.id, pl
                            ),
                            None => format!("Duplicate {}{} card", prefix, sp.id),
                        },
                    });
                }
            }
            DataCard::Ds(ds) => {
                if ds.id == 0 || ds.id > 999 {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!(
                            "Dependent source ID DS{} is out of valid range (1..=999)",
                            ds.id
                        ),
                    });
                }
                if let Some((prev_idx, _)) = ds_cards.insert(ds.id, (idx, ds)) {
                    let prev_line = data_line(prev_idx);
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => {
                                format!("Duplicate DS{} card (first defined at line {})", ds.id, pl)
                            }
                            None => format!("Duplicate DS{} card", ds.id),
                        },
                    });
                }
            }
            DataCard::Sc { id, .. } => {
                check_dup(&mut sc_cards, *id, "SC", &mut diagnostics);
            }
            DataCard::Dose(dose) => {
                let target_map = if dose.is_function {
                    &mut df_cards
                } else {
                    &mut de_cards
                };
                let prefix = if dose.is_function { "DF" } else { "DE" };
                if let Some((prev_idx, _)) = target_map.insert(dose.id, (idx, dose)) {
                    let prev_line = data_line(prev_idx);
                    diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => format!(
                                "Duplicate {}{} card (first defined at line {})",
                                prefix, dose.id, pl
                            ),
                            None => format!("Duplicate {}{} card", prefix, dose.id),
                        },
                    });
                }
            }
            DataCard::TallyMultiplier(fm) => check_dup(&mut fm_cards, fm.id, "FM", &mut diagnostics),
            DataCard::TallySegment { id, .. } => check_dup(&mut fs_cards, *id, "FS", &mut diagnostics),
            DataCard::SegmentDivisor { id, .. } => check_dup(&mut sd_cards, *id, "SD", &mut diagnostics),
            DataCard::TallyEnergy { id, .. } => check_dup(&mut e_cards, *id, "E", &mut diagnostics),
            DataCard::TallyTime { id, .. } => check_dup(&mut t_cards, *id, "T", &mut diagnostics),
            DataCard::TallyCosine { id, .. } => check_dup(&mut c_cards, *id, "C", &mut diagnostics),
            DataCard::TallyComment { id, .. } => check_dup(&mut fc_cards, *id, "FC", &mut diagnostics),
            DataCard::Mode { .. } => mode_indices.push(idx),
            DataCard::Sdef { .. } => sdef_indices.push(idx),
            DataCard::Kcode { .. } => kcode_indices.push(idx),
            DataCard::Ksrc { .. } => ksrc_indices.push(idx),
            DataCard::Nps { .. } => nps_indices.push(idx),
            _ => {}
        }
    }

    for (indices, name) in [
        (&mode_indices, "MODE"),
        (&sdef_indices, "SDEF"),
        (&kcode_indices, "KCODE"),
        (&ksrc_indices, "KSRC"),
        (&nps_indices, "NPS"),
    ] {
        if indices.len() > 1 {
            let first_line = data_line(indices[0]);
            for &dup_idx in &indices[1..] {
                diagnostics.push(ValidationError {
                    code: ErrorCode::DuplicateDataCard,
                    severity: Severity::Error,
                    category: ValidationCategory::DuplicateId,
                    line_number: data_line(dup_idx),
                    message: match first_line {
                        Some(fl) => {
                            format!("Duplicate {} card (first defined at line {})", name, fl)
                        }
                        None => format!("Duplicate {} card", name),
                    },
                });
            }
        }
    }

    // Active transport particles from MODE card (defaults to `n` in MCNP if omitted)
    let active_particles: HashSet<Particle> = if let Some(&mode_idx) = mode_indices.first() {
        if let DataCard::Mode { particles } = &input.data_cards[mode_idx] {
            if particles.is_empty() {
                diagnostics.push(ValidationError {
                    code: ErrorCode::EmptyModeCard,
                    severity: Severity::Error,
                    category: ValidationCategory::Mode,
                    line_number: data_line(mode_idx),
                    message: "MODE card must specify at least one particle designator".to_string(),
                });
            }
            particles.iter().cloned().collect()
        } else {
            HashSet::from([Particle::new("n")])
        }
    } else {
        HashSet::from([Particle::new("n")])
    };

    // ========================================================================
    // 3. Surface & Macrobody Validation
    // ========================================================================
    for (idx, surf) in input.surfaces.iter().enumerate() {
        let line = surf_line(idx, surf.id);

        // Validate transform_id or periodic boundary
        if let Some(tr_id) = surf.transform_id {
            if tr_id > 0 {
                let tr_u32 = tr_id as u32;
                if !transforms.contains_key(&tr_u32) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::UndefinedTransform,
                        severity: Severity::Error,
                        category: ValidationCategory::Reference,
                        line_number: line,
                        message: format!(
                            "Surface {} references undefined transformation TR{}",
                            surf.id, tr_u32
                        ),
                    });
                }
            } else if tr_id < 0 {
                let peer_surf_id = tr_id.unsigned_abs();
                used_surfaces.insert(peer_surf_id);
                if surf.boundary != BoundaryCondition::Transmissive {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidPeriodicBoundary,
                        severity: Severity::Error,
                        category: ValidationCategory::Surface,
                        line_number: line,
                        message: format!(
                            "Surface {} cannot combine periodic boundary ({}) with reflective/white boundary condition",
                            surf.id, tr_id
                        ),
                    });
                }
                if !matches!(
                    surf.kind,
                    SurfaceKind::P | SurfaceKind::PX | SurfaceKind::PY | SurfaceKind::PZ
                ) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidPeriodicBoundary,
                        severity: Severity::Error,
                        category: ValidationCategory::Surface,
                        line_number: line,
                        message: format!(
                            "Surface {} has periodic boundary ({}), which is only valid on planes (P, PX, PY, PZ)",
                            surf.id, tr_id
                        ),
                    });
                }
                if peer_surf_id == surf.id || !surface_map.contains_key(&peer_surf_id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::UndefinedSurface,
                        severity: Severity::Error,
                        category: ValidationCategory::Reference,
                        line_number: line,
                        message: format!(
                            "Surface {} periodic boundary references undefined or self surface {}",
                            surf.id, peer_surf_id
                        ),
                    });
                }
            } else {
                diagnostics.push(ValidationError {
                    code: ErrorCode::InvalidPeriodicBoundary,
                    severity: Severity::Error,
                    category: ValidationCategory::Surface,
                    line_number: line,
                    message: format!("Surface {} has invalid transform/periodic ID 0", surf.id),
                });
            }
        }

        validate_surface_kind_and_params(surf, line, &mut diagnostics);
    }

    // ========================================================================
    // 4. Cell Validation & Reference Checks
    // ========================================================================
    // Map of Universe ID -> list of Cell IDs belonging to that universe
    let mut universe_cells: HashMap<i32, Vec<u32>> = HashMap::new();
    // Map of Universe ID -> list of (Cell ID, line, filled Universe IDs)
    let mut cell_fills: Vec<(u32, Option<usize>, Option<i32>, Vec<i32>)> = Vec::new();
    // Track per-particle cell importance definitions
    let mut cell_has_imp: HashMap<Particle, HashSet<u32>> = HashMap::new();
    let mut cell_nonzero_imp: HashMap<Particle, bool> = HashMap::new();

    for (idx, cell) in input.cells.iter().enumerate() {
        let line = cell_line(idx, cell.id);

        match &cell.definition {
            CellDefinition::Normal {
                material_id,
                density,
                geometry,
            } => {
                if *material_id > 0 {
                    used_materials.insert(*material_id);
                    if !materials.contains_key(material_id) {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::UndefinedMaterial,
                            severity: Severity::Error,
                            category: ValidationCategory::Reference,
                            line_number: line,
                            message: format!(
                                "Cell {} references undefined material M{}",
                                cell.id, material_id
                            ),
                        });
                    }
                    if let Some(d) = density {
                        if *d == 0.0 {
                            diagnostics.push(ValidationError {
                                code: ErrorCode::ZeroDensityNonVoidCell,
                                severity: Severity::Error,
                                category: ValidationCategory::Cell,
                                line_number: line,
                                message: format!(
                                    "Cell {} has non-void material M{} with zero density",
                                    cell.id, material_id
                                ),
                            });
                        }
                    }
                }

                validate_geometry_expr(
                    cell.id,
                    line,
                    geometry,
                    &surface_map,
                    &cell_map,
                    &mut used_surfaces,
                    &mut diagnostics,
                );
            }
            CellDefinition::LikeBut { base_cell_id } => {
                if *base_cell_id == cell.id {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::CircularCellReference,
                        severity: Severity::Error,
                        category: ValidationCategory::Reference,
                        line_number: line,
                        message: format!("Cell {} references itself in LIKE {} BUT", cell.id, base_cell_id),
                    });
                } else if let Some((_, base_cell)) = cell_map.get(base_cell_id) {
                    if matches!(base_cell.definition, CellDefinition::LikeBut { .. }) {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::CircularCellReference,
                            severity: Severity::Error,
                            category: ValidationCategory::Reference,
                            line_number: line,
                            message: format!(
                                "Cell {} uses LIKE {} BUT where target cell {} is also a LIKE-BUT cell",
                                cell.id, base_cell_id, base_cell_id
                            ),
                        });
                    }
                } else {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::UndefinedCell,
                        severity: Severity::Error,
                        category: ValidationCategory::Reference,
                        line_number: line,
                        message: format!(
                            "Cell {} references undefined base cell {} in LIKE {} BUT",
                            cell.id, base_cell_id, base_cell_id
                        ),
                    });
                }
            }
        }

        // Check cell parameters
        let mut seen_param_keys: HashSet<String> = HashSet::new();
        let mut this_universe: Option<i32> = None;
        let mut filled_universes: Vec<i32> = Vec::new();

        for param in &cell.params {
            let key_sig = match param {
                CellParam::Importance { particles, .. } => {
                    let mut p_names: Vec<&str> = particles.iter().map(|p| p.0.as_str()).collect();
                    p_names.sort_unstable();
                    format!("imp:{}", p_names.join(","))
                }
                CellParam::Volume(_) => "vol".to_string(),
                CellParam::Universe(_) => "u".to_string(),
                CellParam::Fill { .. } => "fill".to_string(),
                CellParam::Lattice(_) => "lat".to_string(),
                CellParam::Temperature(_) => "tmp".to_string(),
                CellParam::Density(_) => "rho".to_string(),
                CellParam::Material(_) => "mat".to_string(),
                CellParam::Transformation { .. } => "trcl".to_string(),
                CellParam::Other { key, particles, .. } => {
                    let mut p_names: Vec<&str> = particles.iter().map(|p| p.0.as_str()).collect();
                    p_names.sort_unstable();
                    format!("{}:{}", key, p_names.join(","))
                }
            };

            if !seen_param_keys.insert(key_sig.clone()) {
                diagnostics.push(ValidationError {
                    code: ErrorCode::DuplicateCellParameter,
                    severity: Severity::Error,
                    category: ValidationCategory::Cell,
                    line_number: line,
                    message: format!(
                        "Cell {} has duplicate cell parameter `{}`",
                        cell.id,
                        key_sig.trim_end_matches(':')
                    ),
                });
            }

            match param {
                CellParam::Importance { particles, value } => {
                    if *value < 0.0 {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidCellParameterValue,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: line,
                            message: format!(
                                "Cell {} has negative importance value ({})",
                                cell.id, value
                            ),
                        });
                    }
                    let targets: Vec<Particle> = if particles.is_empty() {
                        vec![Particle::new("n")]
                    } else {
                        particles.clone()
                    };
                    for p in &targets {
                        check_particle_in_mode(
                            p,
                            &active_particles,
                            line,
                            &format!("Cell {} IMP", cell.id),
                            &mut diagnostics,
                        );
                        cell_has_imp.entry(p.clone()).or_default().insert(cell.id);
                        if *value > 0.0 {
                            cell_nonzero_imp.insert(p.clone(), true);
                        }
                    }
                }
                CellParam::Volume(v) => {
                    if *v <= 0.0 {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidCellParameterValue,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: line,
                            message: format!(
                                "Cell {} has non-positive volume VOL={}",
                                cell.id, v
                            ),
                        });
                    }
                }
                CellParam::Universe(u) => {
                    let abs_u = u.abs();
                    this_universe = Some(abs_u);
                    universe_cells.entry(abs_u).or_default().push(cell.id);
                }
                CellParam::Lattice(lat) => {
                    if *lat != 1 && *lat != 2 {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidCellParameterValue,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: line,
                            message: format!(
                                "Cell {} has invalid lattice type LAT={} (must be 1 or 2)",
                                cell.id, lat
                            ),
                        });
                    }
                }
                CellParam::Temperature(entries) => {
                    for e in entries {
                        if let Entry::Value(t) = e {
                            if *t <= 0.0 {
                                diagnostics.push(ValidationError {
                                    code: ErrorCode::InvalidCellParameterValue,
                                    severity: Severity::Error,
                                    category: ValidationCategory::Cell,
                                    line_number: line,
                                    message: format!(
                                        "Cell {} has non-positive temperature TMP={}",
                                        cell.id, t
                                    ),
                                });
                            }
                        }
                    }
                }
                CellParam::Density(rho) => {
                    if *rho == 0.0 {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::ZeroDensityNonVoidCell,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: line,
                            message: format!("Cell {} specifies zero density RHO=0", cell.id),
                        });
                    }
                }
                CellParam::Material(mat_id) => {
                    if *mat_id > 0 {
                        used_materials.insert(*mat_id);
                        if !materials.contains_key(mat_id) {
                            diagnostics.push(ValidationError {
                                code: ErrorCode::UndefinedMaterial,
                                severity: Severity::Error,
                                category: ValidationCategory::Reference,
                                line_number: line,
                                message: format!(
                                    "Cell {} references undefined material MAT={}",
                                    cell.id, mat_id
                                ),
                            });
                        }
                    }
                }
                CellParam::Transformation { values, .. } => {
                    let len = values.len();
                    if len == 1 {
                        if let Some(tr_f) = values[0].as_f64() {
                            let tr_id = tr_f.round() as i32;
                            if tr_id <= 0 || !transforms.contains_key(&(tr_id as u32)) {
                                diagnostics.push(ValidationError {
                                    code: ErrorCode::UndefinedTransform,
                                    severity: Severity::Error,
                                    category: ValidationCategory::Reference,
                                    line_number: line,
                                    message: format!(
                                        "Cell {} TRCL references undefined transformation TR{}",
                                        cell.id, tr_id
                                    ),
                                });
                            }
                        }
                    } else if !matches!(len, 3 | 5 | 6 | 9 | 12 | 13) {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidCellParameterValue,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: line,
                            message: format!(
                                "Cell {} inline TRCL has {} entries (expected 1, 3, 5, 6, 9, 12, or 13)",
                                cell.id, len
                            ),
                        });
                    }
                }
                CellParam::Fill { values, .. } => {
                    extract_fill_universes(values, &mut filled_universes);
                }
                CellParam::Other { particles, key, .. } => {
                    for p in particles {
                        check_particle_in_mode(
                            p,
                            &active_particles,
                            line,
                            &format!("Cell {} {}", cell.id, key.to_uppercase()),
                            &mut diagnostics,
                        );
                    }
                }
            }
        }

        // Propagate importance from base cell if `LIKE n BUT`
        if let CellDefinition::LikeBut { base_cell_id } = &cell.definition {
            if let Some((_, base_cell)) = cell_map.get(base_cell_id) {
                for param in &base_cell.params {
                    if let CellParam::Importance { particles, value } = param {
                        let targets: Vec<Particle> = if particles.is_empty() {
                            vec![Particle::new("n")]
                        } else {
                            particles.clone()
                        };
                        for p in targets {
                            cell_has_imp.entry(p.clone()).or_default().insert(cell.id);
                            if *value > 0.0 {
                                cell_nonzero_imp.insert(p, true);
                            }
                        }
                    }
                }
            }
        }

        if !filled_universes.is_empty() {
            cell_fills.push((cell.id, line, this_universe, filled_universes));
        }
    }

    // Check `#n` ComplementCell cycles
    check_complement_cell_cycles(&cell_map, spans, &mut diagnostics);

    // Check Universe & Fill references and cycles
    let mut univ_adj: HashMap<i32, Vec<(i32, u32, Option<usize>)>> = HashMap::new();
    for (c_id, line, u_opt, filled_list) in &cell_fills {
        for &target_u in filled_list {
            if target_u > 0 && !universe_cells.contains_key(&target_u) {
                diagnostics.push(ValidationError {
                    code: ErrorCode::UndefinedUniverse,
                    severity: Severity::Error,
                    category: ValidationCategory::Reference,
                    line_number: *line,
                    message: format!(
                        "Cell {} FILL references undefined universe U={}",
                        c_id, target_u
                    ),
                });
            }
            if let Some(src_u) = u_opt {
                if target_u > 0 {
                    univ_adj
                        .entry(*src_u)
                        .or_default()
                        .push((target_u, *c_id, *line));
                }
            }
        }
    }
    check_universe_cycles(&univ_adj, &mut diagnostics);

    // ========================================================================
    // 5. Data Cards Validation (Materials, Tallies, Sources, VR, Physics)
    // ========================================================================
    let mut referenced_dists: Vec<(u32, Option<usize>, String)> = Vec::new();

    for (idx, card) in input.data_cards.iter().enumerate() {
        let line = data_line(idx);

        match card {
            DataCard::Material { id, nuclides, .. } => {
                if nuclides.is_empty() {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::EmptyMaterialCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Material,
                        line_number: line,
                        message: format!("Material M{} has no nuclides specified", id),
                    });
                } else {
                    let mut has_pos = false;
                    let mut has_neg = false;
                    for nuc in nuclides {
                        if !is_valid_zaid(&nuc.zaid) {
                            diagnostics.push(ValidationError {
                                code: ErrorCode::InvalidZaidFormat,
                                severity: Severity::Error,
                                category: ValidationCategory::Material,
                                line_number: line,
                                message: format!(
                                    "Material M{} has invalid ZAID identifier `{}`",
                                    id, nuc.zaid
                                ),
                            });
                        }
                        if nuc.fraction > 0.0 {
                            has_pos = true;
                        } else if nuc.fraction < 0.0 {
                            has_neg = true;
                        } else {
                            diagnostics.push(ValidationError {
                                code: ErrorCode::MixedMaterialFractionSigns,
                                severity: Severity::Error,
                                category: ValidationCategory::Material,
                                line_number: line,
                                message: format!(
                                    "Material M{} nuclide `{}` has zero fraction",
                                    id, nuc.zaid
                                ),
                            });
                        }
                    }
                    if has_pos && has_neg {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::MixedMaterialFractionSigns,
                            severity: Severity::Error,
                            category: ValidationCategory::Material,
                            line_number: line,
                            message: format!(
                                "Material M{} mixes positive (atomic) and negative (weight) fractions",
                                id
                            ),
                        });
                    }
                }
            }

            DataCard::ThermalScattering { id, tables } => {
                if !materials.contains_key(id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::UndefinedMaterial,
                        severity: Severity::Error,
                        category: ValidationCategory::Reference,
                        line_number: line,
                        message: format!(
                            "Thermal scattering card MT{} has no corresponding material card M{}",
                            id, id
                        ),
                    });
                }
                if tables.is_empty() {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::EmptyMaterialCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Material,
                        line_number: line,
                        message: format!("Thermal scattering card MT{} specifies no tables", id),
                    });
                }
            }

            DataCard::Sdef { params } => {
                for param in params {
                    match &param.value {
                        SdefValue::Distribution(d) => {
                            referenced_dists.push((
                                *d,
                                line,
                                format!("SDEF {:?}", param.variable).to_uppercase(),
                            ));
                        }
                        SdefValue::Function { dist_id, .. } => {
                            referenced_dists.push((
                                *dist_id,
                                line,
                                format!("SDEF {:?}", param.variable).to_uppercase(),
                            ));
                        }
                        SdefValue::Scalar(v) => match param.variable {
                            SdefVariable::Cel => {
                                let cid = v.round() as u32;
                                if cid > 0 && !cell_map.contains_key(&cid) {
                                    diagnostics.push(ValidationError {
                                        code: ErrorCode::UndefinedCell,
                                        severity: Severity::Error,
                                        category: ValidationCategory::Reference,
                                        line_number: line,
                                        message: format!(
                                            "SDEF CEL={} references undefined cell {}",
                                            cid, cid
                                        ),
                                    });
                                }
                            }
                            SdefVariable::Sur => {
                                let sid = v.round() as u32;
                                used_surfaces.insert(sid);
                                if sid > 0 && !surface_map.contains_key(&sid) {
                                    diagnostics.push(ValidationError {
                                        code: ErrorCode::UndefinedSurface,
                                        severity: Severity::Error,
                                        category: ValidationCategory::Reference,
                                        line_number: line,
                                        message: format!(
                                            "SDEF SUR={} references undefined surface {}",
                                            sid, sid
                                        ),
                                    });
                                }
                            }
                            SdefVariable::Tr => {
                                let tid = v.round() as u32;
                                if tid > 0 && !transforms.contains_key(&tid) {
                                    diagnostics.push(ValidationError {
                                        code: ErrorCode::UndefinedTransform,
                                        severity: Severity::Error,
                                        category: ValidationCategory::Reference,
                                        line_number: line,
                                        message: format!(
                                            "SDEF TR={} references undefined transform TR{}",
                                            tid, tid
                                        ),
                                    });
                                }
                            }
                            _ => {}
                        },
                        SdefValue::Symbol(sym) => {
                            if param.variable == SdefVariable::Cel {
                                for part in sym.split(':') {
                                    if let Ok(cid) = part.trim().parse::<u32>() {
                                        if cid > 0 && !cell_map.contains_key(&cid) {
                                            diagnostics.push(ValidationError {
                                                code: ErrorCode::UndefinedCell,
                                                severity: Severity::Error,
                                                category: ValidationCategory::Reference,
                                                line_number: line,
                                                message: format!(
                                                    "SDEF CEL={} references undefined cell {}",
                                                    sym, cid
                                                ),
                                            });
                                        }
                                    }
                                }
                            } else if param.variable == SdefVariable::Par {
                                let p = Particle::new(sym.trim());
                                check_particle_in_mode(
                                    &p,
                                    &active_particles,
                                    line,
                                    "SDEF PAR",
                                    &mut diagnostics,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }

            DataCard::Si(si) => {
                if matches!(si.option, SiOption::Histogram | SiOption::ArbitraryPdf) {
                    let mut vals = Vec::new();
                    for e in &si.entries {
                        if let SiEntry::Value(v) = e {
                            vals.push(*v);
                        }
                    }
                    if vals.len() >= 2 && !is_strictly_increasing(&vals) {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::NonMonotonicSourceBins,
                            severity: Severity::Error,
                            category: ValidationCategory::Source,
                            line_number: line,
                            message: format!(
                                "SI{} bin boundaries must be strictly increasing",
                                si.id
                            ),
                        });
                    }
                }
                for e in &si.entries {
                    if let SiEntry::Distribution(d) = e {
                        referenced_dists.push((*d, line, format!("SI{}", si.id)));
                    }
                }
            }

            DataCard::SpSb(sp) => {
                let prefix = if sp.is_bias { "SB" } else { "SP" };
                if let SpSbContent::Probabilities { values, .. } = &sp.content {
                    let mut any_positive = false;
                    for e in values {
                        if let Entry::Value(v) = e {
                            if *v < 0.0 {
                                diagnostics.push(ValidationError {
                                    code: ErrorCode::InvalidSourceProbability,
                                    severity: Severity::Error,
                                    category: ValidationCategory::Source,
                                    line_number: line,
                                    message: format!(
                                        "{}{} contains negative probability value ({})",
                                        prefix, sp.id, v
                                    ),
                                });
                            } else if *v > 0.0 {
                                any_positive = true;
                            }
                        }
                    }
                    if !values.is_empty() && !any_positive {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidSourceProbability,
                            severity: Severity::Error,
                            category: ValidationCategory::Source,
                            line_number: line,
                            message: format!(
                                "{}{} has no positive probability values",
                                prefix, sp.id
                            ),
                        });
                    }
                }
            }

            DataCard::Ds(ds) => {
                for e in &ds.entries {
                    if let SiEntry::Distribution(d) = e {
                        referenced_dists.push((*d, line, format!("DS{}", ds.id)));
                    }
                }
                for (_, d) in &ds.pairs {
                    referenced_dists.push((*d, line, format!("DS{}", ds.id)));
                }
            }

            DataCard::Tally {
                id,
                kind,
                particles,
                targets,
                ..
            } => {
                for p in particles {
                    check_particle_in_mode(
                        p,
                        &active_particles,
                        line,
                        &format!("F{}", id),
                        &mut diagnostics,
                    );
                }
                if matches!(kind, TallyKind::Other(_)) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::TallyTargetKindMismatch,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!(
                            "Tally F{} has invalid last digit {} (must end in 1, 2, 4, 5, 6, 7, or 8)",
                            id,
                            id % 10
                        ),
                    });
                }

                let mut flat_ids = Vec::new();
                for t in targets {
                    match t {
                        TallyTarget::Single(tid) => flat_ids.push(*tid),
                        TallyTarget::Group(g) => flat_ids.extend(g.iter().copied()),
                        TallyTarget::ComplexGroup(_) => {}
                    }
                }

                match kind {
                    TallyKind::F1SurfaceCurrent | TallyKind::F2SurfaceFlux => {
                        for sid in flat_ids {
                            used_surfaces.insert(sid);
                            if !surface_map.contains_key(&sid) {
                                diagnostics.push(ValidationError {
                                    code: ErrorCode::UndefinedSurface,
                                    severity: Severity::Error,
                                    category: ValidationCategory::Reference,
                                    line_number: line,
                                    message: format!(
                                        "Surface tally F{} references undefined surface {}",
                                        id, sid
                                    ),
                                });
                            }
                        }
                    }
                    TallyKind::F4CellFlux
                    | TallyKind::F6EnergyDeposition
                    | TallyKind::F7FissionEnergyDeposition
                    | TallyKind::F8PulseHeight => {
                        for cid in flat_ids {
                            if !cell_map.contains_key(&cid) {
                                diagnostics.push(ValidationError {
                                    code: ErrorCode::UndefinedCell,
                                    severity: Severity::Error,
                                    category: ValidationCategory::Reference,
                                    line_number: line,
                                    message: format!(
                                        "Cell tally F{} references undefined cell {}",
                                        id, cid
                                    ),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }

            DataCard::DetectorTally {
                id,
                particles,
                detectors,
                ..
            } => {
                for p in particles {
                    check_particle_in_mode(
                        p,
                        &active_particles,
                        line,
                        &format!("F{}", id),
                        &mut diagnostics,
                    );
                }
                for det in detectors {
                    let r0 = match det {
                        DetectorEntry::Point { r0, .. } => *r0,
                        DetectorEntry::Ring { r0, .. } => *r0,
                    };
                    if r0 < 0.0 {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidDetectorRadius,
                            severity: Severity::Error,
                            category: ValidationCategory::Tally,
                            line_number: line,
                            message: format!(
                                "Detector tally F{} has negative exclusion radius R0={}",
                                id, r0
                            ),
                        });
                    }
                }
            }

            DataCard::Dose(dose) => {
                if dose.id > 0 && !tallies.contains_key(&dose.id) && !fmeshes.contains_key(&dose.id)
                {
                    let prefix = if dose.is_function { "DF" } else { "DE" };
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!(
                            "{}{} references undefined tally F{}",
                            prefix, dose.id, dose.id
                        ),
                    });
                }
                if !dose.is_function
                    && dose.values.len() >= 2
                    && !is_strictly_increasing(&dose.values)
                {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::NonMonotonicTallyBins,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!(
                            "DE{} energy points must be strictly increasing",
                            dose.id
                        ),
                    });
                }
            }

            DataCard::TallyMultiplier(fm) => {
                if fm.id > 0 && !tallies.contains_key(&fm.id) && !fmeshes.contains_key(&fm.id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("FM{} references undefined tally F{}", fm.id, fm.id),
                    });
                }
                for bin in &fm.bins {
                    if let FmBin::MultiplierSet { material_id, .. } = bin {
                        if *material_id > 0 {
                            let m_u32 = *material_id as u32;
                            used_materials.insert(m_u32);
                            if !materials.contains_key(&m_u32) {
                                diagnostics.push(ValidationError {
                                    code: ErrorCode::UndefinedMaterial,
                                    severity: Severity::Error,
                                    category: ValidationCategory::Reference,
                                    line_number: line,
                                    message: format!(
                                        "FM{} references undefined material M{}",
                                        fm.id, m_u32
                                    ),
                                });
                            }
                        }
                    }
                }
            }

            DataCard::TallySegment { id, surfaces, .. } => {
                if *id > 0 && !tallies.contains_key(id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("FS{} references undefined tally F{}", id, id),
                    });
                }
                for &s in surfaces {
                    let sid = s.unsigned_abs();
                    used_surfaces.insert(sid);
                    if sid == 0 || !surface_map.contains_key(&sid) {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::UndefinedSurface,
                            severity: Severity::Error,
                            category: ValidationCategory::Reference,
                            line_number: line,
                            message: format!(
                                "Tally segment card FS{} references undefined surface {}",
                                id, sid
                            ),
                        });
                    }
                }
            }

            DataCard::SegmentDivisor { id, .. } => {
                if *id > 0 && !tallies.contains_key(id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("SD{} references undefined tally F{}", id, id),
                    });
                }
            }

            DataCard::TallyEnergy { id, bins, .. } => {
                if *id > 0 && !tallies.contains_key(id) && !fmeshes.contains_key(id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("E{} references undefined tally F{}", id, id),
                    });
                }
                let vals: Vec<f64> = bins.iter().filter_map(|e| e.as_f64()).collect();
                if vals.len() >= 2 && !is_strictly_increasing(&vals) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::NonMonotonicTallyBins,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("E{} energy bins must be strictly increasing", id),
                    });
                }
            }

            DataCard::TallyTime { id, bins, .. } => {
                if *id > 0 && !tallies.contains_key(id) && !fmeshes.contains_key(id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("T{} references undefined tally F{}", id, id),
                    });
                }
                let vals: Vec<f64> = bins.iter().filter_map(|e| e.as_f64()).collect();
                if vals.len() >= 2 && !is_strictly_increasing(&vals) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::NonMonotonicTallyBins,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("T{} time bins must be strictly increasing", id),
                    });
                }
            }

            DataCard::TallyCosine { id, bins, .. } => {
                if *id > 0 && !tallies.contains_key(id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("C{} references undefined tally F{}", id, id),
                    });
                }
                let vals: Vec<f64> = bins.iter().filter_map(|e| e.as_f64()).collect();
                for &v in &vals {
                    if !(-1.0..=1.0).contains(&v) {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidCosineBinRange,
                            severity: Severity::Error,
                            category: ValidationCategory::Tally,
                            line_number: line,
                            message: format!(
                                "C{} cosine bin value {} is outside valid range [-1.0, 1.0]",
                                id, v
                            ),
                        });
                    }
                }
                if vals.len() >= 2 && !is_strictly_increasing(&vals) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::NonMonotonicTallyBins,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("C{} cosine bins must be strictly increasing", id),
                    });
                }
            }

            DataCard::TallyComment { id, .. } => {
                if *id > 0 && !tallies.contains_key(id) && !fmeshes.contains_key(id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("FC{} references undefined tally F{}", id, id),
                    });
                }
            }

            DataCard::Fmesh(fm) => {
                for p in &fm.particles {
                    check_particle_in_mode(
                        p,
                        &active_particles,
                        line,
                        &format!("FMESH{}", fm.id),
                        &mut diagnostics,
                    );
                }
                validate_fmesh(fm, line, &mut diagnostics);
            }

            DataCard::Importance { particles, values } => {
                let targets: Vec<Particle> = if particles.is_empty() {
                    vec![Particle::new("n")]
                } else {
                    particles.clone()
                };
                for p in &targets {
                    check_particle_in_mode(p, &active_particles, line, "IMP", &mut diagnostics);
                }
                if !input.cells.is_empty() && values.len() != input.cells.len() {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::ImportanceCardLengthMismatch,
                        severity: Severity::Error,
                        category: ValidationCategory::Cell,
                        line_number: line,
                        message: format!(
                            "IMP data card has {} entries, but problem defines {} cells",
                            values.len(),
                            input.cells.len()
                        ),
                    });
                }
                for (c_idx, entry) in values.iter().enumerate() {
                    if let Entry::Value(v) = entry {
                        if *v < 0.0 {
                            diagnostics.push(ValidationError {
                                code: ErrorCode::InvalidCellParameterValue,
                                severity: Severity::Error,
                                category: ValidationCategory::Cell,
                                line_number: line,
                                message: format!("IMP data card contains negative value ({})", v),
                            });
                        }
                        if let Some(cell) = input.cells.get(c_idx) {
                            for p in &targets {
                                cell_has_imp.entry(p.clone()).or_default().insert(cell.id);
                                if *v > 0.0 {
                                    cell_nonzero_imp.insert(p.clone(), true);
                                }
                            }
                        }
                    }
                }
            }

            DataCard::WeightWindowBounds {
                group_id,
                particles,
                lower_bounds,
            } => {
                for p in particles {
                    check_particle_in_mode(
                        p,
                        &active_particles,
                        line,
                        &format!("WWN{}", group_id),
                        &mut diagnostics,
                    );
                }
                let has_ww_mesh = input
                    .data_cards
                    .iter()
                    .any(|c| matches!(c, DataCard::WeightWindowMesh { .. }));
                if !has_ww_mesh
                    && !input.cells.is_empty()
                    && lower_bounds.len() != input.cells.len()
                {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::ImportanceCardLengthMismatch,
                        severity: Severity::Error,
                        category: ValidationCategory::Cell,
                        line_number: line,
                        message: format!(
                            "WWN{} card has {} entries, but problem defines {} cells",
                            group_id,
                            lower_bounds.len(),
                            input.cells.len()
                        ),
                    });
                }
            }

            DataCard::WeightWindowEnergy { particles, .. }
            | DataCard::WeightWindowParams { particles, .. }
            | DataCard::WeightWindowGeneratorEnergy { particles, .. }
            | DataCard::Physics { particles, .. }
            | DataCard::Cutoff { particles, .. } => {
                for p in particles {
                    check_particle_in_mode(
                        p,
                        &active_particles,
                        line,
                        "Physics/VR card",
                        &mut diagnostics,
                    );
                }
            }

            DataCard::WeightWindowGenerator {
                tally_id,
                reference_cell,
                ..
            } => {
                if !tallies.contains_key(tally_id) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("WWG references undefined tally F{}", tally_id),
                    });
                }
                if *reference_cell > 0 && !cell_map.contains_key(reference_cell) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::UndefinedCell,
                        severity: Severity::Error,
                        category: ValidationCategory::Reference,
                        line_number: line,
                        message: format!(
                            "WWG references undefined reference cell {}",
                            reference_cell
                        ),
                    });
                }
            }

            DataCard::Transform { id, values, .. } => {
                let len = values.len();
                if !matches!(len, 3 | 5 | 6 | 9 | 12 | 13) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidTransformCardLength,
                        severity: Severity::Error,
                        category: ValidationCategory::DataCard,
                        line_number: line,
                        message: format!(
                            "TR{} has {} parameters (expected 3, 5, 6, 9, 12, or 13)",
                            id, len
                        ),
                    });
                }
            }

            DataCard::Nps { count, .. } => {
                if *count == 0 {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidNpsCount,
                        severity: Severity::Error,
                        category: ValidationCategory::DataCard,
                        line_number: line,
                        message: "NPS history count must be greater than 0".to_string(),
                    });
                }
            }

            DataCard::Kcode {
                nsrck, ikz, kct, ..
            } => {
                if *nsrck == Some(0) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidKcodeParameters,
                        severity: Severity::Error,
                        category: ValidationCategory::DataCard,
                        line_number: line,
                        message: "KCODE nsrck (histories per cycle) must be greater than 0"
                            .to_string(),
                    });
                }
                if let (Some(skip), Some(total)) = (ikz, kct) {
                    if *total == 0 || *skip >= *total {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidKcodeParameters,
                            severity: Severity::Error,
                            category: ValidationCategory::DataCard,
                            line_number: line,
                            message: format!(
                                "KCODE total cycles (kct={}) must be greater than skipped cycles (ikz={})",
                                total, skip
                            ),
                        });
                    }
                }
            }

            DataCard::Generic {
                name, particles, ..
            } => {
                for p in particles {
                    check_particle_in_mode(
                        p,
                        &active_particles,
                        line,
                        &name.to_uppercase(),
                        &mut diagnostics,
                    );
                }
                let alpha_prefix: String = name
                    .trim_start_matches(['*', '+'])
                    .chars()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .collect::<String>()
                    .to_lowercase();
                if !KNOWN_MCNP_DATA_PREFIXES.contains(&alpha_prefix.as_str()) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::UnrecognizedDataCard,
                        severity: Severity::Warning,
                        category: ValidationCategory::DataCard,
                        line_number: line,
                        message: format!("Unrecognized data card mnemonic `{}`", name),
                    });
                }
            }

            _ => {}
        }
    }

    // ========================================================================
    // 6. Cross-Card Source, Dose, Importance, and Unused Entity Checks
    // ========================================================================
    // Check Source presence (SDEF vs KCODE vs SSR)
    if !input.data_cards.is_empty() {
        let has_sdef = !sdef_indices.is_empty();
        let has_kcode = !kcode_indices.is_empty();
        let has_ssr = input.data_cards.iter().any(|c| {
            matches!(c, DataCard::Generic { name, .. } if name.eq_ignore_ascii_case("ssr"))
        });

        if !has_sdef && !has_kcode && !has_ssr {
            diagnostics.push(ValidationError {
                code: ErrorCode::MissingSourceDefinition,
                severity: Severity::Error,
                category: ValidationCategory::Source,
                line_number: None,
                message: "No source definition card (SDEF, KCODE, or SSR) found in data block"
                    .to_string(),
            });
        } else if has_sdef && has_kcode {
            diagnostics.push(ValidationError {
                code: ErrorCode::ConflictingSourceDefinitions,
                severity: Severity::Error,
                category: ValidationCategory::Source,
                line_number: data_line(kcode_indices[0]),
                message: "Both SDEF (fixed source) and KCODE (criticality source) are specified"
                    .to_string(),
            });
        }

        if has_kcode && ksrc_indices.is_empty() && !has_sdef {
            diagnostics.push(ValidationError {
                code: ErrorCode::MissingKsrcForKcode,
                severity: Severity::Warning,
                category: ValidationCategory::Source,
                line_number: data_line(kcode_indices[0]),
                message: "KCODE is specified without a KSRC initial source points card".to_string(),
            });
        }
    }

    // Check referenced source distributions (Dn)
    for (dist_id, line, context) in referenced_dists {
        if dist_id == 0 {
            continue;
        }
        let has_si = si_cards.contains_key(&dist_id);
        let has_sp = sp_cards.contains_key(&dist_id);
        let has_ds = ds_cards.contains_key(&dist_id);
        if !has_si && !has_sp && !has_ds {
            diagnostics.push(ValidationError {
                code: ErrorCode::UndefinedSourceDistribution,
                severity: Severity::Error,
                category: ValidationCategory::Source,
                line_number: line,
                message: format!(
                    "{} references undefined source distribution D{}",
                    context, dist_id
                ),
            });
        }
    }

    // Check SI / SP / SB pairing and lengths
    let all_dist_ids: HashSet<u32> = si_cards
        .keys()
        .chain(sp_cards.keys())
        .chain(sb_cards.keys())
        .copied()
        .collect();
    let mut sorted_dist_ids: Vec<u32> = all_dist_ids.into_iter().collect();
    sorted_dist_ids.sort_unstable();

    for dist_id in sorted_dist_ids {
        let si_entry = si_cards.get(&dist_id);
        let sp_entry = sp_cards.get(&dist_id);
        let sb_entry = sb_cards.get(&dist_id);

        if let Some(&(si_idx, si)) = si_entry {
            if sp_entry.is_none() {
                diagnostics.push(ValidationError {
                    code: ErrorCode::IncompleteSourceDistribution,
                    severity: Severity::Error,
                    category: ValidationCategory::Source,
                    line_number: data_line(si_idx),
                    message: format!(
                        "SI{} is defined without a corresponding SP{} card",
                        dist_id, dist_id
                    ),
                });
            } else if let Some(&(sp_idx, sp)) = sp_entry {
                if let SpSbContent::Probabilities { option, values } = &sp.content {
                    if *option != SpOption::Volume {
                        if values.len() != si.entries.len() {
                            diagnostics.push(ValidationError {
                                code: ErrorCode::SourceDistributionLengthMismatch,
                                severity: Severity::Error,
                                category: ValidationCategory::Source,
                                line_number: data_line(sp_idx),
                                message: format!(
                                    "SP{} has {} entries, but SI{} has {} entries",
                                    dist_id,
                                    values.len(),
                                    dist_id,
                                    si.entries.len()
                                ),
                            });
                        } else if si.option == SiOption::Histogram {
                            if let Some(Entry::Value(first_v)) = values.first() {
                                if *first_v != 0.0 {
                                    diagnostics.push(ValidationError {
                                        code: ErrorCode::SourceDistributionLengthMismatch,
                                        severity: Severity::Error,
                                        category: ValidationCategory::Source,
                                        line_number: data_line(sp_idx),
                                        message: format!(
                                            "SP{} for histogram SI{} must start with 0.0 as the first entry (got {})",
                                            dist_id, dist_id, first_v
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        } else if let Some(&(sp_idx, sp)) = sp_entry {
            // SP without SI is only valid if SP is an AnalyticFunction or Volume option
            if let SpSbContent::Probabilities { option, .. } = &sp.content {
                if *option != SpOption::Volume {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::IncompleteSourceDistribution,
                        severity: Severity::Error,
                        category: ValidationCategory::Source,
                        line_number: data_line(sp_idx),
                        message: format!(
                            "SP{} with explicit probabilities requires a corresponding SI{} card",
                            dist_id, dist_id
                        ),
                    });
                }
            }
        }

        if let Some(&(sb_idx, sb)) = sb_entry {
            if sp_entry.is_none() {
                diagnostics.push(ValidationError {
                    code: ErrorCode::IncompleteSourceDistribution,
                    severity: Severity::Error,
                    category: ValidationCategory::Source,
                    line_number: data_line(sb_idx),
                    message: format!(
                        "SB{} is defined without a corresponding SP{} card",
                        dist_id, dist_id
                    ),
                });
            }
            if let (Some(&(_, si)), SpSbContent::Probabilities { values, .. }) =
                (si_entry, &sb.content)
            {
                if values.len() != si.entries.len() {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::SourceDistributionLengthMismatch,
                        severity: Severity::Error,
                        category: ValidationCategory::Source,
                        line_number: data_line(sb_idx),
                        message: format!(
                            "SB{} has {} entries, but SI{} has {} entries",
                            dist_id,
                            values.len(),
                            dist_id,
                            si.entries.len()
                        ),
                    });
                }
            }
        }
    }

    // Check DEn / DFn pairing
    let all_dose_ids: HashSet<u32> = de_cards.keys().chain(df_cards.keys()).copied().collect();
    let mut sorted_dose_ids: Vec<u32> = all_dose_ids.into_iter().collect();
    sorted_dose_ids.sort_unstable();

    for dose_id in sorted_dose_ids {
        match (de_cards.get(&dose_id), df_cards.get(&dose_id)) {
            (Some(&(de_idx, _)), None) => {
                diagnostics.push(ValidationError {
                    code: ErrorCode::IncompleteDoseCards,
                    severity: Severity::Error,
                    category: ValidationCategory::Tally,
                    line_number: data_line(de_idx),
                    message: format!(
                        "DE{} is defined without a corresponding DF{} card",
                        dose_id, dose_id
                    ),
                });
            }
            (None, Some(&(df_idx, df))) => {
                let has_ic = df.options.iter().any(|(k, _)| k.eq_ignore_ascii_case("ic"));
                if !has_ic && dose_id != 0 && !de_cards.contains_key(&0) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::IncompleteDoseCards,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: data_line(df_idx),
                        message: format!(
                            "DF{} is defined without a corresponding DE{} (or DE0) card",
                            dose_id, dose_id
                        ),
                    });
                }
            }
            (Some(&(_, de)), Some(&(df_idx, df))) => {
                if !df.values.is_empty() && de.values.len() != df.values.len() {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::IncompleteDoseCards,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: data_line(df_idx),
                        message: format!(
                            "DE{} has {} energy points, but DF{} has {} dose values",
                            dose_id,
                            de.values.len(),
                            dose_id,
                            df.values.len()
                        ),
                    });
                }
            }
            (None, None) => {}
        }
    }

    // Check Cell Importance coverage for active particles (when IMP is used anywhere in the input)
    let any_imp_specified = !cell_has_imp.is_empty();
    if any_imp_specified && !input.cells.is_empty() {
        let mut sorted_particles: Vec<&Particle> = active_particles.iter().collect();
        sorted_particles.sort_by(|a, b| a.0.cmp(&b.0));

        for p in sorted_particles {
            let covered = cell_has_imp.get(p);
            let count = covered.map(|s| s.len()).unwrap_or(0);
            if count == 0 {
                diagnostics.push(ValidationError {
                    code: ErrorCode::MissingCellImportance,
                    severity: Severity::Error,
                    category: ValidationCategory::Cell,
                    line_number: None,
                    message: format!(
                        "No cell importance (IMP:{}) specified for active transport particle `{}`",
                        p.0, p.0
                    ),
                });
            } else {
                if !cell_nonzero_imp.get(p).copied().unwrap_or(false) {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::MissingCellImportance,
                        severity: Severity::Error,
                        category: ValidationCategory::Cell,
                        line_number: None,
                        message: format!(
                            "All cells have zero importance (IMP:{}=0) for particle `{}`",
                            p.0, p.0
                        ),
                    });
                }
                for (c_idx, cell) in input.cells.iter().enumerate() {
                    if !covered.unwrap().contains(&cell.id) {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::MissingCellImportance,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: cell_line(c_idx, cell.id),
                            message: format!(
                                "Cell {} is missing importance (IMP:{}) for particle `{}`",
                                cell.id, p.0, p.0
                            ),
                        });
                    }
                }
            }
        }
    }

    // Check unused surfaces and materials (Warnings)
    for (idx, surf) in input.surfaces.iter().enumerate() {
        if !used_surfaces.contains(&surf.id) {
            diagnostics.push(ValidationError {
                code: ErrorCode::UnusedSurface,
                severity: Severity::Warning,
                category: ValidationCategory::Reference,
                line_number: surf_line(idx, surf.id),
                message: format!(
                    "Surface {} is defined but never referenced by any cell, tally, or source",
                    surf.id
                ),
            });
        }
    }

    for (idx, card) in input.data_cards.iter().enumerate() {
        if let DataCard::Material { id, .. } = card {
            if !used_materials.contains(id) {
                diagnostics.push(ValidationError {
                    code: ErrorCode::UnusedMaterial,
                    severity: Severity::Warning,
                    category: ValidationCategory::Reference,
                    line_number: data_line(idx),
                    message: format!(
                        "Material M{} is defined but never referenced by any cell or tally multiplier",
                        id
                    ),
                });
            }
        }
    }

    diagnostics
}

// ============================================================================
// Helper Validation Functions
// ============================================================================

fn validate_surface_kind_and_params(
    surf: &crate::ast::SurfaceCard,
    line: Option<usize>,
    diagnostics: &mut Vec<ValidationError>,
) {
    let n = surf.params.len();
    let p = &surf.params;

    let check_count = |expected: &[usize], kind_name: &str, diag: &mut Vec<ValidationError>| -> bool {
        if !expected.contains(&n) {
            let exp_str = expected
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" or ");
            diag.push(ValidationError {
                code: ErrorCode::InvalidSurfaceParamCount,
                severity: Severity::Error,
                category: ValidationCategory::Surface,
                line_number: line,
                message: format!(
                    "Surface {} ({}) has {} parameter(s), expected {}",
                    surf.id, kind_name, n, exp_str
                ),
            });
            false
        } else {
            true
        }
    };

    let check_pos_radius = |r: f64, label: &str, diag: &mut Vec<ValidationError>| {
        if r <= 0.0 {
            diag.push(ValidationError {
                code: ErrorCode::InvalidSurfaceGeometry,
                severity: Severity::Error,
                category: ValidationCategory::Surface,
                line_number: line,
                message: format!(
                    "Surface {} has non-positive {} ({})",
                    surf.id, label, r
                ),
            });
        }
    };

    let norm_sq = |dx: f64, dy: f64, dz: f64| dx * dx + dy * dy + dz * dz;

    match &surf.kind {
        SurfaceKind::Custom(name) => {
            diagnostics.push(ValidationError {
                code: ErrorCode::UnknownSurfaceMnemonic,
                severity: Severity::Error,
                category: ValidationCategory::Surface,
                line_number: line,
                message: format!("Surface {} has unrecognized mnemonic `{}`", surf.id, name),
            });
        }
        SurfaceKind::PX | SurfaceKind::PY | SurfaceKind::PZ => {
            check_count(&[1], &format!("{:?}", surf.kind), diagnostics);
        }
        SurfaceKind::P => {
            if check_count(&[4, 9], "P", diagnostics) && n == 4 && norm_sq(p[0], p[1], p[2]) == 0.0
            {
                diagnostics.push(ValidationError {
                    code: ErrorCode::InvalidSurfaceGeometry,
                    severity: Severity::Error,
                    category: ValidationCategory::Surface,
                    line_number: line,
                    message: format!(
                        "Surface {} (P) has zero normal vector (A=B=C=0)",
                        surf.id
                    ),
                });
            }
        }
        SurfaceKind::SO | SurfaceKind::CX | SurfaceKind::CY | SurfaceKind::CZ => {
            if check_count(&[1], &format!("{:?}", surf.kind), diagnostics) {
                check_pos_radius(p[0], "radius R", diagnostics);
            }
        }
        SurfaceKind::SX | SurfaceKind::SY | SurfaceKind::SZ => {
            if check_count(&[2], &format!("{:?}", surf.kind), diagnostics) {
                check_pos_radius(p[1], "radius R", diagnostics);
            }
        }
        SurfaceKind::C_X | SurfaceKind::C_Y | SurfaceKind::C_Z => {
            if check_count(&[3], &format!("{:?}", surf.kind), diagnostics) {
                check_pos_radius(p[2], "radius R", diagnostics);
            }
        }
        SurfaceKind::S | SurfaceKind::SPH => {
            if check_count(&[4], &format!("{:?}", surf.kind), diagnostics) {
                check_pos_radius(p[3], "radius R", diagnostics);
            }
        }
        SurfaceKind::KX | SurfaceKind::KY | SurfaceKind::KZ => {
            if check_count(&[2, 3], &format!("{:?}", surf.kind), diagnostics) {
                check_pos_radius(p[1], "cone slope squared t^2", diagnostics);
            }
        }
        SurfaceKind::K_X | SurfaceKind::K_Y | SurfaceKind::K_Z => {
            if check_count(&[4, 5], &format!("{:?}", surf.kind), diagnostics) {
                check_pos_radius(p[3], "cone slope squared t^2", diagnostics);
            }
        }
        SurfaceKind::SQ | SurfaceKind::GQ => {
            check_count(&[10], &format!("{:?}", surf.kind), diagnostics);
        }
        SurfaceKind::TX | SurfaceKind::TY | SurfaceKind::TZ => {
            if check_count(&[6], &format!("{:?}", surf.kind), diagnostics) {
                check_pos_radius(p[3], "torus major radius A", diagnostics);
                check_pos_radius(p[4], "torus semi-axis B", diagnostics);
                check_pos_radius(p[5], "torus semi-axis C", diagnostics);
            }
        }
        SurfaceKind::X | SurfaceKind::Y | SurfaceKind::Z => {
            if check_count(&[2, 4, 6], &format!("{:?}", surf.kind), diagnostics) {
                for pair in p.chunks_exact(2) {
                    if pair[1] < 0.0 {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidSurfaceGeometry,
                            severity: Severity::Error,
                            category: ValidationCategory::Surface,
                            line_number: line,
                            message: format!(
                                "Surface {} ({:?}) has negative radial coordinate r={}",
                                surf.id, surf.kind, pair[1]
                            ),
                        });
                    }
                }
            }
        }
        SurfaceKind::RPP => {
            if check_count(&[6], "RPP", diagnostics) {
                if p[0] >= p[1] {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidSurfaceGeometry,
                        severity: Severity::Error,
                        category: ValidationCategory::Surface,
                        line_number: line,
                        message: format!(
                            "Surface {} (RPP) has xmin ({}) >= xmax ({})",
                            surf.id, p[0], p[1]
                        ),
                    });
                }
                if p[2] >= p[3] {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidSurfaceGeometry,
                        severity: Severity::Error,
                        category: ValidationCategory::Surface,
                        line_number: line,
                        message: format!(
                            "Surface {} (RPP) has ymin ({}) >= ymax ({})",
                            surf.id, p[2], p[3]
                        ),
                    });
                }
                if p[4] >= p[5] {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidSurfaceGeometry,
                        severity: Severity::Error,
                        category: ValidationCategory::Surface,
                        line_number: line,
                        message: format!(
                            "Surface {} (RPP) has zmin ({}) >= zmax ({})",
                            surf.id, p[4], p[5]
                        ),
                    });
                }
            }
        }
        SurfaceKind::BOX | SurfaceKind::WED => {
            if check_count(&[12], &format!("{:?}", surf.kind), diagnostics) {
                for (vec_idx, chunk) in p[3..12].chunks_exact(3).enumerate() {
                    if norm_sq(chunk[0], chunk[1], chunk[2]) == 0.0 {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidSurfaceGeometry,
                            severity: Severity::Error,
                            category: ValidationCategory::Surface,
                            line_number: line,
                            message: format!(
                                "Surface {} ({:?}) edge vector v{} has zero length",
                                surf.id,
                                surf.kind,
                                vec_idx + 1
                            ),
                        });
                    }
                }
            }
        }
        SurfaceKind::RCC => {
            if check_count(&[7], "RCC", diagnostics) {
                if norm_sq(p[3], p[4], p[5]) == 0.0 {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidSurfaceGeometry,
                        severity: Severity::Error,
                        category: ValidationCategory::Surface,
                        line_number: line,
                        message: format!("Surface {} (RCC) height vector has zero length", surf.id),
                    });
                }
                check_pos_radius(p[6], "radius R", diagnostics);
            }
        }
        SurfaceKind::TRC => {
            if check_count(&[8], "TRC", diagnostics) {
                if norm_sq(p[3], p[4], p[5]) == 0.0 {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidSurfaceGeometry,
                        severity: Severity::Error,
                        category: ValidationCategory::Surface,
                        line_number: line,
                        message: format!("Surface {} (TRC) height vector has zero length", surf.id),
                    });
                }
                check_pos_radius(p[6], "lower base radius R1", diagnostics);
                check_pos_radius(p[7], "upper base radius R2", diagnostics);
            }
        }
        SurfaceKind::RHP | SurfaceKind::HEX => {
            if check_count(&[9, 15], &format!("{:?}", surf.kind), diagnostics)
                && norm_sq(p[3], p[4], p[5]) == 0.0
            {
                diagnostics.push(ValidationError {
                    code: ErrorCode::InvalidSurfaceGeometry,
                    severity: Severity::Error,
                    category: ValidationCategory::Surface,
                    line_number: line,
                    message: format!(
                        "Surface {} ({:?}) height vector has zero length",
                        surf.id, surf.kind
                    ),
                });
            }
        }
        SurfaceKind::REC => {
            if check_count(&[10, 12], "REC", diagnostics) && norm_sq(p[3], p[4], p[5]) == 0.0 {
                diagnostics.push(ValidationError {
                    code: ErrorCode::InvalidSurfaceGeometry,
                    severity: Severity::Error,
                    category: ValidationCategory::Surface,
                    line_number: line,
                    message: format!("Surface {} (REC) height vector has zero length", surf.id),
                });
            }
        }
        SurfaceKind::ELL => {
            if check_count(&[7], "ELL", diagnostics) && p[6] == 0.0 {
                diagnostics.push(ValidationError {
                    code: ErrorCode::InvalidSurfaceGeometry,
                    severity: Severity::Error,
                    category: ValidationCategory::Surface,
                    line_number: line,
                    message: format!("Surface {} (ELL) major/minor radius Rm cannot be 0", surf.id),
                });
            }
        }
        SurfaceKind::ARB => {
            check_count(&[30], "ARB", diagnostics);
        }
    }
}

fn macrobody_max_facets(kind: &SurfaceKind) -> Option<u32> {
    match kind {
        SurfaceKind::BOX | SurfaceKind::RPP | SurfaceKind::ARB => Some(6),
        SurfaceKind::RCC | SurfaceKind::REC | SurfaceKind::TRC => Some(3),
        SurfaceKind::SPH | SurfaceKind::ELL => Some(1),
        SurfaceKind::RHP | SurfaceKind::HEX => Some(8),
        SurfaceKind::WED => Some(5),
        _ => None,
    }
}

fn validate_geometry_expr(
    cell_id: u32,
    line: Option<usize>,
    expr: &GeometryExpr,
    surface_map: &HashMap<u32, (usize, &crate::ast::SurfaceCard)>,
    cell_map: &HashMap<u32, (usize, &CellCard)>,
    used_surfaces: &mut HashSet<u32>,
    diagnostics: &mut Vec<ValidationError>,
) {
    match expr {
        GeometryExpr::Surface(s) => {
            let sid = s.unsigned_abs();
            used_surfaces.insert(sid);
            if !surface_map.contains_key(&sid) {
                diagnostics.push(ValidationError {
                    code: ErrorCode::UndefinedSurface,
                    severity: Severity::Error,
                    category: ValidationCategory::Reference,
                    line_number: line,
                    message: format!("Cell {} references undefined surface {}", cell_id, sid),
                });
            }
        }
        GeometryExpr::Facet { surface, facet } => {
            let sid = surface.unsigned_abs();
            used_surfaces.insert(sid);
            if let Some((_, surf)) = surface_map.get(&sid) {
                match macrobody_max_facets(&surf.kind) {
                    Some(max_f) => {
                        if *facet == 0 || *facet > max_f {
                            diagnostics.push(ValidationError {
                                code: ErrorCode::InvalidMacrobodyFacet,
                                severity: Severity::Error,
                                category: ValidationCategory::Reference,
                                line_number: line,
                                message: format!(
                                    "Cell {} references invalid facet {}.{} on {:?} macrobody (valid facets: 1..={})",
                                    cell_id, sid, facet, surf.kind, max_f
                                ),
                            });
                        }
                    }
                    None => {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidMacrobodyFacet,
                            severity: Severity::Error,
                            category: ValidationCategory::Reference,
                            line_number: line,
                            message: format!(
                                "Cell {} uses facet notation {}.{} on non-macrobody surface ({:?})",
                                cell_id, sid, facet, surf.kind
                            ),
                        });
                    }
                }
            } else {
                diagnostics.push(ValidationError {
                    code: ErrorCode::UndefinedSurface,
                    severity: Severity::Error,
                    category: ValidationCategory::Reference,
                    line_number: line,
                    message: format!(
                        "Cell {} references facet {}.{} of undefined surface {}",
                        cell_id, sid, facet, sid
                    ),
                });
            }
        }
        GeometryExpr::ComplementCell(target_id) => {
            if !cell_map.contains_key(target_id) {
                diagnostics.push(ValidationError {
                    code: ErrorCode::UndefinedCell,
                    severity: Severity::Error,
                    category: ValidationCategory::Reference,
                    line_number: line,
                    message: format!(
                        "Cell {} complement #{} references undefined cell {}",
                        cell_id, target_id, target_id
                    ),
                });
            }
        }
        GeometryExpr::Complement(inner) => {
            validate_geometry_expr(
                cell_id,
                line,
                inner,
                surface_map,
                cell_map,
                used_surfaces,
                diagnostics,
            );
        }
        GeometryExpr::Intersection(terms) | GeometryExpr::Union(terms) => {
            for t in terms {
                validate_geometry_expr(
                    cell_id,
                    line,
                    t,
                    surface_map,
                    cell_map,
                    used_surfaces,
                    diagnostics,
                );
            }
        }
    }
}

fn collect_complement_targets(expr: &GeometryExpr, out: &mut Vec<u32>) {
    match expr {
        GeometryExpr::ComplementCell(id) => out.push(*id),
        GeometryExpr::Complement(inner) => collect_complement_targets(inner, out),
        GeometryExpr::Intersection(terms) | GeometryExpr::Union(terms) => {
            for t in terms {
                collect_complement_targets(t, out);
            }
        }
        _ => {}
    }
}

fn check_complement_cell_cycles(
    cell_map: &HashMap<u32, (usize, &CellCard)>,
    spans: &CardLineSpans,
    diagnostics: &mut Vec<ValidationError>,
) {
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&id, &(_, cell)) in cell_map {
        let mut targets = Vec::new();
        match &cell.definition {
            CellDefinition::Normal { geometry, .. } => {
                collect_complement_targets(geometry, &mut targets);
            }
            CellDefinition::LikeBut { base_cell_id } => {
                targets.push(*base_cell_id);
            }
        }
        adj.insert(id, targets);
    }

    // 0 = unvisited, 1 = visiting, 2 = visited
    let mut state: HashMap<u32, u8> = HashMap::new();
    let mut sorted_ids: Vec<u32> = cell_map.keys().copied().collect();
    sorted_ids.sort_unstable();

    fn dfs(
        node: u32,
        adj: &HashMap<u32, Vec<u32>>,
        state: &mut HashMap<u32, u8>,
        cell_map: &HashMap<u32, (usize, &CellCard)>,
        spans: &CardLineSpans,
        diagnostics: &mut Vec<ValidationError>,
    ) {
        state.insert(node, 1);
        if let Some(neighbors) = adj.get(&node) {
            for &next in neighbors {
                match state.get(&next).copied().unwrap_or(0) {
                    0 => dfs(next, adj, state, cell_map, spans, diagnostics),
                    1 => {
                        let line = cell_map
                            .get(&node)
                            .and_then(|(idx, _)| spans.cell_card_lines.get(*idx).copied())
                            .or_else(|| spans.cell_lines.get(&node).copied());
                        diagnostics.push(ValidationError {
                            code: ErrorCode::CircularCellReference,
                            severity: Severity::Error,
                            category: ValidationCategory::Reference,
                            line_number: line,
                            message: format!(
                                "Circular cell reference detected between Cell {} and Cell #{}",
                                node, next
                            ),
                        });
                    }
                    _ => {}
                }
            }
        }
        state.insert(node, 2);
    }

    for id in sorted_ids {
        if state.get(&id).copied().unwrap_or(0) == 0 {
            dfs(id, &adj, &mut state, cell_map, spans, diagnostics);
        }
    }
}

fn extract_fill_universes(values: &[String], out: &mut Vec<i32>) {
    if values.is_empty() {
        return;
    }
    // Check if lattice range notation `i1:i2 j1:j2 k1:k2 u1 u2 ...` is used
    let has_colon = values.iter().take(3).any(|tok| tok.contains(':'));
    let start_idx = if has_colon {
        values.iter().take_while(|tok| tok.contains(':')).count()
    } else {
        0
    };

    for tok in &values[start_idx..] {
        if tok.starts_with('(') {
            // Transformation group `(dx dy dz ...)` or `(TRn)`
            continue;
        }
        // Skip repeat shortcuts like `3r`
        let lower = tok.to_lowercase();
        if lower.ends_with('r') && lower[..lower.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if let Ok(u) = tok.parse::<i32>() {
            out.push(u.abs());
        }
        // If not lattice array, only the first token is the universe ID (subsequent non-paren tokens might be inline transform)
        if !has_colon {
            break;
        }
    }
}

fn check_universe_cycles(
    univ_adj: &HashMap<i32, Vec<(i32, u32, Option<usize>)>>,
    diagnostics: &mut Vec<ValidationError>,
) {
    let mut state: HashMap<i32, u8> = HashMap::new();
    let mut sorted_u: Vec<i32> = univ_adj.keys().copied().collect();
    sorted_u.sort_unstable();

    fn dfs_u(
        u: i32,
        adj: &HashMap<i32, Vec<(i32, u32, Option<usize>)>>,
        state: &mut HashMap<i32, u8>,
        diagnostics: &mut Vec<ValidationError>,
    ) {
        state.insert(u, 1);
        if let Some(edges) = adj.get(&u) {
            for &(next_u, cell_id, line) in edges {
                match state.get(&next_u).copied().unwrap_or(0) {
                    0 => dfs_u(next_u, adj, state, diagnostics),
                    1 => {
                        diagnostics.push(ValidationError {
                            code: ErrorCode::CircularUniverseFill,
                            severity: Severity::Error,
                            category: ValidationCategory::Reference,
                            line_number: line,
                            message: format!(
                                "Circular universe FILL detected: Cell {} (in U={}) fills U={}",
                                cell_id, u, next_u
                            ),
                        });
                    }
                    _ => {}
                }
            }
        }
        state.insert(u, 2);
    }

    for u in sorted_u {
        if state.get(&u).copied().unwrap_or(0) == 0 {
            dfs_u(u, univ_adj, &mut state, diagnostics);
        }
    }
}

fn check_particle_in_mode(
    particle: &Particle,
    active_particles: &HashSet<Particle>,
    line: Option<usize>,
    context: &str,
    diagnostics: &mut Vec<ValidationError>,
) {
    if !active_particles.is_empty() && !active_particles.contains(particle) {
        diagnostics.push(ValidationError {
            code: ErrorCode::ParticleNotInMode,
            severity: Severity::Warning,
            category: ValidationCategory::Mode,
            line_number: line,
            message: format!(
                "{} specifies particle `{}` which is not listed on the MODE card",
                context, particle.0
            ),
        });
    }
}

fn is_strictly_increasing(vals: &[f64]) -> bool {
    vals.windows(2).all(|w| w[0] < w[1])
}

fn is_valid_zaid(zaid: &str) -> bool {
    let base = zaid.split('.').next().unwrap_or("");
    !base.is_empty() && base.len() <= 7 && base.chars().all(|c| c.is_ascii_digit())
}

fn validate_fmesh(
    fm: &crate::ast::FmeshCard,
    line: Option<usize>,
    diagnostics: &mut Vec<ValidationError>,
) {
    for (axis_name, mesh, ints) in [
        ("I (IMESH/IINTS)", &fm.imesh, &fm.iints),
        ("J (JMESH/JINTS)", &fm.jmesh, &fm.jints),
        ("K (KMESH/KINTS)", &fm.kmesh, &fm.kints),
    ] {
        if !ints.is_empty() && mesh.len() != ints.len() && mesh.len() != ints.len() + 1 {
            diagnostics.push(ValidationError {
                code: ErrorCode::InvalidFmeshConfiguration,
                severity: Severity::Error,
                category: ValidationCategory::Tally,
                line_number: line,
                message: format!(
                    "FMESH{} {} has {} mesh points but {} interval counts",
                    fm.id,
                    axis_name,
                    mesh.len(),
                    ints.len()
                ),
            });
        }
        if mesh.len() >= 2 && !is_strictly_increasing(mesh) {
            diagnostics.push(ValidationError {
                code: ErrorCode::InvalidFmeshConfiguration,
                severity: Severity::Error,
                category: ValidationCategory::Tally,
                line_number: line,
                message: format!(
                    "FMESH{} {} coordinates must be strictly increasing",
                    fm.id, axis_name
                ),
            });
        }
        if ints.contains(&0) {
            diagnostics.push(ValidationError {
                code: ErrorCode::InvalidFmeshConfiguration,
                severity: Severity::Error,
                category: ValidationCategory::Tally,
                line_number: line,
                message: format!(
                    "FMESH{} {} interval count must be greater than 0",
                    fm.id, axis_name
                ),
            });
        }
    }

    if !fm.eints.is_empty()
        && fm.emesh.len() != fm.eints.len()
        && fm.emesh.len() != fm.eints.len() + 1
    {
        diagnostics.push(ValidationError {
            code: ErrorCode::InvalidFmeshConfiguration,
            severity: Severity::Error,
            category: ValidationCategory::Tally,
            line_number: line,
            message: format!(
                "FMESH{} EMESH ({}) and EINTS ({}) lengths do not match",
                fm.id,
                fm.emesh.len(),
                fm.eints.len()
            ),
        });
    }
    if fm.emesh.len() >= 2 && !is_strictly_increasing(&fm.emesh) {
        diagnostics.push(ValidationError {
            code: ErrorCode::InvalidFmeshConfiguration,
            severity: Severity::Error,
            category: ValidationCategory::Tally,
            line_number: line,
            message: format!("FMESH{} EMESH bins must be strictly increasing", fm.id),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_mcnp_input_lossy;

    #[test]
    fn test_valid_input_has_no_errors() {
        let sample = "\
Valid Input Test
1  1 -1.0  -1   imp:n=1.0
2  0        1   imp:n=0.0

1  so 10.0

mode n
sdef pos=0 0 0 erg=d1 par=n cel=1
si1  h 0.0 1.0 10.0
sp1  d 0.0 0.5 0.5
m1   1001.80c 2.0 8016.80c 1.0
mt1  lwtr.10t
f4:n 1
e4   0.1 1.0 10.0
nps  10000
";
        let res = parse_mcnp_input_lossy(sample);
        assert!(res.errors.is_empty());
        let diags = validate_mcnp_input(&res.input, &res.spans);
        assert!(diags.is_empty(), "Expected no diagnostics, got: {:?}", diags);
    }

    #[test]
    fn test_duplicate_ids_and_undefined_references() {
        let sample = "\
Error Detection Test
1  1 -1.0  -10 -99  imp:n=1.0
1  2 -2.0   10 #88  imp:n=1.0

10 so 5.0
10 so 8.0

mode n
m1 1001.80c 1.0
sdef pos=0 0 0 erg=d5
nps 1000
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::DuplicateCellId));
        assert!(codes.contains(&ErrorCode::DuplicateSurfaceId));
        assert!(codes.contains(&ErrorCode::UndefinedSurface)); // -99
        assert!(codes.contains(&ErrorCode::UndefinedCell)); // #88
        assert!(codes.contains(&ErrorCode::UndefinedMaterial)); // M2
        assert!(codes.contains(&ErrorCode::UndefinedSourceDistribution)); // D5
    }

    #[test]
    fn test_surface_and_macrobody_errors() {
        let sample = "\
Surface Error Test
1  0  -1 -2.7 -3.1  imp:n=1.0
2  0   1            imp:n=0.0

1  so -5.0
2  rpp 10.0 5.0  -1.0 1.0  -1.0 1.0
3  px 1.0 2.0

mode n
sdef pos=0 0 0
nps 100
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::InvalidSurfaceGeometry)); // SO -5.0 and RPP xmin > xmax
        assert!(codes.contains(&ErrorCode::InvalidSurfaceParamCount)); // PX with 2 params
        assert!(codes.contains(&ErrorCode::InvalidMacrobodyFacet)); // -2.7 on RPP (1..=6) and -3.1 on PX
    }

    #[test]
    fn test_circular_references_and_material_signs() {
        let sample = "\
Cycle & Material Error Test
1  1 -1.0  -1 #2  imp:n=1.0
2  1 -1.0   1 #1  imp:n=0.0

1  so 5.0

mode n
m1 1001.80c 0.5 8016.80c -0.5
sdef pos=0 0 0
kcode 1000 1.0 10 50
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::CircularCellReference));
        assert!(codes.contains(&ErrorCode::MixedMaterialFractionSigns));
        assert!(codes.contains(&ErrorCode::ConflictingSourceDefinitions));
    }

    #[test]
    fn test_tally_and_source_distribution_validation() {
        let sample = "\
Tally & Source Validation Test
1  0  -1  imp:n=1.0
2  0   1  imp:n=0.0

1  so 10.0

mode n
sdef pos=0 0 0 erg=d1
si1  h 10.0 5.0 20.0
sp1  d 0.5 0.5
f4:p 99
e4   10.0 1.0
c4   -1.5 0.0 1.0
de14 0.1 1.0
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::NonMonotonicSourceBins)); // SI1 10.0 > 5.0
        assert!(codes.contains(&ErrorCode::SourceDistributionLengthMismatch)); // SP1 len 2 vs SI1 len 3
        assert!(codes.contains(&ErrorCode::ParticleNotInMode)); // F4:p when MODE n
        assert!(codes.contains(&ErrorCode::UndefinedCell)); // F4 target 99
        assert!(codes.contains(&ErrorCode::NonMonotonicTallyBins)); // E4 10.0 > 1.0
        assert!(codes.contains(&ErrorCode::InvalidCosineBinRange)); // C4 -1.5
        assert!(codes.contains(&ErrorCode::OrphanTallyModifierCard)); // DE14 without F14
        assert!(codes.contains(&ErrorCode::IncompleteDoseCards)); // DE14 without DF14
    }

    #[test]
    fn test_universe_transform_cell_params_and_warnings() {
        let sample = "\
Universe & Cell Param Validation Test
1  1  0.0  -1  u=1 fill=2 vol=-10.0 vol=5.0 lat=3 trcl=99 imp:n=1.0
2  0        1  u=2 fill=1                                  imp:n=0.0
3  0       -2  fill=999

1  so 10.0
2  5 so 20.0
3  so 30.0

mode n
m1   1001.80c 1.0
m2   8016.80c 1.0
tr1  0 0 1 2
fmesh4:n geom=xyz imesh=10 5 iints=0
unknowncard 1 2 3
";
        let res = parse_mcnp_input_lossy(sample);
        let diags = validate_mcnp_input(&res.input, &res.spans);
        let codes: Vec<ErrorCode> = diags.iter().map(|d| d.code).collect();

        assert!(codes.contains(&ErrorCode::ZeroDensityNonVoidCell)); // Cell 1 density 0.0
        assert!(codes.contains(&ErrorCode::DuplicateCellParameter)); // duplicate VOL
        assert!(codes.contains(&ErrorCode::InvalidCellParameterValue)); // VOL=-10.0, LAT=3
        assert!(codes.contains(&ErrorCode::UndefinedTransform)); // TRCL=99 and Surface 2 TR5
        assert!(codes.contains(&ErrorCode::CircularUniverseFill)); // U=1 <-> U=2
        assert!(codes.contains(&ErrorCode::UndefinedUniverse)); // FILL=999
        assert!(codes.contains(&ErrorCode::MissingCellImportance)); // Cell 3 missing IMP:n
        assert!(codes.contains(&ErrorCode::UnusedSurface)); // Surface 3 unused
        assert!(codes.contains(&ErrorCode::UnusedMaterial)); // M2 unused
        assert!(codes.contains(&ErrorCode::InvalidTransformCardLength)); // TR1 has 4 params
        assert!(codes.contains(&ErrorCode::InvalidFmeshConfiguration)); // FMESH4 imesh 10 > 5 & iints=0
        assert!(codes.contains(&ErrorCode::UnrecognizedDataCard)); // unknowncard
        assert!(codes.contains(&ErrorCode::MissingSourceDefinition)); // no SDEF or KCODE
    }
}
