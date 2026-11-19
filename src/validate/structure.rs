use super::context::{cell_line, data_line, surf_line, ValidationContext};
use super::types::{ErrorCode, Severity, ValidationCategory, ValidationError, MAX_MCNP_ID};
use crate::ast::{DataCard, Particle, TallyKind};
use std::collections::{HashMap, HashSet};

pub(super) fn check_block_structure(ctx: &mut ValidationContext<'_>) {
    let input = ctx.input;
    let spans = ctx.spans;
    // ========================================================================
    // 1. Block Structure Checks
    // ========================================================================
    let has_extra_blank_lines = spans.blank_line_numbers.len() > 2
        || !spans.leading_blank_lines.is_empty()
        || !spans.consecutive_blank_lines.is_empty()
        || !spans.trailing_ignored_cards.is_empty();

    if has_extra_blank_lines {
        let primary_blank_line = spans
            .leading_blank_lines
            .first()
            .or_else(|| spans.consecutive_blank_lines.first())
            .or_else(|| spans.blank_line_numbers.get(2))
            .or_else(|| spans.blank_line_numbers.first())
            .copied();

        let blanks_str = spans
            .blank_line_numbers
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(", ");

        let mut msg = format!(
            "Too many or misplaced blank lines: found {} blank line(s) at line(s) [{}], but MCNP requires EXACTLY 2 blank lines total (1 between Block 1 and Block 2, and 1 between Block 2 and Block 3)",
            spans.blank_line_numbers.len(),
            blanks_str
        );

        if !spans.leading_blank_lines.is_empty() {
            let lead_str = spans
                .leading_blank_lines
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            msg.push_str(&format!(
                "; DELETE blank line(s) at line(s) [{}] immediately after the title/header comments (do NOT put a blank line before the first Cell card)",
                lead_str
            ));
        }

        if !spans.consecutive_blank_lines.is_empty() {
            let consec_str = spans
                .consecutive_blank_lines
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            msg.push_str(&format!(
                "; DELETE consecutive blank line(s) at line(s) [{}]",
                consec_str
            ));
        }

        if let Some(first_ignored) = spans.trailing_ignored_cards.first() {
            msg.push_str(&format!(
                "; {} card(s) starting at line {} (`{}`) were ignored because they appear after the 3rd blank line",
                spans.trailing_ignored_cards.len(),
                first_ignored.line_number,
                first_ignored.text
            ));
        }

        ctx.diagnostics.push(ValidationError {
            code: ErrorCode::UnexpectedBlankLine,
            severity: Severity::Error,
            category: ValidationCategory::Structure,
            line_number: primary_blank_line,
            message: msg,
        });
    }

    if input.cells.is_empty() {
        ctx.diagnostics.push(ValidationError {
            code: ErrorCode::EmptyCellsBlock,
            severity: Severity::Error,
            category: ValidationCategory::Structure,
            line_number: spans.blank_line_numbers.first().copied(),
            message: "Cell cards block (Block 1) is empty".to_string(),
        });
    }
    if input.surfaces.is_empty() {
        ctx.diagnostics.push(ValidationError {
            code: ErrorCode::EmptySurfacesBlock,
            severity: Severity::Error,
            category: ValidationCategory::Structure,
            line_number: spans.blank_line_numbers.get(1).copied(),
            message: "Surface cards block (Block 2) is empty".to_string(),
        });
    }
    if input.data_cards.is_empty() {
        ctx.diagnostics.push(ValidationError {
            code: ErrorCode::EmptyDataBlock,
            severity: Severity::Error,
            category: ValidationCategory::Structure,
            line_number: spans.blank_line_numbers.get(2).copied(),
            message: "Data cards block (Block 3) is empty".to_string(),
        });
    }

}

