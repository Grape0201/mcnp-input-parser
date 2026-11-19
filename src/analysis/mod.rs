pub mod cursor;
pub mod diagnostics;
pub mod inspect;
pub mod symbols;

pub use cursor::{
    detect_block_at_line, resolve_symbol_at_position, McnpBlockKind, ResolvedCursorTarget,
};
pub use diagnostics::{
    analyze_diagnostics, format_validation_report, hint_for_validation_error, DiagnosticItem,
    ValidationReport,
};
pub use inspect::{
    format_inspect_report, inspect_target, inspect_target_entity, parse_inspect_target,
    DefinitionLocation, InspectReport, InspectTarget, ReferenceLocation,
};
pub use symbols::{
    analyze_symbols, build_symbols_report, format_symbols_report, CellSymbol, DistributionSymbol,
    MaterialSymbol, NextAvailableIds, SurfaceSymbol, SymbolsReport, TallySymbol, TransformSymbol,
    UniverseSymbol,
};

use crate::ast::GeometryExpr;
use std::collections::BTreeSet;

/// Extracts fill universe IDs from a `CellParam::Fill` values array.
pub(super) fn extract_fill_universes(lat: Option<u32>, values: &[String]) -> Vec<i32> {
    let mut result = BTreeSet::new();
    let mut idx = 0;
    if lat.is_some() {
        while idx < values.len() && values[idx].contains(':') {
            idx += 1;
        }
    }
    let mut in_parens = false;
    while idx < values.len() {
        let tok = &values[idx];
        if tok.starts_with('(') {
            in_parens = true;
            if tok.ends_with(')') {
                in_parens = false;
            }
            idx += 1;
            continue;
        }
        if in_parens {
            if tok.ends_with(')') {
                in_parens = false;
            }
            idx += 1;
            continue;
        }
        let head = tok.split('(').next().unwrap_or(tok);
        if let Ok(u) = head.parse::<i32>() {
            if u > 0 {
                result.insert(u);
            }
        }
        if tok.contains('(') && !tok.ends_with(')') {
            in_parens = true;
        }
        idx += 1;
    }
    result.into_iter().collect()
}

/// Collects all surface IDs and cell complement IDs referenced in a geometry expression.
pub(super) fn collect_geometry_refs(expr: &GeometryExpr, surf_refs: &mut Vec<u32>, cell_refs: &mut Vec<u32>) {
    match expr {
        GeometryExpr::Surface(s) => surf_refs.push(s.unsigned_abs()),
        GeometryExpr::Facet { surface, .. } => surf_refs.push(surface.unsigned_abs()),
        GeometryExpr::ComplementCell(c) => cell_refs.push(*c),
        GeometryExpr::Complement(inner) => collect_geometry_refs(inner, surf_refs, cell_refs),
        GeometryExpr::Intersection(list) | GeometryExpr::Union(list) => {
            for item in list {
                collect_geometry_refs(item, surf_refs, cell_refs);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::{get_template, list_templates};

    #[test]
    fn test_all_skeleton_templates_validate_cleanly() {
        for topic in list_templates() {
            if topic.name.starts_with("skeleton-") {
                let report = analyze_diagnostics(topic.content);
                assert_eq!(
                    report.error_count, 0,
                    "Template `{}` had errors: {:?}",
                    topic.name, report.diagnostics
                );
                assert_eq!(
                    report.warning_count, 0,
                    "Template `{}` had warnings: {:?}",
                    topic.name, report.diagnostics
                );
            }
        }
    }

    #[test]
    fn test_symbols_and_next_available_ids() {
        let tpl = get_template("skeleton-sdef").expect("skeleton-sdef should exist");
        let symbols = analyze_symbols(tpl.content);
        assert_eq!(symbols.cells.len(), 2);
        assert_eq!(symbols.surfaces.len(), 1);
        assert_eq!(symbols.materials.len(), 1);
        assert_eq!(symbols.tallies.len(), 1);
        assert_eq!(symbols.distributions.len(), 0);
        assert_eq!(symbols.next_available_ids.cell, 2);
        assert_eq!(symbols.next_available_ids.surface, 11);
        assert_eq!(symbols.next_available_ids.material, 2);
        assert_eq!(symbols.next_available_ids.tally_f4, 14);
    }

    #[test]
    fn test_inspect_surface_and_material_references() {
        let tpl = get_template("skeleton-sdef").expect("skeleton-sdef should exist");

        let surf_rep = inspect_target(tpl.content, "s10").expect("valid target s10");
        assert!(surf_rep.found);
        assert_eq!(surf_rep.definitions.len(), 1);
        assert_eq!(surf_rep.references.len(), 2); // Cell 1 (-10) and Cell 99 (10)

        let mat_rep = inspect_target(tpl.content, "m1").expect("valid target m1");
        assert!(mat_rep.found);
        assert_eq!(mat_rep.definitions.len(), 1);
        assert_eq!(mat_rep.references.len(), 1); // Cell 1

        let tally_rep = inspect_target(tpl.content, "f4").expect("valid target f4");
        assert!(tally_rep.found);
        assert_eq!(tally_rep.definitions.len(), 1);
        assert_eq!(tally_rep.references.len(), 0);
    }

    #[test]
    fn test_extra_blank_lines_reported_first_as_e004() {
        let input_with_extra_blanks = "\
Sphere Shielding Problem

1 1 -7.87 -10 imp:n=1.0
2 0 10 imp:n=0.0

10 so 5.0

mode n

sdef pos=0 0 0 erg=14.1

m1 26056.80c 1.0
nps 10000
";
        let report = analyze_diagnostics(input_with_extra_blanks);
        assert!(report.error_count > 0);
        let first = &report.diagnostics[0];
        assert_eq!(first.code, "E004");
        assert_eq!(first.line_number, Some(2));
        assert!(first.message.contains("[2, 5, 7, 9, 11]"));
        assert!(first.message.contains("mode n"));
        assert!(first.hint.contains("EXACTLY 2 blank lines"));
    }
}
