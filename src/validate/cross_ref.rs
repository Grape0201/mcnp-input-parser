use super::context::{
    cell_line, data_line, is_strictly_increasing, surf_line, ValidationContext,
};
use super::types::{ErrorCode, Severity, ValidationCategory, ValidationError};
use crate::ast::{DataCard, Entry, Particle, SiEntry, SiOption, SpOption, SpSbContent};
use std::collections::HashSet;

pub(super) fn validate_cross_references(ctx: &mut ValidationContext<'_>) {
    let input = ctx.input;
    let spans = ctx.spans;
    // ========================================================================
    // 6. Cross-Card Source, Dose, Importance, and Unused Entity Checks
    // ========================================================================
    // Check Source presence (SDEF vs KCODE vs SSR)
    if !input.data_cards.is_empty() {
        let has_sdef = !ctx.sdef_indices.is_empty();
        let has_kcode = !ctx.kcode_indices.is_empty();
        let has_ssr = input.data_cards.iter().any(|c| {
            matches!(c, DataCard::Generic { name, .. } if name.eq_ignore_ascii_case("ssr"))
        });

        if !has_sdef && !has_kcode && !has_ssr {
            ctx.diagnostics.push(ValidationError {
                code: ErrorCode::MissingSourceDefinition,
                severity: Severity::Error,
                category: ValidationCategory::Source,
                line_number: None,
                message: "No source definition card (SDEF, KCODE, or SSR) found in data block"
                    .to_string(),
            });
        } else if has_sdef && has_kcode {
            ctx.diagnostics.push(ValidationError {
                code: ErrorCode::ConflictingSourceDefinitions,
                severity: Severity::Error,
                category: ValidationCategory::Source,
                line_number: data_line(spans, ctx.kcode_indices[0]),
                message: "Both SDEF (fixed source) and KCODE (criticality source) are specified"
                    .to_string(),
            });
        }

        if has_kcode && ctx.ksrc_indices.is_empty() && !has_sdef {
            ctx.diagnostics.push(ValidationError {
                code: ErrorCode::MissingKsrcForKcode,
                severity: Severity::Warning,
                category: ValidationCategory::Source,
                line_number: data_line(spans, ctx.kcode_indices[0]),
                message: "KCODE is specified without a KSRC initial source points card".to_string(),
            });
        }
    }

    // Check referenced source distributions (Dn)
    for (dist_id, line, context) in &ctx.referenced_dists {
        let dist_id = *dist_id;
        let line = *line;
        if dist_id == 0 {
            continue;
        }
        let has_si = ctx.si_cards.contains_key(&dist_id);
        let has_sp = ctx.sp_cards.contains_key(&dist_id);
        let has_ds = ctx.ds_cards.contains_key(&dist_id);
        if !has_si && !has_sp && !has_ds {
            ctx.diagnostics.push(ValidationError {
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

    // Propagate 3D vector distribution property through second-level SI (S option) and DS cards
    loop {
        let mut added = false;
        let current_vec_ids: Vec<u32> = ctx.vector_dist_ids.iter().copied().collect();
        for d_id in current_vec_ids {
            if let Some(&(_, si)) = ctx.si_cards.get(&d_id) {
                if si.option == SiOption::DistributionList {
                    for e in &si.entries {
                        if let SiEntry::Distribution(sub_d) = e {
                            if *sub_d > 0 && ctx.vector_dist_ids.insert(*sub_d) {
                                added = true;
                            }
                        }
                    }
                }
            }
            if let Some(&(_, ds)) = ctx.ds_cards.get(&d_id) {
                for e in &ds.entries {
                    if let SiEntry::Distribution(sub_d) = e {
                        if *sub_d > 0 && ctx.vector_dist_ids.insert(*sub_d) {
                            added = true;
                        }
                    }
                }
                for &(_, sub_d) in &ds.pairs {
                    if sub_d > 0 && ctx.vector_dist_ids.insert(sub_d) {
                        added = true;
                    }
                }
            }
        }
        if !added {
            break;
        }
    }

    // Check SI / SP / SB pairing and lengths
    let all_dist_ids: HashSet<u32> = ctx.si_cards
        .keys()
        .chain(ctx.sp_cards.keys())
        .chain(ctx.sb_cards.keys())
        .copied()
        .collect();
    let mut sorted_dist_ids: Vec<u32> = all_dist_ids.into_iter().collect();
    sorted_dist_ids.sort_unstable();

    for dist_id in sorted_dist_ids {
        let si_entry = ctx.si_cards.get(&dist_id);
        let sp_entry = ctx.sp_cards.get(&dist_id);
        let sb_entry = ctx.sb_cards.get(&dist_id);

        if let Some(&(si_idx, si)) = si_entry {
            let is_vector_coords =
                ctx.vector_dist_ids.contains(&dist_id) && si.option != SiOption::DistributionList;

            if !is_vector_coords && matches!(si.option, SiOption::Histogram | SiOption::ArbitraryPdf) {
                let mut vals = Vec::new();
                for e in &si.entries {
                    if let SiEntry::Value(v) = e {
                        vals.push(*v);
                    }
                }
                if vals.len() >= 2 && !is_strictly_increasing(&vals) {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::NonMonotonicSourceBins,
                        severity: Severity::Error,
                        category: ValidationCategory::Source,
                        line_number: data_line(spans, si_idx),
                        message: format!(
                            "SI{} bin boundaries must be strictly increasing",
                            si.id
                        ),
                    });
                }
            }

            if sp_entry.is_none() {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::IncompleteSourceDistribution,
                    severity: Severity::Error,
                    category: ValidationCategory::Source,
                    line_number: data_line(spans, si_idx),
                    message: format!(
                        "SI{} is defined without a corresponding SP{} card",
                        dist_id, dist_id
                    ),
                });
            } else if let Some(&(sp_idx, sp)) = sp_entry {
                if let SpSbContent::Probabilities { option, values } = &sp.content {
                    if *option != SpOption::Volume {
                        let expected_si_len = if is_vector_coords {
                            values.len() * 3
                        } else {
                            values.len()
                        };
                        if si.entries.len() != expected_si_len {
                            let message = if is_vector_coords {
                                format!(
                                    "SP{} has {} entries (expecting {} coordinate entries in SI{} for 3D vector distribution), but SI{} has {} entries",
                                    dist_id,
                                    values.len(),
                                    expected_si_len,
                                    dist_id,
                                    dist_id,
                                    si.entries.len()
                                )
                            } else {
                                format!(
                                    "SP{} has {} entries, but SI{} has {} entries",
                                    dist_id,
                                    values.len(),
                                    dist_id,
                                    si.entries.len()
                                )
                            };
                            ctx.diagnostics.push(ValidationError {
                                code: ErrorCode::SourceDistributionLengthMismatch,
                                severity: Severity::Error,
                                category: ValidationCategory::Source,
                                line_number: data_line(spans, sp_idx),
                                message,
                            });
                        } else if !is_vector_coords && si.option == SiOption::Histogram {
                            if let Some(Entry::Value(first_v)) = values.first() {
                                if *first_v != 0.0 {
                                    ctx.diagnostics.push(ValidationError {
                                        code: ErrorCode::SourceDistributionLengthMismatch,
                                        severity: Severity::Error,
                                        category: ValidationCategory::Source,
                                        line_number: data_line(spans, sp_idx),
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
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::IncompleteSourceDistribution,
                        severity: Severity::Error,
                        category: ValidationCategory::Source,
                        line_number: data_line(spans, sp_idx),
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
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::IncompleteSourceDistribution,
                    severity: Severity::Error,
                    category: ValidationCategory::Source,
                    line_number: data_line(spans, sb_idx),
                    message: format!(
                        "SB{} is defined without a corresponding SP{} card",
                        dist_id, dist_id
                    ),
                });
            }
            if let (Some(&(_, si)), SpSbContent::Probabilities { values, .. }) =
                (si_entry, &sb.content)
            {
                let is_vector_coords =
                    ctx.vector_dist_ids.contains(&dist_id) && si.option != SiOption::DistributionList;
                let expected_si_len = if is_vector_coords {
                    values.len() * 3
                } else {
                    values.len()
                };
                if si.entries.len() != expected_si_len {
                    let message = if is_vector_coords {
                        format!(
                            "SB{} has {} entries (expecting {} coordinate entries in SI{} for 3D vector distribution), but SI{} has {} entries",
                            dist_id,
                            values.len(),
                            expected_si_len,
                            dist_id,
                            dist_id,
                            si.entries.len()
                        )
                    } else {
                        format!(
                            "SB{} has {} entries, but SI{} has {} entries",
                            dist_id,
                            values.len(),
                            dist_id,
                            si.entries.len()
                        )
                    };
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::SourceDistributionLengthMismatch,
                        severity: Severity::Error,
                        category: ValidationCategory::Source,
                        line_number: data_line(spans, sb_idx),
                        message,
                    });
                }
            }
        }
    }

    // Check DEn / DFn pairing
    let all_dose_ids: HashSet<u32> = ctx.de_cards.keys().chain(ctx.df_cards.keys()).copied().collect();
    let mut sorted_dose_ids: Vec<u32> = all_dose_ids.into_iter().collect();
    sorted_dose_ids.sort_unstable();

    for dose_id in sorted_dose_ids {
        match (ctx.de_cards.get(&dose_id), ctx.df_cards.get(&dose_id)) {
            (Some(&(de_idx, _)), None) => {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::IncompleteDoseCards,
                    severity: Severity::Error,
                    category: ValidationCategory::Tally,
                    line_number: data_line(spans, de_idx),
                    message: format!(
                        "DE{} is defined without a corresponding DF{} card",
                        dose_id, dose_id
                    ),
                });
            }
            (None, Some(&(df_idx, df))) => {
                let has_ic = df.options.iter().any(|(k, _)| k.eq_ignore_ascii_case("ic"));
                if !has_ic && dose_id != 0 && !ctx.de_cards.contains_key(&0) {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::IncompleteDoseCards,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: data_line(spans, df_idx),
                        message: format!(
                            "DF{} is defined without a corresponding DE{} (or DE0) card",
                            dose_id, dose_id
                        ),
                    });
                }
            }
            (Some(&(_, de)), Some(&(df_idx, df))) => {
                if !df.values.is_empty() && de.values.len() != df.values.len() {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::IncompleteDoseCards,
                        severity: Severity::Error,
                        category: ValidationCategory::Tally,
                        line_number: data_line(spans, df_idx),
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
    let any_imp_specified = !ctx.cell_has_imp.is_empty();
    if any_imp_specified && !input.cells.is_empty() {
        let mut sorted_particles: Vec<&Particle> = ctx.active_particles.iter().collect();
        sorted_particles.sort_by(|a, b| a.0.cmp(&b.0));

        for p in sorted_particles {
            let covered = ctx.cell_has_imp.get(p);
            let count = covered.map(|s| s.len()).unwrap_or(0);
            if count == 0 {
                ctx.diagnostics.push(ValidationError {
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
                if !ctx.cell_nonzero_imp.get(p).copied().unwrap_or(false) {
                    ctx.diagnostics.push(ValidationError {
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
                        ctx.diagnostics.push(ValidationError {
                            code: ErrorCode::MissingCellImportance,
                            severity: Severity::Error,
                            category: ValidationCategory::Cell,
                            line_number: cell_line(spans, c_idx, cell.id),
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
        if !ctx.used_surfaces.contains(&surf.id) {
            ctx.diagnostics.push(ValidationError {
                code: ErrorCode::UnusedSurface,
                severity: Severity::Warning,
                category: ValidationCategory::Reference,
                line_number: surf_line(spans, idx, surf.id),
                message: format!(
                    "Surface {} is defined but never referenced by any cell, tally, or source",
                    surf.id
                ),
            });
        }
    }

    for (idx, card) in input.data_cards.iter().enumerate() {
        if let DataCard::Material { id, .. } = card {
            if !ctx.used_materials.contains(id) {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::UnusedMaterial,
                    severity: Severity::Warning,
                    category: ValidationCategory::Reference,
                    line_number: data_line(spans, idx),
                    message: format!(
                        "Material M{} is defined but never referenced by any cell or tally multiplier",
                        id
                    ),
                });
            }
        }
    }
}
