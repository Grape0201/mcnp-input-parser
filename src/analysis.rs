use crate::ast::{
    BoundaryCondition, CellCard, CellDefinition, CellParam, DataCard, FmBin, GeometryExpr,
    McnpInput, SdefValue, SdefVariable, SiEntry, SurfaceCard, SurfaceKind, TallyTarget,
};
use crate::validate::{validate_mcnp_input, ErrorCode, Severity, ValidationError};
use crate::{parse_mcnp_input_lossy, CardLineSpans, ParseError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// ============================================================================
// 1. Diagnostics & Fix Hints (LSP publishDiagnostics equivalent)
// ============================================================================

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
fn hint_for_parse_error(err: &ParseError) -> String {
    match err {
        ParseError::CellError { message, .. } => {
            if message.contains("shortcut") || message.contains("repeat") || message.contains("interp") {
                "Check MCNP shortcut syntax (e.g. `3R` requires a preceding value, `ILOG` requires positive bounds).".to_string()
            } else {
                "Cell card format is `<id> <mat> [<density>] <geometry_expr> [<params...>]` or `<id> LIKE <n> BUT <params...>`. Note: Void cells (`mat=0`) omit `<density>`; non-void cells (`mat>0`) require `<density>`. Ensure no blank lines appear inside Block 1.".to_string()
            }
        }
        ParseError::SurfaceError { .. } => {
            "Surface card format is `[*/+]<id> [<tr_id>] <mnemonic> <params...>`. Ensure all parameters are valid floats and no blank lines appear inside Block 2.".to_string()
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
    }
}

/// Generates an actionable remediation hint for a semantic validation error/warning.
pub fn hint_for_validation_error(err: &ValidationError, input: &McnpInput) -> String {
    match err.code {
        ErrorCode::EmptyCellsBlock => {
            "Add at least one cell card in Block 1 after the title line.".to_string()
        }
        ErrorCode::EmptySurfacesBlock => {
            "Ensure Block 1 and Block 2 are separated by a single blank line and Block 2 contains surface definitions.".to_string()
        }
        ErrorCode::EmptyDataBlock => {
            "Ensure Block 2 and Block 3 are separated by a single blank line and Block 3 contains data cards (MODE, SDEF/KCODE, M<n>, etc.).".to_string()
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

    let mut diagnostics = Vec::with_capacity(lossy.errors.len() + semantic.len());
    let mut error_count = 0;
    let mut warning_count = 0;

    for err in &lossy.errors {
        error_count += 1;
        diagnostics.push(DiagnosticItem {
            code: "SYNTAX".to_string(),
            severity: "Error".to_string(),
            category: "Syntax".to_string(),
            line_number: Some(err.line_number()),
            message: err.to_string(),
            hint: hint_for_parse_error(err),
        });
    }

    for diag in &semantic {
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

// ============================================================================
// 2. Document Symbols & Next Available IDs (LSP documentSymbol equivalent)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellSymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub material_id: Option<u32>,
    pub density: Option<f64>,
    pub like_base_cell: Option<u32>,
    pub universe: Option<i32>,
    pub fill_universes: Vec<i32>,
    pub lattice: Option<u32>,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub boundary: BoundaryCondition,
    pub transform_id: Option<i32>,
    pub kind: SurfaceKind,
    pub params: Vec<f64>,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialSymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub nuclide_count: usize,
    pub zaids: Vec<String>,
    pub mt_tables: Vec<String>,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniverseSymbol {
    pub id: i32,
    pub member_cells: Vec<u32>,
    pub filled_by_cells: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformSymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub is_degrees: bool,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TallySymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub tally_type: String,
    pub particles: Vec<String>,
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistributionSymbol {
    pub id: u32,
    pub defined_cards: Vec<String>,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextAvailableIds {
    pub cell: u32,
    pub surface: u32,
    pub material: u32,
    pub universe: i32,
    pub transform: u32,
    pub distribution: u32,
    pub tally_f4: u32,
    pub fmesh: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolsReport {
    pub title: String,
    pub mode: Vec<String>,
    pub source_type: String,
    pub cells: Vec<CellSymbol>,
    pub surfaces: Vec<SurfaceSymbol>,
    pub materials: Vec<MaterialSymbol>,
    pub universes: Vec<UniverseSymbol>,
    pub transforms: Vec<TransformSymbol>,
    pub tallies: Vec<TallySymbol>,
    pub distributions: Vec<DistributionSymbol>,
    pub next_available_ids: NextAvailableIds,
}

/// Extracts fill universe IDs from a `CellParam::Fill` values array.
fn extract_fill_universes(lat: Option<u32>, values: &[String]) -> Vec<i32> {
    let mut result = BTreeSet::new();
    let mut idx = 0;
    if lat.is_some() {
        while idx < values.len() && values[idx].contains(':') {
            idx += 1;
        }
    }
    let mut in_parens = false;
    while idx < values.len() {
        let tok = &values[idx];
        if tok.starts_with('(') {
            in_parens = true;
            if tok.ends_with(')') {
                in_parens = false;
            }
            idx += 1;
            continue;
        }
        if in_parens {
            if tok.ends_with(')') {
                in_parens = false;
            }
            idx += 1;
            continue;
        }
        let head = tok.split('(').next().unwrap_or(tok);
        if let Ok(u) = head.parse::<i32>() {
            if u > 0 {
                result.insert(u);
            }
        }
        if tok.contains('(') && !tok.ends_with(')') {
            in_parens = true;
        }
        idx += 1;
    }
    result.into_iter().collect()
}

/// Collects all surface IDs and cell complement IDs referenced in a geometry expression.
fn collect_geometry_refs(expr: &GeometryExpr, surf_refs: &mut Vec<u32>, cell_refs: &mut Vec<u32>) {
    match expr {
        GeometryExpr::Surface(s) => surf_refs.push(s.unsigned_abs()),
        GeometryExpr::Facet { surface, .. } => surf_refs.push(surface.unsigned_abs()),
        GeometryExpr::ComplementCell(c) => cell_refs.push(*c),
        GeometryExpr::Complement(inner) => collect_geometry_refs(inner, surf_refs, cell_refs),
        GeometryExpr::Intersection(list) | GeometryExpr::Union(list) => {
            for item in list {
                collect_geometry_refs(item, surf_refs, cell_refs);
            }
        }
    }
}

/// Analyzes an MCNP input string and builds a complete [`SymbolsReport`].
pub fn analyze_symbols(content: &str) -> SymbolsReport {
    let lossy = parse_mcnp_input_lossy(content);
    build_symbols_report(&lossy.input, &lossy.spans)
}

/// Builds a [`SymbolsReport`] from a parsed [`McnpInput`] and [`CardLineSpans`].
pub fn build_symbols_report(input: &McnpInput, spans: &CardLineSpans) -> SymbolsReport {
    let all_refs = collect_all_references(input, spans);

    let mut mode = vec!["n".to_string()];
    let mut has_sdef = false;
    let mut has_kcode = false;

    for card in &input.data_cards {
        match card {
            DataCard::Mode { particles } if !particles.is_empty() => {
                mode = particles.iter().map(|p| p.0.clone()).collect();
            }
            DataCard::Sdef { .. } => has_sdef = true,
            DataCard::Kcode { .. } => has_kcode = true,
            _ => {}
        }
    }

    let source_type = match (has_sdef, has_kcode) {
        (true, false) => "Fixed-Source (SDEF)".to_string(),
        (false, true) => "Criticality (KCODE)".to_string(),
        (true, true) => "Conflicting (Both SDEF and KCODE)".to_string(),
        (false, false) => "None (Missing SDEF/KCODE)".to_string(),
    };

    // 1. Cells & Universes
    let mut universe_members: BTreeMap<i32, Vec<u32>> = BTreeMap::new();
    let mut universe_fillers: BTreeMap<i32, Vec<u32>> = BTreeMap::new();
    let mut cells = Vec::with_capacity(input.cells.len());

    for (idx, cell) in input.cells.iter().enumerate() {
        let line = spans.cell_card_lines.get(idx).copied();
        let mut mat_id = None;
        let mut density = None;
        let mut like_base = None;

        match &cell.definition {
            CellDefinition::Normal {
                material_id,
                density: d,
                ..
            } => {
                mat_id = Some(*material_id);
                density = *d;
            }
            CellDefinition::LikeBut { base_cell_id } => {
                like_base = Some(*base_cell_id);
            }
        }

        let mut univ = None;
        let mut lat = None;
        for p in &cell.params {
            match p {
                CellParam::Universe(u) => univ = Some(*u),
                CellParam::Lattice(l) => lat = Some(*l),
                CellParam::Material(m) => mat_id = Some(*m),
                CellParam::Density(d) => density = Some(*d),
                _ => {}
            }
        }

        let mut fill_univs = Vec::new();
        for p in &cell.params {
            if let CellParam::Fill { values, .. } = p {
                fill_univs = extract_fill_universes(lat, values);
            }
        }

        if let Some(u) = univ {
            universe_members.entry(u).or_default().push(cell.id);
        }
        for &fu in &fill_univs {
            universe_fillers.entry(fu).or_default().push(cell.id);
        }

        let ref_count = all_refs
            .cell_refs
            .get(&cell.id)
            .map(|v| v.len())
            .unwrap_or(0);

        cells.push(CellSymbol {
            id: cell.id,
            line_number: line,
            material_id: mat_id,
            density,
            like_base_cell: like_base,
            universe: univ,
            fill_universes: fill_univs,
            lattice: lat,
            reference_count: ref_count,
        });
    }

    let all_univ_ids: BTreeSet<i32> = universe_members
        .keys()
        .chain(universe_fillers.keys())
        .copied()
        .collect();
    let universes: Vec<UniverseSymbol> = all_univ_ids
        .into_iter()
        .map(|uid| UniverseSymbol {
            id: uid,
            member_cells: universe_members.remove(&uid).unwrap_or_default(),
            filled_by_cells: universe_fillers.remove(&uid).unwrap_or_default(),
        })
        .collect();

    // 2. Surfaces
    let surfaces: Vec<SurfaceSymbol> = input
        .surfaces
        .iter()
        .enumerate()
        .map(|(idx, s)| SurfaceSymbol {
            id: s.id,
            line_number: spans.surface_card_lines.get(idx).copied(),
            boundary: s.boundary,
            transform_id: s.transform_id,
            kind: s.kind.clone(),
            params: s.params.clone(),
            reference_count: all_refs.surf_refs.get(&s.id).map(|v| v.len()).unwrap_or(0),
        })
        .collect();

    // 3. Materials, Transforms, Tallies, Distributions
    let mut mt_map: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut tally_modifiers: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut dist_cards: BTreeMap<u32, Vec<String>> = BTreeMap::new();

    for (idx, card) in input.data_cards.iter().enumerate() {
        let line = spans.data_card_lines.get(idx).copied();
        let line_suffix = line.map(|l| format!(" (L{})", l)).unwrap_or_default();
        match card {
            DataCard::ThermalScattering { id, tables } => {
                mt_map.entry(*id).or_default().extend(tables.clone());
            }
            DataCard::TallyEnergy { id, .. } => {
                tally_modifiers.entry(*id).or_default().push(format!("E{}", id));
            }
            DataCard::TallyTime { id, .. } => {
                tally_modifiers.entry(*id).or_default().push(format!("T{}", id));
            }
            DataCard::TallyCosine { id, .. } => {
                tally_modifiers.entry(*id).or_default().push(format!("C{}", id));
            }
            DataCard::Dose(d) => {
                let prefix = if d.is_function { "DF" } else { "DE" };
                tally_modifiers
                    .entry(d.id)
                    .or_default()
                    .push(format!("{}{}", prefix, d.id));
            }
            DataCard::TallyMultiplier(fm) => {
                tally_modifiers
                    .entry(fm.id)
                    .or_default()
                    .push(format!("FM{}", fm.id));
            }
            DataCard::TallySegment { id, .. } => {
                tally_modifiers
                    .entry(*id)
                    .or_default()
                    .push(format!("FS{}", id));
            }
            DataCard::SegmentDivisor { id, .. } => {
                tally_modifiers
                    .entry(*id)
                    .or_default()
                    .push(format!("SD{}", id));
            }
            DataCard::TallyComment { id, .. } => {
                tally_modifiers
                    .entry(*id)
                    .or_default()
                    .push(format!("FC{}", id));
            }
            DataCard::Si(si) => {
                dist_cards
                    .entry(si.id)
                    .or_default()
                    .push(format!("SI{}{}", si.id, line_suffix));
            }
            DataCard::Sp(sp) => {
                dist_cards
                    .entry(sp.id)
                    .or_default()
                    .push(format!("SP{}{}", sp.id, line_suffix));
            }
            DataCard::Sb(sb) => {
                dist_cards
                    .entry(sb.id)
                    .or_default()
                    .push(format!("SB{}{}", sb.id, line_suffix));
            }
            DataCard::Ds(ds) => {
                dist_cards
                    .entry(ds.id)
                    .or_default()
                    .push(format!("DS{}{}", ds.id, line_suffix));
            }
            _ => {}
        }
    }

    let mut materials = Vec::new();
    let mut transforms = Vec::new();
    let mut tallies = Vec::new();
    let mut fmesh_ids = Vec::new();

    for (idx, card) in input.data_cards.iter().enumerate() {
        let line = spans.data_card_lines.get(idx).copied();
        match card {
            DataCard::Material { id, nuclides, .. } => {
                materials.push(MaterialSymbol {
                    id: *id,
                    line_number: line,
                    nuclide_count: nuclides.len(),
                    zaids: nuclides.iter().map(|n| n.zaid.clone()).collect(),
                    mt_tables: mt_map.remove(id).unwrap_or_default(),
                    reference_count: all_refs.mat_refs.get(id).map(|v| v.len()).unwrap_or(0),
                });
            }
            DataCard::Transform {
                id, is_degrees, ..
            } => {
                transforms.push(TransformSymbol {
                    id: *id,
                    line_number: line,
                    is_degrees: *is_degrees,
                    reference_count: all_refs.tr_refs.get(id).map(|v| v.len()).unwrap_or(0),
                });
            }
            DataCard::Tally {
                id,
                kind,
                particles,
                ..
            } => {
                tallies.push(TallySymbol {
                    id: *id,
                    line_number: line,
                    tally_type: format!("{:?}", kind),
                    particles: particles.iter().map(|p| p.0.clone()).collect(),
                    modifiers: tally_modifiers.remove(id).unwrap_or_default(),
                });
            }
            DataCard::DetectorTally {
                id,
                axis,
                particles,
                ..
            } => {
                let kind_label = match axis {
                    None => "F5PointDetector".to_string(),
                    Some(ax) => format!("F{}5RingDetector", ax.to_ascii_uppercase()),
                };
                tallies.push(TallySymbol {
                    id: *id,
                    line_number: line,
                    tally_type: kind_label,
                    particles: particles.iter().map(|p| p.0.clone()).collect(),
                    modifiers: tally_modifiers.remove(id).unwrap_or_default(),
                });
            }
            DataCard::Fmesh(fm) => {
                fmesh_ids.push(fm.id);
                tallies.push(TallySymbol {
                    id: fm.id,
                    line_number: line,
                    tally_type: "FMESH".to_string(),
                    particles: fm.particles.iter().map(|p| p.0.clone()).collect(),
                    modifiers: Vec::new(),
                });
            }
            _ => {}
        }
    }

    let distributions: Vec<DistributionSymbol> = dist_cards
        .into_iter()
        .map(|(id, defined_cards)| DistributionSymbol {
            id,
            defined_cards,
            reference_count: all_refs.dist_refs.get(&id).map(|v| v.len()).unwrap_or(0),
        })
        .collect();

    // Compute conflict-free next available IDs
    let next_cell = cells
        .iter()
        .filter(|c| c.id < 90)
        .map(|c| c.id)
        .max()
        .unwrap_or(0)
        + 1;
    let next_cell = if cells.iter().any(|c| c.id == next_cell) {
        cells.iter().map(|c| c.id).max().unwrap_or(0) + 1
    } else {
        next_cell
    };

    let next_surface = surfaces.iter().map(|s| s.id).max().unwrap_or(0) + 1;
    let next_material = materials.iter().map(|m| m.id).max().unwrap_or(0) + 1;
    let next_universe = universes.iter().map(|u| u.id).max().unwrap_or(0) + 1;
    let next_transform = transforms.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let next_dist = distributions.iter().map(|d| d.id).max().unwrap_or(0) + 1;

    let mut next_f4 = 4;
    while tallies.iter().any(|t| t.id == next_f4) {
        next_f4 += 10;
    }
    let mut next_fmesh = 14;
    while tallies.iter().any(|t| t.id == next_fmesh) || fmesh_ids.contains(&next_fmesh) {
        next_fmesh += 10;
    }

    SymbolsReport {
        title: input.title.clone(),
        mode,
        source_type,
        cells,
        surfaces,
        materials,
        universes,
        transforms,
        tallies,
        distributions,
        next_available_ids: NextAvailableIds {
            cell: next_cell,
            surface: next_surface,
            material: next_material,
            universe: next_universe,
            transform: next_transform,
            distribution: next_dist,
            tally_f4: next_f4,
            fmesh: next_fmesh,
        },
    }
}

/// Formats a [`SymbolsReport`] as human/LLM-friendly text.
pub fn format_symbols_report(rep: &SymbolsReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("Title: {}\n", rep.title));
    out.push_str(&format!("Mode: {}\n", rep.mode.join(" ")));
    out.push_str(&format!("Source Type: {}\n\n", rep.source_type));

    out.push_str(&format!("=== Cells ({}) ===\n", rep.cells.len()));
    for c in &rep.cells {
        let line = c.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
        let mat_str = match (c.like_base_cell, c.material_id, c.density) {
            (Some(base), _, _) => format!("LIKE cell {}", base),
            (None, Some(0), _) => "void (mat=0)".to_string(),
            (None, Some(m), Some(d)) => format!("mat=M{}, rho={}", m, d),
            (None, Some(m), None) => format!("mat=M{}", m),
            _ => "unknown".to_string(),
        };
        let mut extras = Vec::new();
        if let Some(u) = c.universe {
            extras.push(format!("U={}", u));
        }
        if let Some(lat) = c.lattice {
            extras.push(format!("LAT={}", lat));
        }
        if !c.fill_universes.is_empty() {
            let fu: Vec<String> = c.fill_universes.iter().map(|u| u.to_string()).collect();
            extras.push(format!("FILL=[{}]", fu.join(",")));
        }
        let extra_str = if extras.is_empty() {
            String::new()
        } else {
            format!(" [{}]", extras.join(", "))
        };
        out.push_str(&format!(
            "  Cell {:>4} ({:<4}): {}{} (referenced {}x)\n",
            c.id, line, mat_str, extra_str, c.reference_count
        ));
    }

    out.push_str(&format!("\n=== Surfaces ({}) ===\n", rep.surfaces.len()));
    for s in &rep.surfaces {
        let line = s.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
        let bnd = match s.boundary {
            BoundaryCondition::Transmissive => "",
            BoundaryCondition::Reflective => "*",
            BoundaryCondition::White => "+",
        };
        let tr = s
            .transform_id
            .map(|t| format!(" tr={}", t))
            .unwrap_or_default();
        out.push_str(&format!(
            "  Surf {:>4} ({:<4}): {}{:?}{} {:?} (referenced {}x)\n",
            s.id, line, bnd, s.kind, tr, s.params, s.reference_count
        ));
    }

    out.push_str(&format!("\n=== Materials ({}) ===\n", rep.materials.len()));
    for m in &rep.materials {
        let line = m.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
        let mt = if m.mt_tables.is_empty() {
            String::new()
        } else {
            format!(" MT=[{}]", m.mt_tables.join(", "))
        };
        out.push_str(&format!(
            "  M{:<4} ({:<4}): {} nuclide(s) [{}]{} (referenced {}x)\n",
            m.id,
            line,
            m.nuclide_count,
            m.zaids.join(", "),
            mt,
            m.reference_count
        ));
    }

    if !rep.universes.is_empty() {
        out.push_str(&format!("\n=== Universes ({}) ===\n", rep.universes.len()));
        for u in &rep.universes {
            out.push_str(&format!(
                "  U={:<3}: member cells {:?}, filled into cells {:?}\n",
                u.id, u.member_cells, u.filled_by_cells
            ));
        }
    }

    if !rep.transforms.is_empty() {
        out.push_str(&format!("\n=== Transforms ({}) ===\n", rep.transforms.len()));
        for tr in &rep.transforms {
            let line = tr.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
            out.push_str(&format!(
                "  TR{:<3} ({:<4}): degrees={} (referenced {}x)\n",
                tr.id, line, tr.is_degrees, tr.reference_count
            ));
        }
    }

    if !rep.tallies.is_empty() {
        out.push_str(&format!("\n=== Tallies ({}) ===\n", rep.tallies.len()));
        for t in &rep.tallies {
            let line = t.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
            let mods = if t.modifiers.is_empty() {
                String::new()
            } else {
                format!(" modifiers=[{}]", t.modifiers.join(", "))
            };
            out.push_str(&format!(
                "  ID {:>4} ({:<4}): {}:{}{}\n",
                t.id,
                line,
                t.tally_type,
                t.particles.join(","),
                mods
            ));
        }
    }

    if !rep.distributions.is_empty() {
        out.push_str(&format!(
            "\n=== Source Distributions ({}) ===\n",
            rep.distributions.len()
        ));
        for d in &rep.distributions {
            out.push_str(&format!(
                "  D{:<3}: [{}] (referenced {}x)\n",
                d.id,
                d.defined_cards.join(", "),
                d.reference_count
            ));
        }
    }

    let next = &rep.next_available_ids;
    out.push_str("\n=== Next Available IDs (Conflict-Free) ===\n");
    out.push_str(&format!(
        "  Cell: {} | Surface: {} | Material: M{} | Universe: U={} | Transform: TR{} | Dist: D{} | Tally: F{} / FMESH{}\n",
        next.cell,
        next.surface,
        next.material,
        next.universe,
        next.transform,
        next.distribution,
        next.tally_f4,
        next.fmesh
    ));

    out
}

// ============================================================================
// 3. Find References & Hover Inspection (LSP hover / definition / references)
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionLocation {
    pub line_number: Option<usize>,
    pub card_kind: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceLocation {
    pub line_number: Option<usize>,
    pub source_card: String,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectReport {
    pub query: String,
    pub canonical_target: String,
    pub found: bool,
    pub definitions: Vec<DefinitionLocation>,
    pub references: Vec<ReferenceLocation>,
}

#[derive(Default)]
struct ReferenceIndex {
    cell_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    surf_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    mat_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    univ_refs: BTreeMap<i32, Vec<ReferenceLocation>>,
    tr_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    tally_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    dist_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
}

fn collect_all_references(input: &McnpInput, spans: &CardLineSpans) -> ReferenceIndex {
    let mut idx = ReferenceIndex::default();

    // 1. Scan Cell Cards
    for (i, cell) in input.cells.iter().enumerate() {
        let line = spans.cell_card_lines.get(i).copied();
        let src = format!("Cell {}", cell.id);

        match &cell.definition {
            CellDefinition::Normal {
                material_id,
                geometry,
                ..
            } => {
                if *material_id > 0 {
                    idx.mat_refs.entry(*material_id).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} material M{}", cell.id, material_id),
                    });
                }
                let mut s_refs = Vec::new();
                let mut c_refs = Vec::new();
                collect_geometry_refs(geometry, &mut s_refs, &mut c_refs);
                s_refs.sort_unstable();
                s_refs.dedup();
                c_refs.sort_unstable();
                c_refs.dedup();
                for sid in s_refs {
                    idx.surf_refs.entry(sid).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} geometry expression", cell.id),
                    });
                }
                for cid in c_refs {
                    idx.cell_refs.entry(cid).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} complement `#{}", cell.id, cid),
                    });
                }
            }
            CellDefinition::LikeBut { base_cell_id } => {
                idx.cell_refs.entry(*base_cell_id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: src.clone(),
                    context: format!("Cell {} `LIKE {} BUT`", cell.id, base_cell_id),
                });
            }
        }

        let mut lat = None;
        for p in &cell.params {
            if let CellParam::Lattice(l) = p {
                lat = Some(*l);
            }
        }

        for p in &cell.params {
            match p {
                CellParam::Material(m) if *m > 0 => {
                    idx.mat_refs.entry(*m).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} parameter `MAT={}`", cell.id, m),
                    });
                }
                CellParam::Universe(u) => {
                    idx.univ_refs.entry(*u).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} belongs to universe `U={}`", cell.id, u),
                    });
                }
                CellParam::Fill { values, .. } => {
                    for fu in extract_fill_universes(lat, values) {
                        idx.univ_refs.entry(fu).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: src.clone(),
                            context: format!("Cell {} filled with universe `FILL={}`", cell.id, fu),
                        });
                    }
                }
                CellParam::Transformation { values, .. } if values.len() == 1 => {
                    if let Some(v) = values[0].as_f64() {
                        if v > 0.0 {
                            let tr_id = v.round() as u32;
                            idx.tr_refs.entry(tr_id).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: src.clone(),
                                context: format!("Cell {} transformation `TRCL={}`", cell.id, tr_id),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // 2. Scan Surface Cards
    for (i, surf) in input.surfaces.iter().enumerate() {
        let line = spans.surface_card_lines.get(i).copied();
        let src = format!("Surface {}", surf.id);
        if let Some(tr_id) = surf.transform_id {
            if tr_id > 0 {
                let tid = tr_id as u32;
                idx.tr_refs.entry(tid).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: src,
                    context: format!("Surface {} coordinate transform TR{}", surf.id, tid),
                });
            } else if tr_id < 0 {
                let peer = tr_id.unsigned_abs();
                idx.surf_refs.entry(peer).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: src,
                    context: format!("Surface {} periodic boundary peer ({})", surf.id, tr_id),
                });
            }
        }
    }

    // 3. Scan Data Cards
    for (i, card) in input.data_cards.iter().enumerate() {
        let line = spans.data_card_lines.get(i).copied();
        match card {
            DataCard::ThermalScattering { id, tables } => {
                idx.mat_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("MT{}", id),
                    context: format!("Thermal scattering S(a,b) tables {:?}", tables),
                });
            }
            DataCard::Sdef { params } => {
                for p in params {
                    match (&p.variable, &p.value) {
                        (_, SdefValue::Distribution(d)) => {
                            idx.dist_refs.entry(*d).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!("SDEF {:?}=D{}", p.variable, d),
                            });
                        }
                        (_, SdefValue::Function { independent_var, dist_id }) => {
                            idx.dist_refs.entry(*dist_id).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!(
                                    "SDEF {:?}=F{:?} D{}",
                                    p.variable, independent_var, dist_id
                                ),
                            });
                        }
                        (SdefVariable::Cel, SdefValue::Scalar(v)) if *v > 0.0 => {
                            let cid = v.round() as u32;
                            idx.cell_refs.entry(cid).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!("SDEF CEL={}", cid),
                            });
                        }
                        (SdefVariable::Sur, SdefValue::Scalar(v)) if *v != 0.0 => {
                            let sid = v.abs().round() as u32;
                            idx.surf_refs.entry(sid).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!("SDEF SUR={}", sid),
                            });
                        }
                        (SdefVariable::Tr, SdefValue::Scalar(v)) if *v > 0.0 => {
                            let tid = v.round() as u32;
                            idx.tr_refs.entry(tid).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!("SDEF TR={}", tid),
                            });
                        }
                        _ => {}
                    }
                }
            }
            DataCard::Si(si) => {
                for entry in &si.entries {
                    if let SiEntry::Distribution(d) = entry {
                        idx.dist_refs.entry(*d).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("SI{}", si.id),
                            context: format!("SI{} S references distribution D{}", si.id, d),
                        });
                    }
                }
            }
            DataCard::Ds(ds) => {
                for entry in &ds.entries {
                    if let SiEntry::Distribution(d) = entry {
                        idx.dist_refs.entry(*d).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("DS{}", ds.id),
                            context: format!("DS{} references distribution D{}", ds.id, d),
                        });
                    }
                }
                for &(_, d) in &ds.pairs {
                    idx.dist_refs.entry(d).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: format!("DS{}", ds.id),
                        context: format!("DS{} pair references distribution D{}", ds.id, d),
                    });
                }
            }
            DataCard::Tally {
                id, kind, targets, ..
            } => {
                let is_surf = matches!(
                    kind,
                    crate::ast::TallyKind::F1SurfaceCurrent | crate::ast::TallyKind::F2SurfaceFlux
                );
                let mut ids = Vec::new();
                for t in targets {
                    match t {
                        TallyTarget::Single(n) => ids.push(*n),
                        TallyTarget::Group(list) => ids.extend(list.iter().copied()),
                        TallyTarget::ComplexGroup(_) => {}
                    }
                }
                ids.sort_unstable();
                ids.dedup();
                for target_id in ids {
                    if is_surf {
                        idx.surf_refs.entry(target_id).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("F{}", id),
                            context: format!("Surface tally F{} target surface {}", id, target_id),
                        });
                    } else {
                        idx.cell_refs.entry(target_id).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("F{}", id),
                            context: format!("Cell tally F{} target cell {}", id, target_id),
                        });
                    }
                }
            }
            DataCard::TallyMultiplier(fm) => {
                idx.tally_refs.entry(fm.id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("FM{}", fm.id),
                    context: format!("Tally multiplier FM{} for tally F{}", fm.id, fm.id),
                });
                for bin in &fm.bins {
                    if let FmBin::MultiplierSet { material_id, .. } = bin {
                        if *material_id > 0 {
                            let mid = *material_id as u32;
                            idx.mat_refs.entry(mid).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: format!("FM{}", fm.id),
                                context: format!("Tally multiplier FM{} uses material M{}", fm.id, mid),
                            });
                        }
                    }
                }
            }
            DataCard::TallySegment { id, surfaces, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("FS{}", id),
                    context: format!("Tally segment FS{} for tally F{}", id, id),
                });
                for s in surfaces {
                    let sid = s.unsigned_abs();
                    idx.surf_refs.entry(sid).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: format!("FS{}", id),
                        context: format!("Tally segment FS{} dividing surface {}", id, s),
                    });
                }
            }
            DataCard::SegmentDivisor { id, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("SD{}", id),
                    context: format!("Segment divisor SD{} for tally F{}", id, id),
                });
            }
            DataCard::TallyEnergy { id, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("E{}", id),
                    context: format!("Energy bins E{} for tally F{}", id, id),
                });
            }
            DataCard::TallyTime { id, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("T{}", id),
                    context: format!("Time bins T{} for tally F{}", id, id),
                });
            }
            DataCard::TallyCosine { id, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("C{}", id),
                    context: format!("Cosine bins C{} for tally F{}", id, id),
                });
            }
            DataCard::TallyComment { id, comment } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("FC{}", id),
                    context: format!("Tally comment FC{}: {}", id, comment),
                });
            }
            DataCard::Dose(dose) => {
                let prefix = if dose.is_function { "DF" } else { "DE" };
                idx.tally_refs.entry(dose.id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("{}{}", prefix, dose.id),
                    context: format!("Dose card {}{} ({} values)", prefix, dose.id, dose.values.len()),
                });
            }
            DataCard::WeightWindowGenerator {
                tally_id,
                reference_cell,
                ..
            } => {
                idx.tally_refs.entry(*tally_id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: "WWG".to_string(),
                    context: format!("WWG target tally F{}", tally_id),
                });
                idx.cell_refs
                    .entry(*reference_cell)
                    .or_default()
                    .push(ReferenceLocation {
                        line_number: line,
                        source_card: "WWG".to_string(),
                        context: format!("WWG reference cell {}", reference_cell),
                    });
            }
            _ => {}
        }
    }

    idx
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectTarget {
    Cell(u32),
    Surface(u32),
    Material(u32),
    Universe(i32),
    Transform(u32),
    Tally(u32),
    Distribution(u32),
}

/// Parses a target selector string such as `c10`, `cell:10`, `s20`, `surf:20`, `m1`, `u1`, `f4`, `tr1`, `d1`.
pub fn parse_inspect_target(raw: &str) -> Result<InspectTarget, String> {
    let s = raw.trim().to_lowercase();
    if let Some((prefix, rest)) = s.split_once(':') {
        return match prefix {
            "c" | "cell" => rest
                .parse::<u32>()
                .map(InspectTarget::Cell)
                .map_err(|_| format!("Invalid cell ID in `{}`", raw)),
            "s" | "surf" | "surface" => rest
                .parse::<u32>()
                .map(InspectTarget::Surface)
                .map_err(|_| format!("Invalid surface ID in `{}`", raw)),
            "m" | "mat" | "material" => rest
                .parse::<u32>()
                .map(InspectTarget::Material)
                .map_err(|_| format!("Invalid material ID in `{}`", raw)),
            "u" | "univ" | "universe" => rest
                .parse::<i32>()
                .map(InspectTarget::Universe)
                .map_err(|_| format!("Invalid universe ID in `{}`", raw)),
            "tr" | "transform" => rest
                .parse::<u32>()
                .map(InspectTarget::Transform)
                .map_err(|_| format!("Invalid transform ID in `{}`", raw)),
            "f" | "tally" | "fmesh" => rest
                .parse::<u32>()
                .map(InspectTarget::Tally)
                .map_err(|_| format!("Invalid tally ID in `{}`", raw)),
            "d" | "dist" | "si" | "sp" | "sb" | "ds" => rest
                .parse::<u32>()
                .map(InspectTarget::Distribution)
                .map_err(|_| format!("Invalid distribution ID in `{}`", raw)),
            _ => Err(format!(
                "Unknown target prefix `{}:`. Use `c<id>`, `s<id>`, `m<id>`, `u<id>`, `f<id>`, `tr<id>`, or `d<id>`.",
                prefix
            )),
        };
    }

    for (prefix, kind) in [
        ("surface", "s"),
        ("surf", "s"),
        ("cell", "c"),
        ("material", "m"),
        ("mat", "m"),
        ("universe", "u"),
        ("univ", "u"),
        ("fmesh", "f"),
        ("tally", "f"),
        ("transform", "tr"),
        ("dist", "d"),
        ("tr", "tr"),
        ("si", "d"),
        ("sp", "d"),
        ("sb", "d"),
        ("ds", "d"),
        ("c", "c"),
        ("s", "s"),
        ("m", "m"),
        ("u", "u"),
        ("f", "f"),
        ("d", "d"),
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let clean = rest.trim_start_matches('=');
            if !clean.is_empty() {
                return match kind {
                    "c" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Cell)
                        .map_err(|_| format!("Invalid cell ID `{}`", raw)),
                    "s" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Surface)
                        .map_err(|_| format!("Invalid surface ID `{}`", raw)),
                    "m" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Material)
                        .map_err(|_| format!("Invalid material ID `{}`", raw)),
                    "u" => clean
                        .parse::<i32>()
                        .map(InspectTarget::Universe)
                        .map_err(|_| format!("Invalid universe ID `{}`", raw)),
                    "tr" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Transform)
                        .map_err(|_| format!("Invalid transform ID `{}`", raw)),
                    "f" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Tally)
                        .map_err(|_| format!("Invalid tally ID `{}`", raw)),
                    "d" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Distribution)
                        .map_err(|_| format!("Invalid distribution ID `{}`", raw)),
                    _ => unreachable!(),
                };
            }
        }
    }

    Err(format!(
        "Could not parse target `{}`. Examples: `c1` (Cell 1), `s10` (Surface 10), `m1` (Material 1), `u1` (Universe 1), `f4` (Tally 4), `tr1` (Transform 1), `d1` (Distribution 1).",
        raw
    ))
}

/// Inspects a specific target entity (`Cell`, `Surface`, `Material`, `Universe`, `Transform`, `Tally`, `Distribution`)
/// and returns its definitions and all cross-references across the input file.
pub fn inspect_target(content: &str, raw_target: &str) -> Result<InspectReport, String> {
    let target = parse_inspect_target(raw_target)?;
    Ok(inspect_target_entity(content, target, raw_target))
}

/// Inspects a resolved [`InspectTarget`] directly.
pub fn inspect_target_entity(
    content: &str,
    target: InspectTarget,
    query_label: &str,
) -> InspectReport {
    let lossy = parse_mcnp_input_lossy(content);
    let input = &lossy.input;
    let spans = &lossy.spans;
    let mut refs_idx = collect_all_references(input, spans);

    let mut definitions = Vec::new();
    let (canonical_target, references) = match target {
        InspectTarget::Cell(id) => {
            for (idx, c) in input.cells.iter().enumerate() {
                if c.id == id {
                    definitions.push(DefinitionLocation {
                        line_number: spans.cell_card_lines.get(idx).copied(),
                        card_kind: "CellCard".to_string(),
                        summary: summarize_cell(c),
                    });
                }
            }
            (
                format!("Cell {}", id),
                refs_idx.cell_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Surface(id) => {
            for (idx, s) in input.surfaces.iter().enumerate() {
                if s.id == id {
                    definitions.push(DefinitionLocation {
                        line_number: spans.surface_card_lines.get(idx).copied(),
                        card_kind: "SurfaceCard".to_string(),
                        summary: summarize_surface(s),
                    });
                }
            }
            (
                format!("Surface {}", id),
                refs_idx.surf_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Material(id) => {
            for (idx, card) in input.data_cards.iter().enumerate() {
                let line = spans.data_card_lines.get(idx).copied();
                match card {
                    DataCard::Material {
                        id: mid,
                        nuclides,
                        options,
                    } if *mid == id => {
                        let pairs: Vec<String> = nuclides
                            .iter()
                            .map(|n| format!("{}: {}", n.zaid, n.fraction))
                            .collect();
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("M{}", id),
                            summary: format!(
                                "Material M{}: {} nuclides [{}], options={:?}",
                                id,
                                nuclides.len(),
                                pairs.join(", "),
                                options
                            ),
                        });
                    }
                    DataCard::ThermalScattering { id: mid, tables } if *mid == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("MT{}", id),
                            summary: format!("Thermal Scattering MT{}: {:?}", id, tables),
                        });
                    }
                    _ => {}
                }
            }
            (
                format!("Material M{}", id),
                refs_idx.mat_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Universe(uid) => {
            for (idx, c) in input.cells.iter().enumerate() {
                if c.params.iter().any(|p| matches!(p, CellParam::Universe(u) if *u == uid)) {
                    definitions.push(DefinitionLocation {
                        line_number: spans.cell_card_lines.get(idx).copied(),
                        card_kind: format!("Cell {} (U={})", c.id, uid),
                        summary: summarize_cell(c),
                    });
                }
            }
            (
                format!("Universe U={}", uid),
                refs_idx.univ_refs.remove(&uid).unwrap_or_default(),
            )
        }
        InspectTarget::Transform(id) => {
            for (idx, card) in input.data_cards.iter().enumerate() {
                if let DataCard::Transform {
                    id: tid,
                    is_degrees,
                    values,
                } = card
                {
                    if *tid == id {
                        definitions.push(DefinitionLocation {
                            line_number: spans.data_card_lines.get(idx).copied(),
                            card_kind: format!("TR{}", id),
                            summary: format!(
                                "Transform TR{} (degrees={}): {:?}",
                                id, is_degrees, values
                            ),
                        });
                    }
                }
            }
            (
                format!("Transform TR{}", id),
                refs_idx.tr_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Tally(id) => {
            for (idx, card) in input.data_cards.iter().enumerate() {
                let line = spans.data_card_lines.get(idx).copied();
                match card {
                    DataCard::Tally {
                        id: tid,
                        kind,
                        particles,
                        targets,
                        total_bin,
                        ..
                    } if *tid == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("F{}", id),
                            summary: format!(
                                "Tally F{} ({:?}) particles={:?} targets={:?} total_bin={}",
                                id, kind, particles, targets, total_bin
                            ),
                        });
                    }
                    DataCard::DetectorTally {
                        id: tid,
                        axis,
                        particles,
                        detectors,
                        ..
                    } if *tid == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("F{}", id),
                            summary: format!(
                                "Detector Tally F{} (axis={:?}) particles={:?} detectors={:?}",
                                id, axis, particles, detectors
                            ),
                        });
                    }
                    DataCard::Fmesh(fm) if fm.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("FMESH{}", id),
                            summary: format!(
                                "Mesh Tally FMESH{} geom={:?} origin={:?} imesh={:?} jmesh={:?} kmesh={:?}",
                                id, fm.geom, fm.origin, fm.imesh, fm.jmesh, fm.kmesh
                            ),
                        });
                    }
                    _ => {}
                }
            }
            (
                format!("Tally {}", id),
                refs_idx.tally_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Distribution(id) => {
            for (idx, card) in input.data_cards.iter().enumerate() {
                let line = spans.data_card_lines.get(idx).copied();
                match card {
                    DataCard::Si(si) if si.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("SI{}", id),
                            summary: format!(
                                "Source Info SI{} (option={:?}, {} entries): {:?}",
                                id,
                                si.option,
                                si.entries.len(),
                                si.entries
                            ),
                        });
                    }
                    DataCard::Sp(sp) if sp.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("SP{}", id),
                            summary: format!("Source Probability SP{}: {:?}", id, sp.content),
                        });
                    }
                    DataCard::Sb(sb) if sb.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("SB{}", id),
                            summary: format!("Source Bias SB{}: {:?}", id, sb.content),
                        });
                    }
                    DataCard::Ds(ds) if ds.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("DS{}", id),
                            summary: format!(
                                "Dependent Source DS{} (option={:?}): entries={:?} pairs={:?}",
                                id, ds.option, ds.entries, ds.pairs
                            ),
                        });
                    }
                    DataCard::Sc { id: sid, comment } if *sid == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("SC{}", id),
                            summary: format!("Source Comment SC{}: {}", id, comment),
                        });
                    }
                    _ => {}
                }
            }
            (
                format!("Distribution D{}", id),
                refs_idx.dist_refs.remove(&id).unwrap_or_default(),
            )
        }
    };

    InspectReport {
        query: query_label.to_string(),
        canonical_target,
        found: !definitions.is_empty(),
        definitions,
        references,
    }
}

fn summarize_cell(c: &CellCard) -> String {
    match &c.definition {
        CellDefinition::Normal {
            material_id,
            density,
            geometry,
        } => {
            format!(
                "Cell {}: mat={}, density={:?}, geometry={:?}, params={:?}",
                c.id, material_id, density, geometry, c.params
            )
        }
        CellDefinition::LikeBut { base_cell_id } => {
            format!(
                "Cell {}: LIKE {} BUT params={:?}",
                c.id, base_cell_id, c.params
            )
        }
    }
}

fn summarize_surface(s: &SurfaceCard) -> String {
    format!(
        "Surface {}: boundary={:?}, transform={:?}, kind={:?}, params={:?}",
        s.id, s.boundary, s.transform_id, s.kind, s.params
    )
}

/// Formats an [`InspectReport`] as human/LLM-friendly text.
pub fn format_inspect_report(rep: &InspectReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== Inspect: {} ===\n", rep.canonical_target));
    if rep.definitions.is_empty() {
        out.push_str("Definitions: NOT DEFINED in this input file.\n");
    } else {
        out.push_str(&format!("Definitions ({}):\n", rep.definitions.len()));
        for def in &rep.definitions {
            let line = def
                .line_number
                .map(|l| format!("Line {}", l))
                .unwrap_or_else(|| "Line ?".to_string());
            out.push_str(&format!("  - [{}] {}\n", line, def.summary));
        }
    }

    if rep.references.is_empty() {
        out.push_str("References (0): Not referenced by any card.\n");
    } else {
        out.push_str(&format!("References ({}):\n", rep.references.len()));
        for r in &rep.references {
            let line = r
                .line_number
                .map(|l| format!("Line {}", l))
                .unwrap_or_else(|| "Line ?".to_string());
            out.push_str(&format!(
                "  - [{}] {} -> {}\n",
                line, r.source_card, r.context
            ));
        }
    }
    out
}

// ============================================================================
// 4. Cursor Position & Block Context Resolution (LSP hover / definition / refs)
// ============================================================================

/// Identifies which MCNP input block a given line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum McnpBlockKind {
    Header,
    Cells,
    Surfaces,
    Data,
    Trailing,
}

/// Result of resolving a cursor position `(line, character)` in an MCNP document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCursorTarget {
    /// A cross-referenceable MCNP entity (Cell, Surface, Material, Universe, Transform, Tally, Distribution).
    Entity {
        target: InspectTarget,
        token: String,
    },
    /// A built-in MCNP surface mnemonic, card keyword, or parameter with documentation.
    Keyword {
        keyword: String,
        summary: String,
        documentation: String,
    },
}

/// Determines which MCNP block `line_0based` belongs to.
pub fn detect_block_at_line(content: &str, line_0based: u32) -> McnpBlockKind {
    let lines: Vec<&str> = content.lines().collect();
    let target_idx = line_0based as usize;
    let total = lines.len();

    let mut cursor = 0;
    while cursor < total && lines[cursor].trim().is_empty() {
        if cursor == target_idx {
            return McnpBlockKind::Header;
        }
        cursor += 1;
    }

    if cursor >= total {
        return McnpBlockKind::Header;
    }

    // Optional message block
    let first_trimmed = lines[cursor].trim_start();
    let leading_ws = lines[cursor].len() - first_trimmed.len();
    if leading_ws < 5
        && first_trimmed
            .get(..8)
            .map(|s| s.eq_ignore_ascii_case("message:"))
            .unwrap_or(false)
    {
        if target_idx <= cursor {
            return McnpBlockKind::Header;
        }
        cursor += 1;
        while cursor < total && !lines[cursor].trim().is_empty() {
            if cursor == target_idx {
                return McnpBlockKind::Header;
            }
            cursor += 1;
        }
        if cursor < total && lines[cursor].trim().is_empty() {
            if cursor == target_idx {
                return McnpBlockKind::Header;
            }
            cursor += 1;
        }
    }

    // Title line
    if target_idx <= cursor {
        return McnpBlockKind::Header;
    }
    cursor += 1;

    // Track blocks separated by blank lines
    let mut block_idx = 0usize; // 0 = Cells, 1 = Surfaces, 2 = Data, >=3 = Trailing
    while cursor < total {
        let is_blank = lines[cursor].trim().is_empty();
        if cursor == target_idx {
            return match block_idx {
                0 => McnpBlockKind::Cells,
                1 => McnpBlockKind::Surfaces,
                2 => McnpBlockKind::Data,
                _ => McnpBlockKind::Trailing,
            };
        }
        if is_blank {
            block_idx += 1;
        }
        cursor += 1;
    }

    match block_idx {
        0 => McnpBlockKind::Cells,
        1 => McnpBlockKind::Surfaces,
        2 => McnpBlockKind::Data,
        _ => McnpBlockKind::Trailing,
    }
}

#[derive(Debug, Clone)]
struct SpanToken {
    text: String,
    line_0based: u32,
    start_col: u32,
    end_col: u32,
}

fn tokenize_line_with_cols(line_0based: u32, code_slice: &str) -> Vec<SpanToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start_col = 0u32;

    for (col_idx, ch) in code_slice.chars().enumerate() {
        let col = col_idx as u32;
        if ch.is_whitespace() || ch == '(' || ch == ')' || ch == ',' {
            if !current.is_empty() {
                tokens.push(SpanToken {
                    text: std::mem::take(&mut current),
                    line_0based,
                    start_col,
                    end_col: col,
                });
            }
        } else {
            if current.is_empty() {
                start_col = col;
            }
            current.push(ch);
        }
    }
    if !current.is_empty() {
        tokens.push(SpanToken {
            text: current,
            line_0based,
            start_col,
            end_col: code_slice.chars().count() as u32,
        });
    }
    tokens
}

/// Collects the full sequence of tokens for the logical card containing `target_line_0based`.
fn extract_logical_card_tokens_at_line(content: &str, target_line_0based: u32) -> Vec<SpanToken> {
    let lines: Vec<&str> = content.lines().collect();
    let target_idx = target_line_0based as usize;
    if target_idx >= lines.len() {
        return Vec::new();
    }

    let target_block = detect_block_at_line(content, target_line_0based);
    if !matches!(
        target_block,
        McnpBlockKind::Cells | McnpBlockKind::Surfaces | McnpBlockKind::Data
    ) {
        return Vec::new();
    }

    // Find start and end line indices of the current block
    let mut block_start = target_idx;
    while block_start > 0 {
        if lines[block_start - 1].trim().is_empty()
            || detect_block_at_line(content, (block_start - 1) as u32) != target_block
        {
            break;
        }
        block_start -= 1;
    }
    let mut block_end = target_idx;
    while block_end + 1 < lines.len() && !lines[block_end + 1].trim().is_empty() {
        block_end += 1;
    }

    // Group lines in [block_start..=block_end] into logical cards
    let mut cards: Vec<(usize, usize, Vec<SpanToken>)> = Vec::new();
    let mut force_continue_next = false;

    for line_idx in block_start..=block_end {
        let raw_line = lines[line_idx];
        if crate::preprocess::is_comment_line(raw_line) {
            continue;
        }
        let without_dollar = crate::preprocess::strip_inline_comment(raw_line);
        let trimmed_end = without_dollar.trim_end();
        if trimmed_end.trim().is_empty() {
            continue;
        }

        let is_indented = crate::preprocess::is_indented_continuation(without_dollar);
        let (clean_slice, ends_with_amp) = if let Some(stripped) = trimmed_end.strip_suffix('&') {
            (stripped, true)
        } else {
            (trimmed_end, false)
        };

        let line_tokens = tokenize_line_with_cols(line_idx as u32, clean_slice);
        if (force_continue_next || is_indented) && !cards.is_empty() {
            let last = cards.last_mut().unwrap();
            last.1 = line_idx;
            last.2.extend(line_tokens);
        } else {
            cards.push((line_idx, line_idx, line_tokens));
        }

        force_continue_next = ends_with_amp;
    }

    for (start_l, end_l, toks) in cards {
        if target_idx >= start_l && target_idx <= end_l {
            return toks;
        }
    }
    Vec::new()
}

/// Resolves the MCNP symbol or keyword under the cursor at `(line_0based, character_0based)`.
pub fn resolve_symbol_at_position(
    content: &str,
    line_0based: u32,
    character_0based: u32,
) -> Option<ResolvedCursorTarget> {
    let lines: Vec<&str> = content.lines().collect();
    let raw_line = *lines.get(line_0based as usize)?;
    if crate::preprocess::is_comment_line(raw_line) {
        return None;
    }

    let without_dollar = crate::preprocess::strip_inline_comment(raw_line);
    if (character_0based as usize) > without_dollar.chars().count() {
        return None;
    }

    let block = detect_block_at_line(content, line_0based);
    let card_tokens = extract_logical_card_tokens_at_line(content, line_0based);
    if card_tokens.is_empty() {
        return None;
    }

    // Find the token on `line_0based` covering `character_0based`
    let tok_idx = card_tokens.iter().position(|t| {
        t.line_0based == line_0based
            && character_0based >= t.start_col
            && character_0based <= t.end_col
    })?;
    let tok = &card_tokens[tok_idx];
    let raw_tok = tok.text.as_str();
    let lower_tok = raw_tok.to_ascii_lowercase();

    // 1. Check `key=val` token (e.g. `u=1`, `fill=2`, `trcl=1`, `mat=1`, `erg=d1`, `cel=1`, `sur=10`, `imp:n=1.0`)
    if let Some((key_part, val_part)) = lower_tok.split_once('=') {
        let base_key = key_part.split(':').next().unwrap_or(key_part).trim_start_matches('*');
        if let Some(target) = resolve_key_value_entity(base_key, val_part) {
            return Some(ResolvedCursorTarget::Entity {
                target,
                token: raw_tok.to_string(),
            });
        }
        if let Some(kw) = lookup_keyword_docs(base_key) {
            return Some(kw);
        }
    }

    // 2. Check `#<cell_id>` complement syntax
    if let Some(rest) = lower_tok.strip_prefix('#') {
        if let Ok(cid) = rest.parse::<u32>() {
            if cid > 0 {
                return Some(ResolvedCursorTarget::Entity {
                    target: InspectTarget::Cell(cid),
                    token: raw_tok.to_string(),
                });
            }
        }
    }

    // 3. Check explicit prefixed tokens (`m1`, `mt1`, `tr1`, `*tr1`, `f4:n`, `fmesh14:n`, `e4`, `fm4`, `si1`, `sp1`, `d1`, etc.)
    let head_before_colon = lower_tok.split(':').next().unwrap_or(&lower_tok);
    if let Some(target) = parse_explicit_prefixed_token(head_before_colon, block, tok_idx) {
        return Some(ResolvedCursorTarget::Entity {
            target,
            token: raw_tok.to_string(),
        });
    }

    // 4. Context-sensitive resolution by block kind
    match block {
        McnpBlockKind::Cells => resolve_in_cell_card(&card_tokens, tok_idx),
        McnpBlockKind::Surfaces => resolve_in_surface_card(&card_tokens, tok_idx),
        McnpBlockKind::Data => resolve_in_data_card(&card_tokens, tok_idx),
        _ => None,
    }
}

fn parse_distribution_ref(val: &str) -> Option<u32> {
    let s = val.trim().to_ascii_lowercase();
    let d_part = if let Some(rest) = s.strip_prefix('d') {
        rest
    } else if let Some((_, after_d)) = s.rsplit_once('d') {
        if s.starts_with('f') {
            after_d
        } else {
            return None;
        }
    } else {
        return None;
    };
    d_part.parse::<u32>().ok().filter(|&id| id > 0)
}

fn resolve_key_value_entity(base_key: &str, val_part: &str) -> Option<InspectTarget> {
    if val_part.is_empty() {
        return None;
    }
    if let Some(d_id) = parse_distribution_ref(val_part) {
        return Some(InspectTarget::Distribution(d_id));
    }
    match base_key {
        "u" => val_part.parse::<i32>().ok().map(InspectTarget::Universe),
        "fill" => {
            if !val_part.contains(':') {
                val_part
                    .parse::<i32>()
                    .ok()
                    .filter(|&u| u > 0)
                    .map(InspectTarget::Universe)
            } else {
                None
            }
        }
        "trcl" | "tr" => val_part
            .parse::<u32>()
            .ok()
            .filter(|&id| id > 0)
            .map(InspectTarget::Transform),
        "mat" => val_part
            .parse::<u32>()
            .ok()
            .filter(|&id| id > 0)
            .map(InspectTarget::Material),
        "cel" => val_part
            .parse::<u32>()
            .ok()
            .filter(|&id| id > 0)
            .map(InspectTarget::Cell),
        "sur" => val_part
            .trim_start_matches(['+', '-'])
            .split('.')
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .filter(|&id| id > 0)
            .map(InspectTarget::Surface),
        _ => None,
    }
}

fn parse_explicit_prefixed_token(
    head: &str,
    block: McnpBlockKind,
    tok_idx: usize,
) -> Option<InspectTarget> {
    let clean = head.trim_start_matches('*');
    if let Some(rest) = clean.strip_prefix("fmesh") {
        return rest
            .parse::<u32>()
            .ok()
            .filter(|&id| id > 0)
            .map(InspectTarget::Tally);
    }
    if let Some(rest) = clean.strip_prefix("mt") {
        return rest
            .parse::<u32>()
            .ok()
            .filter(|&id| id > 0)
            .map(InspectTarget::Material);
    }
    if let Some(rest) = clean.strip_prefix("tr") {
        if rest != "cl" {
            return rest
                .parse::<u32>()
                .ok()
                .filter(|&id| id > 0)
                .map(InspectTarget::Transform);
        }
    }
    for dist_prefix in ["si", "sp", "sb", "ds", "sc"] {
        if let Some(rest) = clean.strip_prefix(dist_prefix) {
            if let Ok(id) = rest.parse::<u32>() {
                if id > 0 {
                    return Some(InspectTarget::Distribution(id));
                }
            }
        }
    }
    if let Some(rest) = clean.strip_prefix('d') {
        if let Ok(id) = rest.parse::<u32>() {
            if id > 0 {
                return Some(InspectTarget::Distribution(id));
            }
        }
    }
    if let Some(rest) = clean.strip_prefix('m') {
        if let Ok(id) = rest.parse::<u32>() {
            if id > 0 {
                return Some(InspectTarget::Material(id));
            }
        }
    }
    for tally_prefix in ["fx", "fy", "fz", "de", "df", "fm", "fs", "sd", "fc"] {
        if let Some(rest) = clean.strip_prefix(tally_prefix) {
            if let Ok(id) = rest.parse::<u32>() {
                if id > 0 {
                    return Some(InspectTarget::Tally(id));
                }
            }
        }
    }
    if block == McnpBlockKind::Data && tok_idx == 0 {
        for mod_prefix in ["f", "e", "t", "c"] {
            if let Some(rest) = clean.strip_prefix(mod_prefix) {
                if let Ok(id) = rest.parse::<u32>() {
                    if id > 0 {
                        return Some(InspectTarget::Tally(id));
                    }
                }
            }
        }
    } else if let Some(rest) = clean.strip_prefix('f') {
        if let Ok(id) = rest.parse::<u32>() {
            if id > 0 {
                return Some(InspectTarget::Tally(id));
            }
        }
    }
    None
}

fn is_cell_param_token(lower: &str) -> bool {
    if lower.contains('=') {
        return true;
    }
    let base = lower.split(':').next().unwrap_or(lower).trim_start_matches('*');
    matches!(
        base,
        "imp"
            | "vol"
            | "pwt"
            | "ext"
            | "fcl"
            | "wwn"
            | "dxc"
            | "nonu"
            | "pd"
            | "tmp"
            | "u"
            | "trcl"
            | "lat"
            | "fill"
            | "elpt"
            | "cosy"
            | "bflcl"
            | "unc"
            | "mat"
            | "rho"
    )
}

fn parse_surface_or_facet_id(raw: &str) -> Option<u32> {
    let clean = raw.trim_start_matches(['+', '-', ':']);
    let clean = clean.trim_end_matches(':');
    if clean.is_empty() {
        return None;
    }
    if let Some((surf_part, facet_part)) = clean.split_once('.') {
        let sid = surf_part.parse::<u32>().ok().filter(|&n| n > 0)?;
        let _facet = facet_part.parse::<u32>().ok().filter(|&f| (1..=8).contains(&f))?;
        Some(sid)
    } else {
        clean.parse::<u32>().ok().filter(|&n| n > 0)
    }
}

fn resolve_in_cell_card(card_tokens: &[SpanToken], tok_idx: usize) -> Option<ResolvedCursorTarget> {
    let raw_tok = &card_tokens[tok_idx].text;
    let lower_tok = raw_tok.to_ascii_lowercase();

    // Token 0: Cell ID
    if tok_idx == 0 {
        if let Ok(cid) = lower_tok.parse::<u32>() {
            return Some(ResolvedCursorTarget::Entity {
                target: InspectTarget::Cell(cid),
                token: raw_tok.clone(),
            });
        }
    }

    let is_like = card_tokens
        .get(1)
        .map(|t| t.text.eq_ignore_ascii_case("like"))
        .unwrap_or(false);

    if is_like {
        if tok_idx == 1 || lower_tok == "but" {
            return lookup_keyword_docs("like");
        }
        if tok_idx == 2 {
            if let Ok(base_id) = lower_tok.parse::<u32>() {
                return Some(ResolvedCursorTarget::Entity {
                    target: InspectTarget::Cell(base_id),
                    token: raw_tok.clone(),
                });
            }
        }
    } else {
        // Normal cell: token 1 is material_id
        let mat_id = card_tokens
            .get(1)
            .and_then(|t| t.text.parse::<u32>().ok())
            .unwrap_or(0);

        if tok_idx == 1 {
            if mat_id > 0 {
                return Some(ResolvedCursorTarget::Entity {
                    target: InspectTarget::Material(mat_id),
                    token: raw_tok.clone(),
                });
            } else if lower_tok == "0" {
                return Some(ResolvedCursorTarget::Keyword {
                    keyword: "0 (Void Cell)".to_string(),
                    summary: "Void cell material (`mat = 0`)".to_string(),
                    documentation: "Material ID `0` indicates a void (vacuum) cell. Void cells must **not** specify a density token before the geometry expression.".to_string(),
                });
            }
        }

        let geom_start_idx = if mat_id == 0 { 2 } else { 3 };
        if mat_id > 0 && tok_idx == 2 {
            return Some(ResolvedCursorTarget::Keyword {
                keyword: format!("Density ({})", raw_tok),
                summary: "Cell material density".to_string(),
                documentation: "Negative values specify mass density in **g/cm³**; positive values specify atomic density in **atoms/(barn·cm)** (10²⁴ atoms/cm³).".to_string(),
            });
        }

        let param_start_idx = card_tokens
            .iter()
            .enumerate()
            .skip(geom_start_idx)
            .find(|(_, t)| is_cell_param_token(&t.text.to_ascii_lowercase()))
            .map(|(i, _)| i)
            .unwrap_or(card_tokens.len());

        // Inside geometry expression
        if tok_idx >= geom_start_idx && tok_idx < param_start_idx {
            // Handle union colon glued to surface IDs (e.g. `10:20` or `-10`)
            for part in lower_tok.split(':') {
                if let Some(sid) = parse_surface_or_facet_id(part) {
                    return Some(ResolvedCursorTarget::Entity {
                        target: InspectTarget::Surface(sid),
                        token: raw_tok.clone(),
                    });
                }
            }
        }

        // Inside cell parameters (e.g. `fill=` array elements or space-separated `u = 1`)
        if tok_idx >= param_start_idx {
            if let Some(kw) = lookup_keyword_docs(lower_tok.split(':').next().unwrap_or(&lower_tok)) {
                return Some(kw);
            }
            // Check if preceded by `fill=`
            let mut active_param_key = None;
            for prev in &card_tokens[param_start_idx..tok_idx] {
                let pl = prev.text.to_ascii_lowercase();
                if let Some((k, _)) = pl.split_once('=') {
                    active_param_key = Some(k.split(':').next().unwrap_or(k).trim_start_matches('*').to_string());
                } else if is_cell_param_token(&pl) {
                    active_param_key = Some(pl.split(':').next().unwrap_or(&pl).trim_start_matches('*').to_string());
                }
            }
            if let Some(ref key) = active_param_key {
                if let Some(target) = resolve_key_value_entity(key, lower_tok.trim_start_matches('=')) {
                    return Some(ResolvedCursorTarget::Entity {
                        target,
                        token: raw_tok.clone(),
                    });
                }
            }
        }
    }

    None
}

fn resolve_in_surface_card(
    card_tokens: &[SpanToken],
    tok_idx: usize,
) -> Option<ResolvedCursorTarget> {
    let raw_tok = &card_tokens[tok_idx].text;
    let lower_tok = raw_tok.to_ascii_lowercase();

    // Token 0: `[*/+]<surf_id>`
    if tok_idx == 0 {
        let clean = lower_tok.trim_start_matches(['*', '+']);
        if let Ok(sid) = clean.parse::<u32>() {
            return Some(ResolvedCursorTarget::Entity {
                target: InspectTarget::Surface(sid),
                token: raw_tok.clone(),
            });
        }
    }

    // Token 1: either `tr_id` (integer) or surface mnemonic
    if tok_idx == 1 {
        if let Ok(tr_or_peer) = lower_tok.parse::<i32>() {
            if tr_or_peer > 0 {
                return Some(ResolvedCursorTarget::Entity {
                    target: InspectTarget::Transform(tr_or_peer as u32),
                    token: raw_tok.clone(),
                });
            } else if tr_or_peer < 0 {
                return Some(ResolvedCursorTarget::Entity {
                    target: InspectTarget::Surface(tr_or_peer.unsigned_abs()),
                    token: raw_tok.clone(),
                });
            }
        }
    }

    if matches!(tok_idx, 1 | 2) {
        if let Some(kw) = lookup_surface_mnemonic_docs(&lower_tok) {
            return Some(kw);
        }
    }

    None
}

fn resolve_in_data_card(card_tokens: &[SpanToken], tok_idx: usize) -> Option<ResolvedCursorTarget> {
    let raw_tok = &card_tokens[tok_idx].text;
    let lower_tok = raw_tok.to_ascii_lowercase();
    let head_lower = card_tokens[0].text.to_ascii_lowercase();
    let head_base = head_lower
        .split(':')
        .next()
        .unwrap_or(&head_lower)
        .trim_start_matches('*');

    if tok_idx == 0 {
        if let Some(kw) = lookup_keyword_docs(head_base) {
            return Some(kw);
        }
    }

    // Check space-separated `key = val` in SDEF/FMESH
    if tok_idx > 0 {
        let prev = card_tokens[tok_idx - 1].text.to_ascii_lowercase();
        let prev_key = prev.trim_end_matches('=');
        if let Some(target) = resolve_key_value_entity(prev_key, lower_tok.trim_start_matches('=')) {
            return Some(ResolvedCursorTarget::Entity {
                target,
                token: raw_tok.clone(),
            });
        }
    }

    // Tally target cards: `F<n>` and `FS<n>`
    if let Some(rest) = head_base.strip_prefix("fs") {
        if rest.parse::<u32>().is_ok() && tok_idx > 0 {
            if let Some(sid) = parse_surface_or_facet_id(&lower_tok) {
                return Some(ResolvedCursorTarget::Entity {
                    target: InspectTarget::Surface(sid),
                    token: raw_tok.clone(),
                });
            }
        }
    } else if let Some(rest) = head_base.strip_prefix('f') {
        if let Ok(tally_id) = rest.parse::<u32>() {
            let rem = tally_id % 10;
            if matches!(rem, 1 | 2) && tok_idx > 0 {
                if let Some(sid) = parse_surface_or_facet_id(&lower_tok) {
                    return Some(ResolvedCursorTarget::Entity {
                        target: InspectTarget::Surface(sid),
                        token: raw_tok.clone(),
                    });
                }
            } else if matches!(rem, 4 | 6 | 7 | 8) && tok_idx > 0 {
                if let Ok(cid) = lower_tok.parse::<u32>() {
                    if cid > 0 {
                        return Some(ResolvedCursorTarget::Entity {
                            target: InspectTarget::Cell(cid),
                            token: raw_tok.clone(),
                        });
                    }
                }
            }
        }
    }

    // WWG `<tally_id> <reference_cell_id> ...`
    if head_base == "wwg" {
        if tok_idx == 1 {
            if let Ok(tid) = lower_tok.parse::<u32>() {
                if tid > 0 {
                    return Some(ResolvedCursorTarget::Entity {
                        target: InspectTarget::Tally(tid),
                        token: raw_tok.clone(),
                    });
                }
            }
        } else if tok_idx == 2 {
            if let Ok(cid) = lower_tok.parse::<u32>() {
                if cid > 0 {
                    return Some(ResolvedCursorTarget::Entity {
                        target: InspectTarget::Cell(cid),
                        token: raw_tok.clone(),
                    });
                }
            }
        }
    }

    if let Some(kw) = lookup_keyword_docs(lower_tok.split(':').next().unwrap_or(&lower_tok)) {
        return Some(kw);
    }

    None
}

fn lookup_surface_mnemonic_docs(mnemonic: &str) -> Option<ResolvedCursorTarget> {
    let (title, params, desc) = match mnemonic {
        "px" => ("PX — Plane Normal to X-Axis", "`D` (1 param)", "Equation: $x - D = 0$. Sense `-s` is $x < D$, `+s` is $x > D$."),
        "py" => ("PY — Plane Normal to Y-Axis", "`D` (1 param)", "Equation: $y - D = 0$. Sense `-s` is $y < D$, `+s` is $y > D$."),
        "pz" => ("PZ — Plane Normal to Z-Axis", "`D` (1 param)", "Equation: $z - D = 0$. Sense `-s` is $z < D$, `+s` is $z > D$."),
        "p" => ("P — General Plane", "`A B C D` (4 params) or 3 points (9 params)", "Equation: $Ax + By + Cz - D = 0$."),
        "so" => ("SO — Sphere Centered at Origin", "`R` (1 param, $R > 0$)", "Equation: $x^2 + y^2 + z^2 - R^2 = 0$. Sense `-s` is inside sphere."),
        "s" => ("S — General Sphere", "`x0 y0 z0 R` (4 params, $R > 0$)", "Sphere centered at `(x0, y0, z0)` with radius `R`."),
        "sx" | "sy" | "sz" => ("SX/SY/SZ — Sphere Centered on Axis", "`coord R` (2 params, $R > 0$)", "Sphere centered on X/Y/Z axis at `coord` with radius `R`."),
        "cx" | "cy" | "cz" => ("CX/CY/CZ — Cylinder on Axis", "`R` (1 param, $R > 0$)", "Infinite cylinder along X/Y/Z axis with radius `R`. Sense `-s` is inside cylinder."),
        "c/x" | "c/y" | "c/z" => ("C/X, C/Y, C/Z — Cylinder Parallel to Axis", "`c1 c2 R` (3 params, $R > 0$)", "Infinite cylinder parallel to X/Y/Z axis centered at `(c1, c2)` with radius `R`."),
        "kx" | "ky" | "kz" => ("KX/KY/KZ — Cone on Axis", "`coord t^2 [±1]` (2 or 3 params, $t^2 > 0$)", "Cone on X/Y/Z axis with apex at `coord` and $\\tan^2\\theta = t^2$."),
        "k/x" | "k/y" | "k/z" => ("K/X, K/Y, K/Z — Cone Parallel to Axis", "`x0 y0 z0 t^2 [±1]` (4 or 5 params, $t^2 > 0$)", "Cone parallel to X/Y/Z axis with apex at `(x0, y0, z0)`."),
        "sq" => ("SQ — Axis-Aligned Quadric", "`A B C D E F G x0 y0 z0` (10 params)", "Special quadric parallel to coordinate axes."),
        "gq" => ("GQ — General Quadric", "`A B C D E F G H J K` (10 params)", "General second-degree surface."),
        "tx" | "ty" | "tz" => ("TX/TY/TZ — Elliptical Torus", "`x0 y0 z0 A B C` (6 params, $A,B,C > 0$)", "Torus symmetric about axis parallel to X/Y/Z."),
        "x" | "y" | "z" => ("X/Y/Z — Axisymmetric Surface by Points", "`c1 r1 [c2 r2 [c3 r3]]` (2, 4, or 6 params)", "Surface symmetric about X/Y/Z axis defined by 1 to 3 `(coord, radius)` pairs."),
        "rpp" => ("RPP — Rectangular Parallelepiped Macrobody", "`xmin xmax ymin ymax zmin zmax` (6 params)", "Axis-aligned box. Facets: `.1` ($x_{\\max}$), `.2` ($x_{\\min}$), `.3` ($y_{\\max}$), `.4` ($y_{\\min}$), `.5` ($z_{\\max}$), `.6` ($z_{\\min}$)."),
        "box" => ("BOX — Orthogonal Box Macrobody", "`vx vy vz a1x a1y a1z a2x a2y a2z a3x a3y a3z` (12 params)", "Corner `(vx,vy,vz)` and 3 orthogonal edge vectors. Facets: `.1`..`.6`."),
        "sph" => ("SPH — Sphere Macrobody", "`vx vy vz R` (4 params, $R > 0$)", "Sphere macrobody. Facet: `.1`."),
        "rcc" => ("RCC — Right Circular Cylinder Macrobody", "`vx vy vz hx hy hz R` (7 params, $R > 0$)", "Base center `(vx,vy,vz)`, height axis vector `(hx,hy,hz)`, radius `R`. Facets: `.1` (side), `.2` (top), `.3` (bottom)."),
        "trc" => ("TRC — Truncated Right Cone Macrobody", "`vx vy vz hx hy hz R1 R2` (8 params)", "Base radius `R1`, top radius `R2`. Facets: `.1` (side), `.2` (top), `.3` (bottom)."),
        "rec" => ("REC — Right Elliptical Cylinder Macrobody", "`vx vy vz hx hy hz v1x v1y v1z v2x v2y v2z` (10 or 12 params)", "Elliptical cylinder macrobody. Facets: `.1` (side), `.2` (top), `.3` (bottom)."),
        "rhp" | "hex" => ("RHP/HEX — Right Hexagonal Prism Macrobody", "`vx vy vz hx hy hz r1x r1y r1z ...` (9 or 15 params)", "Hexagonal prism macrobody. Facets: `.1`..`.6` (sides), `.7` (top), `.8` (bottom)."),
        "ell" => ("ELL — Ellipsoid Macrobody", "`v1x v1y v1z v2x v2y v2z Rm` (7 params)", "Ellipsoid macrobody. Facet: `.1`."),
        "wed" => ("WED — Right Angle Wedge Macrobody", "`vx vy vz v1x v1y v1z v2x v2y v2z v3x v3y v3z` (12 params)", "Wedge macrobody. Facets: `.1`..`.5`."),
        "arb" => ("ARB — Arbitrary Polyhedron Macrobody", "24 corner coordinates + 6 face 4-digit numbers (30 params)", "Polyhedron macrobody. Facets: `.1`..`.6`."),
        _ => return None,
    };

    Some(ResolvedCursorTarget::Keyword {
        keyword: mnemonic.to_ascii_uppercase(),
        summary: title.to_string(),
        documentation: format!("**Parameters**: {}\n\n{}", params, desc),
    })
}

fn lookup_keyword_docs(key: &str) -> Option<ResolvedCursorTarget> {
    if let Some(surf_kw) = lookup_surface_mnemonic_docs(key) {
        return Some(surf_kw);
    }
    let (title, doc) = match key {
        "mode" => ("MODE — Particle Transport Mode", "Specifies transported particles (e.g. `mode n p e`). Defaults to `mode n` if omitted."),
        "sdef" => ("SDEF — Fixed-Source Definition", "Defines fixed-source parameters (`POS`, `ERG`, `PAR`, `CEL`, `SUR`, `RAD`, `EXT`, `AXS`, `DIR`, `VEC`, `NRM`, `WGT`, `TME`, `TR`). Cannot be combined with `KCODE`."),
        "kcode" => ("KCODE — Criticality Source Card", "Syntax: `kcode <nsrck> <rkk> <ikz> <kct>`\n- `nsrck`: nominal histories per cycle (`> 0`)\n- `rkk`: initial $k_{\\text{eff}}$ guess (`> 0`)\n- `ikz`: skipped cycles before active accumulation\n- `kct`: total cycles (`kct > ikz`)"),
        "ksrc" => ("KSRC — Initial Fission Source Points", "Syntax: `ksrc x1 y1 z1 [x2 y2 z2 ...]`\nSupplies starting fission coordinates for `KCODE`."),
        "nps" => ("NPS — Number of Particle Histories", "Syntax: `nps <count>`\nTerminates fixed-source simulation after `<count>` histories."),
        "imp" => ("IMP — Cell Particle Importance", "Syntax: `imp:n,p=1.0` (cell param) or `imp:n v1 v2 ...` (Block 3 card).\nSet `imp=0` for the exterior graveyard cell to terminate escaping particles."),
        "vol" => ("VOL — Cell Volume [cm³]", "Explicitly specifies cell volume in cm³ for track-length tally normalization."),
        "tmp" => ("TMP — Cell Thermal Temperature [MeV]", "Specifies free-gas thermal temperature $k_B T$ in MeV (room temperature $293.6\\text{ K} \\approx 2.53\\times 10^{-8}\\text{ MeV}$)."),
        "lat" => ("LAT — Lattice Type", "`LAT=1` for hexahedral (rectangular) prism lattice; `LAT=2` for hexagonal prism lattice. Requires `FILL`."),
        "like" => ("LIKE n BUT — Cell Cloning Syntax", "Syntax: `<id> LIKE <base_cell_id> BUT <key=val ...>`\nCopies geometry and attributes from Normal cell `<base_cell_id>` with parameter overrides."),
        "wwg" => ("WWG — Weight Window Generator", "Syntax: `wwg <tally_id> <reference_cell_id> <target_lower_bound>`\nGenerates optimum weight windows for `<tally_id>`."),
        "wwge" => ("WWGE — Weight Window Generator Energy Bins", "Syntax: `wwge:<p> e1 e2 ...`\nDefines monotonically increasing energy bin upper bounds for `WWG`."),
        "wwe" => ("WWE — Weight Window Energy/Time Bounds", "Syntax: `wwe:<p> e1 e2 ...`\nDefines energy intervals for cell-based weight window cards `WWN<g>:<p>`."),
        "wwp" => ("WWP — Weight Window Parameters", "Syntax: `wwp:<p> wupn wsurvn mxspln mwhere switchn`"),
        "phys" => ("PHYS — Energy Physics Cutoff & Options", "Syntax: `phys:<p> <emax> ...`\nControls upper energy limits and physics model options."),
        "cut" => ("CUT — Particle Cutoff Card", "Syntax: `cut:<p> <time_cutoff> <energy_cutoff> <wc1> <wc2> [swtm]`"),
        _ => return None,
    };

    Some(ResolvedCursorTarget::Keyword {
        keyword: key.to_ascii_uppercase(),
        summary: title.to_string(),
        documentation: doc.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::{get_template, list_templates};

    #[test]
    fn test_all_skeleton_templates_validate_cleanly() {
        for topic in list_templates() {
            if topic.name.starts_with("skeleton-") {
                let report = analyze_diagnostics(topic.content);
                assert_eq!(
                    report.error_count, 0,
                    "Template `{}` had errors: {:?}",
                    topic.name, report.diagnostics
                );
                assert_eq!(
                    report.warning_count, 0,
                    "Template `{}` had warnings: {:?}",
                    topic.name, report.diagnostics
                );
            }
        }
    }

    #[test]
    fn test_symbols_and_next_available_ids() {
        let tpl = get_template("skeleton-sdef").expect("skeleton-sdef should exist");
        let symbols = analyze_symbols(tpl.content);
        assert_eq!(symbols.cells.len(), 4);
        assert_eq!(symbols.surfaces.len(), 3);
        assert_eq!(symbols.materials.len(), 2);
        assert_eq!(symbols.tallies.len(), 2);
        assert_eq!(symbols.distributions.len(), 1);
        assert_eq!(symbols.next_available_ids.cell, 4);
        assert_eq!(symbols.next_available_ids.surface, 91);
        assert_eq!(symbols.next_available_ids.material, 3);
        assert_eq!(symbols.next_available_ids.tally_f4, 14);
    }

    #[test]
    fn test_inspect_surface_and_material_references() {
        let tpl = get_template("skeleton-sdef").expect("skeleton-sdef should exist");

        let surf_rep = inspect_target(tpl.content, "s10").expect("valid target s10");
        assert!(surf_rep.found);
        assert_eq!(surf_rep.definitions.len(), 1);
        assert_eq!(surf_rep.references.len(), 2); // Cell 1 (-10) and Cell 2 (10 -20)

        let mat_rep = inspect_target(tpl.content, "m1").expect("valid target m1");
        assert!(mat_rep.found);
        assert_eq!(mat_rep.definitions.len(), 1);
        assert_eq!(mat_rep.references.len(), 1); // Cell 2

        let tally_rep = inspect_target(tpl.content, "f4").expect("valid target f4");
        assert!(tally_rep.found);
        assert_eq!(tally_rep.definitions.len(), 1);
        assert_eq!(tally_rep.references.len(), 2); // FC4 and E4
    }
}
