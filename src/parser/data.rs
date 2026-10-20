use crate::ast::{DataCard, NuclideFraction, Particle};
use crate::lexer::{is_token_boundary, mcnp_float, particle_designators, raw_token, sep0, sep1, ws0};
use crate::parser::source::{
    parse_ds_body, parse_sc_body, parse_sdef_body, parse_si_body, parse_sp_sb_body,
};
use crate::parser::tally::{
    parse_detector_tally_body, parse_dose_body, parse_fc_body, parse_fm_body, parse_fmesh_body,
    parse_fs_body, parse_sd_body, parse_standard_tally_body, parse_tally_cosine_body,
    parse_tally_energy_body, parse_tally_time_body,
};
use crate::shortcut::{parse_expanded_entries, parse_expanded_floats};
use nom::{
    bytes::complete::{take_until, take_while1},
    character::complete::{char, one_of},
    combinator::{all_consuming, opt, verify},
    sequence::{delimited, preceded},
    IResult,
};

#[derive(Debug, Clone)]
struct CardHeader {
    modifier: Option<char>,
    mnemonic: String,
    number: Option<u32>,
    particles: Vec<Particle>,
    raw_name: String,
}

/// Parses the header of a Data card, e.g. `m1`, `*tr2`, `f14:n,p`, `imp:n`, `wwn1:n`, `fmesh4:n`.
fn parse_card_header(input: &str) -> IResult<&str, CardHeader> {
    let (rem, _) = ws0(input)?;
    let (rem, modifier) = opt(one_of("*+"))(rem)?;
    let (rem, alpha_part) = verify(
        take_while1(|c: char| c.is_ascii_alphabetic()),
        |s: &str| !s.is_empty(),
    )(rem)?;
    let (rem, num_part) = opt(take_while1(|c: char| c.is_ascii_digit()))(rem)?;
    let (rem, particles) = opt(particle_designators)(rem)?;

    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }

    let number = match num_part {
        Some(digits) => Some(digits.parse::<u32>().map_err(|_| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
        })?),
        None => None,
    };

    let mut raw_name = String::new();
    if let Some(m) = modifier {
        raw_name.push(m);
    }
    raw_name.push_str(&alpha_part.to_lowercase());
    if let Some(n) = number {
        raw_name.push_str(&n.to_string());
    }

    Ok((
        rem,
        CardHeader {
            modifier,
            mnemonic: alpha_part.to_lowercase(),
            number,
            particles: particles.unwrap_or_default(),
            raw_name,
        },
    ))
}

/// Collects remaining tokens (respecting parenthesized groups if present).
pub(crate) fn collect_all_tokens(mut input: &str) -> IResult<&str, Vec<String>> {
    let mut tokens = Vec::new();
    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }
        if r.starts_with('(') {
            let (next_r, inner) = delimited(char('('), take_until(")"), char(')'))(r)?;
            tokens.push(format!("({})", inner.trim()));
            input = next_r;
        } else {
            let (next_r, tok) = raw_token(r)?;
            tokens.push(tok.to_string());
            input = next_r;
        }
    }
    Ok((input, tokens))
}

/// Parses Material card body (`m<n>`).
fn parse_material_body(id: u32, mut input: &str) -> IResult<&str, DataCard> {
    let mut nuclides = Vec::new();
    let mut options = Vec::new();

    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }

        let first_ch = r.chars().next().unwrap();
        if first_ch.is_ascii_alphabetic() {
            let (r_key, key) = take_while1(|c: char| c.is_ascii_alphanumeric() || c == '_')(r)?;
            let (r_val, _) = sep1(r_key)?;
            let (r_next, val) = raw_token(r_val)?;
            options.push((key.to_lowercase(), val.to_string()));
            input = r_next;
        } else {
            let (r_zaid, zaid) = raw_token(r)?;
            let (r_sep, _) = sep1(r_zaid)?;
            let (r_frac, fraction) = mcnp_float(r_sep)?;
            nuclides.push(NuclideFraction {
                zaid: zaid.to_string(),
                fraction,
            });
            input = r_frac;
        }
    }

    Ok((
        input,
        DataCard::Material {
            id,
            nuclides,
            options,
        },
    ))
}