pub(super) fn build_lookup_maps(ctx: &mut ValidationContext<'_>) {
    let input = ctx.input;
    let spans = ctx.spans;
    // ========================================================================
    // 2. Build Lookup Maps & Check ID Ranges / Duplicates
    // ========================================================================
    for (idx, cell) in input.cells.iter().enumerate() {
        let line = cell_line(spans, idx, cell.id);
        if cell.id == 0 || cell.id > MAX_MCNP_ID {
            ctx.diagnostics.push(ValidationError {
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
        if let Some((first_idx, _)) = ctx.cell_map.insert(cell.id, (idx, cell)) {
            let first_line = cell_line(spans, first_idx, cell.id);
            ctx.diagnostics.push(ValidationError {
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

    for (idx, surf) in input.surfaces.iter().enumerate() {
        let line = surf_line(spans, idx, surf.id);
        if surf.id == 0 || surf.id > MAX_MCNP_ID {
            ctx.diagnostics.push(ValidationError {
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
        if let Some((first_idx, _)) = ctx.surface_map.insert(surf.id, (idx, surf)) {
            let first_line = surf_line(spans, first_idx, surf.id);
            ctx.diagnostics.push(ValidationError {
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


    for (idx, card) in input.data_cards.iter().enumerate() {
        let line = data_line(spans, idx);
        let check_dup = |map: &mut HashMap<u32, usize>,
                         id: u32,
                         name: &str,
                         diag: &mut Vec<ValidationError>| {
            if let Some(prev_idx) = map.insert(id, idx) {
                let prev_line = data_line(spans, prev_idx);
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
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!("Material ID M{} is out of valid range", id),
                    });
                }
                check_dup(&mut ctx.materials, *id, "M", &mut ctx.diagnostics);
            }
            DataCard::ThermalScattering { id, .. } => {
                check_dup(&mut ctx.mt_cards, *id, "MT", &mut ctx.diagnostics);
            }
            DataCard::Transform { id, .. } => {
                if *id == 0 || *id > 999 {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!("Transform ID TR{} is out of valid range (1..=999)", id),
                    });
                }
                check_dup(&mut ctx.transforms, *id, "TR", &mut ctx.diagnostics);
            }
            DataCard::Tally { id, kind, .. } => {
                if *id == 0 || *id > MAX_MCNP_ID {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!("Tally ID F{} is out of valid range", id),
                    });
                }
                if let Some((prev_idx, _)) = ctx.tallies.insert(*id, (idx, *kind)) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
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
                if let Some(&prev_idx) = ctx.fmeshes.get(id) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => format!(
                                "F{} conflicts with mesh tally FMESH{} (first defined at line {})",
                                id, id, pl
                            ),
                            None => format!("F{} conflicts with mesh tally FMESH{}", id, id),
                        },
                    });
                }
            }
            DataCard::DetectorTally { id, .. } => {
                if let Some((prev_idx, _)) = ctx.tallies.insert(*id, (idx, TallyKind::F5DetectorFlux)) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
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
                if fm.id == 0 || fm.id > MAX_MCNP_ID {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!("Mesh tally ID FMESH{} is out of valid range", fm.id),
                    });
                }
                check_dup(&mut ctx.fmeshes, fm.id, "FMESH", &mut ctx.diagnostics);
                if let Some(&(prev_idx, _)) = ctx.tallies.get(&fm.id) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => format!(
                                "FMESH{} conflicts with tally F{} (first defined at line {})",
                                fm.id, fm.id, pl
                            ),
                            None => format!("FMESH{} conflicts with tally F{}", fm.id, fm.id),
                        },
                    });
                }
            }
            DataCard::Si(si) => {
                if si.id == 0 || si.id > 999 {
                    ctx.diagnostics.push(ValidationError {
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
                if let Some((prev_idx, _)) = ctx.si_cards.insert(si.id, (idx, si)) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
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
            DataCard::Sp(sp) => {
                if sp.id == 0 || sp.id > 999 {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!(
                            "Source distribution ID SP{} is out of valid range (1..=999)",
                            sp.id
                        ),
                    });
                }
                if let Some((prev_idx, _)) = ctx.sp_cards.insert(sp.id, (idx, sp)) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => format!(
                                "Duplicate SP{} card (first defined at line {})",
                                sp.id, pl
                            ),
                            None => format!("Duplicate SP{} card", sp.id),
                        },
                    });
                }
            }
            DataCard::Sb(sb) => {
                if sb.id == 0 || sb.id > 999 {
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DataCardIdOutOfRange,
                        severity: Severity::Error,
                        category: ValidationCategory::IdRange,
                        line_number: line,
                        message: format!(
                            "Source distribution ID SB{} is out of valid range (1..=999)",
                            sb.id
                        ),
                    });
                }
                if let Some((prev_idx, _)) = ctx.sb_cards.insert(sb.id, (idx, sb)) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
                        code: ErrorCode::DuplicateDataCard,
                        severity: Severity::Error,
                        category: ValidationCategory::DuplicateId,
                        line_number: line,
                        message: match prev_line {
                            Some(pl) => format!(
                                "Duplicate SB{} card (first defined at line {})",
                                sb.id, pl
                            ),
                            None => format!("Duplicate SB{} card", sb.id),
                        },
                    });
                }
            }
            DataCard::Ds(ds) => {
                if ds.id == 0 || ds.id > 999 {
                    ctx.diagnostics.push(ValidationError {
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
                if let Some((prev_idx, _)) = ctx.ds_cards.insert(ds.id, (idx, ds)) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
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
                check_dup(&mut ctx.sc_cards, *id, "SC", &mut ctx.diagnostics);
            }
            DataCard::Dose(dose) => {
                let target_map = if dose.is_function {
                    &mut ctx.df_cards
                } else {
                    &mut ctx.de_cards
                };
                let prefix = if dose.is_function { "DF" } else { "DE" };
                if let Some((prev_idx, _)) = target_map.insert(dose.id, (idx, dose)) {
                    let prev_line = data_line(spans, prev_idx);
                    ctx.diagnostics.push(ValidationError {
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
            DataCard::TallyMultiplier(fm) => check_dup(&mut ctx.fm_cards, fm.id, "FM", &mut ctx.diagnostics),
            DataCard::TallySegment { id, .. } => check_dup(&mut ctx.fs_cards, *id, "FS", &mut ctx.diagnostics),
            DataCard::SegmentDivisor { id, .. } => check_dup(&mut ctx.sd_cards, *id, "SD", &mut ctx.diagnostics),
            DataCard::TallyEnergy { id, .. } => check_dup(&mut ctx.e_cards, *id, "E", &mut ctx.diagnostics),
            DataCard::TallyTime { id, .. } => check_dup(&mut ctx.t_cards, *id, "T", &mut ctx.diagnostics),
            DataCard::TallyCosine { id, .. } => check_dup(&mut ctx.c_cards, *id, "C", &mut ctx.diagnostics),
            DataCard::TallyComment { id, .. } => check_dup(&mut ctx.fc_cards, *id, "FC", &mut ctx.diagnostics),
            DataCard::Mode { .. } => ctx.mode_indices.push(idx),
            DataCard::Sdef { .. } => ctx.sdef_indices.push(idx),
            DataCard::Kcode { .. } => ctx.kcode_indices.push(idx),
            DataCard::Ksrc { .. } => ctx.ksrc_indices.push(idx),
            DataCard::Nps { .. } => ctx.nps_indices.push(idx),
            _ => {}
        }
    }

    for (indices, name) in [
        (&ctx.mode_indices, "MODE"),
        (&ctx.sdef_indices, "SDEF"),
        (&ctx.kcode_indices, "KCODE"),
        (&ctx.ksrc_indices, "KSRC"),
        (&ctx.nps_indices, "NPS"),
    ] {
        if indices.len() > 1 {
            let first_line = data_line(spans, indices[0]);
            for &dup_idx in &indices[1..] {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::DuplicateDataCard,
                    severity: Severity::Error,
                    category: ValidationCategory::DuplicateId,
                    line_number: data_line(spans, dup_idx),
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
    ctx.active_particles = if let Some(&mode_idx) = ctx.mode_indices.first() {
        if let DataCard::Mode { particles } = &input.data_cards[mode_idx] {
            if particles.is_empty() {
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::EmptyModeCard,
                    severity: Severity::Error,
                    category: ValidationCategory::Mode,
                    line_number: data_line(spans, mode_idx),
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

}
