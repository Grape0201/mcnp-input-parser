use super::context::{
    check_particle_in_mode, data_line, is_strictly_increasing, ValidationContext,
};
use super::fmesh::validate_fmesh;
use super::types::{
    ErrorCode, Severity, ValidationCategory, ValidationError, KNOWN_MCNP_DATA_PREFIXES,
};
use crate::ast::{
    DataCard, DetectorEntry, Entry, FmBin, Particle, SdefValue, SdefVariable, SiEntry,
    SpSbContent, TallyKind, TallyTarget,
};

pub(super) fn validate_data_cards(ctx: &mut ValidationContext<'_>) {
    let input = ctx.input;
    let spans = ctx.spans;
    // ========================================================================
    // 5. Data Cards Validation (Materials, Tallies, Sources, VR, Physics)
    // ========================================================================
    let has_ssr = input.data_cards.iter().any(|c| {
        matches!(c, DataCard::Generic { name, .. } if name.eq_ignore_ascii_case("ssr"))
    });
    let any_fmesh_xdmf = input.data_cards.iter().any(|c| {
        matches!(c, DataCard::Fmesh(fm) if fm.out.as_deref() == Some("xdmf"))
    });

    for (idx, card) in input.data_cards.iter().enumerate() {
        let line = data_line(spans, idx);

        match card {
            DataCard::Material { id, nuclides, .. } => {
                if nuclides.is_empty() {
                    ctx.diagnostics.push(ValidationError {
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
                            ctx.diagnostics.push(ValidationError {
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
                            ctx.diagnostics.push(ValidationError {
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
                        ctx.diagnostics.push(ValidationError {
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
                if !ctx.materials.contains_key(id) {
                    ctx.diagnostics.push(ValidationError {
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
                    ctx.diagnostics.push(ValidationError {
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
                            if param.variable.is_vector() && *d > 0 {
                                ctx.vector_dist_ids.insert(*d);
                            }
                            ctx.referenced_dists.push((
                                *d,
                                line,
                                format!("SDEF {:?}", param.variable).to_uppercase(),
                            ));
                        }
                        SdefValue::Function { dist_id, .. } => {
                            if param.variable.is_vector() && *dist_id > 0 {
                                ctx.vector_dist_ids.insert(*dist_id);
                            }
                            ctx.referenced_dists.push((
                                *dist_id,
                                line,
                                format!("SDEF {:?}", param.variable).to_uppercase(),
                            ));
                        }
                        SdefValue::Scalar(v) => match param.variable {
                            SdefVariable::Cel => {
                                let cid = v.round() as u32;
                                if cid > 0 && !ctx.cell_map.contains_key(&cid) {
                                    ctx.diagnostics.push(ValidationError {
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
                                ctx.used_surfaces.insert(sid);
                                if sid > 0 && !ctx.surface_map.contains_key(&sid) {
                                    ctx.diagnostics.push(ValidationError {
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
                                if tid > 0 && !ctx.transforms.contains_key(&tid) {
                                    ctx.diagnostics.push(ValidationError {
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
                                        if cid > 0 && !ctx.cell_map.contains_key(&cid) {
                                            ctx.diagnostics.push(ValidationError {
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
                                    &ctx.active_particles,
                                    line,
                                    "SDEF PAR",
                                    &mut ctx.diagnostics,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }

            DataCard::Si(si) => {
                for e in &si.entries {
                    if let SiEntry::Distribution(d) = e {
                        ctx.referenced_dists.push((*d, line, format!("SI{}", si.id)));
                    }
                }
            }

            DataCard::Sp(sp) | DataCard::Sb(sp) => {
                let prefix = match card {
                    DataCard::Sp(_) => "SP",
                    _ => "SB",
                };
                if let SpSbContent::Probabilities { values, .. } = &sp.content {
                    let mut any_positive = false;
                    for e in values {
                        if let Entry::Value(v) = e {
                            if *v < 0.0 {
                                ctx.diagnostics.push(ValidationError {
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
                        ctx.diagnostics.push(ValidationError {
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
                        ctx.referenced_dists.push((*d, line, format!("DS{}", ds.id)));
                    }
                }
                for (_, d) in &ds.pairs {
                    ctx.referenced_dists.push((*d, line, format!("DS{}", ds.id)));
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
                        &ctx.active_particles,
                        line,
                        &format!("F{}", id),
                        &mut ctx.diagnostics,
                    );
                }
                if matches!(kind, TallyKind::Other(_)) {
                    ctx.diagnostics.push(ValidationError {
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
                            ctx.used_surfaces.insert(sid);
                            if !ctx.surface_map.contains_key(&sid) {
                                ctx.diagnostics.push(ValidationError {
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
                            if !ctx.cell_map.contains_key(&cid) {
                                ctx.diagnostics.push(ValidationError {
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
                        &ctx.active_particles,
                        line,
                        &format!("F{}", id),
                        &mut ctx.diagnostics,
                    );
                }
                for det in detectors {
                    let r0 = match det {
                        DetectorEntry::Point { r0, .. } => *r0,
                        DetectorEntry::Ring { r0, .. } => *r0,
                    };
                    if r0 < 0.0 {
                        ctx.diagnostics.push(ValidationError {
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
                if dose.id > 0 && !ctx.tallies.contains_key(&dose.id) && !ctx.fmeshes.contains_key(&dose.id)
                {
                    let prefix = if dose.is_function { "DF" } else { "DE" };
                    ctx.diagnostics.push(ValidationError {
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
                    ctx.diagnostics.push(ValidationError {
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
                if fm.id > 0 && !ctx.tallies.contains_key(&fm.id) && !ctx.fmeshes.contains_key(&fm.id) {
                    ctx.diagnostics.push(ValidationError {
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
                            ctx.used_materials.insert(m_u32);
                            if !ctx.materials.contains_key(&m_u32) {
                                ctx.diagnostics.push(ValidationError {
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
                if *id > 0 && !ctx.tallies.contains_key(id) {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("FS{} references undefined tally F{}", id, id),
                    });
                }
                for &s in surfaces {
                    let sid = s.unsigned_abs();
                    ctx.used_surfaces.insert(sid);
                    if sid == 0 || !ctx.surface_map.contains_key(&sid) {
                        ctx.diagnostics.push(ValidationError {
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
                if *id > 0 && !ctx.tallies.contains_key(id) {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("SD{} references undefined tally F{}", id, id),
                    });
                }
            }

            DataCard::TallyEnergy { id, bins, .. } => {
                if *id > 0 && !ctx.tallies.contains_key(id) {
                    if ctx.fmeshes.contains_key(id) {
                        ctx.diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidFmeshConfiguration,
                            severity: Severity::Error,
                            category: ValidationCategory::Tally,
                            line_number: line,
                            message: format!(
                                "E{} cannot be used with mesh tally FMESH{}; use EMESH/EINTS keywords on the FMESH card instead",
                                id, id
                            ),
                        });
                    } else {
                        ctx.diagnostics.push(ValidationError {
                            code: ErrorCode::OrphanTallyModifierCard,
                            severity: Severity::Error,
                            category: ValidationCategory::Tally,
                            line_number: line,
                            message: format!("E{} references undefined tally F{}", id, id),
                        });
                    }
                }
                let vals: Vec<f64> = bins.iter().filter_map(|e| e.as_f64()).collect();
                if vals.len() >= 2 && !is_strictly_increasing(&vals) {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::NonMonotonicTallyBins,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("E{} energy bins must be strictly increasing", id),
                    });
                }
            }

            DataCard::TallyTime { id, bins, .. } => {
                if *id > 0 && !ctx.tallies.contains_key(id) {
                    if ctx.fmeshes.contains_key(id) {
                        ctx.diagnostics.push(ValidationError {
                            code: ErrorCode::InvalidFmeshConfiguration,
                            severity: Severity::Error,
                            category: ValidationCategory::Tally,
                            line_number: line,
                            message: format!(
                                "T{} cannot be used with mesh tally FMESH{}; use TMESH/TINTS keywords on the FMESH card instead",
                                id, id
                            ),
                        });
                    } else {
                        ctx.diagnostics.push(ValidationError {
                            code: ErrorCode::OrphanTallyModifierCard,
                            severity: Severity::Error,
                            category: ValidationCategory::Tally,
                            line_number: line,
                            message: format!("T{} references undefined tally F{}", id, id),
                        });
                    }
                }
                let vals: Vec<f64> = bins.iter().filter_map(|e| e.as_f64()).collect();
                if vals.len() >= 2 && !is_strictly_increasing(&vals) {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::NonMonotonicTallyBins,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("T{} time bins must be strictly increasing", id),
                    });
                }
            }

            DataCard::TallyCosine { id, bins, .. } => {
                if *id > 0 && !ctx.tallies.contains_key(id) {
                    ctx.diagnostics.push(ValidationError {
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
                        ctx.diagnostics.push(ValidationError {
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
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::NonMonotonicTallyBins,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("C{} cosine bins must be strictly increasing", id),
                    });
                }
            }

            DataCard::TallyComment { id, .. } => {
                if *id > 0 && !ctx.tallies.contains_key(id) && !ctx.fmeshes.contains_key(id) {
                    ctx.diagnostics.push(ValidationError {
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
                        &ctx.active_particles,
                        line,
                        &format!("FMESH{}", fm.id),
                        &mut ctx.diagnostics,
                    );
                }
                validate_fmesh(
                    fm,
                    line,
                    &ctx.transforms,
                    has_ssr,
                    any_fmesh_xdmf,
                    &mut ctx.diagnostics,
                );
            }

            DataCard::Importance { particles, values } => {
                let targets: Vec<Particle> = if particles.is_empty() {
                    vec![Particle::new("n")]
                } else {
                    particles.clone()
                };
                for p in &targets {
                    check_particle_in_mode(p, &ctx.active_particles, line, "IMP", &mut ctx.diagnostics);
                }
                if !input.cells.is_empty() && values.len() != input.cells.len() {
                    ctx.diagnostics.push(ValidationError {
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
                            ctx.diagnostics.push(ValidationError {
                                code: ErrorCode::InvalidCellParameterValue,
                                severity: Severity::Error,
                                category: ValidationCategory::Cell,
                                line_number: line,
                                message: format!("IMP data card contains negative value ({})", v),
                            });
                        }
                        if let Some(cell) = input.cells.get(c_idx) {
                            for p in &targets {
                                ctx.cell_has_imp.entry(p.clone()).or_default().insert(cell.id);
                                if *v > 0.0 {
                                    ctx.cell_nonzero_imp.insert(p.clone(), true);
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
                        &ctx.active_particles,
                        line,
                        &format!("WWN{}", group_id),
                        &mut ctx.diagnostics,
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
                    ctx.diagnostics.push(ValidationError {
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
                        &ctx.active_particles,
                        line,
                        "Physics/VR card",
                        &mut ctx.diagnostics,
                    );
                }
            }

            DataCard::WeightWindowGenerator {
                tally_id,
                reference_cell,
                ..
            } => {
                if !ctx.tallies.contains_key(tally_id) {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::OrphanTallyModifierCard,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: line,
                        message: format!("WWG references undefined tally F{}", tally_id),
                    });
                }
                if *reference_cell > 0 && !ctx.cell_map.contains_key(reference_cell) {
                    ctx.diagnostics.push(ValidationError {
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
                    ctx.diagnostics.push(ValidationError {
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
                    ctx.diagnostics.push(ValidationError {
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
                    ctx.diagnostics.push(ValidationError {
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
                        ctx.diagnostics.push(ValidationError {
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
                        &ctx.active_particles,
                        line,
                        &name.to_uppercase(),
                        &mut ctx.diagnostics,
                    );
                }
                let alpha_prefix: String = name
                    .trim_start_matches(['*', '+'])
                    .chars()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .collect::<String>()
                    .to_lowercase();
                if !KNOWN_MCNP_DATA_PREFIXES.contains(&alpha_prefix.as_str()) {
                    ctx.diagnostics.push(ValidationError {
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

}

fn is_valid_zaid(zaid: &str) -> bool {
    let base = zaid.split('.').next().unwrap_or("");
    !base.is_empty() && base.len() <= 7 && base.chars().all(|c| c.is_ascii_digit())
}