/// Parses `WWG` Weight Window Generator card body (`wwg itally icell wlower ...`).
fn parse_wwg_body(input: &str) -> IResult<&str, DataCard> {
    let (rem, entries) = parse_expanded_entries(input)?;
    if entries.len() < 3 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Verify,
        )));
    }
    let tally_id = entries[0].as_f64().unwrap_or(0.0).round() as u32;
    let reference_cell = entries[1].as_f64().unwrap_or(0.0).round() as u32;
    let target_lower_bound = entries[2].as_f64().unwrap_or(0.0);
    let extra = entries.into_iter().skip(3).collect();

    Ok((
        rem,
        DataCard::WeightWindowGenerator {
            tally_id,
            reference_cell,
            target_lower_bound,
            extra,
        },
    ))
}

/// Parses `MESH` Weight Window superimposed mesh card body.
fn parse_ww_mesh_body(input: &str) -> IResult<&str, DataCard> {
    let (rem, raw_tokens) = collect_all_tokens(input)?;
    let known_mesh_keys: &[&str] = &[
        "geom", "ref", "origin", "axs", "vec", "imesh", "iints", "jmesh", "jints", "kmesh", "kints",
    ];
    let mut params: Vec<(String, Vec<String>)> = Vec::new();
    for tok in raw_tokens {
        let lower = tok.to_lowercase();
        if known_mesh_keys.contains(&lower.as_str()) {
            params.push((lower, Vec::new()));
        } else if let Some((_, ref mut vals)) = params.last_mut() {
            vals.push(tok);
        } else {
            params.push((lower, Vec::new()));
        }
    }
    Ok((rem, DataCard::WeightWindowMesh { params }))
}

/// Parses `CUT:<p>` card body (`cut:n tco elpt wc1 wc2 swtm`).
fn parse_cut_body(particles: Vec<Particle>, input: &str) -> IResult<&str, DataCard> {
    let (rem, entries) = parse_expanded_entries(input)?;
    let get_f64 = |idx: usize| -> Option<f64> { entries.get(idx).and_then(|e| e.as_f64()) };
    Ok((
        rem,
        DataCard::Cutoff {
            particles,
            tco: get_f64(0),
            elpt: get_f64(1),
            wc1: get_f64(2),
            wc2: get_f64(3),
            swtm: get_f64(4),
        },
    ))
}

/// Parses `kcode` card body.
fn parse_kcode_body(input: &str) -> IResult<&str, DataCard> {
    let (rem, entries) = parse_expanded_entries(input)?;

    let get_u32 = |idx: usize| -> Option<u32> {
        entries
            .get(idx)
            .and_then(|e| e.as_f64())
            .map(|v| v.round() as u32)
    };
    let get_f64 = |idx: usize| -> Option<f64> { entries.get(idx).and_then(|e| e.as_f64()) };

    Ok((
        rem,
        DataCard::Kcode {
            nsrck: get_u32(0),
            rkk: get_f64(1),
            ikz: get_u32(2),
            kct: get_u32(3),
            msrk: get_u32(4),
            knrm: get_u32(5),
            mrkp: get_u32(6),
            kc8: get_u32(7),
        },
    ))
}

/// Parses `ksrc` card body (triplets of x, y, z coordinates).
fn parse_ksrc_body(input: &str) -> IResult<&str, DataCard> {
    let (rem, coords) = parse_expanded_floats(input)?;
    if coords.is_empty() || coords.len() % 3 != 0 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Verify,
        )));
    }
    let points = coords
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    Ok((rem, DataCard::Ksrc { points }))
}

