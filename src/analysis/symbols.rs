use super::inspect::collect_all_references;
use super::extract_fill_universes;
use crate::ast::{
    BoundaryCondition, CellDefinition, CellParam, DataCard, McnpInput, SurfaceKind,
};
use crate::{parse_mcnp_input_lossy, CardLineSpans};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellSymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub material_id: Option<u32>,
    pub density: Option<f64>,
    pub like_base_cell: Option<u32>,
    pub universe: Option<i32>,
    pub fill_universes: Vec<i32>,
    pub lattice: Option<u32>,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceSymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub boundary: BoundaryCondition,
    pub transform_id: Option<i32>,
    pub kind: SurfaceKind,
    pub params: Vec<f64>,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialSymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub nuclide_count: usize,
    pub zaids: Vec<String>,
    pub mt_tables: Vec<String>,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UniverseSymbol {
    pub id: i32,
    pub member_cells: Vec<u32>,
    pub filled_by_cells: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformSymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub is_degrees: bool,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TallySymbol {
    pub id: u32,
    pub line_number: Option<usize>,
    pub tally_type: String,
    pub particles: Vec<String>,
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistributionSymbol {
    pub id: u32,
    pub defined_cards: Vec<String>,
    pub reference_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextAvailableIds {
    pub cell: u32,
    pub surface: u32,
    pub material: u32,
    pub universe: i32,
    pub transform: u32,
    pub distribution: u32,
    pub tally_f4: u32,
    pub fmesh: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolsReport {
    pub title: String,
    pub mode: Vec<String>,
    pub source_type: String,
    pub cells: Vec<CellSymbol>,
    pub surfaces: Vec<SurfaceSymbol>,
    pub materials: Vec<MaterialSymbol>,
    pub universes: Vec<UniverseSymbol>,
    pub transforms: Vec<TransformSymbol>,
    pub tallies: Vec<TallySymbol>,
    pub distributions: Vec<DistributionSymbol>,
    pub next_available_ids: NextAvailableIds,
}


/// Analyzes an MCNP input string and builds a complete [`SymbolsReport`].
pub fn analyze_symbols(content: &str) -> SymbolsReport {
    let lossy = parse_mcnp_input_lossy(content);
    build_symbols_report(&lossy.input, &lossy.spans)
}

/// Builds a [`SymbolsReport`] from a parsed [`McnpInput`] and [`CardLineSpans`].
pub fn build_symbols_report(input: &McnpInput, spans: &CardLineSpans) -> SymbolsReport {
    let all_refs = collect_all_references(input, spans);

    let mut mode = vec!["n".to_string()];
    let mut has_sdef = false;
    let mut has_kcode = false;

    for card in &input.data_cards {
        match card {
            DataCard::Mode { particles } if !particles.is_empty() => {
                mode = particles.iter().map(|p| p.0.clone()).collect();
            }
            DataCard::Sdef { .. } => has_sdef = true,
            DataCard::Kcode { .. } => has_kcode = true,
            _ => {}
        }
    }

    let source_type = match (has_sdef, has_kcode) {
        (true, false) => "Fixed-Source (SDEF)".to_string(),
        (false, true) => "Criticality (KCODE)".to_string(),
        (true, true) => "Conflicting (Both SDEF and KCODE)".to_string(),
        (false, false) => "None (Missing SDEF/KCODE)".to_string(),
    };

    // 1. Cells & Universes
    let mut universe_members: BTreeMap<i32, Vec<u32>> = BTreeMap::new();
    let mut universe_fillers: BTreeMap<i32, Vec<u32>> = BTreeMap::new();
    let mut cells = Vec::with_capacity(input.cells.len());

    for (idx, cell) in input.cells.iter().enumerate() {
        let line = spans.cell_card_lines.get(idx).copied();
        let mut mat_id = None;
        let mut density = None;
        let mut like_base = None;

        match &cell.definition {
            CellDefinition::Normal {
                material_id,
                density: d,
                ..
            } => {
                mat_id = Some(*material_id);
                density = *d;
            }
            CellDefinition::LikeBut { base_cell_id } => {
                like_base = Some(*base_cell_id);
            }
        }

        let mut univ = None;
        let mut lat = None;
        for p in &cell.params {
            match p {
                CellParam::Universe(u) => univ = Some(*u),
                CellParam::Lattice(l) => lat = Some(*l),
                CellParam::Material(m) => mat_id = Some(*m),
                CellParam::Density(d) => density = Some(*d),
                _ => {}
            }
        }

        let mut fill_univs = Vec::new();
        for p in &cell.params {
            if let CellParam::Fill { values, .. } = p {
                fill_univs = extract_fill_universes(lat, values);
            }
        }

        if let Some(u) = univ {
            universe_members.entry(u).or_default().push(cell.id);
        }
        for &fu in &fill_univs {
            universe_fillers.entry(fu).or_default().push(cell.id);
        }

        let ref_count = all_refs
            .cell_refs
            .get(&cell.id)
            .map(|v| v.len())
            .unwrap_or(0);

        cells.push(CellSymbol {
            id: cell.id,
            line_number: line,
            material_id: mat_id,
            density,
            like_base_cell: like_base,
            universe: univ,
            fill_universes: fill_univs,
            lattice: lat,
            reference_count: ref_count,
        });
    }

    let all_univ_ids: BTreeSet<i32> = universe_members
        .keys()
        .chain(universe_fillers.keys())
        .copied()
        .collect();
    let universes: Vec<UniverseSymbol> = all_univ_ids
        .into_iter()
        .map(|uid| UniverseSymbol {
            id: uid,
            member_cells: universe_members.remove(&uid).unwrap_or_default(),
            filled_by_cells: universe_fillers.remove(&uid).unwrap_or_default(),
        })
        .collect();

    // 2. Surfaces
    let surfaces: Vec<SurfaceSymbol> = input
        .surfaces
        .iter()
        .enumerate()
        .map(|(idx, s)| SurfaceSymbol {
            id: s.id,
            line_number: spans.surface_card_lines.get(idx).copied(),
            boundary: s.boundary,
            transform_id: s.transform_id,
            kind: s.kind.clone(),
            params: s.params.clone(),
            reference_count: all_refs.surf_refs.get(&s.id).map(|v| v.len()).unwrap_or(0),
        })
        .collect();

    // 3. Materials, Transforms, Tallies, Distributions
    let mut mt_map: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut tally_modifiers: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    let mut dist_cards: BTreeMap<u32, Vec<String>> = BTreeMap::new();

    for (idx, card) in input.data_cards.iter().enumerate() {
        let line = spans.data_card_lines.get(idx).copied();
        let line_suffix = line.map(|l| format!(" (L{})", l)).unwrap_or_default();
        match card {
            DataCard::ThermalScattering { id, tables } => {
                mt_map.entry(*id).or_default().extend(tables.clone());
            }
            DataCard::TallyEnergy { id, .. } => {
                tally_modifiers.entry(*id).or_default().push(format!("E{}", id));
            }
            DataCard::TallyTime { id, .. } => {
                tally_modifiers.entry(*id).or_default().push(format!("T{}", id));
            }
            DataCard::TallyCosine { id, .. } => {
                tally_modifiers.entry(*id).or_default().push(format!("C{}", id));
            }
            DataCard::Dose(d) => {
                let prefix = if d.is_function { "DF" } else { "DE" };
                tally_modifiers
                    .entry(d.id)
                    .or_default()
                    .push(format!("{}{}", prefix, d.id));
            }
            DataCard::TallyMultiplier(fm) => {
                tally_modifiers
                    .entry(fm.id)
                    .or_default()
                    .push(format!("FM{}", fm.id));
            }
            DataCard::TallySegment { id, .. } => {
                tally_modifiers
                    .entry(*id)
                    .or_default()
                    .push(format!("FS{}", id));
            }
            DataCard::SegmentDivisor { id, .. } => {
                tally_modifiers
                    .entry(*id)
                    .or_default()
                    .push(format!("SD{}", id));
            }
            DataCard::TallyComment { id, .. } => {
                tally_modifiers
                    .entry(*id)
                    .or_default()
                    .push(format!("FC{}", id));
            }
            DataCard::Si(si) => {
                dist_cards
                    .entry(si.id)
                    .or_default()
                    .push(format!("SI{}{}", si.id, line_suffix));
            }
            DataCard::Sp(sp) => {
                dist_cards
                    .entry(sp.id)
                    .or_default()
                    .push(format!("SP{}{}", sp.id, line_suffix));
            }
            DataCard::Sb(sb) => {
                dist_cards
                    .entry(sb.id)
                    .or_default()
                    .push(format!("SB{}{}", sb.id, line_suffix));
            }
            DataCard::Ds(ds) => {
                dist_cards
                    .entry(ds.id)
                    .or_default()
                    .push(format!("DS{}{}", ds.id, line_suffix));
            }
            _ => {}
        }
    }

    let mut materials = Vec::new();
    let mut transforms = Vec::new();
    let mut tallies = Vec::new();
    let mut fmesh_ids = Vec::new();

    for (idx, card) in input.data_cards.iter().enumerate() {
        let line = spans.data_card_lines.get(idx).copied();
        match card {
            DataCard::Material { id, nuclides, .. } => {
                materials.push(MaterialSymbol {
                    id: *id,
                    line_number: line,
                    nuclide_count: nuclides.len(),
                    zaids: nuclides.iter().map(|n| n.zaid.clone()).collect(),
                    mt_tables: mt_map.remove(id).unwrap_or_default(),
                    reference_count: all_refs.mat_refs.get(id).map(|v| v.len()).unwrap_or(0),
                });
            }
            DataCard::Transform {
                id, is_degrees, ..
            } => {
                transforms.push(TransformSymbol {
                    id: *id,
                    line_number: line,
                    is_degrees: *is_degrees,
                    reference_count: all_refs.tr_refs.get(id).map(|v| v.len()).unwrap_or(0),
                });
            }
            DataCard::Tally {
                id,
                kind,
                particles,
                ..
            } => {
                tallies.push(TallySymbol {
                    id: *id,
                    line_number: line,
                    tally_type: format!("{:?}", kind),
                    particles: particles.iter().map(|p| p.0.clone()).collect(),
                    modifiers: tally_modifiers.remove(id).unwrap_or_default(),
                });
            }
            DataCard::DetectorTally {
                id,
                axis,
                particles,
                ..
            } => {
                let kind_label = match axis {
                    None => "F5PointDetector".to_string(),
                    Some(ax) => format!("F{}5RingDetector", ax.to_ascii_uppercase()),
                };
                tallies.push(TallySymbol {
                    id: *id,
                    line_number: line,
                    tally_type: kind_label,
                    particles: particles.iter().map(|p| p.0.clone()).collect(),
                    modifiers: tally_modifiers.remove(id).unwrap_or_default(),
                });
            }
            DataCard::Fmesh(fm) => {
                fmesh_ids.push(fm.id);
                tallies.push(TallySymbol {
                    id: fm.id,
                    line_number: line,
                    tally_type: "FMESH".to_string(),
                    particles: fm.particles.iter().map(|p| p.0.clone()).collect(),
                    modifiers: Vec::new(),
                });
            }
            _ => {}
        }
    }

    let distributions: Vec<DistributionSymbol> = dist_cards
        .into_iter()
        .map(|(id, defined_cards)| DistributionSymbol {
            id,
            defined_cards,
            reference_count: all_refs.dist_refs.get(&id).map(|v| v.len()).unwrap_or(0),
        })
        .collect();

    // Compute conflict-free next available IDs
    let next_cell = cells
        .iter()
        .filter(|c| c.id < 90)
        .map(|c| c.id)
        .max()
        .unwrap_or(0)
        + 1;
    let next_cell = if cells.iter().any(|c| c.id == next_cell) {
        cells.iter().map(|c| c.id).max().unwrap_or(0) + 1
    } else {
        next_cell
    };

    let next_surface = surfaces.iter().map(|s| s.id).max().unwrap_or(0) + 1;
    let next_material = materials.iter().map(|m| m.id).max().unwrap_or(0) + 1;
    let next_universe = universes.iter().map(|u| u.id).max().unwrap_or(0) + 1;
    let next_transform = transforms.iter().map(|t| t.id).max().unwrap_or(0) + 1;
    let next_dist = distributions.iter().map(|d| d.id).max().unwrap_or(0) + 1;

    let mut next_f4 = 4;
    while tallies.iter().any(|t| t.id == next_f4) {
        next_f4 += 10;
    }
    let mut next_fmesh = 14;
    while tallies.iter().any(|t| t.id == next_fmesh) || fmesh_ids.contains(&next_fmesh) {
        next_fmesh += 10;
    }

    SymbolsReport {
        title: input.title.clone(),
        mode,
        source_type,
        cells,
        surfaces,
        materials,
        universes,
        transforms,
        tallies,
        distributions,
        next_available_ids: NextAvailableIds {
            cell: next_cell,
            surface: next_surface,
            material: next_material,
            universe: next_universe,
            transform: next_transform,
            distribution: next_dist,
            tally_f4: next_f4,
            fmesh: next_fmesh,
        },
    }
}

/// Formats a [`SymbolsReport`] as human/LLM-friendly text.
pub fn format_symbols_report(rep: &SymbolsReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("Title: {}\n", rep.title));
    out.push_str(&format!("Mode: {}\n", rep.mode.join(" ")));
    out.push_str(&format!("Source Type: {}\n\n", rep.source_type));

    out.push_str(&format!("=== Cells ({}) ===\n", rep.cells.len()));
    for c in &rep.cells {
        let line = c.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
        let mat_str = match (c.like_base_cell, c.material_id, c.density) {
            (Some(base), _, _) => format!("LIKE cell {}", base),
            (None, Some(0), _) => "void (mat=0)".to_string(),
            (None, Some(m), Some(d)) => format!("mat=M{}, rho={}", m, d),
            (None, Some(m), None) => format!("mat=M{}", m),
            _ => "unknown".to_string(),
        };
        let mut extras = Vec::new();
        if let Some(u) = c.universe {
            extras.push(format!("U={}", u));
        }
        if let Some(lat) = c.lattice {
            extras.push(format!("LAT={}", lat));
        }
        if !c.fill_universes.is_empty() {
            let fu: Vec<String> = c.fill_universes.iter().map(|u| u.to_string()).collect();
            extras.push(format!("FILL=[{}]", fu.join(",")));
        }
        let extra_str = if extras.is_empty() {
            String::new()
        } else {
            format!(" [{}]", extras.join(", "))
        };
        out.push_str(&format!(
            "  Cell {:>4} ({:<4}): {}{} (referenced {}x)\n",
            c.id, line, mat_str, extra_str, c.reference_count
        ));
    }

    out.push_str(&format!("\n=== Surfaces ({}) ===\n", rep.surfaces.len()));
    for s in &rep.surfaces {
        let line = s.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
        let bnd = match s.boundary {
            BoundaryCondition::Transmissive => "",
            BoundaryCondition::Reflective => "*",
            BoundaryCondition::White => "+",
        };
        let tr = s
            .transform_id
            .map(|t| format!(" tr={}", t))
            .unwrap_or_default();
        out.push_str(&format!(
            "  Surf {:>4} ({:<4}): {}{:?}{} {:?} (referenced {}x)\n",
            s.id, line, bnd, s.kind, tr, s.params, s.reference_count
        ));
    }

    out.push_str(&format!("\n=== Materials ({}) ===\n", rep.materials.len()));
    for m in &rep.materials {
        let line = m.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
        let mt = if m.mt_tables.is_empty() {
            String::new()
        } else {
            format!(" MT=[{}]", m.mt_tables.join(", "))
        };
        out.push_str(&format!(
            "  M{:<4} ({:<4}): {} nuclide(s) [{}]{} (referenced {}x)\n",
            m.id,
            line,
            m.nuclide_count,
            m.zaids.join(", "),
            mt,
            m.reference_count
        ));
    }

    if !rep.universes.is_empty() {
        out.push_str(&format!("\n=== Universes ({}) ===\n", rep.universes.len()));
        for u in &rep.universes {
            out.push_str(&format!(
                "  U={:<3}: member cells {:?}, filled into cells {:?}\n",
                u.id, u.member_cells, u.filled_by_cells
            ));
        }
    }

    if !rep.transforms.is_empty() {
        out.push_str(&format!("\n=== Transforms ({}) ===\n", rep.transforms.len()));
        for tr in &rep.transforms {
            let line = tr.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
            out.push_str(&format!(
                "  TR{:<3} ({:<4}): degrees={} (referenced {}x)\n",
                tr.id, line, tr.is_degrees, tr.reference_count
            ));
        }
    }

    if !rep.tallies.is_empty() {
        out.push_str(&format!("\n=== Tallies ({}) ===\n", rep.tallies.len()));
        for t in &rep.tallies {
            let line = t.line_number.map(|l| format!("L{}", l)).unwrap_or_default();
            let mods = if t.modifiers.is_empty() {
                String::new()
            } else {
                format!(" modifiers=[{}]", t.modifiers.join(", "))
            };
            out.push_str(&format!(
                "  ID {:>4} ({:<4}): {}:{}{}\n",
                t.id,
                line,
                t.tally_type,
                t.particles.join(","),
                mods
            ));
        }
    }

    if !rep.distributions.is_empty() {
        out.push_str(&format!(
            "\n=== Source Distributions ({}) ===\n",
            rep.distributions.len()
        ));
        for d in &rep.distributions {
            out.push_str(&format!(
                "  D{:<3}: [{}] (referenced {}x)\n",
                d.id,
                d.defined_cards.join(", "),
                d.reference_count
            ));
        }
    }

    let next = &rep.next_available_ids;
    out.push_str("\n=== Next Available IDs (Conflict-Free) ===\n");
    out.push_str(&format!(
        "  Cell: {} | Surface: {} | Material: M{} | Universe: U={} | Transform: TR{} | Dist: D{} | Tally: F{} / FMESH{}\n",
        next.cell,
        next.surface,
        next.material,
        next.universe,
        next.transform,
        next.distribution,
        next.tally_f4,
        next.fmesh
    ));

    out
}

