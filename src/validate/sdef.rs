use super::context::{check_particle_in_mode, ValidationContext};
use super::types::{ErrorCode, Severity, ValidationCategory, ValidationError};
use crate::ast::{Particle, SdefParam, SdefValue, SdefVariable, SiOption};
use std::collections::{HashMap, HashSet};

fn push_diag(
    ctx: &mut ValidationContext,
    code: ErrorCode,
    severity: Severity,
    category: ValidationCategory,
    line: Option<usize>,
    message: String,
) {
    ctx.diagnostics.push(ValidationError {
        code,
        severity,
        category,
        line_number: line,
        message,
    });
}

fn var_name(var: &SdefVariable) -> String {
    match var {
        SdefVariable::Custom(s) => s.to_uppercase(),
        _ => format!("{:?}", var).to_uppercase(),
    }
}

fn is_position_related(var: &SdefVariable) -> bool {
    matches!(
        var,
        SdefVariable::Cel
            | SdefVariable::Sur
            | SdefVariable::Rad
            | SdefVariable::Axs
            | SdefVariable::Ext
            | SdefVariable::X
            | SdefVariable::Y
            | SdefVariable::Z
            | SdefVariable::Ccc
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecialParKind {
    Cosmic,
    Background,
    Spontaneous,
}

fn classify_special_par(sym: &str) -> Option<SpecialParKind> {
    let trimmed = sym.trim();
    let unsigned = trimmed.strip_prefix('-').unwrap_or(trimmed);
    let upper = unsigned.to_ascii_uppercase();
    match upper.as_str() {
        "CR" | "CH" | "CA" => Some(SpecialParKind::Cosmic),
        "BG" | "BN" | "BP" => Some(SpecialParKind::Background),
        "SF" | "SN" | "SP" | "SB" | "ST" | "SA" | "SD" => Some(SpecialParKind::Spontaneous),
        _ if upper.starts_with('C')
            && upper.len() >= 5
            && upper[1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            Some(SpecialParKind::Cosmic)
        }
        _ => None,
    }
}

fn extract_numeric_list(value: &SdefValue) -> Option<Vec<f64>> {
    match value {
        SdefValue::Vector(v) => Some(v.to_vec()),
        SdefValue::List(entries) => {
            let nums: Vec<f64> = entries.iter().filter_map(|e| e.as_f64()).collect();
            if nums.len() == entries.len() {
                Some(nums)
            } else {
                None
            }
        }
        SdefValue::Scalar(v) => Some(vec![*v]),
        _ => None,
    }
}

fn validate_cell_ref(cid: u32, raw_display: &str, line: Option<usize>, ctx: &mut ValidationContext) {
    if cid > 0 && !ctx.cell_map.contains_key(&cid) {
        push_diag(
            ctx,
            ErrorCode::UndefinedCell,
            Severity::Error,
            ValidationCategory::Reference,
            line,
            format!("SDEF {} references undefined cell {}", raw_display, cid),
        );
    }
}

fn validate_scalar_param(
    var: &SdefVariable,
    v: f64,
    line: Option<usize>,
    ctx: &mut ValidationContext,
) {
    let name = var_name(var);
    match var {
        SdefVariable::Cel => {
            if v < 0.0 {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF CEL={} cannot be negative", v),
                );
            } else {
                validate_cell_ref(v.round() as u32, &format!("CEL={}", v.round() as u32), line, ctx);
            }
        }
        SdefVariable::Ccc => {
            if v <= 0.0 {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF CCC={} must be a positive cell ID", v),
                );
            } else {
                validate_cell_ref(v.round() as u32, &format!("CCC={}", v.round() as u32), line, ctx);
            }
        }
        SdefVariable::Sur => {
            if v < 0.0 {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF SUR={} cannot be negative", v),
                );
            } else {
                let sid = v.round() as u32;
                ctx.used_surfaces.insert(sid);
                if sid > 0 && !ctx.surface_map.contains_key(&sid) {
                    push_diag(
                        ctx,
                        ErrorCode::UndefinedSurface,
                        Severity::Error,
                        ValidationCategory::Reference,
                        line,
                        format!("SDEF SUR={} references undefined surface {}", sid, sid),
                    );
                }
            }
        }
        SdefVariable::Tr => {
            if v < 0.0 {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF TR={} cannot be negative", v),
                );
            } else {
                let tid = v.round() as u32;
                if tid > 0 && !ctx.transforms.contains_key(&tid) {
                    push_diag(
                        ctx,
                        ErrorCode::UndefinedTransform,
                        Severity::Error,
                        ValidationCategory::Reference,
                        line,
                        format!("SDEF TR={} references undefined transform TR{}", tid, tid),
                    );
                }
            }
        }
        SdefVariable::Erg if v < 0.0 => {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF ERG={} cannot be negative", v),
            );
        }
        SdefVariable::Dir if !(-1.0..=1.0).contains(&v) => {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF DIR={} must be in range [-1, 1]", v),
            );
        }
        SdefVariable::Rad if v < 0.0 => {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF RAD={} cannot be negative", v),
            );
        }
        SdefVariable::Ara if v <= 0.0 => {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF ARA={} must be positive", v),
            );
        }
        SdefVariable::Wgt if v <= 0.0 => {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF WGT={} must be positive", v),
            );
        }
        SdefVariable::Eff if !(v > 0.0 && v <= 1.0) => {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF EFF={} must be in range (0, 1]", v),
            );
        }
        SdefVariable::Nrm if (v - 1.0).abs() > 1e-9 && (v + 1.0).abs() > 1e-9 => {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF NRM={} must be +1 or -1", v),
            );
        }
        SdefVariable::Dat | SdefVariable::Loc | SdefVariable::Bem | SdefVariable::Bap => {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF {} requires 3 numeric arguments", name),
            );
        }
        _ => {}
    }
}

