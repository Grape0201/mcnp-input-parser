use crate::ast::Entry;
use crate::lexer::{is_token_boundary, mcnp_float, sep0, sep1};
use nom::{
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{digit1, one_of},
    combinator::{map, opt, recognize, verify},
    multi::many0,
    sequence::{pair, preceded, tuple},
    IResult,
};

/// Raw shortcut token before expansion.
#[derive(Debug, Clone, PartialEq)]
pub enum ShortcutItem {
    /// Concrete numeric value.
    Value(f64),
    /// Repeat previous value `n` times (`nR` or `R` -> `n = 1`).
    Repeat(usize),
    /// Linearly interpolate `n` points between previous and next value (`nI` or `I` -> `n = 1`).
    InterpolateLinear(usize),
    /// Logarithmically interpolate `n` points between previous and next value (`nILOG` or `ILOG`).
    InterpolateLog(usize),
    /// Multiply previous value by factor (`xM`).
    Multiply(f64),
    /// Jump over `n` default entries (`nJ` or `J` -> `n = 1`).
    Jump(usize),
}

fn parse_optional_count(input: &str) -> IResult<&str, usize> {
    let (rem, digits) = opt(digit1)(input)?;
    let count = match digits {
        Some(d) => d.parse::<usize>().map_err(|_| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
        })?,
        None => 1,
    };
    Ok((rem, count))
}

fn parse_repeat(input: &str) -> IResult<&str, ShortcutItem> {
    let (rem, (count, _)) = verify(
        pair(parse_optional_count, tag_no_case("r")),
        |&(c, _)| c > 0,
    )(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }
    Ok((rem, ShortcutItem::Repeat(count)))
}

fn parse_interpolate_log(input: &str) -> IResult<&str, ShortcutItem> {
    let (rem, (count, _)) = verify(
        pair(parse_optional_count, tag_no_case("ilog")),
        |&(c, _)| c > 0,
    )(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }
    Ok((rem, ShortcutItem::InterpolateLog(count)))
}

fn parse_interpolate_linear(input: &str) -> IResult<&str, ShortcutItem> {
    let (rem, (count, _)) = verify(
        pair(parse_optional_count, tag_no_case("i")),
        |&(c, _)| c > 0,
    )(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }
    Ok((rem, ShortcutItem::InterpolateLinear(count)))
}

fn parse_jump(input: &str) -> IResult<&str, ShortcutItem> {
    let (rem, (count, _)) = verify(
        pair(parse_optional_count, tag_no_case("j")),
        |&(c, _)| c > 0,
    )(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }
    Ok((rem, ShortcutItem::Jump(count)))
}

fn parse_multiply(input: &str) -> IResult<&str, ShortcutItem> {
    let (rem, (sign, mantissa, exp, _)) = tuple((
        opt(one_of("+-")),
        alt((
            recognize(tuple((
                digit1,
                opt(pair(nom::character::complete::char('.'), opt(digit1))),
            ))),
            recognize(pair(nom::character::complete::char('.'), digit1)),
        )),
        opt(alt((
            recognize(tuple((one_of("eEdD"), opt(one_of("+-")), digit1))),
            recognize(pair(one_of("+-"), digit1)),
        ))),
        tag_no_case("m"),
    ))(input)?;

    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Float,
        )));
    }

    let mut normalized = String::new();
    if let Some(s) = sign {
        normalized.push(s);
    }
    normalized.push_str(mantissa);
    if let Some(exp_str) = exp {
        let first = exp_str.chars().next().unwrap();
        if first == '+' || first == '-' {
            normalized.push('e');
            normalized.push_str(exp_str);
        } else if first == 'd' || first == 'D' {
            normalized.push('e');
            normalized.push_str(&exp_str[1..]);
        } else {
            normalized.push_str(exp_str);
        }
    }
    let factor = normalized.parse::<f64>().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Float))
    })?;

    Ok((rem, ShortcutItem::Multiply(factor)))
}

/// Parses a single `ShortcutItem` (either a shortcut token or a number).
pub fn shortcut_item(input: &str) -> IResult<&str, ShortcutItem> {
    alt((
        parse_interpolate_log,
        parse_repeat,
        parse_interpolate_linear,
        parse_jump,
        parse_multiply,
        map(mcnp_float, ShortcutItem::Value),
    ))(input)
}

