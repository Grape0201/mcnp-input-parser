use crate::ast::{CellCard, CellDefinition, CellParam, Entry, GeometryExpr};
use crate::lexer::{
    is_token_boundary, kw, mcnp_float, mcnp_i32, mcnp_u32, particle_designators, sep0, ws0, ws1,
};
use crate::shortcut::{expand_shortcut_items, shortcut_item, ShortcutItem};
use nom::{
    branch::alt,
    bytes::complete::{take_until, take_while1},
    character::complete::{char, digit1, one_of},
    combinator::{all_consuming, opt, recognize, verify},
    multi::{many0, many1, separated_list1},
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};

// ============================================================================
// Geometry Boolean Expression Parser
// ============================================================================

/// Parses a signed macrobody facet like `-10.1` or `+5.6`.
fn parse_facet(input: &str) -> IResult<&str, GeometryExpr> {
    let (rem, (surf_raw, _, facet_raw)) = tuple((
        recognize(pair(opt(one_of("+-")), digit1)),
        char('.'),
        digit1,
    ))(input)?;
    if !is_token_boundary(rem) {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Digit,
        )));
    }
    let surface = surf_raw.parse::<i32>().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
    })?;
    let facet = facet_raw.parse::<u32>().map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Digit))
    })?;
    if surface == 0 || facet == 0 {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Verify,
        )));
    }
    Ok((rem, GeometryExpr::Facet { surface, facet }))
}

/// Parses a signed surface number like `-10`, `+20`, `30`.
fn parse_surface_literal(input: &str) -> IResult<&str, GeometryExpr> {
    let (rem, val) = verify(mcnp_i32, |&v| v != 0)(input)?;
    Ok((rem, GeometryExpr::Surface(val)))
}

/// Parses a complement expression: `#20` (cell complement) or `#( ... )` (sub-expression complement).
fn parse_complement(input: &str) -> IResult<&str, GeometryExpr> {
    let (rem, _) = char('#')(input)?;
    let (rem, _) = ws0(rem)?;
    if rem.starts_with('(') {
        let (rem, inner) = delimited(
            pair(char('('), ws0),
            parse_union_expr,
            pair(ws0, char(')')),
        )(rem)?;
        Ok((rem, GeometryExpr::Complement(Box::new(inner))))
    } else {
        let (rem, cell_id) = verify(mcnp_u32, |&id| id > 0)(rem)?;
        Ok((rem, GeometryExpr::ComplementCell(cell_id)))
    }
}

/// Parses a primary geometry atom: parenthesized group, complement, facet, or surface.
fn parse_primary_expr(input: &str) -> IResult<&str, GeometryExpr> {
    alt((
        delimited(
            pair(char('('), ws0),
            parse_union_expr,
            pair(ws0, char(')')),
        ),
        parse_complement,
        parse_facet,
        parse_surface_literal,
    ))(input)
}

/// Parses an intersection sequence of one or more primary geometry atoms separated by optional spaces.
/// Note: in MCNP, `(1 -2)(3 -4)` or `-1 #2` without explicit operator is an intersection.
fn parse_intersection_expr(input: &str) -> IResult<&str, GeometryExpr> {
    let (mut rem, first) = parse_primary_expr(input)?;
    let mut terms = vec![first];

    loop {
        let (r_ws, _) = ws0(rem)?;
        // Stop if we hit `:`, `)`, end of input, or start of a cell parameter keyword
        if r_ws.is_empty() || r_ws.starts_with(':') || r_ws.starts_with(')') {
            rem = r_ws;
            break;
        }
        match parse_primary_expr(r_ws) {
            Ok((next_rem, expr)) => {
                terms.push(expr);
                rem = next_rem;
            }
            Err(_) => {
                break;
            }
        }
    }

    if terms.len() == 1 {
        Ok((rem, terms.pop().unwrap()))
    } else {
        Ok((rem, GeometryExpr::Intersection(terms)))
    }
}

/// Parses a union expression (`A : B : C`) where `:` has lower precedence than intersection.
pub fn parse_union_expr(input: &str) -> IResult<&str, GeometryExpr> {
    let (rem, mut terms) = separated_list1(
        delimited(ws0, char(':'), ws0),
        parse_intersection_expr,
    )(input)?;

    if terms.len() == 1 {
        Ok((rem, terms.pop().unwrap()))
    } else {
        Ok((rem, GeometryExpr::Union(terms)))
    }
}

// ============================================================================
// Cell Parameters Parser
// ============================================================================

/// Checks if the current input starts with a cell parameter keyword (e.g. `imp:n`, `u=`, `*trcl`, etc.).
fn starts_with_param_keyword(input: &str) -> bool {
    let s = input.trim_start();
    let Some(first) = s.chars().next() else {
        return false;
    };
    first.is_ascii_alphabetic() || first == '*'
}

