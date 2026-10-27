pub mod analysis;
pub mod ast;
pub mod lexer;
pub mod lsp;
pub mod parser;
pub mod preprocess;
pub mod shortcut;
pub mod templates;
pub mod validate;

pub use crate::analysis::{
    analyze_diagnostics, analyze_symbols, detect_block_at_line, format_inspect_report,
    format_symbols_report, format_validation_report, inspect_target, inspect_target_entity,
    resolve_symbol_at_position, InspectReport, InspectTarget, McnpBlockKind, ResolvedCursorTarget,
    SymbolsReport, ValidationReport,
};
pub use crate::templates::{get_template, list_templates, TemplateTopic};
pub use crate::validate::{
    validate_mcnp_input, ErrorCode, Severity, ValidationCategory, ValidationError,
};

use crate::ast::McnpInput;
use crate::parser::cell::parse_cell_card_complete;
use crate::parser::data::parse_data_card_complete;
use crate::parser::surface::parse_surface_card_complete;
use crate::preprocess::preprocess_mcnp;
use std::collections::HashMap;
use thiserror::Error;

/// Error returned when parsing an MCNP input file fails.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseError {
    #[error("Error in Cell Card at line {line_number}: {message}")]
    CellError { line_number: usize, message: String },

    #[error("Error in Surface Card at line {line_number}: {message}")]
    SurfaceError { line_number: usize, message: String },

    #[error("Error in Data Card at line {line_number}: {message}")]
    DataError { line_number: usize, message: String },
}

impl ParseError {
    /// Returns the 1-based starting line number where the error occurred.
    pub fn line_number(&self) -> usize {
        match self {
            ParseError::CellError { line_number, .. }
            | ParseError::SurfaceError { line_number, .. }
            | ParseError::DataError { line_number, .. } => *line_number,
        }
    }
}

/// 1-based line number mappings for parsed MCNP cards.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CardLineSpans {
    /// Maps `CellCard::id` to its 1-based starting line number.
    pub cell_lines: HashMap<u32, usize>,
    /// Maps `SurfaceCard::id` to its 1-based starting line number.
    pub surface_lines: HashMap<u32, usize>,
    /// 1-based starting line numbers corresponding to `McnpInput::cells` by index.
    pub cell_card_lines: Vec<usize>,
    /// 1-based starting line numbers corresponding to `McnpInput::surfaces` by index.
    pub surface_card_lines: Vec<usize>,
    /// 1-based starting line numbers corresponding to `McnpInput::data_cards` by index.
    pub data_card_lines: Vec<usize>,
}

/// Result of lossy MCNP parsing that recovers valid cards even when some cards fail to parse.
#[derive(Debug, Clone, PartialEq)]
pub struct LossyParseResult {
    pub input: McnpInput,
    pub spans: CardLineSpans,
    pub errors: Vec<ParseError>,
}

/// Parses a complete MCNP input string into an [`McnpInput`] AST.
pub fn parse_mcnp_input(input: &str) -> Result<McnpInput, ParseError> {
    let pre = preprocess_mcnp(input);

    let mut cells = Vec::with_capacity(pre.cell_cards.len());
    for card in &pre.cell_cards {
        let parsed = parse_cell_card_complete(&card.text).map_err(|msg| ParseError::CellError {
            line_number: card.line_number,
            message: msg,
        })?;
        cells.push(parsed);
    }

    let mut surfaces = Vec::with_capacity(pre.surface_cards.len());
    for card in &pre.surface_cards {
        let parsed =
            parse_surface_card_complete(&card.text).map_err(|msg| ParseError::SurfaceError {
                line_number: card.line_number,
                message: msg,
            })?;
        surfaces.push(parsed);
    }

    let mut data_cards = Vec::with_capacity(pre.data_cards.len());
    for card in &pre.data_cards {
        let parsed = parse_data_card_complete(&card.text).map_err(|msg| ParseError::DataError {
            line_number: card.line_number,
            message: msg,
        })?;
        data_cards.push(parsed);
    }

    Ok(McnpInput {
        message: pre.message,
        title: pre.title,
        cells,
        surfaces,
        data_cards,
    })
}