/// Expands a sequence of `ShortcutItem`s into concrete `Entry` values.
pub fn expand_shortcut_items(items: &[ShortcutItem]) -> Result<Vec<Entry>, String> {
    let mut result: Vec<Entry> = Vec::new();
    let mut i = 0;

    while i < items.len() {
        match items[i] {
            ShortcutItem::Value(v) => {
                result.push(Entry::Value(v));
                i += 1;
            }
            ShortcutItem::Jump(count) => {
                for _ in 0..count {
                    result.push(Entry::Jump);
                }
                i += 1;
            }
            ShortcutItem::Repeat(count) => {
                let prev = result
                    .last()
                    .cloned()
                    .ok_or_else(|| "Repeat shortcut (R) used without a preceding value".to_string())?;
                for _ in 0..count {
                    result.push(prev.clone());
                }
                i += 1;
            }
            ShortcutItem::Multiply(factor) => {
                let prev_val = match result.last() {
                    Some(Entry::Value(v)) => *v,
                    _ => {
                        return Err(
                            "Multiply shortcut (M) requires a preceding numeric value".to_string()
                        )
                    }
                };
                result.push(Entry::Value(prev_val * factor));
                i += 1;
            }
            ShortcutItem::InterpolateLinear(count) => {
                let start_val = match result.last() {
                    Some(Entry::Value(v)) => *v,
                    _ => {
                        return Err(
                            "Linear interpolation (I) requires a preceding numeric value"
                                .to_string(),
                        )
                    }
                };
                let end_val = match items.get(i + 1) {
                    Some(ShortcutItem::Value(v)) => *v,
                    _ => {
                        return Err(
                            "Linear interpolation (I) requires a following numeric value"
                                .to_string(),
                        )
                    }
                };
                let step = (end_val - start_val) / ((count + 1) as f64);
                for k in 1..=count {
                    result.push(Entry::Value(start_val + step * (k as f64)));
                }
                result.push(Entry::Value(end_val));
                i += 2; // Consumed both `nI` and the following `end_val`
            }
            ShortcutItem::InterpolateLog(count) => {
                let start_val = match result.last() {
                    Some(Entry::Value(v)) if *v > 0.0 => *v,
                    _ => {
                        return Err(
                            "Log interpolation (ILOG) requires a positive preceding numeric value"
                                .to_string(),
                        )
                    }
                };
                let end_val = match items.get(i + 1) {
                    Some(ShortcutItem::Value(v)) if *v > 0.0 => *v,
                    _ => {
                        return Err(
                            "Log interpolation (ILOG) requires a positive following numeric value"
                                .to_string(),
                        )
                    }
                };
                let ratio = (end_val / start_val).powf(1.0 / ((count + 1) as f64));
                for k in 1..=count {
                    result.push(Entry::Value(start_val * ratio.powi(k as i32)));
                }
                result.push(Entry::Value(end_val));
                i += 2;
            }
        }
    }

    Ok(result)
}

/// Parses zero or more numeric/shortcut items and expands them into `Vec<Entry>`.
pub fn parse_expanded_entries(input: &str) -> IResult<&str, Vec<Entry>> {
    let (rem, first_opt) = preceded(sep0, opt(shortcut_item))(input)?;
    let Some(first) = first_opt else {
        return Ok((rem, Vec::new()));
    };
    let (rem, mut rest) = many0(preceded(sep1, shortcut_item))(rem)?;
    let mut items = Vec::with_capacity(1 + rest.len());
    items.push(first);
    items.append(&mut rest);

    let expanded = expand_shortcut_items(&items).map_err(|_| {
        nom::Err::Failure(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
    })?;
    Ok((rem, expanded))
}

/// Parses numeric/shortcut items and expands them into `Vec<f64>` (fails if `J` jump is present).
pub fn parse_expanded_floats(input: &str) -> IResult<&str, Vec<f64>> {
    let (rem, entries) = parse_expanded_entries(input)?;
    let mut floats = Vec::with_capacity(entries.len());
    for entry in entries {
        match entry {
            Entry::Value(v) => floats.push(v),
            Entry::Jump => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Verify,
                )))
            }
        }
    }
    Ok((rem, floats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repeat_and_multiply() {
        let (rem, entries) = parse_expanded_entries("1.5 3r 2.0m").unwrap();
        assert_eq!(rem, "");
        assert_eq!(
            entries,
            vec![
                Entry::Value(1.5),
                Entry::Value(1.5),
                Entry::Value(1.5),
                Entry::Value(1.5),
                Entry::Value(3.0),
            ]
        );
    }

    #[test]
    fn test_linear_interpolation_and_jump() {
        let (rem, entries) = parse_expanded_entries("0.0 3I 4.0 2J 10").unwrap();
        assert_eq!(rem, "");
        assert_eq!(
            entries,
            vec![
                Entry::Value(0.0),
                Entry::Value(1.0),
                Entry::Value(2.0),
                Entry::Value(3.0),
                Entry::Value(4.0),
                Entry::Jump,
                Entry::Jump,
                Entry::Value(10.0),
            ]
        );
    }

    #[test]
    fn test_log_interpolation() {
        let (rem, floats) = parse_expanded_floats("1.0 2ILOG 1000.0").unwrap();
        assert_eq!(rem, "");
        assert_eq!(floats.len(), 4);
        assert!((floats[0] - 1.0).abs() < 1e-9);
        assert!((floats[1] - 10.0).abs() < 1e-9);
        assert!((floats[2] - 100.0).abs() < 1e-9);
        assert!((floats[3] - 1000.0).abs() < 1e-9);
    }
}
