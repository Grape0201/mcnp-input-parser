use super::context::is_strictly_increasing;
use super::types::{ErrorCode, Severity, ValidationCategory, ValidationError};
use crate::ast::FmeshCard;
use std::collections::HashMap;

pub(super) fn validate_fmesh(
    fm: &FmeshCard,
    line: Option<usize>,
    transforms: &HashMap<u32, usize>,
    has_ssr: bool,
    any_fmesh_xdmf: bool,
    diagnostics: &mut Vec<ValidationError>,
) {
    let mut push_err = |message: String| {
        diagnostics.push(ValidationError {
            code: ErrorCode::InvalidFmeshConfiguration,
            severity: Severity::Error,
            category: ValidationCategory::Tally,
            line_number: line,
            message,
        });
    };

    // 1. Tally number ending with 4 (Detail 10)
    if fm.id % 10 != 4 {
        push_err(format!(
            "FMESH{} ID must end with digit 4 (e.g. FMESH4, FMESH14)",
            fm.id
        ));
    }

    // 2. Single particle designator
    if fm.particles.is_empty() {
        push_err(format!(
            "FMESH{} requires a single particle designator (e.g. FMESH{}:n)",
            fm.id, fm.id
        ));
    } else if fm.particles.len() > 1 {
        push_err(format!(
            "FMESH{} allows only a single particle designator, got {}",
            fm.id,
            fm.particles.len()
        ));
    }

    // 3. Report unrecognized or malformed keyword tokens captured by the parser
    for (tag, vals) in &fm.extra_options {
        if tag == "_unknown" {
            push_err(format!(
                "FMESH{} contains unrecognized option or token `{}`",
                fm.id,
                vals.join(" ")
            ));
        } else if let Some(key) = tag.strip_prefix("_empty:") {
            push_err(format!(
                "FMESH{} keyword `{}` requires a value",
                fm.id,
                key.to_uppercase()
            ));
        } else if let Some(key) = tag.strip_prefix("_extra:") {
            push_err(format!(
                "FMESH{} keyword `{}` has unexpected extra or unrecognized token(s): `{}`",
                fm.id,
                key.to_uppercase(),
                vals.join(" ")
            ));
        } else if let Some(key) = tag.strip_prefix("_invalid:") {
            push_err(format!(
                "FMESH{} keyword `{}` contains invalid value(s): `{}`",
                fm.id,
                key.to_uppercase(),
                vals.join(" ")
            ));
        } else if let Some(key) = tag.strip_prefix("_non_integer:") {
            push_err(format!(
                "FMESH{} keyword `{}` requires non-negative integer value(s), got `{}`",
                fm.id,
                key.to_uppercase(),
                vals.join(" ")
            ));
        } else if let Some(key) = tag.strip_prefix("_vec3_len:") {
            push_err(format!(
                "FMESH{} keyword `{}` requires exactly 3 coordinates, got {}",
                fm.id,
                key.to_uppercase(),
                vals.len()
            ));
        } else if let Some(key) = tag.strip_prefix("_single_len:") {
            push_err(format!(
                "FMESH{} keyword `{}` requires a single numeric value, got {}",
                fm.id,
                key.to_uppercase(),
                vals.len()
            ));
        } else {
            push_err(format!(
                "FMESH{} contains unrecognized option `{}` (`{}`)",
                fm.id,
                tag,
                vals.join(" ")
            ));
        }
    }

    // 4. Geometry keyword validation
    if let Some(ref g) = fm.geom {
        if !matches!(g.as_str(), "xyz" | "rec" | "rzt" | "cyl") {
            push_err(format!(
                "FMESH{} has invalid GEOM `{}` (expected xyz, rec, rzt, or cyl)",
                fm.id, g
            ));
        }
    }
    let is_cyl = matches!(fm.geom.as_deref(), Some("rzt" | "cyl"));

    // 5. Vector validation (AXS, VEC)
    let mut axs_zero = false;
    if let Some(a) = fm.axs {
        let norm_sq = a[0] * a[0] + a[1] * a[1] + a[2] * a[2];
        if norm_sq <= 1e-24 {
            axs_zero = true;
            push_err(format!("FMESH{} AXS vector must not be the zero vector", fm.id));
        }
    }
    let mut vec_zero = false;
    if let Some(v) = fm.vec {
        let norm_sq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
        if norm_sq <= 1e-24 {
            vec_zero = true;
            push_err(format!("FMESH{} VEC vector must not be the zero vector", fm.id));
        }
    }
    if (is_cyl || fm.axs.is_some() || fm.vec.is_some()) && !axs_zero && !vec_zero {
        let a = fm.axs.unwrap_or([0.0, 0.0, 1.0]);
        let v = fm.vec.unwrap_or([1.0, 0.0, 0.0]);
        let cx = a[1] * v[2] - a[2] * v[1];
        let cy = a[2] * v[0] - a[0] * v[2];
        let cz = a[0] * v[1] - a[1] * v[0];
        let cross_sq = cx * cx + cy * cy + cz * cz;
        let a_sq = a[0] * a[0] + a[1] * a[1] + a[2] * a[2];
        let v_sq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
        if cross_sq <= 1e-18 * a_sq * v_sq {
            push_err(format!(
                "FMESH{} AXS and VEC vectors must not be parallel",
                fm.id
            ));
        }
    }

    // 6. Check if this is a restart-only card (Detail 5: only OUT keyword present)
    let is_restart_out_only = fm.out.is_some()
        && fm.geom.is_none()
        && fm.origin.is_none()
        && fm.axs.is_none()
        && fm.vec.is_none()
        && fm.imesh.is_empty()
        && fm.iints.is_empty()
        && fm.jmesh.is_empty()
        && fm.jints.is_empty()
        && fm.kmesh.is_empty()
        && fm.kints.is_empty()
        && fm.emesh.is_empty()
        && fm.eints.is_empty()
        && fm.enorm.is_none()
        && fm.tmesh.is_empty()
        && fm.tints.is_empty()
        && fm.tnorm.is_none()
        && fm.factor.is_none()
        && fm.tr.is_none()
        && fm.inc.is_empty()
        && fm.tally_type.is_none()
        && fm.kclear.is_none()
        && fm.tally.is_none()
        && fm.extra_options.is_empty();

    if !is_restart_out_only {
        for (mesh_name, mesh) in [
            ("IMESH", &fm.imesh),
            ("JMESH", &fm.jmesh),
            ("KMESH", &fm.kmesh),
        ] {
            let has_parse_err = fm.extra_options.iter().any(|(t, _)| {
                t.eq_ignore_ascii_case(&format!("_empty:{}", mesh_name))
                    || t.eq_ignore_ascii_case(&format!("_invalid:{}", mesh_name))
            });
            if mesh.is_empty() && !has_parse_err {
                push_err(format!(
                    "FMESH{} requires at least one coarse mesh point for {}",
                    fm.id, mesh_name
                ));
            }
        }
    }

    // 7. Spatial mesh bounds, monotonicity, and interval counts (Details 1, 2, 3)
    let origin = fm.origin.unwrap_or([0.0, 0.0, 0.0]);
    let lower_bounds = if is_cyl {
        [
            ("I (IMESH/IINTS)", "r origin", 0.0, &fm.imesh, &fm.iints),
            ("J (JMESH/JINTS)", "z origin", 0.0, &fm.jmesh, &fm.jints),
            ("K (KMESH/KINTS)", "theta origin", 0.0, &fm.kmesh, &fm.kints),
        ]
    } else {
        [
            ("I (IMESH/IINTS)", "ORIGIN X", origin[0], &fm.imesh, &fm.iints),
            ("J (JMESH/JINTS)", "ORIGIN Y", origin[1], &fm.jmesh, &fm.jints),
            ("K (KMESH/KINTS)", "ORIGIN Z", origin[2], &fm.kmesh, &fm.kints),
        ]
    };

    for (axis_name, lb_label, lb_val, mesh, ints) in lower_bounds {
        if !ints.is_empty() && mesh.len() != ints.len() {
            push_err(format!(
                "FMESH{} {} has {} mesh points but {} interval counts",
                fm.id,
                axis_name,
                mesh.len(),
                ints.len()
            ));
        }
        if let Some(&first) = mesh.first() {
            if first <= lb_val {
                push_err(format!(
                    "FMESH{} {} first coarse mesh point ({}) must be greater than {} ({})",
                    fm.id, axis_name, first, lb_label, lb_val
                ));
            }
        }
        if mesh.len() >= 2 && !is_strictly_increasing(mesh) {
            push_err(format!(
                "FMESH{} {} coordinates must be strictly increasing",
                fm.id, axis_name
            ));
        }
        if ints.contains(&0) {
            push_err(format!(
                "FMESH{} {} interval count must be greater than 0",
                fm.id, axis_name
            ));
        }
    }

    if is_cyl {
        if let Some(&last_k) = fm.kmesh.last() {
            if (last_k - 1.0).abs() > 1e-9 {
                push_err(format!(
                    "FMESH{} cylindrical KMESH (theta in revolutions) must end with 1.0, got {}",
                    fm.id, last_k
                ));
            }
        }
    }

    // 8. Energy mesh (EMESH / EINTS / ENORM)
    if !fm.eints.is_empty() && fm.emesh.len() != fm.eints.len() {
        push_err(format!(
            "FMESH{} EMESH ({}) and EINTS ({}) lengths do not match",
            fm.id,
            fm.emesh.len(),
            fm.eints.len()
        ));
    }
    if let Some(&first_e) = fm.emesh.first() {
        if first_e <= 0.0 {
            push_err(format!(
                "FMESH{} EMESH first bin boundary ({}) must be greater than 0.0 MeV",
                fm.id, first_e
            ));
        }
    }
    if fm.emesh.len() >= 2 && !is_strictly_increasing(&fm.emesh) {
        push_err(format!("FMESH{} EMESH bins must be strictly increasing", fm.id));
    }
    if fm.eints.contains(&0) {
        push_err(format!(
            "FMESH{} EINTS interval count must be greater than 0",
            fm.id
        ));
    }
    if let Some(ref enorm) = fm.enorm {
        if !matches!(enorm.as_str(), "yes" | "no") {
            push_err(format!(
                "FMESH{} has invalid ENORM `{}` (expected yes or no)",
                fm.id, enorm
            ));
        }
    }

    // 9. Time mesh (TMESH / TINTS / TNORM)
    if !fm.tints.is_empty() && fm.tmesh.len() != fm.tints.len() {
        push_err(format!(
            "FMESH{} TMESH ({}) and TINTS ({}) lengths do not match",
            fm.id,
            fm.tmesh.len(),
            fm.tints.len()
        ));
    }
    if fm.tmesh.len() >= 2 && !is_strictly_increasing(&fm.tmesh) {
        push_err(format!("FMESH{} TMESH bins must be strictly increasing", fm.id));
    }
    if fm.tints.contains(&0) {
        push_err(format!(
            "FMESH{} TINTS interval count must be greater than 0",
            fm.id
        ));
    }
    if let Some(ref tnorm) = fm.tnorm {
        if !matches!(tnorm.as_str(), "yes" | "no") {
            push_err(format!(
                "FMESH{} has invalid TNORM `{}` (expected yes or no)",
                fm.id, tnorm
            ));
        }
    }

    // 10. FACTOR, OUT, TYPE, TALLY, INC, TR
    if let Some(factor) = fm.factor {
        if factor <= 0.0 {
            push_err(format!(
                "FMESH{} FACTOR ({}) must be greater than 0.0",
                fm.id, factor
            ));
        }
    }

    if let Some(ref out) = fm.out {
        if !matches!(
            out.as_str(),
            "col" | "colsci" | "cf" | "cfsci" | "ij" | "ik" | "jk" | "none" | "xdmf"
        ) {
            push_err(format!(
                "FMESH{} has invalid OUT format `{}` (expected col, colsci, cf, cfsci, ij, ik, jk, none, or xdmf)",
                fm.id, out
            ));
        }
    }
    if any_fmesh_xdmf {
        let effective_out = fm.out.as_deref().unwrap_or("col");
        if !matches!(effective_out, "xdmf" | "none") {
            push_err(format!(
                "FMESH{} output format `{}` cannot be combined with XDMF mesh tally output in the same input file (only `xdmf` or `none` is allowed)",
                fm.id, effective_out
            ));
        }
    }

    if let Some(ref ttype) = fm.tally_type {
        if !matches!(ttype.as_str(), "flux" | "source") {
            push_err(format!(
                "FMESH{} has invalid TYPE `{}` (expected flux or source)",
                fm.id, ttype
            ));
        }
    }

    if let Some(ref tally_alg) = fm.tally {
        if !matches!(
            tally_alg.as_str(),
            "hist" | "fast_hist" | "batch" | "rma_batch"
        ) {
            push_err(format!(
                "FMESH{} has invalid TALLY algorithm `{}` (expected hist, fast_hist, batch, or rma_batch)",
                fm.id, tally_alg
            ));
        } else if has_ssr && matches!(tally_alg.as_str(), "batch" | "rma_batch") {
            push_err(format!(
                "FMESH{} TALLY={} is incompatible with an SSR surface source card",
                fm.id, tally_alg
            ));
        }
    }

    if !fm.inc.is_empty() {
        if fm.inc.len() > 2 {
            push_err(format!(
                "FMESH{} INC accepts 1 or 2 arguments (LOW [HIGH]), got {}",
                fm.id,
                fm.inc.len()
            ));
        } else {
            match fm.inc[0].parse::<u64>() {
                Ok(low) => {
                    if fm.inc.len() == 2 && fm.inc[1] != "infinite" {
                        match fm.inc[1].parse::<u64>() {
                            Ok(high) if high < low => {
                                push_err(format!(
                                    "FMESH{} INC HIGH ({}) must be greater than or equal to LOW ({})",
                                    fm.id, high, low
                                ));
                            }
                            Ok(_) => {}
                            Err(_) => {
                                push_err(format!(
                                    "FMESH{} INC HIGH value `{}` must be a non-negative integer or `infinite`",
                                    fm.id, fm.inc[1]
                                ));
                            }
                        }
                    }
                }
                Err(_) => {
                    push_err(format!(
                        "FMESH{} INC LOW value `{}` must be a non-negative integer",
                        fm.id, fm.inc[0]
                    ));
                }
            }
        }
    }

    if let Some(tr_id) = fm.tr {
        if tr_id == 0 || !transforms.contains_key(&tr_id) {
            diagnostics.push(ValidationError {
                code: ErrorCode::UndefinedTransform,
                severity: Severity::Error,
                category: ValidationCategory::Reference,
                line_number: line,
                message: format!("FMESH{} references undefined transform TR{}", fm.id, tr_id),
            });
        }
    }
}
