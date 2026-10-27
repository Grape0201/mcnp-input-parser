use crate::ast::{
    DataCard, DsCard, DsOption, Entry, SdefParam, SdefValue, SdefVariable, SiCard, SiEntry,
    SiOption, SpOption, SpSbCard, SpSbContent,
};
use crate::lexer::{is_token_boundary, mcnp_float, mcnp_i32, raw_token, sep0, ws0};
use crate::shortcut::{parse_expanded_entries, shortcut_item, ShortcutItem};
use nom::{
    bytes::complete::{take_until, take_while1},
    character::complete::char,
    sequence::delimited,
    IResult,
};

const KNOWN_SDEF_KEYS: &[&str] = &[
    "cel", "sur", "erg", "tme", "dir", "vec", "nrm", "pos", "rad", "ext", "axs", "x", "y", "z",
    "ccc", "ara", "wgt", "tr", "eff", "par", "dat", "loc", "bem", "bap",
];

fn parse_dist_ref(token: &str) -> Option<u32> {
    let s = token.trim();
    if let Some(rest) = s.strip_prefix('d').or_else(|| s.strip_prefix('D')) {
        rest.parse::<u32>().ok()
    } else {
        None
    }
}

fn parse_func_var(token: &str) -> Option<SdefVariable> {
    let lower = token.to_lowercase();
    if let Some(rest) = lower.strip_prefix('f') {
        if KNOWN_SDEF_KEYS.contains(&rest) {
            return Some(SdefVariable::from_name(rest));
        }
    }
    None
}

fn collect_sdef_raw_tokens(mut input: &str) -> IResult<&str, Vec<String>> {
    let mut tokens = Vec::new();
    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }
        if r.starts_with('(') {
            let (next_r, inner) = delimited(char('('), take_until(")"), char(')'))(r)?;
            // Expand inner tokens directly into sequence
            let (_, mut inner_toks) = collect_sdef_raw_tokens(inner)?;
            tokens.append(&mut inner_toks);
            input = next_r;
        } else {
            let (next_r, tok) = raw_token(r)?;
            tokens.push(tok.to_string());
            input = next_r;
        }
    }
    Ok((input, tokens))
}

fn convert_sdef_value(var: &SdefVariable, tokens: &[String]) -> SdefValue {
    if tokens.is_empty() {
        return SdefValue::Symbol(String::new());
    }

    // 1. Check for dependent function `F<var> D<n>` (or `F<var> <n>`)
    if let Some(indep_var) = parse_func_var(&tokens[0]) {
        if let Some(second) = tokens.get(1) {
            if let Some(dist_id) = parse_dist_ref(second).or_else(|| second.parse::<u32>().ok()) {
                return SdefValue::Function {
                    independent_var: indep_var,
                    dist_id,
                };
            }
        }
    }

    // 2. Check for single distribution reference `Dn`
    if tokens.len() == 1 {
        if let Some(dist_id) = parse_dist_ref(&tokens[0]) {
            return SdefValue::Distribution(dist_id);
        }
    }

    // 3. Try parsing all tokens as shortcut/numeric items
    let joined = tokens.join(" ");
    if let Ok((rem, entries)) = parse_expanded_entries(&joined) {
        if rem.trim().is_empty() && !entries.is_empty() {
            if entries.len() == 1 {
                if let Entry::Value(v) = entries[0] {
                    return SdefValue::Scalar(v);
                }
            }
            if entries.len() == 3 && var.is_vector() {
                if let (Some(x), Some(y), Some(z)) =
                    (entries[0].as_f64(), entries[1].as_f64(), entries[2].as_f64())
                {
                    return SdefValue::Vector([x, y, z]);
                }
            }
            return SdefValue::List(entries);
        }
    }

    // 4. Fallback to Symbol
    SdefValue::Symbol(joined)
}

/// Parses `SDEF` card body into structured [`SdefParam`] list.
pub fn parse_sdef_body(input: &str) -> IResult<&str, DataCard> {
    let (rem, raw_tokens) = collect_sdef_raw_tokens(input)?;

    let mut grouped: Vec<(SdefVariable, Vec<String>)> = Vec::new();
    for tok in raw_tokens {
        let lower = tok.to_lowercase();
        if KNOWN_SDEF_KEYS.contains(&lower.as_str()) {
            grouped.push((SdefVariable::from_name(&lower), Vec::new()));
        } else if let Some((_, ref mut vals)) = grouped.last_mut() {
            vals.push(tok);
        } else {
            grouped.push((SdefVariable::from_name(&lower), Vec::new()));
        }
    }

    let params = grouped
        .into_iter()
        .map(|(variable, toks)| {
            let value = convert_sdef_value(&variable, &toks);
            SdefParam { variable, value }
        })
        .collect();

    Ok((rem, DataCard::Sdef { params }))
}