fn validate_multi_arg_param(
    var: &SdefVariable,
    value: &SdefValue,
    line: Option<usize>,
    ctx: &mut ValidationContext,
) {
    let name = var_name(var);
    let Some(nums) = extract_numeric_list(value) else {
        push_diag(
            ctx,
            ErrorCode::InvalidSdefParameter,
            Severity::Error,
            ValidationCategory::Source,
            line,
            format!("SDEF {} requires numeric arguments", name),
        );
        return;
    };

    if nums.len() != 3 {
        push_diag(
            ctx,
            ErrorCode::InvalidSdefParameter,
            Severity::Error,
            ValidationCategory::Source,
            line,
            format!("SDEF {} expects 3 arguments, got {}", name, nums.len()),
        );
        return;
    }

    match var {
        SdefVariable::Dat => {
            let (m, d, y) = (nums[0], nums[1], nums[2]);
            let is_int = |x: f64| (x - x.round()).abs() < 1e-9;
            if !is_int(m) || !(1.0..=12.0).contains(&m) {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF DAT month {} must be an integer in 1..=12", m),
                );
            }
            if !is_int(d) || !(1.0..=31.0).contains(&d) {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF DAT day {} must be an integer in 1..=31", d),
                );
            }
            if !is_int(y) || !(1000.0..=9999.0).contains(&y) {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF DAT year {} must be a 4-digit integer", y),
                );
            }
        }
        SdefVariable::Loc => {
            let (lat, lng) = (nums[0], nums[1]);
            if !(-90.0..=90.0).contains(&lat) {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF LOC latitude {} must be in [-90, 90]", lat),
                );
            }
            if !(-180.0..=180.0).contains(&lng) {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefParameter,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!("SDEF LOC longitude {} must be in [-180, 180]", lng),
                );
            }
        }
        _ => {}
    }
}