/// Parses `nps` card body.
fn parse_nps_body(input: &str) -> IResult<&str, DataCard> {
    let (rem, floats) = parse_expanded_floats(input)?;
    if floats.is_empty() {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Digit,
        )));
    }
    let count = floats[0].round() as u64;
    let mgopt = floats.get(1).map(|v| v.round() as u64);
    Ok((rem, DataCard::Nps { count, mgopt }))
}

/// Parses a single logical Data Card.
pub fn parse_data_card(input: &str) -> IResult<&str, DataCard> {
    let (rem, header) = parse_card_header(input)?;

    match (header.mnemonic.as_str(), header.number) {
        // --- Materials & Mode ---
        ("m", Some(id)) if header.modifier.is_none() && header.particles.is_empty() => {
            parse_material_body(id, rem)
        }
        ("mt", Some(id)) if header.modifier.is_none() && header.particles.is_empty() => {
            let (rem, tables) = collect_all_tokens(rem)?;
            Ok((rem, DataCard::ThermalScattering { id, tables }))
        }
        ("mode", None) if header.modifier.is_none() => {
            let (rem, tokens) = collect_all_tokens(rem)?;
            let particles = tokens.into_iter().map(Particle::new).collect();
            Ok((rem, DataCard::Mode { particles }))
        }

        // --- Fixed Source & Distributions (SDEF, SI, SP, SB, DS, SC) ---
        ("sdef", None) if header.modifier.is_none() => parse_sdef_body(rem),
        ("si", Some(id)) if header.modifier.is_none() => parse_si_body(id, rem),
        ("sp", Some(id)) if header.modifier.is_none() => parse_sp_sb_body(id, false, rem),
        ("sb", Some(id)) if header.modifier.is_none() => parse_sp_sb_body(id, true, rem),
        ("ds", Some(id)) if header.modifier.is_none() => parse_ds_body(id, rem),
        ("sc", Some(id)) if header.modifier.is_none() => parse_sc_body(id, rem),

        // --- Tallies (F, FX/FY/FZ, DE, DF, FM, FS, SD, E, T, C, FC, FMESH) ---
        ("f", Some(id)) if id % 10 == 5 => {
            parse_detector_tally_body(id, None, header.particles, header.modifier, rem)
        }
        ("fx", Some(id)) if id % 10 == 5 => {
            parse_detector_tally_body(id, Some('x'), header.particles, header.modifier, rem)
        }
        ("fy", Some(id)) if id % 10 == 5 => {
            parse_detector_tally_body(id, Some('y'), header.particles, header.modifier, rem)
        }
        ("fz", Some(id)) if id % 10 == 5 => {
            parse_detector_tally_body(id, Some('z'), header.particles, header.modifier, rem)
        }
        ("f", Some(id)) => parse_standard_tally_body(id, header.particles, header.modifier, rem),
        ("de", maybe_id) if header.modifier.is_none() => {
            parse_dose_body(maybe_id.unwrap_or(0), false, rem)
        }
        ("df", maybe_id) if header.modifier.is_none() => {
            parse_dose_body(maybe_id.unwrap_or(0), true, rem)
        }
        ("fm", Some(id)) => parse_fm_body(id, header.modifier, rem),
        ("fs", Some(id)) if header.modifier.is_none() => parse_fs_body(id, rem),
        ("sd", Some(id)) if header.modifier.is_none() => parse_sd_body(id, rem),
        ("e", maybe_id) => parse_tally_energy_body(maybe_id.unwrap_or(0), header.modifier, rem),
        ("t", Some(id)) if header.modifier.is_none() => parse_tally_time_body(id, rem),
        ("c", Some(id)) if header.modifier.is_none() => parse_tally_cosine_body(id, rem),
        ("fc", Some(id)) if header.modifier.is_none() => parse_fc_body(id, rem),
        ("fmesh", Some(id)) if header.modifier.is_none() => {
            parse_fmesh_body(id, header.particles, rem)
        }

        // --- Variance Reduction (IMP, WWE, WWN, WWP, WWG, WWGE, MESH) ---
        ("imp", None) if header.modifier.is_none() => {
            let (rem, values) = parse_expanded_entries(rem)?;
            Ok((
                rem,
                DataCard::Importance {
                    particles: header.particles,
                    values,
                },
            ))
        }
        ("wwe", None) if header.modifier.is_none() => {
            let (rem, bounds) = parse_expanded_entries(rem)?;
            Ok((
                rem,
                DataCard::WeightWindowEnergy {
                    particles: header.particles,
                    bounds,
                },
            ))
        }
        ("wwn", Some(group_id)) if header.modifier.is_none() => {
            let (rem, lower_bounds) = parse_expanded_entries(rem)?;
            Ok((
                rem,
                DataCard::WeightWindowBounds {
                    group_id,
                    particles: header.particles,
                    lower_bounds,
                },
            ))
        }
        ("wwp", None) if header.modifier.is_none() => {
            let (rem, params) = parse_expanded_entries(rem)?;
            Ok((
                rem,
                DataCard::WeightWindowParams {
                    particles: header.particles,
                    params,
                },
            ))
        }
        ("wwg", None) if header.modifier.is_none() => parse_wwg_body(rem),
        ("wwge", None) if header.modifier.is_none() => {
            let (rem, bounds) = parse_expanded_entries(rem)?;
            Ok((
                rem,
                DataCard::WeightWindowGeneratorEnergy {
                    particles: header.particles,
                    bounds,
                },
            ))
        }
        ("mesh", None) if header.modifier.is_none() => parse_ww_mesh_body(rem),

        // --- Physics, Cutoffs, Transformations & Control ---
        ("phys", None) if header.modifier.is_none() => {
            let (rem, values) = parse_expanded_entries(rem)?;
            Ok((
                rem,
                DataCard::Physics {
                    particles: header.particles,
                    values,
                },
            ))
        }
        ("cut", None) if header.modifier.is_none() => parse_cut_body(header.particles, rem),
        ("tr", Some(id)) => {
            let (rem, values) = parse_expanded_entries(rem)?;
            Ok((
                rem,
                DataCard::Transform {
                    id,
                    is_degrees: header.modifier == Some('*'),
                    values,
                },
            ))
        }
        ("nps", None) if header.modifier.is_none() => parse_nps_body(rem),

        // --- Criticality ---
        ("kcode", None) if header.modifier.is_none() => parse_kcode_body(rem),
        ("ksrc", None) if header.modifier.is_none() => parse_ksrc_body(rem),

        // --- Generic Fallback ---
        _ => {
            let (rem, tokens) = collect_all_tokens(rem)?;
            Ok((
                rem,
                DataCard::Generic {
                    name: header.raw_name,
                    particles: header.particles,
                    tokens,
                },
            ))
        }
    }
}

