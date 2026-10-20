use crate::ast::{BoundaryCondition, SurfaceCard, SurfaceKind};
use crate::lexer::{is_token_boundary, mcnp_i32, mcnp_u32, ws0, ws1};
use crate::shortcut::parse_expanded_floats;
use nom::{
    bytes::complete::take_while1,
    character::complete::one_of,
    combinator::{all_consuming, opt, verify},
    IResult,
};

fn parse_surface_mnemonic(input: &str) -> IResult<&str, SurfaceKind> {
    let (rem, raw) = verify(
        take_while1(|c: char| c.is_ascii_alphabetic() || c == '/'),
        |s: &str| s.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false),
    )(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }
    Ok((rem, SurfaceKind::from_mnemonic(raw)))
}

/// Parses a single logical Surface Card.
pub fn parse_surface_card(input: &str) -> IResult<&str, SurfaceCard> {
    let (rem, _) = ws0(input)?;
    let (rem, bc_char) = opt(one_of("*+"))(rem)?;
    let boundary = match bc_char {
        Some('*') => BoundaryCondition::Reflective,
        Some('+') => BoundaryCondition::White,
        _ => BoundaryCondition::Transmissive,
    };

    let (rem, _) = ws0(rem)?;
    let (rem, id) = verify(mcnp_u32, |&id| id > 0)(rem)?;
    let (rem, _) = ws1(rem)?;

    // Optional coordinate transform ID or periodic boundary surface number
    let (rem, transform_id) = opt(mcnp_i32)(rem)?;
    let (rem, _) = if transform_id.is_some() {
        ws1(rem)?
    } else {
        (rem, "")
    };

    let (rem, kind) = parse_surface_mnemonic(rem)?;
    let (rem, params) = parse_expanded_floats(rem)?;
    let (rem, _) = ws0(rem)?;

    Ok((
        rem,
        SurfaceCard {
            id,
            boundary,
            transform_id,
            kind,
            params,
        },
    ))
}

pub fn parse_surface_card_complete(input: &str) -> Result<SurfaceCard, String> {
    match all_consuming(parse_surface_card)(input) {
        Ok((_, card)) => Ok(card),
        Err(e) => Err(format!("Failed to parse surface card `{}`: {}", input, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_and_reflective_surfaces() {
        let s1 = parse_surface_card_complete("*10 cz 1.26").unwrap();
        assert_eq!(s1.id, 10);
        assert_eq!(s1.boundary, BoundaryCondition::Reflective);
        assert_eq!(s1.transform_id, None);
        assert_eq!(s1.kind, SurfaceKind::CZ);
        assert_eq!(s1.params, vec![1.26]);

        let s2 = parse_surface_card_complete("+20 1 c/z 0.0 0.0 5.0").unwrap();
        assert_eq!(s2.id, 20);
        assert_eq!(s2.boundary, BoundaryCondition::White);
        assert_eq!(s2.transform_id, Some(1));
        assert_eq!(s2.kind, SurfaceKind::C_Z);
        assert_eq!(s2.params, vec![0.0, 0.0, 5.0]);
    }

    #[test]
    fn test_macrobody_surface_with_shortcuts() {
        let s = parse_surface_card_complete("100 rpp -5.0 5.0 2r -10 10").unwrap();
        assert_eq!(s.id, 100);
        assert_eq!(s.kind, SurfaceKind::RPP);
        assert_eq!(s.params, vec![-5.0, 5.0, 5.0, 5.0, -10.0, 10.0]);
    }
}
