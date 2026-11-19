use super::context::{surf_line, ValidationContext};
use super::types::{ErrorCode, Severity, ValidationCategory, ValidationError};
use crate::ast::{BoundaryCondition, SurfaceKind};

pub(super) fn validate_surfaces(ctx: &mut ValidationContext<'_>) {
    let input = ctx.input;
    let spans = ctx.spans;
    // ========================================================================
    // 3. Surface & Macrobody Validation
    // ========================================================================
    for (idx, surf) in input.surfaces.iter().enumerate() {
        let line = surf_line(spans, idx, surf.id);

        // Validate transform_id or periodic boundary
        if let Some(tr_id) = surf.transform_id {
            if tr_id > 0 {
                let tr_u32 = tr_id as u32;
                if !ctx.transforms.contains_key(&tr_u32) {
                    ctx.diagnostics.push(ValidationError {
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
                ctx.used_surfaces.insert(peer_surf_id);
                if surf.boundary != BoundaryCondition::Transmissive {
                    ctx.diagnostics.push(ValidationError {
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
                    ctx.diagnostics.push(ValidationError {
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
                if peer_surf_id == surf.id || !ctx.surface_map.contains_key(&peer_surf_id) {
                    ctx.diagnostics.push(ValidationError {
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
                ctx.diagnostics.push(ValidationError {
                    code: ErrorCode::InvalidPeriodicBoundary,
                    severity: Severity::Error,
                    category: ValidationCategory::Surface,
                    line_number: line,
                    message: format!("Surface {} has invalid transform/periodic ID 0", surf.id),
                });
            }
        }

        validate_surface_kind_and_params(surf, line, &mut ctx.diagnostics);
    }

}

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

pub(super) fn macrobody_max_facets(kind: &SurfaceKind) -> Option<u32> {
    match kind {
        SurfaceKind::BOX | SurfaceKind::RPP | SurfaceKind::ARB => Some(6),
        SurfaceKind::RCC | SurfaceKind::REC | SurfaceKind::TRC => Some(3),
        SurfaceKind::SPH | SurfaceKind::ELL => Some(1),
        SurfaceKind::RHP | SurfaceKind::HEX => Some(8),
        SurfaceKind::WED => Some(5),
        _ => None,
    }
}