fn parse_param_header(input: &str) -> IResult<&str, (bool, String, Vec<crate::ast::Particle>)> {
    let (rem, star) = opt(char('*'))(input)?;
    let (rem, key_raw) = verify(
        take_while1(|c: char| c.is_ascii_alphanumeric() || c == '_'),
        |s: &str| s.chars().next().map(|c| c.is_ascii_alphabetic()).unwrap_or(false),
    )(rem)?;
    let (rem, particles) = opt(particle_designators)(rem)?;
    Ok((
        rem,
        (
            star.is_some(),
            key_raw.to_lowercase(),
            particles.unwrap_or_default(),
        ),
    ))
}

/// Collects shortcut items belonging to the current parameter value until the next parameter keyword or end.
fn collect_param_entries(mut input: &str) -> IResult<&str, Vec<Entry>> {
    let mut items: Vec<ShortcutItem> = Vec::new();
    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }
        // Check if a new parameter header starts here (and is not a valid shortcut like `3r` or `2.5m`)
        if starts_with_param_keyword(r) && shortcut_item(r).is_err() {
            input = r;
            break;
        }
        match shortcut_item(r) {
            Ok((next_r, item)) => {
                items.push(item);
                input = next_r;
            }
            Err(_) => break,
        }
    }
    let expanded = expand_shortcut_items(&items).map_err(|_| {
        nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Verify))
    })?;
    Ok((input, expanded))
}

/// Collects raw tokens for generic or complex parameters (such as `FILL`) until the next parameter keyword.
fn collect_param_tokens(mut input: &str) -> IResult<&str, Vec<String>> {
    let mut tokens = Vec::new();
    loop {
        let (r, _) = sep0(input)?;
        if r.is_empty() {
            input = r;
            break;
        }
        if r.starts_with('(') {
            let (next_r, group) = delimited(char('('), take_until(")"), char(')'))(r)?;
            tokens.push(format!("({})", group.trim()));
            input = next_r;
            continue;
        }
        // If we already have at least one token, stop when we see a new parameter header
        if !tokens.is_empty() && starts_with_param_keyword(r) && shortcut_item(r).is_err() {
            input = r;
            break;
        }
        let (next_r, tok) = take_while1(|c: char| !c.is_whitespace() && c != ',' && c != '=')(r)?;
        tokens.push(tok.to_string());
        input = next_r;
    }
    Ok((input, tokens))
}

fn parse_single_cell_param(input: &str) -> IResult<&str, CellParam> {
    let (rem, (has_star, key, particles)) = preceded(sep0, parse_param_header)(input)?;
    let (rem, _) = sep0(rem)?;

    match key.as_str() {
        "imp" => {
            let (rem, val) = mcnp_float(rem)?;
            Ok((
                rem,
                CellParam::Importance {
                    particles,
                    value: val,
                },
            ))
        }
        "vol" => {
            let (rem, val) = mcnp_float(rem)?;
            Ok((rem, CellParam::Volume(val)))
        }
        "u" => {
            let (rem, val) = mcnp_i32(rem)?;
            Ok((rem, CellParam::Universe(val)))
        }
        "lat" => {
            let (rem, val) = mcnp_u32(rem)?;
            Ok((rem, CellParam::Lattice(val)))
        }
        "rho" => {
            let (rem, val) = mcnp_float(rem)?;
            Ok((rem, CellParam::Density(val)))
        }
        "mat" => {
            let (rem, val) = mcnp_u32(rem)?;
            Ok((rem, CellParam::Material(val)))
        }
        "tmp" => {
            let (rem, entries) = collect_param_entries(rem)?;
            Ok((rem, CellParam::Temperature(entries)))
        }
        "trcl" => {
            if rem.starts_with('(') {
                let (rem, inner) = delimited(char('('), take_until(")"), char(')'))(rem)?;
                let (_, entries) = collect_param_entries(inner)?;
                Ok((
                    rem,
                    CellParam::Transformation {
                        is_degrees: has_star,
                        values: entries,
                    },
                ))
            } else {
                let (rem, entries) = collect_param_entries(rem)?;
                Ok((
                    rem,
                    CellParam::Transformation {
                        is_degrees: has_star,
                        values: entries,
                    },
                ))
            }
        }
        "fill" => {
            let (rem, values) = collect_param_tokens(rem)?;
            Ok((
                rem,
                CellParam::Fill {
                    transform_is_degrees: has_star,
                    values,
                },
            ))
        }
        _ => {
            let full_key = if has_star {
                format!("*{}", key)
            } else {
                key
            };
            let (rem, values) = collect_param_tokens(rem)?;
            Ok((
                rem,
                CellParam::Other {
                    key: full_key,
                    particles,
                    values,
                },
            ))
        }
    }
}