/// Parses `SI<n>` Source Information card body.
pub fn parse_si_body(id: u32, input: &str) -> IResult<&str, DataCard> {
    let (mut rem, _) = sep0(input)?;

    // Check optional letter: H, L, A, S
    let mut option = SiOption::Histogram;
    if let Ok((next_rem, word)) =
        take_while1::<_, _, nom::error::Error<&str>>(|c: char| c.is_ascii_alphabetic())(rem)
    {
        if is_token_boundary(next_rem) {
            match word.to_uppercase().as_str() {
                "H" => {
                    option = SiOption::Histogram;
                    rem = next_rem;
                }
                "L" => {
                    option = SiOption::Discrete;
                    rem = next_rem;
                }
                "A" => {
                    option = SiOption::ArbitraryPdf;
                    rem = next_rem;
                }
                "S" => {
                    option = SiOption::DistributionList;
                    rem = next_rem;
                }
                _ => {}
            }
        }
    }

    let (rem, entries) = parse_si_entries(option == SiOption::DistributionList, rem)?;

    Ok((
        rem,
        DataCard::Si(SiCard {
            id,
            option,
            entries,
        }),
    ))
}

fn parse_si_entries(is_dist_list: bool, mut input: &str) -> IResult<&str, Vec<SiEntry>> {
    // First, if not a distribution list, try pure numeric + shortcut expansion
    if !is_dist_list {
        if let Ok((rem, expanded)) = parse_expanded_entries(input) {
            if rem.trim().is_empty() {
                let entries = expanded
                    .into_iter()
                    .map(|e| match e {
                        Entry::Value(v) => SiEntry::Value(v),
                        Entry::Jump => SiEntry::Jump,
                    })
                    .collect();
                return Ok((rem, entries));
            }
        }
    }

    // General token-by-token parsing (supporting `D1`, `3r`, numbers, and cell paths `1:2:3`)
    let mut entries: Vec<SiEntry> = Vec::new();
    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }

        // Check repeat shortcut `nR` or `R`
        if let Ok((next_r, ShortcutItem::Repeat(count))) = shortcut_item(r) {
            let last = entries.last().cloned().ok_or_else(|| {
                nom::Err::Error(nom::error::Error::new(r, nom::error::ErrorKind::Verify))
            })?;
            for _ in 0..count {
                entries.push(last.clone());
            }
            input = next_r;
            continue;
        }

        let (next_r, tok) = raw_token(r)?;
        if let Some(d_id) = parse_dist_ref(tok) {
            entries.push(SiEntry::Distribution(d_id));
        } else if is_dist_list {
            if let Ok(d_id) = tok.parse::<u32>() {
                entries.push(SiEntry::Distribution(d_id));
            } else {
                entries.push(SiEntry::Symbol(tok.to_string()));
            }
        } else if tok.eq_ignore_ascii_case("j") {
            entries.push(SiEntry::Jump);
        } else if let Ok( (_, v) ) = mcnp_float(tok) {
            entries.push(SiEntry::Value(v));
        } else {
            entries.push(SiEntry::Symbol(tok.to_string()));
        }
        input = next_r;
    }

    Ok((input, entries))
}

/// Parses `SP<n>` (is_bias = false) or `SB<n>` (is_bias = true) card body.
pub fn parse_sp_sb_body(id: u32, is_bias: bool, input: &str) -> IResult<&str, DataCard> {
    let (mut rem, _) = sep0(input)?;

    // Check for built-in analytic function number (negative integer like -2, -3, -21, -31, -41)
    if rem.starts_with('-') {
        if let Ok((next_rem, func_id)) = mcnp_i32(rem) {
            if func_id < 0 {
                let (final_rem, params) = parse_expanded_entries(next_rem)?;
                let card = SpSbCard {
                    id,
                    content: SpSbContent::AnalyticFunction {
                        function_id: func_id,
                        params,
                    },
                };
                return Ok((
                    final_rem,
                    if is_bias {
                        DataCard::Sb(card)
                    } else {
                        DataCard::Sp(card)
                    },
                ));
            }
        }
    }

    // Check optional letter: D, C, V
    let mut option = SpOption::Default;
    if let Ok((next_rem, word)) =
        take_while1::<_, _, nom::error::Error<&str>>(|c: char| c.is_ascii_alphabetic())(rem)
    {
        if is_token_boundary(next_rem) {
            match word.to_uppercase().as_str() {
                "D" => {
                    option = SpOption::Default;
                    rem = next_rem;
                }
                "C" => {
                    option = SpOption::Cumulative;
                    rem = next_rem;
                }
                "V" => {
                    option = SpOption::Volume;
                    rem = next_rem;
                }
                _ => {}
            }
        }
    }

    let (rem, values) = parse_expanded_entries(rem)?;

    let card = SpSbCard {
        id,
        content: SpSbContent::Probabilities { option, values },
    };

    Ok((
        rem,
        if is_bias {
            DataCard::Sb(card)
        } else {
            DataCard::Sp(card)
        },
    ))
}