/// Parses an MCNP input string in lossy mode, collecting valid cards, line number spans,
/// and any card-level [`ParseError`]s without aborting on the first error.
pub fn parse_mcnp_input_lossy(input: &str) -> LossyParseResult {
    let pre = preprocess_mcnp(input);

    let mut cells = Vec::with_capacity(pre.cell_cards.len());
    let mut surfaces = Vec::with_capacity(pre.surface_cards.len());
    let mut data_cards = Vec::with_capacity(pre.data_cards.len());
    let mut spans = CardLineSpans::default();
    let mut errors = Vec::new();

    for card in &pre.cell_cards {
        match parse_cell_card_complete(&card.text) {
            Ok(parsed) => {
                spans.cell_lines.entry(parsed.id).or_insert(card.line_number);
                spans.cell_card_lines.push(card.line_number);
                cells.push(parsed);
            }
            Err(msg) => errors.push(ParseError::CellError {
                line_number: card.line_number,
                message: msg,
            }),
        }
    }

    for card in &pre.surface_cards {
        match parse_surface_card_complete(&card.text) {
            Ok(parsed) => {
                spans
                    .surface_lines
                    .entry(parsed.id)
                    .or_insert(card.line_number);
                spans.surface_card_lines.push(card.line_number);
                surfaces.push(parsed);
            }
            Err(msg) => errors.push(ParseError::SurfaceError {
                line_number: card.line_number,
                message: msg,
            }),
        }
    }

    for card in &pre.data_cards {
        match parse_data_card_complete(&card.text) {
            Ok(parsed) => {
                spans.data_card_lines.push(card.line_number);
                data_cards.push(parsed);
            }
            Err(msg) => errors.push(ParseError::DataError {
                line_number: card.line_number,
                message: msg,
            }),
        }
    }

    LossyParseResult {
        input: McnpInput {
            message: pre.message,
            title: pre.title,
            cells,
            surfaces,
            data_cards,
        },
        spans,
        errors,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        BoundaryCondition, CellDefinition, CellParam, DataCard, Entry, GeometryExpr, Particle,
        SurfaceKind,
    };

    #[test]
    fn test_syntax_cell_and_surface_pipeline() {
        let sample = "\
Synthetic Test Input
c ==========================================
c Block 1: Cell Cards
c ==========================================
1  1 -1.0     -1      imp:n=1.0 tmp=2.53-8 $ Inner sphere
2  2 -2.0      1 -2   imp:n=1.0            $ Middle shell
3  3 -0.5      2 -10  &
     imp:n=1.0                             $ Outer region
99 0          10      imp:n=0.0            $ Outside void

c ==========================================
c Block 2: Surface Cards
c ==========================================
1   cz  0.5
2   cz  1.0
*10 rpp -2.0 2.0 -2.0 2.0 -10.0 10.0       $ Reflective box

c ==========================================
c Block 3: Data Cards
c ==========================================
mode n
kcode 1000 1.0 50 250
ksrc 0.0 0.0 0.0
m1   1001 0.67 8016 0.33
m2   6000 1.0
m3   1001 2.0 8016 1.0
mt3  test.10t
f4:n 1 2 3 t
e4   1.0-5 3ilog 10.0
";

        let ast = parse_mcnp_input(sample).expect("Failed to parse synthetic input");
        assert_eq!(ast.title, "Synthetic Test Input");
        assert_eq!(ast.cells.len(), 4);
        assert_eq!(ast.surfaces.len(), 3);
        assert_eq!(ast.data_cards.len(), 9);

        // Verify Cell 1 with e-less float temperature `2.53-8`
        assert_eq!(ast.cells[0].id, 1);
        assert_eq!(
            ast.cells[0].params,
            vec![
                CellParam::Importance {
                    particles: vec![Particle::new("n")],
                    value: 1.0
                },
                CellParam::Temperature(vec![Entry::Value(2.53e-8)]),
            ]
        );

        // Verify Cell 2 intersection geometry `1 -2`
        match &ast.cells[1].definition {
            CellDefinition::Normal { geometry, .. } => {
                assert_eq!(
                    *geometry,
                    GeometryExpr::Intersection(vec![
                        GeometryExpr::Surface(1),
                        GeometryExpr::Surface(-2)
                    ])
                );
            }
            _ => panic!("Expected Normal cell"),
        }

        // Verify Surface 10 (Reflective RPP)
        assert_eq!(ast.surfaces[2].id, 10);
        assert_eq!(ast.surfaces[2].boundary, BoundaryCondition::Reflective);
        assert_eq!(ast.surfaces[2].kind, SurfaceKind::RPP);
        assert_eq!(
            ast.surfaces[2].params,
            vec![-2.0, 2.0, -2.0, 2.0, -10.0, 10.0]
        );

        // Verify Material 1
        match &ast.data_cards[3] {
            DataCard::Material { id, nuclides, .. } => {
                assert_eq!(*id, 1);
                assert_eq!(nuclides.len(), 2);
                assert_eq!(nuclides[1].zaid, "8016");
                assert_eq!(nuclides[1].fraction, 0.33);
            }
            other => panic!("Expected Material card, got {:?}", other),
        }
    }

    #[test]
    fn test_syntax_fixed_source_and_tallies() {
        let sample = "\
Synthetic Fixed Source Test
c ==========================================
c Block 1: Cell Cards
c ==========================================
1  0          -10
2  1 -2.0      10 -20
3  1 -2.0      20 -30
4  2 -0.001    30 -40
99 0           40

c ==========================================
c Block 2: Surface Cards
c ==========================================
10 so  10.0
20 so  20.0
30 so  30.0
40 so  50.0

c ==========================================
c Block 3: Data Cards
c ==========================================
mode n p
sdef pos=0 0 0 axs=0 0 1 rad=d1 ext=d2 erg=d3 dir=d4 par=n cel=1
si1  0.0 5.0
sp1  -21 1
si2  h -5.0 5.0
sp2  d  0.0 1.0
sc3  Synthetic two-line energy distribution
si3  l  1.0 10.0
sp3  d  0.1 0.9
sb3  d  0.2 0.8
si4  h -1.0 0.0 0.8 1.0
sp4  d  0.0 0.5 0.4 0.1
sb4  d  0.0 0.1 0.3 0.6
m1   1001 -0.1 8016 -0.9
m2   7014 -0.8 8016 -0.2
fc4  Track-length flux tally
f4:n 2 3 (2 3) 4 t
sd4  1.0e3 2.0e3 3.0e3 1.0e4
e4   1.0-9 1.0-3 0.1 1.0 10.0 20.0 nt
de4  log 1.0-9 1.0-3 0.1 1.0 10.0 20.0
df4  lin 1.0 2.0 10.0 50.0 100.0 200.0
fm4  (1.0 1 (102) (-6))
fc5  Point detector
f5:n 0.0 0.0 45.0 1.0 nd
fmesh14:n geom=xyz origin=-50 -50 -50
     imesh=50 iints=10
     jmesh=50 jints=10
     kmesh=50 kints=10
     emesh=1.0 20.0 eints=1 1
     out=col
wwe:n 1.0-3 20.0
wwn1:n 1.0 0.5 0.1 0.05 -1
wwn2:n 1.0 0.25 0.05 0.01 -1
wwp:n  5 3 5 0 -1
phys:n 20.0 0.0
cut:n  1.0+8 1.0-11 -0.5 -0.25
nps    1e5
";
        let ast = parse_mcnp_input(sample).expect("Failed to parse synthetic input");
        assert_eq!(ast.title, "Synthetic Fixed Source Test");
        assert_eq!(ast.cells.len(), 5);
        assert_eq!(ast.surfaces.len(), 4);
        assert_eq!(ast.data_cards.len(), 32);

        // Verify SDEF card
        match &ast.data_cards[1] {
            DataCard::Sdef { params } => {
                assert_eq!(params.len(), 8);
            }
            other => panic!("Expected Sdef card, got {:?}", other),
        }

        // Verify F4 tally
        let has_f4 = ast.data_cards.iter().any(|c| {
            matches!(
                c,
                DataCard::Tally {
                    id: 4,
                    total_bin: true,
                    targets,
                    ..
                } if targets.len() == 4
            )
        });
        assert!(has_f4);

        // Verify F5 detector
        let has_f5 = ast.data_cards.iter().any(|c| {
            matches!(
                c,
                DataCard::DetectorTally {
                    id: 5,
                    axis: None,
                    no_direct: true,
                    detectors,
                    ..
                } if detectors.len() == 1
            )
        });
        assert!(has_f5);
    }

    #[test]
    fn test_parse_mcnp_input_lossy_and_spans() {
        let sample = "\
Lossy Parse Test
1  1 -1.0  -10  imp:n=1
bad_cell_id 1 -1.0 -20
2  0        10  imp:n=0

10 so 5.0
20 px not_a_float

mode n
m1 1001.80c 1.0
";
        let res = parse_mcnp_input_lossy(sample);
        assert_eq!(res.input.cells.len(), 2);
        assert_eq!(res.input.surfaces.len(), 1);
        assert_eq!(res.input.data_cards.len(), 2);
        assert_eq!(res.errors.len(), 2);
        assert_eq!(res.errors[0].line_number(), 3);
        assert_eq!(res.errors[1].line_number(), 7);
        assert_eq!(res.spans.cell_lines.get(&1), Some(&2));
        assert_eq!(res.spans.cell_lines.get(&2), Some(&4));
        assert_eq!(res.spans.surface_lines.get(&10), Some(&6));
    }
}
