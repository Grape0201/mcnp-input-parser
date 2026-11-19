use super::inspect::InspectTarget;
use serde::{Deserialize, Serialize};

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

