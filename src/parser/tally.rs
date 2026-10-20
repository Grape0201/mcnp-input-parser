use crate::ast::{
    DataCard, DetectorEntry, DoseCard, Entry, FmBin, FmCard, FmeshCard, InterpMode, Particle,
    TallyKind, TallyTarget,
};
use crate::lexer::{is_token_boundary, mcnp_float, mcnp_i32, raw_token, sep0, sep1, ws0};
use crate::shortcut::{
    expand_shortcut_items, parse_expanded_entries, parse_expanded_floats, shortcut_item,
    ShortcutItem,
};
use nom::{
    bytes::complete::{take_until, take_while1},
    character::complete::char,
    sequence::delimited,
    IResult,
};

/// Takes a balanced parenthesized group `( ... )`, returning the inner slice.
fn take_balanced_parens(input: &str) -> IResult<&str, &str> {
    if !input.starts_with('(') {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Char,
        )));
    }
    let mut depth = 0usize;
    for (idx, ch) in input.char_indices() {
        if ch == '(' {
            depth += 1;
        } else if ch == ')' {
            depth -= 1;
            if depth == 0 {
                return Ok((&input[idx + 1..], &input[1..idx]));
            }
        }
    }
    Err(nom::Err::Error(nom::error::Error::new(
        input,
        nom::error::ErrorKind::Char,
    )))
}

/// Parses standard tally card (`F1`, `F2`, `F4`, `F6`, `F7`, `F8`).
pub fn parse_standard_tally_body(
    id: u32,
    particles: Vec<Particle>,
    modifier: Option<char>,
    mut input: &str,
) -> IResult<&str, DataCard> {
    let kind = TallyKind::from_id(id);
    let mut targets: Vec<TallyTarget> = Vec::new();
    let mut pending_shortcuts: Vec<ShortcutItem> = Vec::new();
    let mut total_bin = false;
    let mut no_direct = false;

    let flush_pending =
        |pending: &mut Vec<ShortcutItem>, out: &mut Vec<TallyTarget>| -> Result<(), String> {
            if pending.is_empty() {
                return Ok(());
            }
            let expanded = expand_shortcut_items(pending)?;
            pending.clear();
            for e in expanded {
                if let Entry::Value(v) = e {
                    out.push(TallyTarget::Single(v.round() as u32));
                }
            }
            Ok(())
        };

    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }

        if r.starts_with('(') {
            flush_pending(&mut pending_shortcuts, &mut targets).map_err(|_| {
                nom::Err::Error(nom::error::Error::new(r, nom::error::ErrorKind::Verify))
            })?;
            let (next_r, inner) = take_balanced_parens(r)?;
            let trimmed = inner.trim();
            // If inner contains lattice operators `<` or `[` or nested parens, treat as ComplexGroup
            if trimmed.contains('<') || trimmed.contains('[') || trimmed.contains('(') {
                targets.push(TallyTarget::ComplexGroup(trimmed.to_string()));
            } else if let Ok((rem_inner, floats)) = parse_expanded_floats(trimmed) {
                if rem_inner.trim().is_empty() {
                    let ids = floats.into_iter().map(|v| v.round() as u32).collect();
                    targets.push(TallyTarget::Group(ids));
                } else {
                    targets.push(TallyTarget::ComplexGroup(trimmed.to_string()));
                }
            } else {
                targets.push(TallyTarget::ComplexGroup(trimmed.to_string()));
            }
            input = next_r;
            continue;
        }

        if let Ok((next_r, item)) = shortcut_item(r) {
            pending_shortcuts.push(item);
            input = next_r;
            continue;
        }

        flush_pending(&mut pending_shortcuts, &mut targets).map_err(|_| {
            nom::Err::Error(nom::error::Error::new(r, nom::error::ErrorKind::Verify))
        })?;

        let (next_r, tok) = raw_token(r)?;
        if tok.eq_ignore_ascii_case("t") {
            total_bin = true;
        } else if tok.eq_ignore_ascii_case("nd") {
            no_direct = true;
        } else {
            targets.push(TallyTarget::ComplexGroup(tok.to_string()));
        }
        input = next_r;
    }

    flush_pending(&mut pending_shortcuts, &mut targets).map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
    })?;

    Ok((
        input,
        DataCard::Tally {
            id,
            kind,
            particles,
            modifier,
            targets,
            total_bin,
            no_direct,
        },
    ))
}