/// Parses a complete logical Cell Card.
pub fn parse_cell_card(input: &str) -> IResult<&str, CellCard> {
    let (rem, _) = ws0(input)?;
    let (rem, id) = verify(mcnp_u32, |&id| id > 0)(rem)?;
    let (rem, _) = ws1(rem)?;

    // Check for `LIKE n BUT` syntax
    if let Ok((rem_like, _)) = kw("like")(rem) {
        let (rem_like, _) = ws1(rem_like)?;
        let (rem_like, base_cell_id) = verify(mcnp_u32, |&id| id > 0)(rem_like)?;
        let (rem_like, _) = ws1(rem_like)?;
        let (rem_like, _) = kw("but")(rem_like)?;
        let (rem_like, params) = many1(parse_single_cell_param)(rem_like)?;
        let (rem_like, _) = ws0(rem_like)?;
        return Ok((
            rem_like,
            CellCard {
                id,
                definition: CellDefinition::LikeBut { base_cell_id },
                params,
            },
        ));
    }

    // Standard cell definition: material_id, optional density, geometry, params
    let (rem, material_id) = mcnp_u32(rem)?;
    let (rem, density) = if material_id == 0 {
        (rem, None)
    } else {
        let (r, _) = ws1(rem)?;
        let (r, d) = mcnp_float(r)?;
        (r, Some(d))
    };

    let (rem, _) = ws1(rem)?;
    let (rem, geometry) = parse_union_expr(rem)?;
    let (rem, params) = many0(parse_single_cell_param)(rem)?;
    let (rem, _) = ws0(rem)?;

    Ok((
        rem,
        CellCard {
            id,
            definition: CellDefinition::Normal {
                material_id,
                density,
                geometry,
            },
            params,
        },
    ))
}

pub fn parse_cell_card_complete(input: &str) -> Result<CellCard, String> {
    match all_consuming(parse_cell_card)(input) {
        Ok((_, card)) => Ok(card),
        Err(e) => Err(format!("Failed to parse cell card `{}`: {}", input, e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Particle;

    #[test]
    fn test_geometry_boolean_precedence() {
        // `-1 2 : -3 #4 #(5 : -6)`
        // Should parse as Union( Intersection(-1, 2), Intersection(-3, #4, #(Union(5, -6))) )
        let card = parse_cell_card_complete("10 1 -2.7 -1 2 : -3 #4 #(5 : -6) imp:n=1").unwrap();
        assert_eq!(card.id, 10);
        match card.definition {
            CellDefinition::Normal {
                material_id,
                density,
                geometry,
            } => {
                assert_eq!(material_id, 1);
                assert_eq!(density, Some(-2.7));
                assert_eq!(
                    geometry,
                    GeometryExpr::Union(vec![
                        GeometryExpr::Intersection(vec![
                            GeometryExpr::Surface(-1),
                            GeometryExpr::Surface(2),
                        ]),
                        GeometryExpr::Intersection(vec![
                            GeometryExpr::Surface(-3),
                            GeometryExpr::ComplementCell(4),
                            GeometryExpr::Complement(Box::new(GeometryExpr::Union(vec![
                                GeometryExpr::Surface(5),
                                GeometryExpr::Surface(-6),
                            ]))),
                        ]),
                    ])
                );
            }
            _ => panic!("Expected normal cell"),
        }
        assert_eq!(
            card.params,
            vec![CellParam::Importance {
                particles: vec![Particle::new("n")],
                value: 1.0
            }]
        );
    }

    #[test]
    fn test_like_but_cell_and_trcl() {
        let card =
            parse_cell_card_complete("20 LIKE 10 BUT *TRCL=(0 0 10) MAT=2 RHO=-1.5 IMP:N,P=1")
                .unwrap();
        assert_eq!(card.id, 20);
        assert_eq!(
            card.definition,
            CellDefinition::LikeBut { base_cell_id: 10 }
        );
        assert_eq!(card.params.len(), 4);
    }

    #[test]
    fn test_macrobody_facet_in_cell() {
        let card = parse_cell_card_complete("1 0 -10.1 10.2 imp:n=1").unwrap();
        match card.definition {
            CellDefinition::Normal { geometry, .. } => {
                assert_eq!(
                    geometry,
                    GeometryExpr::Intersection(vec![
                        GeometryExpr::Facet {
                            surface: -10,
                            facet: 1
                        },
                        GeometryExpr::Facet {
                            surface: 10,
                            facet: 2
                        },
                    ])
                );
            }
            _ => panic!("Expected normal cell"),
        }
    }
}
