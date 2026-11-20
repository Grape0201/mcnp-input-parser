use crate::ast::{DataCard, McnpInput};
use crate::validate::{validate_mcnp_input, ErrorCode, Severity, ValidationCategory, ValidationError};
use crate::{parse_mcnp_input_lossy, ParseError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticItem {
    pub code: String,
    pub severity: String,
    pub category: String,
    pub line_number: Option<usize>,
    pub message: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub error_count: usize,
    pub warning_count: usize,
    pub diagnostics: Vec<DiagnosticItem>,
}

/// Generates an actionable remediation hint for a syntax error.
fn hint_for_parse_error(err: &ParseError, has_extra_blank_lines: bool) -> String {
    let base = match err {
        ParseError::CellError { message, .. } => {
            if message.contains("shortcut") || message.contains("repeat") || message.contains("interp") {
                "Check MCNP shortcut syntax (e.g. `3R` requires a preceding value, `ILOG` requires positive bounds).".to_string()
            } else {
                "Cell card format is `<id> <mat> [<density>] <geometry_expr> [<params...>]` or `<id> LIKE <n> BUT <params...>`. Note: Void cells (`mat=0`) omit `<density>`; non-void cells (`mat>0`) require `<density>`. Ensure no blank lines appear inside Block 1.".to_string()
            }
        }
        ParseError::SurfaceError { .. } => {
            "Surface card format is `[*/+]<id> [<tr_id>] <mnemonic> <params...>`. Ensure all parameters are valid floats and no blank lines appear inside Block 1 or Block 2.".to_string()
        }
        ParseError::DataError { message, .. } => {
            if message.contains("KSRC") {
                "KSRC requires `(x y z)` coordinate triples (total number of floats must be a multiple of 3).".to_string()
            } else if message.contains("F5") || message.contains("detector") {
                "Point detector `F5` requires `(x y z R0)` tuples (multiple of 4 floats); ring detector `FX5/FY5/FZ5` requires `(axis_coord radius R0)` triples (multiple of 3 floats).".to_string()
            } else {
                "Check data card syntax and continuation lines (continuation lines must start with at least 5 spaces or follow a trailing `&`).".to_string()
            }
        }
    };
    if has_extra_blank_lines {
        format!(
            "{} (Note: extra blank lines were detected in this file [E004], which may have shifted this card into the wrong block—fix the blank lines first.)",
            base
        )
    } else {
        base
    }
}

/// Generates an actionable remediation hint for a semantic validation error/warning.
pub fn hint_for_validation_error(err: &ValidationError, input: &McnpInput) -> String {
    match err.code {
        ErrorCode::EmptyCellsBlock => {
            "Add at least one cell card in Block 1 immediately after the title line (do NOT put a blank line right after the title line).".to_string()
        }
        ErrorCode::EmptySurfacesBlock => {
            "Ensure Block 1 and Block 2 are separated by a single blank line and Block 2 contains surface definitions.".to_string()
        }
        ErrorCode::EmptyDataBlock => {
            "Ensure Block 2 and Block 3 are separated by a single blank line and Block 3 contains data cards (MODE, SDEF/KCODE, M<n>, etc.).".to_string()
        }
        ErrorCode::UnexpectedBlankLine => {
            "An MCNP input file must have EXACTLY 2 blank lines in total: one between Block 1 (Cells) and Block 2 (Surfaces), and one between Block 2 (Surfaces) and Block 3 (Data). Remove extra blank lines (after the title or inside blocks) or replace them with `c` comment lines.".to_string()
        }
        ErrorCode::CellIdOutOfRange | ErrorCode::SurfaceIdOutOfRange | ErrorCode::DataCardIdOutOfRange => {
            "Use positive integer IDs within MCNP limits (1..=99,999,999 for Cells/Surfaces/Materials/Tallies; 1..=999 for TR/SI/SP/SB/DS).".to_string()
        }
        ErrorCode::DuplicateCellId | ErrorCode::DuplicateSurfaceId | ErrorCode::DuplicateDataCard => {
            "Run `--symbols` to check `next_available_ids` and assign a unique ID or merge duplicate singleton cards.".to_string()
        }
        ErrorCode::UnknownSurfaceMnemonic => {
            "Run `--template surfaces` or `--template macrobodies` to see valid MCNP surface mnemonics.".to_string()
        }
        ErrorCode::InvalidSurfaceParamCount | ErrorCode::InvalidSurfaceGeometry => {
            "Run `--template surfaces` or `--template macrobodies` to verify the exact parameter count and geometric constraints (e.g. R > 0, xmin < xmax).".to_string()
        }
        ErrorCode::InvalidPeriodicBoundary => {
            "Periodic boundaries (negative transform ID) can only be used on transmissive planes (`P`, `PX`, `PY`, `PZ`) without `*` or `+` prefixes.".to_string()
        }
        ErrorCode::UndefinedSurface => {
            let defined: Vec<String> = input.surfaces.iter().map(|s| s.id.to_string()).collect();
            if defined.is_empty() {
                "Define the missing surface in Block 2.".to_string()
            } else {
                format!(
                    "Define the missing surface in Block 2, or use one of the defined Surface IDs: [{}].",
                    defined.join(", ")
                )
            }
        }
        ErrorCode::InvalidMacrobodyFacet => {
            "Facet notation `±s.f` is only valid on macrobodies (`RPP`/`BOX`/`ARB`: .1..6, `RCC`/`TRC`/`REC`: .1..3, `RHP`/`HEX`: .1..8, `WED`: .1..5, `SPH`/`ELL`: .1).".to_string()
        }
        ErrorCode::UndefinedCell => {
            let defined: Vec<String> = input.cells.iter().map(|c| c.id.to_string()).collect();
            format!(
                "Define the referenced cell in Block 1, or reference an existing Cell ID: [{}].",
                defined.join(", ")
            )
        }
        ErrorCode::CircularCellReference => {
            "Break the cycle in `#n` cell complement expressions or `LIKE n BUT` definitions (`LIKE` base cell must be a Normal cell).".to_string()
        }
        ErrorCode::UndefinedMaterial => {
            let mats: Vec<String> = input
                .data_cards
                .iter()
                .filter_map(|c| match c {
                    DataCard::Material { id, .. } => Some(format!("m{}", id)),
                    _ => None,
                })
                .collect();
            if mats.is_empty() {
                "Add a corresponding `m<n> <zaid> <fraction> ...` card in Block 3, or set cell material to `0` (void).".to_string()
            } else {
                format!(
                    "Add the missing `m<n>` card in Block 3, or use one of the defined materials: [{}].",
                    mats.join(", ")
                )
            }
        }
        ErrorCode::UndefinedTransform => {
            "Add a `tr<n>` (or `*tr<n>`) coordinate transformation card in Block 3.".to_string()
        }
        ErrorCode::UndefinedUniverse => {
            "Add at least one cell with `u=<id>` in Block 1 for every universe referenced by `fill=<id>`.".to_string()
        }
        ErrorCode::CircularUniverseFill => {
            "Ensure universe hierarchy (`U=a` -> `FILL=b`) forms a directed acyclic graph (DAG) with no cycles.".to_string()
        }
        ErrorCode::UnusedSurface => {
            "Remove the unused surface card from Block 2 or reference it in a cell geometry expression.".to_string()
        }
        ErrorCode::UnusedMaterial => {
            "Remove the unused `m<n>` card from Block 3 or assign it to a cell in Block 1.".to_string()
        }
        ErrorCode::ZeroDensityNonVoidCell => {
            "Specify a non-zero density for material cells (negative for g/cm3, positive for atoms/b-cm), or set material ID to `0` for a void cell.".to_string()
        }
        ErrorCode::DuplicateCellParameter => {
            "Remove duplicate parameter keywords on the same cell card.".to_string()
        }
        ErrorCode::InvalidCellParameterValue => {
            "Check parameter constraints (`VOL > 0`, `TMP > 0`, `IMP >= 0`, `LAT` in {1, 2}, `U >= 0`, `FILL != 0`).".to_string()
        }
        ErrorCode::MissingCellImportance => {
            "Assign `imp:<p>=<val>` (e.g. `imp:n=1.0` inside the problem, `imp:n=0.0` for exterior graveyard) on every cell for all active particles in `MODE`.".to_string()
        }
        ErrorCode::ImportanceCardLengthMismatch => {
            format!(
                "Block 3 `IMP` or `WWN` card must provide exactly {} entries (matching the {} cells in Block 1).",
                input.cells.len(),
                input.cells.len()
            )
        }
        ErrorCode::UnrecognizedCellParameter => {
            "If this text is an inline comment, prefix it with `$`. Otherwise check spelling of cell parameter keywords (`imp`, `vol`, `u`, `fill`, `lat`, `tmp`, `trcl`, etc.).".to_string()
        }
        ErrorCode::VoidCellWithDensity => {
            "Void cells (`mat = 0`) must not have a density value or `RHO=` parameter.".to_string()
        }
        ErrorCode::FilledCellWithMaterial => {
            "Cells containing `FILL=...` should be void (`mat = 0` with no density) because their material is defined by the filling universe.".to_string()
        }
        ErrorCode::LatticeCellMissingFill => {
            "Cells with `LAT=1` or `LAT=2` must include a `FILL=...` parameter specifying the filling universe(s).".to_string()
        }
        ErrorCode::InvalidCellComplement => {
            "MCNP does not allow the complement operator `#n` to target a cell that has a `FILL` parameter; use surface expressions instead.".to_string()
        }
        ErrorCode::MissingSourceDefinition => {
            "Add either an `sdef ...` card (fixed-source) or a `kcode ...` + `ksrc ...` card (criticality) in Block 3.".to_string()
        }
        ErrorCode::ConflictingSourceDefinitions => {
            "Remove either `SDEF` or `KCODE`/`KSRC`; fixed-source and criticality modes cannot be combined in the same input.".to_string()
        }
        ErrorCode::MissingKsrcForKcode => {
            "Add a `ksrc x y z ...` card (or an `sdef` initial source) to supply starting fission source points for `KCODE`.".to_string()
        }
        ErrorCode::UndefinedSourceDistribution | ErrorCode::IncompleteSourceDistribution => {
            "Ensure every referenced distribution `Dn` has a corresponding `SIn` + `SPn` pair (or `DSn` / standalone analytic `SPn`). Run `--template source` for examples.".to_string()
        }
        ErrorCode::SourceDistributionLengthMismatch => {
            "For `SIn H` (histogram, k+1 bin edges), `SPn D` must start with `0.0` followed by k probabilities (k+1 values total). For `SIn L` or `A` (k values), `SPn D` must have k values.".to_string()
        }
        ErrorCode::NonMonotonicSourceBins | ErrorCode::NonMonotonicTallyBins => {
            "Ensure bin boundary values (`SI`, `E`, `T`, `C`, `DE`, `FMESH`, `WWE`, `WWGE`) are strictly increasing.".to_string()
        }
        ErrorCode::InvalidSourceProbability => {
            "Probabilities on `SP`/`SB` must be non-negative with a positive sum, and built-in analytic function parameters must be positive.".to_string()
        }
        ErrorCode::InvalidSdefParameter => {
            "Check `SDEF` keyword spelling, avoid duplicate keywords, and ensure values are within valid MCNP ranges (e.g. `ERG >= 0`, `-1 <= DIR <= 1`, `RAD >= 0`, `0 < EFF <= 1`, `WGT > 0`, `NRM = ±1`, explicit scalars for `WGT`/`EFF`/`NRM`).".to_string()
        }
        ErrorCode::InvalidSdefDependency => {
            "Each `Dn` distribution may be used by only one `SDEF` variable; dependent functions (`F<var> Dn`) allow only one level of dependence, cannot self-depend, and position variables cannot depend on `POS`.".to_string()
        }
        ErrorCode::InconsistentSdefOptions => {
            "Check `SDEF` option combinations: `PAR=BG/BN/BP` requires `LOC`, `DAT`/`LOC` are only for cosmic/background sources, and `TR=Dm` requires `SIm L`.".to_string()
        }
        ErrorCode::OrphanTallyModifierCard => {
            "Ensure the modifier card ID (`En`, `Tn`, `Cn`, `FMn`, `FSn`, `SDn`, `DEn`, `DFn`, `FCn`) matches a defined `F<n>` tally ID in Block 3.".to_string()
        }
        ErrorCode::TallyTargetKindMismatch => {
            "Surface tallies (`F1`, `F2`) must reference defined Surface IDs; cell tallies (`F4`, `F6`, `F7`, `F8`) must reference defined Cell IDs.".to_string()
        }
        ErrorCode::IncompleteDoseCards => {
            "Provide both `DE<n>` and `DF<n>` with identical bin counts (and positive values when using `LOG` interpolation).".to_string()
        }
        ErrorCode::InvalidCosineBinRange => {
            "Cosine bins on `C<n>` must lie within `[-1.0, 1.0]` in strictly ascending order.".to_string()
        }
        ErrorCode::InvalidDetectorRadius => {
            "Detector exclusion radius `R0` on `F5` must be non-negative, and ring radius on `FX5/FY5/FZ5` must be positive.".to_string()
        }
        ErrorCode::InvalidFmeshConfiguration => {
            "Run `--template tallies` to check `FMESH` syntax (`ID % 10 == 4`, `IMESH`/`IINTS` count match, positive `FACTOR`).".to_string()
        }
        ErrorCode::EmptyMaterialCard | ErrorCode::MixedMaterialFractionSigns | ErrorCode::InvalidZaidFormat => {
            "Material cards (`m<n>`) require `<zaid> <fraction>` pairs where all fractions have the same sign (all `> 0` for atomic or all `< 0` for weight) and valid ZAID format (`ZZZAAA` or `ZZZAAA.nnX`).".to_string()
        }
        ErrorCode::EmptyModeCard | ErrorCode::ParticleNotInMode => {
            "Add all particles used in `IMP`, `F<n>`, `FMESH`, `PHYS`, `CUT`, `WWE/WWN/WWP`, or `SDEF PAR=` to the `MODE` card (e.g. `mode n p`).".to_string()
        }
        ErrorCode::InvalidTransformCardLength => {
            "`TR<n>` card requires 3, 6, 9, 12, or 13 parameters (3 displacement components + optional rotation matrix + optional `M` flag `1` or `-1`).".to_string()
        }
        ErrorCode::InvalidNpsCount => {
            "`NPS` history count must be a positive integer (`> 0`).".to_string()
        }
        ErrorCode::InvalidKcodeParameters => {
            "`KCODE` requires `nsrck > 0`, `rkk > 0`, and total cycles `kct > ikz` (active cycles > skipped cycles).".to_string()
        }
        ErrorCode::UnrecognizedDataCard => {
            "Check card mnemonic spelling or prefix comment lines with `c ` in columns 1-5.".to_string()
        }
    }
}

/// Parses and validates an MCNP input string, returning a structured [`ValidationReport`] with fix hints.
pub fn analyze_diagnostics(content: &str) -> ValidationReport {
    let lossy = parse_mcnp_input_lossy(content);
    let semantic = validate_mcnp_input(&lossy.input, &lossy.spans);

    // When extra/misplaced blank lines (`E004`) break the 3-block boundary structure,
    // subsequent syntax and semantic errors are cascading false positives caused by
    // cards being parsed in the wrong block or truncated after the 3rd blank line.
    // Report only `E004` first so the user/agent fixes the blank-line delimiters cleanly.
    if let Some(e004) = semantic
        .iter()
        .find(|d| d.code == ErrorCode::UnexpectedBlankLine)
    {
        return ValidationReport {
            error_count: 1,
            warning_count: 0,
            diagnostics: vec![DiagnosticItem {
                code: e004.code.as_str().to_string(),
                severity: "Error".to_string(),
                category: format!("{:?}", e004.category),
                line_number: e004.line_number,
                message: e004.message.clone(),
                hint: hint_for_validation_error(e004, &lossy.input),
            }],
        };
    }

    let mut diagnostics = Vec::with_capacity(lossy.errors.len() + semantic.len());
    let mut error_count = 0;
    let mut warning_count = 0;

    for diag in semantic
        .iter()
        .filter(|d| d.category == ValidationCategory::Structure)
    {
        match diag.severity {
            Severity::Error => error_count += 1,
            Severity::Warning => warning_count += 1,
        }
        diagnostics.push(DiagnosticItem {
            code: diag.code.as_str().to_string(),
            severity: match diag.severity {
                Severity::Error => "Error".to_string(),
                Severity::Warning => "Warning".to_string(),
            },
            category: format!("{:?}", diag.category),
            line_number: diag.line_number,
            message: diag.message.clone(),
            hint: hint_for_validation_error(diag, &lossy.input),
        });
    }

    for err in &lossy.errors {
        error_count += 1;
        diagnostics.push(DiagnosticItem {
            code: "SYNTAX".to_string(),
            severity: "Error".to_string(),
            category: "Syntax".to_string(),
            line_number: Some(err.line_number()),
            message: err.to_string(),
            hint: hint_for_parse_error(err, false),
        });
    }

    for diag in semantic
        .iter()
        .filter(|d| d.category != ValidationCategory::Structure)
    {
        match diag.severity {
            Severity::Error => error_count += 1,
            Severity::Warning => warning_count += 1,
        }
        diagnostics.push(DiagnosticItem {
            code: diag.code.as_str().to_string(),
            severity: match diag.severity {
                Severity::Error => "Error".to_string(),
                Severity::Warning => "Warning".to_string(),
            },
            category: format!("{:?}", diag.category),
            line_number: diag.line_number,
            message: diag.message.clone(),
            hint: hint_for_validation_error(diag, &lossy.input),
        });
    }

    ValidationReport {
        error_count,
        warning_count,
        diagnostics,
    }
}

/// Formats a [`ValidationReport`] as human/LLM-friendly text.
pub fn format_validation_report(report: &ValidationReport) -> String {
    let mut out = String::new();
    for item in &report.diagnostics {
        let loc = item
            .line_number
            .map(|n| format!(" at line {}", n))
            .unwrap_or_default();
        out.push_str(&format!(
            "[{}] {}{}: {}\n  -> Hint: {}\n",
            item.code, item.severity, loc, item.message, item.hint
        ));
    }
    out.push_str(&format!(
        "Validation complete: {} error(s), {} warning(s).\n",
        report.error_count, report.warning_count
    ));
    out
}