/// Parses point or ring detector tally (`F5`, `FX5`, `FY5`, `FZ5`).
pub fn parse_detector_tally_body(
    id: u32,
    axis: Option<char>,
    particles: Vec<Particle>,
    modifier: Option<char>,
    mut input: &str,
) -> IResult<&str, DataCard> {
    let mut items = Vec::new();
    let mut no_direct = false;

    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }
        if let Ok((next_r, item)) = shortcut_item(r) {
            items.push(item);
            input = next_r;
        } else {
            let (next_r, tok) = raw_token(r)?;
            if tok.eq_ignore_ascii_case("nd") {
                no_direct = true;
            } else {
                return Err(nom::Err::Error(nom::error::Error::new(
                    r,
                    nom::error::ErrorKind::Tag,
                )));
            }
            input = next_r;
        }
    }

    let entries = expand_shortcut_items(&items).map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
    })?;
    let mut floats = Vec::with_capacity(entries.len());
    for e in entries {
        if let Entry::Value(v) = e {
            floats.push(v);
        }
    }

    let mut detectors = Vec::new();
    match axis {
        None => {
            if floats.is_empty() || floats.len() % 4 != 0 {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Verify,
                )));
            }
            for chunk in floats.chunks_exact(4) {
                detectors.push(DetectorEntry::Point {
                    x: chunk[0],
                    y: chunk[1],
                    z: chunk[2],
                    r0: chunk[3],
                });
            }
        }
        Some(ax) => {
            if floats.is_empty() || floats.len() % 3 != 0 {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Verify,
                )));
            }
            for chunk in floats.chunks_exact(3) {
                detectors.push(DetectorEntry::Ring {
                    axis: ax,
                    axis_coord: chunk[0],
                    radius: chunk[1],
                    r0: chunk[2],
                });
            }
        }
    }

    Ok((
        input,
        DataCard::DetectorTally {
            id,
            axis,
            particles,
            modifier,
            detectors,
            no_direct,
        },
    ))
}

/// Parses `DE<n>` (is_function = false) or `DF<n>` (is_function = true) Dose Conversion cards.
pub fn parse_dose_body(id: u32, is_function: bool, mut input: &str) -> IResult<&str, DataCard> {
    let mut interpolation: Option<InterpMode> = None;
    let mut items: Vec<ShortcutItem> = Vec::new();
    let mut options: Vec<(String, String)> = Vec::new();

    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }

        // Check `LOG` or `LIN` keyword before attempting shortcut_item
        if let Ok((next_r, word)) =
            take_while1::<_, _, nom::error::Error<&str>>(|c: char| c.is_ascii_alphabetic())(r)
        {
            if is_token_boundary(next_r) {
                let upper = word.to_uppercase();
                if upper == "LOG" {
                    interpolation = Some(InterpMode::Log);
                    input = next_r;
                    continue;
                } else if upper == "LIN" {
                    interpolation = Some(InterpMode::Lin);
                    input = next_r;
                    continue;
                } else {
                    // Keyword option on DF card (e.g. `IC=99`, `IU=2`, `FAC=-3`)
                    let (r_val, _) = sep1(next_r)?;
                    let (r_after, val_tok) = raw_token(r_val)?;
                    options.push((word.to_lowercase(), val_tok.to_string()));
                    input = r_after;
                    continue;
                }
            }
        }

        let (next_r, item) = shortcut_item(r)?;
        items.push(item);
        input = next_r;
    }

    let entries = expand_shortcut_items(&items).map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
    })?;
    let mut values = Vec::with_capacity(entries.len());
    for e in entries {
        if let Entry::Value(v) = e {
            values.push(v);
        }
    }

    Ok((
        input,
        DataCard::Dose(DoseCard {
            id,
            is_function,
            interpolation,
            values,
            options,
        }),
    ))
}