pub fn parse_data_card_complete(input: &str) -> Result<DataCard, String> {
    match all_consuming(preceded(ws0, parse_data_card))(input.trim_end()) {
        Ok((_, card)) => Ok(card),
        Err(e) => Err(format!("Failed to parse data card `{}`: {}", input, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Entry;

    #[test]
    fn test_material_and_mt_card() {
        let m = parse_data_card_complete("m1 1001.80c 2.0 8016.80c 1.0-0 gas=0 nlib=80c").unwrap();
        assert_eq!(
            m,
            DataCard::Material {
                id: 1,
                nuclides: vec![
                    NuclideFraction {
                        zaid: "1001.80c".to_string(),
                        fraction: 2.0,
                    },
                    NuclideFraction {
                        zaid: "8016.80c".to_string(),
                        fraction: 1.0,
                    },
                ],
                options: vec![
                    ("gas".to_string(), "0".to_string()),
                    ("nlib".to_string(), "80c".to_string()),
                ],
            }
        );

        let mt = parse_data_card_complete("mt1 lwtr.10t").unwrap();
        assert_eq!(
            mt,
            DataCard::ThermalScattering {
                id: 1,
                tables: vec!["lwtr.10t".to_string()],
            }
        );
    }

    #[test]
    fn test_weight_window_and_physics_cut_cards() {
        let wwe = parse_data_card_complete("wwe:n 1.0-3 1.0 20.0").unwrap();
        assert_eq!(
            wwe,
            DataCard::WeightWindowEnergy {
                particles: vec![Particle::new("n")],
                bounds: vec![Entry::Value(1e-3), Entry::Value(1.0), Entry::Value(20.0)],
            }
        );

        let wwn = parse_data_card_complete("wwn1:n 1.0 0.5 2r -1").unwrap();
        assert_eq!(
            wwn,
            DataCard::WeightWindowBounds {
                group_id: 1,
                particles: vec![Particle::new("n")],
                lower_bounds: vec![
                    Entry::Value(1.0),
                    Entry::Value(0.5),
                    Entry::Value(0.5),
                    Entry::Value(0.5),
                    Entry::Value(-1.0),
                ],
            }
        );

        let wwp = parse_data_card_complete("wwp:n 5 3 5 0 -1").unwrap();
        assert_eq!(
            wwp,
            DataCard::WeightWindowParams {
                particles: vec![Particle::new("n")],
                params: vec![
                    Entry::Value(5.0),
                    Entry::Value(3.0),
                    Entry::Value(5.0),
                    Entry::Value(0.0),
                    Entry::Value(-1.0),
                ],
            }
        );

        let wwg = parse_data_card_complete("wwg 4 1 1.0").unwrap();
        assert_eq!(
            wwg,
            DataCard::WeightWindowGenerator {
                tally_id: 4,
                reference_cell: 1,
                target_lower_bound: 1.0,
                extra: Vec::new(),
            }
        );

        let cut = parse_data_card_complete("cut:n 1e8 1.0-9 -0.5 -0.25").unwrap();
        assert_eq!(
            cut,
            DataCard::Cutoff {
                particles: vec![Particle::new("n")],
                tco: Some(1e8),
                elpt: Some(1e-9),
                wc1: Some(-0.5),
                wc2: Some(-0.25),
                swtm: None,
            }
        );

        let phys = parse_data_card_complete("phys:n 20.0 0.0 j").unwrap();
        assert_eq!(
            phys,
            DataCard::Physics {
                particles: vec![Particle::new("n")],
                values: vec![Entry::Value(20.0), Entry::Value(0.0), Entry::Jump],
            }
        );
    }

    #[test]
    fn test_kcode_ksrc_and_imp() {
        let kc = parse_data_card_complete("kcode 1000 1.0 50 250 2j 5000").unwrap();
        assert_eq!(
            kc,
            DataCard::Kcode {
                nsrck: Some(1000),
                rkk: Some(1.0),
                ikz: Some(50),
                kct: Some(250),
                msrk: None,
                knrm: None,
                mrkp: Some(5000),
                kc8: None,
            }
        );

        let ks = parse_data_card_complete("ksrc 0.0 0.0 0.0 1.26 0.0 0.0").unwrap();
        assert_eq!(
            ks,
            DataCard::Ksrc {
                points: vec![[0.0, 0.0, 0.0], [1.26, 0.0, 0.0]],
            }
        );

        let imp = parse_data_card_complete("imp:n,p 1.0 2r 0").unwrap();
        assert_eq!(
            imp,
            DataCard::Importance {
                particles: vec![Particle::new("n"), Particle::new("p")],
                values: vec![
                    Entry::Value(1.0),
                    Entry::Value(1.0),
                    Entry::Value(1.0),
                    Entry::Value(0.0),
                ],
            }
        );
    }
}