/// Parses `DS<n>` Dependent Source card body.
pub fn parse_ds_body(id: u32, input: &str) -> IResult<&str, DataCard> {
    let (mut rem, _) = sep0(input)?;

    let mut option = DsOption::Histogram;
    if let Ok((next_rem, word)) =
        take_while1::<_, _, nom::error::Error<&str>>(|c: char| c.is_ascii_alphabetic())(rem)
    {
        if is_token_boundary(next_rem) {
            match word.to_uppercase().as_str() {
                "H" => {
                    option = DsOption::Histogram;
                    rem = next_rem;
                }
                "L" => {
                    option = DsOption::Discrete;
                    rem = next_rem;
                }
                "S" => {
                    option = DsOption::DistributionList;
                    rem = next_rem;
                }
                "T" => {
                    option = DsOption::ValueDistPairs;
                    rem = next_rem;
                }
                "Q" => {
                    option = DsOption::IntervalDistPairs;
                    rem = next_rem;
                }
                _ => {}
            }
        }
    }

    if matches!(option, DsOption::ValueDistPairs | DsOption::IntervalDistPairs) {
        let mut pairs = Vec::new();
        loop {
            let (r, _) = sep0(rem)?;
            if r.is_empty() {
                rem = r;
                break;
            }
            let (r, val) = mcnp_float(r)?;
            let (r, _) = sep0(r)?;
            let (r, dist_tok) = raw_token(r)?;
            let dist_id = parse_dist_ref(dist_tok)
                .or_else(|| dist_tok.parse::<u32>().ok())
                .ok_or_else(|| {
                    nom::Err::Error(nom::error::Error::new(r, nom::error::ErrorKind::Digit))
                })?;
            pairs.push((val, dist_id));
            rem = r;
        }
        Ok((
            rem,
            DataCard::Ds(DsCard {
                id,
                option,
                entries: Vec::new(),
                pairs,
            }),
        ))
    } else {
        let (rem, entries) = parse_si_entries(option == DsOption::DistributionList, rem)?;
        Ok((
            rem,
            DataCard::Ds(DsCard {
                id,
                option,
                entries,
                pairs: Vec::new(),
            }),
        ))
    }
}