fn parse_single_fm_group(group_str: &str) -> FmBin {
    let s = group_str.trim();
    let Ok((rem, constant)) = preceded_ws_float(s) else {
        return FmBin::RawGroup(s.to_string());
    };
    let (rem, _) = match sep0(rem) {
        Ok(res) => res,
        Err(_) => return FmBin::Constant(constant),
    };
    if rem.is_empty() {
        return FmBin::Constant(constant);
    }

    let Ok((mut rem, mat_id)) = mcnp_i32(rem) else {
        return FmBin::RawGroup(s.to_string());
    };

    let mut reaction_groups = Vec::new();
    loop {
        let Ok((r, _)) = sep0(rem) else { break };
        if r.is_empty() {
            break;
        }
        if r.starts_with('(') {
            if let Ok((next_r, inner)) = take_balanced_parens(r) {
                reaction_groups.push(inner.trim().to_string());
                rem = next_r;
            } else {
                reaction_groups.push(r.to_string());
                break;
            }
        } else if let Ok((next_r, tok)) = raw_token(r) {
            reaction_groups.push(tok.to_string());
            rem = next_r;
        } else {
            break;
        }
    }

    FmBin::MultiplierSet {
        constant,
        material_id: mat_id,
        reaction_groups,
    }
}

fn preceded_ws_float(input: &str) -> IResult<&str, f64> {
    let (rem, _) = sep0(input)?;
    mcnp_float(rem)
}

/// Parses `FM<n>` Tally Multiplier card body.
pub fn parse_fm_body(id: u32, modifier: Option<char>, mut input: &str) -> IResult<&str, DataCard> {
    let mut bins = Vec::new();

    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }

        if r.starts_with('(') {
            let (next_r, inner) = take_balanced_parens(r)?;
            bins.push(parse_single_fm_group(inner));
            input = next_r;
        } else {
            // Unparenthesized multiplier or single constant: if followed by more tokens on same card without outer parens,
            // parse the rest of the card as a single multiplier group if it has material_id, or single constant.
            let bin = parse_single_fm_group(r);
            bins.push(bin);
            input = "";
            break;
        }
    }

    Ok((
        input,
        DataCard::TallyMultiplier(FmCard { id, modifier, bins }),
    ))
}

/// Parses `FS<n>` Tally Segment card body.
pub fn parse_fs_body(id: u32, mut input: &str) -> IResult<&str, DataCard> {
    let mut items = Vec::new();
    let mut total_bin = false;

    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }
        if let Ok((next_r, item)) = shortcut_item(r) {
            items.push(item);
            input = next_r;
        } else {
            let (next_r, tok) = raw_token(r)?;
            if tok.eq_ignore_ascii_case("t") {
                total_bin = true;
            } else {
                return Err(nom::Err::Error(nom::error::Error::new(
                    r,
                    nom::error::ErrorKind::Tag,
                )));
            }
            input = next_r;
        }
    }

    let entries = expand_shortcut_items(&items).map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
    })?;
    let mut surfaces = Vec::with_capacity(entries.len());
    for e in entries {
        if let Entry::Value(v) = e {
            surfaces.push(v.round() as i32);
        }
    }

    Ok((
        input,
        DataCard::TallySegment {
            id,
            surfaces,
            total_bin,
        },
    ))
}

/// Parses `SD<n>` Segment Divisor card body.
pub fn parse_sd_body(id: u32, input: &str) -> IResult<&str, DataCard> {
    let (rem, values) = parse_expanded_entries(input)?;
    Ok((rem, DataCard::SegmentDivisor { id, values }))
}