pub(super) fn validate_sdef(
    params: &[SdefParam],
    line: Option<usize>,
    ctx: &mut ValidationContext,
) {
    let mut seen_vars: HashSet<String> = HashSet::new();
    let mut used_dists: HashMap<u32, String> = HashMap::new();
    let mut function_vars: HashSet<String> = HashSet::new();

    let mut par_kind: Option<SpecialParKind> = None;
    let mut has_dat = false;
    let mut has_loc = false;
    let mut tr_dist_id: Option<u32> = None;

    for param in params {
        let key = var_name(&param.variable);
        if !seen_vars.insert(key.clone()) {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("Duplicate SDEF keyword `{}`", key),
            );
        }

        if let SdefVariable::Custom(raw) = &param.variable {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Warning,
                ValidationCategory::Source,
                line,
                format!("Unrecognized SDEF keyword `{}`", raw),
            );
        }

        if matches!(param.value, SdefValue::Function { .. }) {
            function_vars.insert(key.clone());
        }

        match param.variable {
            SdefVariable::Dat => has_dat = true,
            SdefVariable::Loc => has_loc = true,
            _ => {}
        }

        // Explicit-value-only keywords (WGT, EFF, NRM)
        if matches!(
            param.variable,
            SdefVariable::Wgt | SdefVariable::Eff | SdefVariable::Nrm
        ) && !matches!(param.value, SdefValue::Scalar(_))
        {
            push_diag(
                ctx,
                ErrorCode::InvalidSdefParameter,
                Severity::Error,
                ValidationCategory::Source,
                line,
                format!("SDEF {} must be an explicit scalar value (distributions are not allowed)", key),
            );
        }

        match &param.value {
            SdefValue::Distribution(d) => {
                if param.variable.is_vector() && *d > 0 {
                    ctx.vector_dist_ids.insert(*d);
                }
                ctx.referenced_dists.push((*d, line, format!("SDEF {}", key)));
                if let Some(prev_var) = used_dists.insert(*d, key.clone()) {
                    push_diag(
                        ctx,
                        ErrorCode::InvalidSdefDependency,
                        Severity::Error,
                        ValidationCategory::Source,
                        line,
                        format!(
                            "Distribution D{} is used by multiple SDEF variables (`{}` and `{}`)",
                            d, prev_var, key
                        ),
                    );
                }
                if param.variable == SdefVariable::Tr {
                    tr_dist_id = Some(*d);
                }
            }
            SdefValue::Function {
                independent_var,
                dist_id,
            } => {
                if param.variable.is_vector() && *dist_id > 0 {
                    ctx.vector_dist_ids.insert(*dist_id);
                }
                ctx.referenced_dists
                    .push((*dist_id, line, format!("SDEF {}", key)));
                if let Some(prev_var) = used_dists.insert(*dist_id, key.clone()) {
                    push_diag(
                        ctx,
                        ErrorCode::InvalidSdefDependency,
                        Severity::Error,
                        ValidationCategory::Source,
                        line,
                        format!(
                            "Distribution D{} is used by multiple SDEF variables (`{}` and `{}`)",
                            dist_id, prev_var, key
                        ),
                    );
                }

                if &param.variable == independent_var {
                    push_diag(
                        ctx,
                        ErrorCode::InvalidSdefDependency,
                        Severity::Error,
                        ValidationCategory::Source,
                        line,
                        format!("SDEF {} cannot depend on itself (F{})", key, key),
                    );
                } else if *independent_var == SdefVariable::Pos && is_position_related(&param.variable)
                {
                    push_diag(
                        ctx,
                        ErrorCode::InvalidSdefDependency,
                        Severity::Error,
                        ValidationCategory::Source,
                        line,
                        format!(
                            "Position-related SDEF variable `{}` cannot depend on POS (FPOS)",
                            key
                        ),
                    );
                }

                if param.variable == SdefVariable::Tr {
                    tr_dist_id = Some(*dist_id);
                }
            }
            SdefValue::Scalar(v) => validate_scalar_param(&param.variable, *v, line, ctx),
            SdefValue::Vector(_) | SdefValue::List(_)
                if matches!(
                    param.variable,
                    SdefVariable::Dat | SdefVariable::Loc | SdefVariable::Bem | SdefVariable::Bap
                ) =>
            {
                validate_multi_arg_param(&param.variable, &param.value, line, ctx);
            }
            SdefValue::Symbol(sym) => {
                if param.variable == SdefVariable::Cel {
                    for part in sym.split(':') {
                        if let Ok(cid) = part.trim().parse::<u32>() {
                            validate_cell_ref(cid, &format!("CEL={}", sym), line, ctx);
                        }
                    }
                } else if param.variable == SdefVariable::Par {
                    if let Some(kind) = classify_special_par(sym) {
                        par_kind = Some(kind);
                    } else {
                        let p = Particle::new(sym.trim());
                        check_particle_in_mode(
                            &p,
                            &ctx.active_particles,
                            line,
                            "SDEF PAR",
                            &mut ctx.diagnostics,
                        );
                    }
                } else if matches!(
                    param.variable,
                    SdefVariable::Dat | SdefVariable::Loc | SdefVariable::Bem | SdefVariable::Bap
                ) {
                    push_diag(
                        ctx,
                        ErrorCode::InvalidSdefParameter,
                        Severity::Error,
                        ValidationCategory::Source,
                        line,
                        format!("SDEF {} requires numeric arguments, got `{}`", key, sym),
                    );
                }
            }
            _ => {}
        }
    }

    // Check 1-level dependency constraint: independent_var cannot itself be a Function
    for param in params {
        if let SdefValue::Function {
            independent_var, ..
        } = &param.value
        {
            let indep_key = var_name(independent_var);
            if function_vars.contains(&indep_key) && var_name(&param.variable) != indep_key {
                push_diag(
                    ctx,
                    ErrorCode::InvalidSdefDependency,
                    Severity::Error,
                    ValidationCategory::Source,
                    line,
                    format!(
                        "SDEF {} depends on `{}` which is itself a dependent distribution (only one level of dependence is allowed)",
                        var_name(&param.variable),
                        indep_key
                    ),
                );
            }
        }
    }

    // Check PAR=BG/BN/BP requires LOC, and DAT/LOC are for cosmic/background sources
    if par_kind == Some(SpecialParKind::Background) && !has_loc {
        push_diag(
            ctx,
            ErrorCode::InconsistentSdefOptions,
            Severity::Warning,
            ValidationCategory::Source,
            line,
            "SDEF PAR=BG/BN/BP requires the LOC keyword".to_string(),
        );
    }

    if (has_dat || has_loc)
        && !matches!(
            par_kind,
            Some(SpecialParKind::Cosmic) | Some(SpecialParKind::Background)
        )
    {
        push_diag(
            ctx,
            ErrorCode::InconsistentSdefOptions,
            Severity::Warning,
            ValidationCategory::Source,
            line,
            "SDEF DAT/LOC keywords are only used with cosmic (PAR=CR/CH/CA) or background (PAR=BG/BN/BP) sources".to_string(),
        );
    }

    // Check TR=Dm requires SIm with option L (Discrete)
    if let Some(d) = tr_dist_id {
        if let Some((_, si_card)) = ctx.si_cards.get(&d) {
            if si_card.option != SiOption::Discrete {
                push_diag(
                    ctx,
                    ErrorCode::InconsistentSdefOptions,
                    Severity::Warning,
                    ValidationCategory::Source,
                    line,
                    format!(
                        "SDEF TR=D{} requires SI{} to use option L (discrete distribution)",
                        d, d
                    ),
                );
            }
        }
    }
}