/// Parses `SC<n>` Source Comment card body.
pub fn parse_sc_body(id: u32, input: &str) -> IResult<&str, DataCard> {
    let (rem, _) = ws0(input)?;
    Ok((
        "",
        DataCard::Sc {
            id,
            comment: rem.trim().to_string(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::data::parse_data_card_complete;

    #[test]
    fn test_sdef_rich_parameters() {
        let card = parse_data_card_complete(
            "sdef pos=0 0 -10.5 axs=0 0 1 ext=d1 rad=d2 erg=fpos d3 par=n cel=1:2 wgt=1.0",
        )
        .unwrap();
        assert_eq!(
            card,
            DataCard::Sdef {
                params: vec![
                    SdefParam {
                        variable: SdefVariable::Pos,
                        value: SdefValue::Vector([0.0, 0.0, -10.5]),
                    },
                    SdefParam {
                        variable: SdefVariable::Axs,
                        value: SdefValue::Vector([0.0, 0.0, 1.0]),
                    },
                    SdefParam {
                        variable: SdefVariable::Ext,
                        value: SdefValue::Distribution(1),
                    },
                    SdefParam {
                        variable: SdefVariable::Rad,
                        value: SdefValue::Distribution(2),
                    },
                    SdefParam {
                        variable: SdefVariable::Erg,
                        value: SdefValue::Function {
                            independent_var: SdefVariable::Pos,
                            dist_id: 3,
                        },
                    },
                    SdefParam {
                        variable: SdefVariable::Par,
                        value: SdefValue::Symbol("n".to_string()),
                    },
                    SdefParam {
                        variable: SdefVariable::Cel,
                        value: SdefValue::Symbol("1:2".to_string()),
                    },
                    SdefParam {
                        variable: SdefVariable::Wgt,
                        value: SdefValue::Scalar(1.0),
                    },
                ]
            }
        );
    }

    #[test]
    fn test_si_sp_sb_and_analytic_functions() {
        let si1 = parse_data_card_complete("si1 h 0.0 2i 3.0 10.0").unwrap();
        assert_eq!(
            si1,
            DataCard::Si(SiCard {
                id: 1,
                option: SiOption::Histogram,
                entries: vec![
                    SiEntry::Value(0.0),
                    SiEntry::Value(1.0),
                    SiEntry::Value(2.0),
                    SiEntry::Value(3.0),
                    SiEntry::Value(10.0),
                ],
            })
        );

        let sp1 = parse_data_card_complete("sp1 d 0.0 0.25 2r 0.25").unwrap();
        assert_eq!(
            sp1,
            DataCard::Sp(SpSbCard {
                id: 1,
                content: SpSbContent::Probabilities {
                    option: SpOption::Default,
                    values: vec![
                        Entry::Value(0.0),
                        Entry::Value(0.25),
                        Entry::Value(0.25),
                        Entry::Value(0.25),
                        Entry::Value(0.25),
                    ],
                },
            })
        );

        // Power-law radial distribution `SP2 -21 1`
        let sp2 = parse_data_card_complete("sp2 -21 1").unwrap();
        assert_eq!(
            sp2,
            DataCard::Sp(SpSbCard {
                id: 2,
                content: SpSbContent::AnalyticFunction {
                    function_id: -21,
                    params: vec![Entry::Value(1.0)],
                },
            })
        );

        // Source biasing `SB1`
        let sb1 = parse_data_card_complete("sb1 0.0 1.0 2.0 4.0 8.0").unwrap();
        assert_eq!(
            sb1,
            DataCard::Sb(SpSbCard {
                id: 1,
                content: SpSbContent::Probabilities {
                    option: SpOption::Default,
                    values: vec![
                        Entry::Value(0.0),
                        Entry::Value(1.0),
                        Entry::Value(2.0),
                        Entry::Value(4.0),
                        Entry::Value(8.0),
                    ],
                },
            })
        );

        // Verify JSON serialization and roundtrip
        let sp_json = serde_json::to_value(&sp2).unwrap();
        assert!(sp_json.get("Sp").is_some());
        assert!(sp_json.get("SpSb").is_none());
        assert!(sp_json["Sp"].get("is_bias").is_none());
        let sp_restored: DataCard = serde_json::from_value(sp_json).unwrap();
        assert_eq!(sp_restored, sp2);

        let sb_json = serde_json::to_value(&sb1).unwrap();
        assert!(sb_json.get("Sb").is_some());
        assert!(sb_json.get("SpSb").is_none());
        assert!(sb_json["Sb"].get("is_bias").is_none());
        let sb_restored: DataCard = serde_json::from_value(sb_json).unwrap();
        assert_eq!(sb_restored, sb1);
    }

    #[test]
    fn test_ds_and_sc_cards() {
        let ds_s = parse_data_card_complete("ds3 s d10 d11 12").unwrap();
        assert_eq!(
            ds_s,
            DataCard::Ds(DsCard {
                id: 3,
                option: DsOption::DistributionList,
                entries: vec![
                    SiEntry::Distribution(10),
                    SiEntry::Distribution(11),
                    SiEntry::Distribution(12),
                ],
                pairs: Vec::new(),
            })
        );

        let ds_q = parse_data_card_complete("ds4 q 1.0 10 5.0 d11").unwrap();
        assert_eq!(
            ds_q,
            DataCard::Ds(DsCard {
                id: 4,
                option: DsOption::IntervalDistPairs,
                entries: Vec::new(),
                pairs: vec![(1.0, 10), (5.0, 11)],
            })
        );

        let sc = parse_data_card_complete("sc1 Co-60 gamma ray source (1.17 & 1.33 MeV)").unwrap();
        assert_eq!(
            sc,
            DataCard::Sc {
                id: 1,
                comment: "Co-60 gamma ray source (1.17 & 1.33 MeV)".to_string(),
            }
        );
    }
}