/// Helper to parse bin cards (`E<n>`, `T<n>`, `C<n>`).
fn parse_bins_and_flags(mut input: &str) -> IResult<&str, (Vec<Entry>, Vec<String>)> {
    let mut items = Vec::new();
    let mut flags = Vec::new();

    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }
        if let Ok((next_r, item)) = shortcut_item(r) {
            items.push(item);
            input = next_r;
        } else {
            let (next_r, tok) = raw_token(r)?;
            flags.push(tok.to_lowercase());
            input = next_r;
        }
    }

    let bins = expand_shortcut_items(&items).map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
    })?;

    Ok((input, (bins, flags)))
}

/// Parses `E<n>` Tally Energy card body.
pub fn parse_tally_energy_body(
    id: u32,
    modifier: Option<char>,
    input: &str,
) -> IResult<&str, DataCard> {
    let (rem, (bins, flags)) = parse_bins_and_flags(input)?;
    let no_total = flags.iter().any(|f| f == "nt");
    let cumulative = flags.iter().any(|f| f == "c");
    Ok((
        rem,
        DataCard::TallyEnergy {
            id,
            modifier,
            bins,
            no_total,
            cumulative,
        },
    ))
}

/// Parses `T<n>` Tally Time card body.
pub fn parse_tally_time_body(id: u32, input: &str) -> IResult<&str, DataCard> {
    let (rem, (bins, flags)) = parse_bins_and_flags(input)?;
    let no_total = flags.iter().any(|f| f == "nt");
    let cumulative = flags.iter().any(|f| f == "c");
    Ok((
        rem,
        DataCard::TallyTime {
            id,
            bins,
            no_total,
            cumulative,
        },
    ))
}

/// Parses `C<n>` Tally Cosine card body.
pub fn parse_tally_cosine_body(id: u32, input: &str) -> IResult<&str, DataCard> {
    let (rem, (bins, flags)) = parse_bins_and_flags(input)?;
    let total_bin = flags.iter().any(|f| f == "t");
    let cumulative = flags.iter().any(|f| f == "c");
    Ok((
        rem,
        DataCard::TallyCosine {
            id,
            bins,
            total_bin,
            cumulative,
        },
    ))
}

/// Parses `FC<n>` Tally Comment card body.
pub fn parse_fc_body(id: u32, input: &str) -> IResult<&str, DataCard> {
    let (rem, _) = ws0(input)?;
    Ok((
        "",
        DataCard::TallyComment {
            id,
            comment: rem.trim().to_string(),
        },
    ))
}

const KNOWN_FMESH_KEYS: &[&str] = &[
    "geom", "origin", "axs", "vec", "imesh", "iints", "jmesh", "jints", "kmesh", "kints", "emesh",
    "eints", "tmesh", "tints", "factor", "out", "tr", "inc", "kclear",
];

