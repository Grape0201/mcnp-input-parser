use super::context::{cell_line, check_particle_in_mode, CellFillRecord, ValidationContext};
use super::surface::macrobody_max_facets;
use super::types::{
    ErrorCode, Severity, ValidationCategory, ValidationError, KNOWN_MCNP_CELL_PARAMETERS,
    MAX_MCNP_ID,
};
use crate::ast::{CellCard, CellDefinition, CellParam, Entry, GeometryExpr, Particle};
use crate::CardLineSpans;
use std::collections::{HashMap, HashSet};

pub(super) fn validate_cells(ctx: &mut ValidationContext<'_>) {
    let input = ctx.input;
    let spans = ctx.spans;
    // ========================================================================
    // 4. Cell Validation & Reference Checks
    // ========================================================================
    // Map of Universe ID -> list of Cell IDs belonging to that universe
    // Map of Universe ID -> list of (Cell ID, line, filled Universe IDs)
    // Track per-particle cell importance definitions

    for (idx, cell) in input.cells.iter().enumerate() {
        let line = cell_line(spans, idx, cell.id);

        // Determine effective material and density (considering parameter overrides or base cell in LIKE BUT)
        let (effective_mat, effective_density) = match &cell.definition {
            CellDefinition::Normal {
                material_id,
                density,
                ..
            } => {
                let mut mat = *material_id;
                let mut dens = *density;
                for param in &cell.params {
                    match param {
                        CellParam::Material(m) => mat = *m,
                        CellParam::Density(d) => dens = Some(*d),
                        _ => {}
                    }
                }
                (mat, dens)
            }
            CellDefinition::LikeBut { base_cell_id } => {
                let (base_mat, base_dens) = if let Some((_, base_cell)) = ctx.cell_map.get(base_cell_id) {
                    match &base_cell.definition {
                        CellDefinition::Normal {
                            material_id,
                            density,
                            ..
                        } => (*material_id, *density),
                        CellDefinition::LikeBut { .. } => (0, None),
                    }
                } else {
                    (0, None)
                };
                let mut mat = base_mat;
                let mut dens = base_dens;
                let mut mat_overridden = false;
                let mut dens_overridden = false;
                for param in &cell.params {
                    match param {
                        CellParam::Material(m) => {
                            mat = *m;
                            mat_overridden = true;
                        }
                        CellParam::Density(d) => {
                            dens = Some(*d);
                            dens_overridden = true;
                        }
                        _ => {}
                    }
                }
                if mat_overridden && !dens_overridden && base_mat == 0 && mat > 0 {
                    dens = None;
                }
                (mat, dens)
            }
        };

        if effective_mat > 0 {
            ctx.used_materials.insert(effective_mat);
            if !ctx.materials.contains_key(&effective_mat) {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::UndefinedMaterial,
                    severity: Severity::Error,
                    category: ValidationCategory::Reference,
                    line_number: line,
                    message: format!(
                        "Cell {} references undefined material M{}",
                        cell.id, effective_mat
                    ),
                });
            }
            match effective_density {
                Some(0.0) => {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::ZeroDensityNonVoidCell,
                        severity: Severity::Error,
                        category: ValidationCategory::Cell,
                        line_number: line,
                        message: format!(
                            "Cell {} has non-void material M{} with zero density",
                            cell.id, effective_mat
                        ),
                    });
                }
                None => {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::ZeroDensityNonVoidCell,
                        severity: Severity::Error,
                        category: ValidationCategory::Cell,
                        line_number: line,
                        message: format!(
                            "Cell {} has non-void material M{} but no density is specified",
                            cell.id, effective_mat
                        ),
                    });
                }
                _ => {}
            }
        } else {
            // Void cell (material 0)
            for param in &cell.params {
                if let CellParam::Density(rho) = param {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::VoidCellWithDensity,
                        severity: Severity::Error,
                        category: ValidationCategory::Cell,
                        line_number: line,
                        message: format!(
                            "Cell {} is void (material 0) but specifies density RHO={}",
                            cell.id, rho
                        ),
                    });
                }
            }
        }

        let has_fill = cell
            .params
            .iter()
            .any(|p| matches!(p, CellParam::Fill { .. }));
        if has_fill && effective_mat > 0 {
            ctx.diagnostics.push(ValidationError {
                code: ErrorCode::FilledCellWithMaterial,
                severity: Severity::Error,
                category: ValidationCategory::Cell,
                line_number: line,
                message: format!(
                    "Cell {} has a FILL parameter but specifies non-void material M{} (filled cells must have material 0)",
                    cell.id, effective_mat
                ),
            });
        }

        let lat_opt = cell.params.iter().find_map(|p| match p {
            CellParam::Lattice(lat) => Some(*lat),
            _ => None,
        });
        if let Some(lat) = lat_opt {
            if !has_fill {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::LatticeCellMissingFill,
                    severity: Severity::Error,
                    category: ValidationCategory::Cell,
                    line_number: line,
                    message: format!(
                        "Cell {} has lattice parameter LAT={} but is missing required FILL parameter",
                        cell.id, lat
                    ),
                });
            }
        }

        match &cell.definition {
            CellDefinition::Normal {
                geometry,
                ..
            } => {
                validate_geometry_expr(
                    cell.id,
                    line,
                    geometry,
                    &ctx.surface_map,
                    &ctx.cell_map,
                    &mut ctx.used_surfaces,
                    &mut ctx.diagnostics,
                );
            }
            CellDefinition::LikeBut { base_cell_id } => {
                if *base_cell_id == cell.id {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::CircularCellReference,
                        severity: Severity::Error,
                        category: ValidationCategory::Reference,
                        line_number: line,
                        message: format!("Cell {} references itself in LIKE {} BUT", cell.id, base_cell_id),
                    });
                } else if let Some((_, base_cell)) = ctx.cell_map.get(base_cell_id) {
                    if matches!(base_cell.definition, CellDefinition::LikeBut { .. }) {
                        ctx.diagnostics.push(ValidationError {
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
                    ctx.diagnostics.push(ValidationError {
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
                ctx.diagnostics.push(ValidationError {
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
                        ctx.diagnostics.push(ValidationError {
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
                            &ctx.active_particles,
                            line,
                            &format!("Cell {} IMP", cell.id),
                            &mut ctx.diagnostics,
                        );
                        ctx.cell_has_imp.entry(p.clone()).or_default().insert(cell.id);
                        if *value > 0.0 {
                            ctx.cell_nonzero_imp.insert(p.clone(), true);
                        }
                    }
                }
                CellParam::Volume(v) => {
                    if *v <= 0.0 {
                        ctx.diagnostics.push(ValidationError {
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
                    if *u < 0 || *u > MAX_MCNP_ID as i32 {
                        ctx.diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidCellParameterValue,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: line,
                            message: format!(
                                "Cell {} has invalid universe U={} (must be in range 0..={})",
                                cell.id, u, MAX_MCNP_ID
                            ),
                        });
                    }
                    let abs_u = u.abs();
                    this_universe = Some(abs_u);
                    ctx.universe_cells.entry(abs_u).or_default().push(cell.id);
                }
                CellParam::Lattice(lat) => {
                    if *lat != 1 && *lat != 2 {
                        ctx.diagnostics.push(ValidationError {
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
                                ctx.diagnostics.push(ValidationError {
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
                        ctx.diagnostics.push(ValidationError {
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
                        ctx.used_materials.insert(*mat_id);
                        if !ctx.materials.contains_key(mat_id) {
                            ctx.diagnostics.push(ValidationError {
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
                            if tr_id <= 0 || !ctx.transforms.contains_key(&(tr_id as u32)) {
                                ctx.diagnostics.push(ValidationError {
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
                        ctx.diagnostics.push(ValidationError {
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
                    if values.is_empty() {
                        ctx.diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidCellParameterValue,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: line,
                            message: format!("Cell {} specifies empty FILL parameter", cell.id),
                        });
                    }
                    extract_fill_universes(values, &mut filled_universes);
                }
                CellParam::Other { particles, key, values } => {
                    let clean_key = key.trim_start_matches('*').to_lowercase();
                    let alpha_key: String = clean_key
                        .chars()
                        .take_while(|c| c.is_ascii_alphabetic())
                        .collect();

                    if !KNOWN_MCNP_CELL_PARAMETERS.contains(&clean_key.as_str())
                        && !KNOWN_MCNP_CELL_PARAMETERS.contains(&alpha_key.as_str())
                    {
                        ctx.diagnostics.push(ValidationError {
                            code: ErrorCode::UnrecognizedCellParameter,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: line,
                            message: format!(
                                "Cell {} has unrecognized parameter keyword `{}`",
                                cell.id, key
                            ),
                        });
                    } else {
                        if values.is_empty() {
                            ctx.diagnostics.push(ValidationError {
                                code: ErrorCode::InvalidCellParameterValue,
                                severity: Severity::Error,
                                category: ValidationCategory::Cell,
                                line_number: line,
                                message: format!(
                                    "Cell {} parameter `{}` has no value specified",
                                    cell.id, key
                                ),
                            });
                        }

                        match alpha_key.as_str() {
                            "nonu" => {
                                for v_str in values {
                                    if let Ok(v) = v_str.parse::<i32>() {
                                        if !matches!(v, 0..=2) {
                                            ctx.diagnostics.push(ValidationError {
                                                code: ErrorCode::InvalidCellParameterValue,
                                                severity: Severity::Error,
                                                category: ValidationCategory::Cell,
                                                line_number: line,
                                                message: format!(
                                                    "Cell {} has invalid NONU={} (must be 0, 1, or 2)",
                                                    cell.id, v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                            "pd" => {
                                for v_str in values {
                                    if let Ok(v) = v_str.parse::<f64>() {
                                        if !(0.0..=1.0).contains(&v) {
                                            ctx.diagnostics.push(ValidationError {
                                                code: ErrorCode::InvalidCellParameterValue,
                                                severity: Severity::Error,
                                                category: ValidationCategory::Cell,
                                                line_number: line,
                                                message: format!(
                                                    "Cell {} has invalid detector contribution PD={} (must be between 0.0 and 1.0)",
                                                    cell.id, v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                            "pwt" => {
                                for v_str in values {
                                    if let Ok(v) = v_str.parse::<f64>() {
                                        if v < 0.0 {
                                            ctx.diagnostics.push(ValidationError {
                                                code: ErrorCode::InvalidCellParameterValue,
                                                severity: Severity::Error,
                                                category: ValidationCategory::Cell,
                                                line_number: line,
                                                message: format!(
                                                    "Cell {} has negative photon production weight PWT={}",
                                                    cell.id, v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                            "fcl" => {
                                for v_str in values {
                                    if let Ok(v) = v_str.parse::<f64>() {
                                        if !(-1.0..=1.0).contains(&v) {
                                            ctx.diagnostics.push(ValidationError {
                                                code: ErrorCode::InvalidCellParameterValue,
                                                severity: Severity::Error,
                                                category: ValidationCategory::Cell,
                                                line_number: line,
                                                message: format!(
                                                    "Cell {} has invalid forced collision parameter FCL={} (must be between -1.0 and 1.0)",
                                                    cell.id, v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                            "wwn" => {
                                for v_str in values {
                                    if let Ok(v) = v_str.parse::<f64>() {
                                        if v < 0.0 {
                                            ctx.diagnostics.push(ValidationError {
                                                code: ErrorCode::InvalidCellParameterValue,
                                                severity: Severity::Error,
                                                category: ValidationCategory::Cell,
                                                line_number: line,
                                                message: format!(
                                                    "Cell {} has negative weight window bound WWN={}",
                                                    cell.id, v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                            "dxc" => {
                                for v_str in values {
                                    if let Ok(v) = v_str.parse::<f64>() {
                                        if !(0.0..=1.0).contains(&v) {
                                            ctx.diagnostics.push(ValidationError {
                                                code: ErrorCode::InvalidCellParameterValue,
                                                severity: Severity::Error,
                                                category: ValidationCategory::Cell,
                                                line_number: line,
                                                message: format!(
                                                    "Cell {} has invalid DXTRAN contribution DXC={} (must be between 0.0 and 1.0)",
                                                    cell.id, v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                            "elpt" => {
                                for v_str in values {
                                    if let Ok(v) = v_str.parse::<f64>() {
                                        if v < 0.0 {
                                            ctx.diagnostics.push(ValidationError {
                                                code: ErrorCode::InvalidCellParameterValue,
                                                severity: Severity::Error,
                                                category: ValidationCategory::Cell,
                                                line_number: line,
                                                message: format!(
                                                    "Cell {} has negative energy cutoff ELPT={}",
                                                    cell.id, v
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    for p in particles {
                        check_particle_in_mode(
                            p,
                            &ctx.active_particles,
                            line,
                            &format!("Cell {} {}", cell.id, key.to_uppercase()),
                            &mut ctx.diagnostics,
                        );
                    }
                }
            }
        }

        // Propagate importance from base cell if `LIKE n BUT`
        if let CellDefinition::LikeBut { base_cell_id } = &cell.definition {
            if let Some((_, base_cell)) = ctx.cell_map.get(base_cell_id) {
                for param in &base_cell.params {
                    if let CellParam::Importance { particles, value } = param {
                        let targets: Vec<Particle> = if particles.is_empty() {
                            vec![Particle::new("n")]
                        } else {
                            particles.clone()
                        };
                        for p in targets {
                            ctx.cell_has_imp.entry(p.clone()).or_default().insert(cell.id);
                            if *value > 0.0 {
                                ctx.cell_nonzero_imp.insert(p, true);
                            }
                        }
                    }
                }
            }
        }

        if !filled_universes.is_empty() {
            ctx.cell_fills.push(CellFillRecord {
                cell_id: cell.id,
                line,
                universe: this_universe,
                filled_universes,
            });
        }
    }

    // Check `#n` ComplementCell cycles
    check_complement_cell_cycles(&ctx.cell_map, spans, &mut ctx.diagnostics);

    // Check Universe & Fill references and cycles
    let mut univ_adj: HashMap<i32, Vec<(i32, u32, Option<usize>)>> = HashMap::new();
    for rec in &ctx.cell_fills {
        let c_id = rec.cell_id;
        let line = rec.line;
        let u_opt = rec.universe;
        for &target_u in &rec.filled_universes {
            if target_u == 0 {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::InvalidCellParameterValue,
                    severity: Severity::Error,
                    category: ValidationCategory::Cell,
                    line_number: line,
                    message: format!(
                        "Cell {} specifies invalid fill universe FILL=0 (root universe 0 cannot fill a cell)",
                        c_id
                    ),
                });
            } else if !ctx.universe_cells.contains_key(&target_u) {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::UndefinedUniverse,
                    severity: Severity::Error,
                    category: ValidationCategory::Reference,
                    line_number: line,
                    message: format!(
                        "Cell {} FILL references undefined universe U={}",
                        c_id, target_u
                    ),
                });
            }
            if let Some(src_u) = u_opt {
                if target_u > 0 {
                    univ_adj
                        .entry(src_u)
                        .or_default()
                        .push((target_u, c_id, line));
                }
            }
        }
    }
    check_universe_cycles(&univ_adj, &mut ctx.diagnostics);

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
            if let Some((_, target_cell)) = cell_map.get(target_id) {
                let is_filled = target_cell
                    .params
                    .iter()
                    .any(|p| matches!(p, CellParam::Fill { .. }));
                if is_filled {
                    diagnostics.push(ValidationError {
                        code: ErrorCode::InvalidCellComplement,
                        severity: Severity::Error,
                        category: ValidationCategory::Cell,
                        line_number: line,
                        message: format!(
                            "Cell {} complements cell #{} which has a FILL parameter (cannot complement a universe-filled cell)",
                            cell_id, target_id
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
