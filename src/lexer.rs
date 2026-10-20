use crate::ast::Particle;
use nom::{
    branch::alt,
    bytes::complete::{take_while, take_while1},
    character::complete::{char, digit1, one_of},
    combinator::{map, opt, recognize, verify},
    multi::separated_list1,
    sequence::{pair, preceded, tuple},
    IResult,
};

/// Zero or more whitespace characters (`' '` or `'\t'`).
pub fn ws0(input: &str) -> IResult<&str, &str> {
    take_while(|c: char| c == ' ' || c == '\t')(input)
}

/// One or more whitespace characters (`' '` or `'\t'`).
pub fn ws1(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| c == ' ' || c == '\t')(input)
}

/// Zero or more MCNP token separators (whitespace, `,`, or `=`).
pub fn sep0(input: &str) -> IResult<&str, &str> {
    take_while(|c: char| c == ' ' || c == '\t' || c == ',' || c == '=')(input)
}

/// One or more MCNP token separators (whitespace, `,`, or `=`).
pub fn sep1(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| c == ' ' || c == '\t' || c == ',' || c == '=')(input)
}

/// Checks if the next character in the remaining input is a token boundary.
pub fn is_token_boundary(rem: &str) -> bool {
    match rem.chars().next() {
        None => true,
        Some(c) => {
            c.is_whitespace()
                || c == ','
                || c == '='
                || c == '('
                || c == ')'
                || c == ':'
                || c == '#'
        }
    }
}

/// Parses an unsigned 32-bit integer at a token boundary.
pub fn mcnp_u32(input: &str) -> IResult<&str, u32> {
    let (rem, digits) = digit1(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Digit,
        )));
    }
    let val = digits.parse::<u32>().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
    })?;
    Ok((rem, val))
}

/// Parses an unsigned 64-bit integer at a token boundary.
pub fn mcnp_u64(input: &str) -> IResult<&str, u64> {
    let (rem, digits) = digit1(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Digit,
        )));
    }
    let val = digits.parse::<u64>().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
    })?;
    Ok((rem, val))
}

/// Parses a signed 32-bit integer at a token boundary.
pub fn mcnp_i32(input: &str) -> IResult<&str, i32> {
    let (rem, raw) = recognize(pair(opt(one_of("+-")), digit1))(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Digit,
        )));
    }
    let val = raw.parse::<i32>().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
    })?;
    Ok((rem, val))
}

/// Parses an MCNP floating point number, supporting both standard scientific notation
/// (`1.5e-3`, `-2.0E+4`) and Fortran/MCNP e-less notation (`1.5-3`, `-2.+4`, `.5-2`, `1-4`).
pub fn mcnp_float(input: &str) -> IResult<&str, f64> {
    let (rem, (sign, mantissa, exp)) = tuple((
        opt(one_of("+-")),
        alt((
            // Case 1: digits with optional decimal part: `123.45`, `123.`, `123`
            recognize(tuple((digit1, opt(pair(char('.'), opt(digit1)))))),
            // Case 2: leading decimal point: `.45`
            recognize(pair(char('.'), digit1)),
        )),
        opt(alt((
            // Standard exponent with e/E/d/D: `e-3`, `E+4`, `d2`
            recognize(tuple((one_of("eEdD"), opt(one_of("+-")), digit1))),
            // E-less exponent: `-3`, `+4` directly attached
            recognize(pair(one_of("+-"), digit1)),
        ))),
    ))(input)?;

    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Float,
        )));
    }

    let mut normalized = String::with_capacity(24);
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

    let val = normalized.parse::<f64>().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Float))
    })?;

    Ok((rem, val))
}

/// Parses a particle designator list starting with `:`, e.g. `:n`, `:n,p`, `:n,p,e`.
pub fn particle_designators(input: &str) -> IResult<&str, Vec<Particle>> {
    preceded(
        char(':'),
        separated_list1(
            char(','),
            map(
                verify(
                    take_while1(|c: char| {
                        c.is_ascii_alphanumeric() || c == '/' || c == '|' || c == '#' || c == '?'
                    }),
                    |s: &str| !s.is_empty(),
                ),
                Particle::new,
            ),
        ),
    )(input)
}

/// Parses a generic non-delimiter token.
pub fn raw_token(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| !c.is_whitespace() && c != ',' && c != '=')(input)
}

/// Parses a key-value identifier (e.g., `imp`, `vol`, `*trcl`, `fill`).
pub fn identifier(input: &str) -> IResult<&str, &str> {
    recognize(pair(
        opt(one_of("*+")),
        take_while1(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '/'),
    ))(input)
}

/// Helper for case-insensitive keyword matching at token boundary.
pub fn kw<'a>(expected: &'static str) -> impl FnMut(&'a str) -> IResult<&'a str, &'a str> {
    move |input: &'a str| {
        let (rem, matched) = nom::bytes::complete::tag_no_case(expected)(input)?;
        if !is_token_boundary(rem) {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            )));
        }
        Ok((rem, matched))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcnp_float_standard_and_eless() {
        assert_eq!(mcnp_float("1.5"), Ok(("", 1.5)));
        assert_eq!(mcnp_float("-10.25"), Ok(("", -10.25)));
        assert_eq!(mcnp_float("1.5e-3"), Ok(("", 1.5e-3)));
        assert_eq!(mcnp_float("1.5-3"), Ok(("", 1.5e-3)));
        assert_eq!(mcnp_float("-2.0+4"), Ok(("", -2.0e4)));
        assert_eq!(mcnp_float(".5-2"), Ok(("", 0.005)));
        assert_eq!(mcnp_float("1-4"), Ok(("", 1e-4)));
        assert_eq!(mcnp_float("2.5d-3"), Ok(("", 2.5e-3)));

        // Must reject if followed immediately by non-boundary letters (e.g. "3r" or "1001.80c")
        assert!(mcnp_float("3r").is_err());
        assert!(mcnp_float("1001.80c").is_err());
    }

    #[test]
    fn test_particle_designators() {
        let (rem, parts) = particle_designators(":n,p,e=1.0").unwrap();
        assert_eq!(rem, "=1.0");
        assert_eq!(
            parts,
            vec![Particle::new("n"), Particle::new("p"), Particle::new("e")]
        );
    }
}