/// Parses `FMESH<n>:<p>` Superimposed Mesh Tally card body.
pub fn parse_fmesh_body(id: u32, particles: Vec<Particle>, mut input: &str) -> IResult<&str, DataCard> {
    let mut raw_tokens = Vec::new();
    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }
        if r.starts_with('(') {
            let (next_r, inner) = delimited(char('('), take_until(")"), char(')'))(r)?;
            for t in inner.split(|c: char| c.is_whitespace() || c == ',') {
                if !t.is_empty() {
                    raw_tokens.push(t.to_string());
                }
            }
            input = next_r;
        } else {
            let (next_r, tok) = raw_token(r)?;
            raw_tokens.push(tok.to_string());
            input = next_r;
        }
    }

    let mut kv_groups: Vec<(String, Vec<String>)> = Vec::new();
    for tok in raw_tokens {
        let lower = tok.to_lowercase();
        if KNOWN_FMESH_KEYS.contains(&lower.as_str()) {
            kv_groups.push((lower, Vec::new()));
        } else if let Some((_, ref mut vals)) = kv_groups.last_mut() {
            vals.push(tok);
        }
    }

    let mut card = FmeshCard {
        id,
        particles,
        geom: None,
        origin: None,
        axs: None,
        vec: None,
        imesh: Vec::new(),
        iints: Vec::new(),
        jmesh: Vec::new(),
        jints: Vec::new(),
        kmesh: Vec::new(),
        kints: Vec::new(),
        emesh: Vec::new(),
        eints: Vec::new(),
        tmesh: Vec::new(),
        tints: Vec::new(),
        factor: None,
        out: None,
        extra_options: Vec::new(),
    };

    let to_floats = |vals: &[String]| -> Vec<f64> {
        let joined = vals.join(" ");
        parse_expanded_floats(&joined)
            .map(|(_, v)| v)
            .unwrap_or_default()
    };
    let to_u32s = |vals: &[String]| -> Vec<u32> {
        to_floats(vals)
            .into_iter()
            .map(|v| v.round() as u32)
            .collect()
    };
    let to_vec3 = |vals: &[String]| -> Option<[f64; 3]> {
        let f = to_floats(vals);
        if f.len() == 3 {
            Some([f[0], f[1], f[2]])
        } else {
            None
        }
    };

    for (key, vals) in kv_groups {
        match key.as_str() {
            "geom" => card.geom = vals.first().map(|s| s.to_lowercase()),
            "origin" => card.origin = to_vec3(&vals),
            "axs" => card.axs = to_vec3(&vals),
            "vec" => card.vec = to_vec3(&vals),
            "imesh" => card.imesh = to_floats(&vals),
            "iints" => card.iints = to_u32s(&vals),
            "jmesh" => card.jmesh = to_floats(&vals),
            "jints" => card.jints = to_u32s(&vals),
            "kmesh" => card.kmesh = to_floats(&vals),
            "kints" => card.kints = to_u32s(&vals),
            "emesh" => card.emesh = to_floats(&vals),
            "eints" => card.eints = to_u32s(&vals),
            "tmesh" => card.tmesh = to_floats(&vals),
            "tints" => card.tints = to_u32s(&vals),
            "factor" => card.factor = to_floats(&vals).first().copied(),
            "out" => card.out = vals.first().map(|s| s.to_lowercase()),
            _ => card.extra_options.push((key, vals)),
        }
    }

    Ok((input, DataCard::Fmesh(card)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::data::parse_data_card_complete;

    #[test]
    fn test_f4_and_f5_tallies() {
        let f4 = parse_data_card_complete("f4:n 10 20 (30 31 32) t").unwrap();
        assert_eq!(
            f4,
            DataCard::Tally {
                id: 4,
                kind: TallyKind::F4CellFlux,
                particles: vec![Particle::new("n")],
                modifier: None,
                targets: vec![
                    TallyTarget::Single(10),
                    TallyTarget::Single(20),
                    TallyTarget::Group(vec![30, 31, 32]),
                ],
                total_bin: true,
                no_direct: false,
            }
        );

        let f5 = parse_data_card_complete("f5:n,p 0 0 100 0.5  0 50 100 0.5 nd").unwrap();
        assert_eq!(
            f5,
            DataCard::DetectorTally {
                id: 5,
                axis: None,
                particles: vec![Particle::new("n"), Particle::new("p")],
                modifier: None,
                detectors: vec![
                    DetectorEntry::Point {
                        x: 0.0,
                        y: 0.0,
                        z: 100.0,
                        r0: 0.5,
                    },
                    DetectorEntry::Point {
                        x: 0.0,
                        y: 50.0,
                        z: 100.0,
                        r0: 0.5,
                    },
                ],
                no_direct: true,
            }
        );

        let fz15 = parse_data_card_complete("fz15:n 10.0 25.0 0.5").unwrap();
        assert_eq!(
            fz15,
            DataCard::DetectorTally {
                id: 15,
                axis: Some('z'),
                particles: vec![Particle::new("n")],
                modifier: None,
                detectors: vec![DetectorEntry::Ring {
                    axis: 'z',
                    axis_coord: 10.0,
                    radius: 25.0,
                    r0: 0.5,
                }],
                no_direct: false,
            }
        );
    }

    #[test]
    fn test_dose_de_df_and_fm_cards() {
        let de = parse_data_card_complete("de4 log 1.0-3 1.0-1 1.0 10.0").unwrap();
        assert_eq!(
            de,
            DataCard::Dose(DoseCard {
                id: 4,
                is_function: false,
                interpolation: Some(InterpMode::Log),
                values: vec![1e-3, 1e-1, 1.0, 10.0],
                options: Vec::new(),
            })
        );

        let df = parse_data_card_complete("df4 lin 3.67-6 1.0-5 2.5-4 4.0-4").unwrap();
        assert_eq!(
            df,
            DataCard::Dose(DoseCard {
                id: 4,
                is_function: true,
                interpolation: Some(InterpMode::Lin),
                values: vec![3.67e-6, 1.0e-5, 2.5e-4, 4.0e-4],
                options: Vec::new(),
            })
        );

        let df_ic = parse_data_card_complete("df14 ic=99 iu=2 fac=-3").unwrap();
        assert_eq!(
            df_ic,
            DataCard::Dose(DoseCard {
                id: 14,
                is_function: true,
                interpolation: None,
                values: Vec::new(),
                options: vec![
                    ("ic".to_string(), "99".to_string()),
                    ("iu".to_string(), "2".to_string()),
                    ("fac".to_string(), "-3".to_string()),
                ],
            })
        );

        let fm = parse_data_card_complete("fm4 (1.0 1 (102 103) (-6)) 2.5").unwrap();
        assert_eq!(
            fm,
            DataCard::TallyMultiplier(FmCard {
                id: 4,
                modifier: None,
                bins: vec![
                    FmBin::MultiplierSet {
                        constant: 1.0,
                        material_id: 1,
                        reaction_groups: vec!["102 103".to_string(), "-6".to_string()],
                    },
                    FmBin::Constant(2.5),
                ],
            })
        );
    }

    #[test]
    fn test_fmesh_and_fs_sd_fc_cards() {
        let fmesh = parse_data_card_complete(
            "fmesh14:n geom=xyz origin=-50 -50 0 imesh=0 50 iints=10 10 jmesh=50 jints=20 kmesh=100 kints=50 emesh=0.1 1 10 eints=1 1r factor=1.5 out=col",
        )
        .unwrap();
        match fmesh {
            DataCard::Fmesh(m) => {
                assert_eq!(m.id, 14);
                assert_eq!(m.geom.as_deref(), Some("xyz"));
                assert_eq!(m.origin, Some([-50.0, -50.0, 0.0]));
                assert_eq!(m.imesh, vec![0.0, 50.0]);
                assert_eq!(m.iints, vec![10, 10]);
                assert_eq!(m.emesh, vec![0.1, 1.0, 10.0]);
                assert_eq!(m.eints, vec![1, 1]);
                assert_eq!(m.factor, Some(1.5));
                assert_eq!(m.out.as_deref(), Some("col"));
            }
            other => panic!("Expected Fmesh, got {:?}", other),
        }

        let fs = parse_data_card_complete("fs4 -10 -20 t").unwrap();
        assert_eq!(
            fs,
            DataCard::TallySegment {
                id: 4,
                surfaces: vec![-10, -20],
                total_bin: true,
            }
        );

        let sd = parse_data_card_complete("sd4 100.0 2r").unwrap();
        assert_eq!(
            sd,
            DataCard::SegmentDivisor {
                id: 4,
                values: vec![
                    Entry::Value(100.0),
                    Entry::Value(100.0),
                    Entry::Value(100.0)
                ],
            }
        );

        let fc = parse_data_card_complete("fc4 Dose rate behind concrete shield [rem/h]").unwrap();
        assert_eq!(
            fc,
            DataCard::TallyComment {
                id: 4,
                comment: "Dose rate behind concrete shield [rem/h]".to_string(),
            }
        );
    }
}
