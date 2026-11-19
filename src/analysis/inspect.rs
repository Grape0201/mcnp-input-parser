use super::{collect_geometry_refs, extract_fill_universes};
use crate::ast::{
    CellCard, CellDefinition, CellParam, DataCard, FmBin, McnpInput, SdefValue, SdefVariable,
    SiEntry, SurfaceCard, TallyTarget,
};
use crate::{parse_mcnp_input_lossy, CardLineSpans};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionLocation {
    pub line_number: Option<usize>,
    pub card_kind: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceLocation {
    pub line_number: Option<usize>,
    pub source_card: String,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectReport {
    pub query: String,
    pub canonical_target: String,
    pub found: bool,
    pub definitions: Vec<DefinitionLocation>,
    pub references: Vec<ReferenceLocation>,
}

#[derive(Default)]
pub(super) struct ReferenceIndex {
    pub(super) cell_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    pub(super) surf_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    pub(super) mat_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    pub(super) univ_refs: BTreeMap<i32, Vec<ReferenceLocation>>,
    pub(super) tr_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    pub(super) tally_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
    pub(super) dist_refs: BTreeMap<u32, Vec<ReferenceLocation>>,
}

pub(super) fn collect_all_references(input: &McnpInput, spans: &CardLineSpans) -> ReferenceIndex {
    let mut idx = ReferenceIndex::default();

    // 1. Scan Cell Cards
    for (i, cell) in input.cells.iter().enumerate() {
        let line = spans.cell_card_lines.get(i).copied();
        let src = format!("Cell {}", cell.id);

        match &cell.definition {
            CellDefinition::Normal {
                material_id,
                geometry,
                ..
            } => {
                if *material_id > 0 {
                    idx.mat_refs.entry(*material_id).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} material M{}", cell.id, material_id),
                    });
                }
                let mut s_refs = Vec::new();
                let mut c_refs = Vec::new();
                collect_geometry_refs(geometry, &mut s_refs, &mut c_refs);
                s_refs.sort_unstable();
                s_refs.dedup();
                c_refs.sort_unstable();
                c_refs.dedup();
                for sid in s_refs {
                    idx.surf_refs.entry(sid).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} geometry expression", cell.id),
                    });
                }
                for cid in c_refs {
                    idx.cell_refs.entry(cid).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} complement `#{}", cell.id, cid),
                    });
                }
            }
            CellDefinition::LikeBut { base_cell_id } => {
                idx.cell_refs.entry(*base_cell_id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: src.clone(),
                    context: format!("Cell {} `LIKE {} BUT`", cell.id, base_cell_id),
                });
            }
        }

        let mut lat = None;
        for p in &cell.params {
            if let CellParam::Lattice(l) = p {
                lat = Some(*l);
            }
        }

        for p in &cell.params {
            match p {
                CellParam::Material(m) if *m > 0 => {
                    idx.mat_refs.entry(*m).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} parameter `MAT={}`", cell.id, m),
                    });
                }
                CellParam::Universe(u) => {
                    idx.univ_refs.entry(*u).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: src.clone(),
                        context: format!("Cell {} belongs to universe `U={}`", cell.id, u),
                    });
                }
                CellParam::Fill { values, .. } => {
                    for fu in extract_fill_universes(lat, values) {
                        idx.univ_refs.entry(fu).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: src.clone(),
                            context: format!("Cell {} filled with universe `FILL={}`", cell.id, fu),
                        });
                    }
                }
                CellParam::Transformation { values, .. } if values.len() == 1 => {
                    if let Some(v) = values[0].as_f64() {
                        if v > 0.0 {
                            let tr_id = v.round() as u32;
                            idx.tr_refs.entry(tr_id).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: src.clone(),
                                context: format!("Cell {} transformation `TRCL={}`", cell.id, tr_id),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // 2. Scan Surface Cards
    for (i, surf) in input.surfaces.iter().enumerate() {
        let line = spans.surface_card_lines.get(i).copied();
        let src = format!("Surface {}", surf.id);
        if let Some(tr_id) = surf.transform_id {
            if tr_id > 0 {
                let tid = tr_id as u32;
                idx.tr_refs.entry(tid).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: src,
                    context: format!("Surface {} coordinate transform TR{}", surf.id, tid),
                });
            } else if tr_id < 0 {
                let peer = tr_id.unsigned_abs();
                idx.surf_refs.entry(peer).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: src,
                    context: format!("Surface {} periodic boundary peer ({})", surf.id, tr_id),
                });
            }
        }
    }

    // 3. Scan Data Cards
    for (i, card) in input.data_cards.iter().enumerate() {
        let line = spans.data_card_lines.get(i).copied();
        match card {
            DataCard::ThermalScattering { id, tables } => {
                idx.mat_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("MT{}", id),
                    context: format!("Thermal scattering S(a,b) tables {:?}", tables),
                });
            }
            DataCard::Sdef { params } => {
                for p in params {
                    match (&p.variable, &p.value) {
                        (_, SdefValue::Distribution(d)) => {
                            idx.dist_refs.entry(*d).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!("SDEF {:?}=D{}", p.variable, d),
                            });
                        }
                        (_, SdefValue::Function { independent_var, dist_id }) => {
                            idx.dist_refs.entry(*dist_id).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!(
                                    "SDEF {:?}=F{:?} D{}",
                                    p.variable, independent_var, dist_id
                                ),
                            });
                        }
                        (SdefVariable::Cel, SdefValue::Scalar(v)) if *v > 0.0 => {
                            let cid = v.round() as u32;
                            idx.cell_refs.entry(cid).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!("SDEF CEL={}", cid),
                            });
                        }
                        (SdefVariable::Sur, SdefValue::Scalar(v)) if *v != 0.0 => {
                            let sid = v.abs().round() as u32;
                            idx.surf_refs.entry(sid).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!("SDEF SUR={}", sid),
                            });
                        }
                        (SdefVariable::Tr, SdefValue::Scalar(v)) if *v > 0.0 => {
                            let tid = v.round() as u32;
                            idx.tr_refs.entry(tid).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: "SDEF".to_string(),
                                context: format!("SDEF TR={}", tid),
                            });
                        }
                        _ => {}
                    }
                }
            }
            DataCard::Si(si) => {
                for entry in &si.entries {
                    if let SiEntry::Distribution(d) = entry {
                        idx.dist_refs.entry(*d).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("SI{}", si.id),
                            context: format!("SI{} S references distribution D{}", si.id, d),
                        });
                    }
                }
            }
            DataCard::Ds(ds) => {
                for entry in &ds.entries {
                    if let SiEntry::Distribution(d) = entry {
                        idx.dist_refs.entry(*d).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("DS{}", ds.id),
                            context: format!("DS{} references distribution D{}", ds.id, d),
                        });
                    }
                }
                for &(_, d) in &ds.pairs {
                    idx.dist_refs.entry(d).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: format!("DS{}", ds.id),
                        context: format!("DS{} pair references distribution D{}", ds.id, d),
                    });
                }
            }
            DataCard::Tally {
                id, kind, targets, ..
            } => {
                let is_surf = matches!(
                    kind,
                    crate::ast::TallyKind::F1SurfaceCurrent | crate::ast::TallyKind::F2SurfaceFlux
                );
                let mut ids = Vec::new();
                for t in targets {
                    match t {
                        TallyTarget::Single(n) => ids.push(*n),
                        TallyTarget::Group(list) => ids.extend(list.iter().copied()),
                        TallyTarget::ComplexGroup(_) => {}
                    }
                }
                ids.sort_unstable();
                ids.dedup();
                for target_id in ids {
                    if is_surf {
                        idx.surf_refs.entry(target_id).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("F{}", id),
                            context: format!("Surface tally F{} target surface {}", id, target_id),
                        });
                    } else {
                        idx.cell_refs.entry(target_id).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("F{}", id),
                            context: format!("Cell tally F{} target cell {}", id, target_id),
                        });
                    }
                }
            }
            DataCard::TallyMultiplier(fm) => {
                idx.tally_refs.entry(fm.id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("FM{}", fm.id),
                    context: format!("Tally multiplier FM{} for tally F{}", fm.id, fm.id),
                });
                for bin in &fm.bins {
                    if let FmBin::MultiplierSet { material_id, .. } = bin {
                        if *material_id > 0 {
                            let mid = *material_id as u32;
                            idx.mat_refs.entry(mid).or_default().push(ReferenceLocation {
                                line_number: line,
                                source_card: format!("FM{}", fm.id),
                                context: format!("Tally multiplier FM{} uses material M{}", fm.id, mid),
                            });
                        }
                    }
                }
            }
            DataCard::TallySegment { id, surfaces, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("FS{}", id),
                    context: format!("Tally segment FS{} for tally F{}", id, id),
                });
                for s in surfaces {
                    let sid = s.unsigned_abs();
                    idx.surf_refs.entry(sid).or_default().push(ReferenceLocation {
                        line_number: line,
                        source_card: format!("FS{}", id),
                        context: format!("Tally segment FS{} dividing surface {}", id, s),
                    });
                }
            }
            DataCard::SegmentDivisor { id, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("SD{}", id),
                    context: format!("Segment divisor SD{} for tally F{}", id, id),
                });
            }
            DataCard::TallyEnergy { id, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("E{}", id),
                    context: format!("Energy bins E{} for tally F{}", id, id),
                });
            }
            DataCard::TallyTime { id, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("T{}", id),
                    context: format!("Time bins T{} for tally F{}", id, id),
                });
            }
            DataCard::TallyCosine { id, .. } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("C{}", id),
                    context: format!("Cosine bins C{} for tally F{}", id, id),
                });
            }
            DataCard::TallyComment { id, comment } => {
                idx.tally_refs.entry(*id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("FC{}", id),
                    context: format!("Tally comment FC{}: {}", id, comment),
                });
            }
            DataCard::Dose(dose) => {
                let prefix = if dose.is_function { "DF" } else { "DE" };
                idx.tally_refs.entry(dose.id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: format!("{}{}", prefix, dose.id),
                    context: format!("Dose card {}{} ({} values)", prefix, dose.id, dose.values.len()),
                });
            }
            DataCard::Fmesh(fm) => {
                if let Some(tid) = fm.tr {
                    if tid > 0 {
                        idx.tr_refs.entry(tid).or_default().push(ReferenceLocation {
                            line_number: line,
                            source_card: format!("FMESH{}", fm.id),
                            context: format!("FMESH{} TR={}", fm.id, tid),
                        });
                    }
                }
            }
            DataCard::WeightWindowGenerator {
                tally_id,
                reference_cell,
                ..
            } => {
                idx.tally_refs.entry(*tally_id).or_default().push(ReferenceLocation {
                    line_number: line,
                    source_card: "WWG".to_string(),
                    context: format!("WWG target tally F{}", tally_id),
                });
                idx.cell_refs
                    .entry(*reference_cell)
                    .or_default()
                    .push(ReferenceLocation {
                        line_number: line,
                        source_card: "WWG".to_string(),
                        context: format!("WWG reference cell {}", reference_cell),
                    });
            }
            _ => {}
        }
    }

    idx
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InspectTarget {
    Cell(u32),
    Surface(u32),
    Material(u32),
    Universe(i32),
    Transform(u32),
    Tally(u32),
    Distribution(u32),
}

/// Parses a target selector string such as `c10`, `cell:10`, `s20`, `surf:20`, `m1`, `u1`, `f4`, `tr1`, `d1`.
pub fn parse_inspect_target(raw: &str) -> Result<InspectTarget, String> {
    let s = raw.trim().to_lowercase();
    if let Some((prefix, rest)) = s.split_once(':') {
        return match prefix {
            "c" | "cell" => rest
                .parse::<u32>()
                .map(InspectTarget::Cell)
                .map_err(|_| format!("Invalid cell ID in `{}`", raw)),
            "s" | "surf" | "surface" => rest
                .parse::<u32>()
                .map(InspectTarget::Surface)
                .map_err(|_| format!("Invalid surface ID in `{}`", raw)),
            "m" | "mat" | "material" => rest
                .parse::<u32>()
                .map(InspectTarget::Material)
                .map_err(|_| format!("Invalid material ID in `{}`", raw)),
            "u" | "univ" | "universe" => rest
                .parse::<i32>()
                .map(InspectTarget::Universe)
                .map_err(|_| format!("Invalid universe ID in `{}`", raw)),
            "tr" | "transform" => rest
                .parse::<u32>()
                .map(InspectTarget::Transform)
                .map_err(|_| format!("Invalid transform ID in `{}`", raw)),
            "f" | "tally" | "fmesh" => rest
                .parse::<u32>()
                .map(InspectTarget::Tally)
                .map_err(|_| format!("Invalid tally ID in `{}`", raw)),
            "d" | "dist" | "si" | "sp" | "sb" | "ds" => rest
                .parse::<u32>()
                .map(InspectTarget::Distribution)
                .map_err(|_| format!("Invalid distribution ID in `{}`", raw)),
            _ => Err(format!(
                "Unknown target prefix `{}:`. Use `c<id>`, `s<id>`, `m<id>`, `u<id>`, `f<id>`, `tr<id>`, or `d<id>`.",
                prefix
            )),
        };
    }

    for (prefix, kind) in [
        ("surface", "s"),
        ("surf", "s"),
        ("cell", "c"),
        ("material", "m"),
        ("mat", "m"),
        ("universe", "u"),
        ("univ", "u"),
        ("fmesh", "f"),
        ("tally", "f"),
        ("transform", "tr"),
        ("dist", "d"),
        ("tr", "tr"),
        ("si", "d"),
        ("sp", "d"),
        ("sb", "d"),
        ("ds", "d"),
        ("c", "c"),
        ("s", "s"),
        ("m", "m"),
        ("u", "u"),
        ("f", "f"),
        ("d", "d"),
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let clean = rest.trim_start_matches('=');
            if !clean.is_empty() {
                return match kind {
                    "c" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Cell)
                        .map_err(|_| format!("Invalid cell ID `{}`", raw)),
                    "s" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Surface)
                        .map_err(|_| format!("Invalid surface ID `{}`", raw)),
                    "m" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Material)
                        .map_err(|_| format!("Invalid material ID `{}`", raw)),
                    "u" => clean
                        .parse::<i32>()
                        .map(InspectTarget::Universe)
                        .map_err(|_| format!("Invalid universe ID `{}`", raw)),
                    "tr" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Transform)
                        .map_err(|_| format!("Invalid transform ID `{}`", raw)),
                    "f" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Tally)
                        .map_err(|_| format!("Invalid tally ID `{}`", raw)),
                    "d" => clean
                        .parse::<u32>()
                        .map(InspectTarget::Distribution)
                        .map_err(|_| format!("Invalid distribution ID `{}`", raw)),
                    _ => unreachable!(),
                };
            }
        }
    }

    Err(format!(
        "Could not parse target `{}`. Examples: `c1` (Cell 1), `s10` (Surface 10), `m1` (Material 1), `u1` (Universe 1), `f4` (Tally 4), `tr1` (Transform 1), `d1` (Distribution 1).",
        raw
    ))
}

/// Inspects a specific target entity (`Cell`, `Surface`, `Material`, `Universe`, `Transform`, `Tally`, `Distribution`)
/// and returns its definitions and all cross-references across the input file.
pub fn inspect_target(content: &str, raw_target: &str) -> Result<InspectReport, String> {
    let target = parse_inspect_target(raw_target)?;
    Ok(inspect_target_entity(content, target, raw_target))
}

/// Inspects a resolved [`InspectTarget`] directly.
pub fn inspect_target_entity(
    content: &str,
    target: InspectTarget,
    query_label: &str,
) -> InspectReport {
    let lossy = parse_mcnp_input_lossy(content);
    let input = &lossy.input;
    let spans = &lossy.spans;
    let mut refs_idx = collect_all_references(input, spans);

    let mut definitions = Vec::new();
    let (canonical_target, references) = match target {
        InspectTarget::Cell(id) => {
            for (idx, c) in input.cells.iter().enumerate() {
                if c.id == id {
                    definitions.push(DefinitionLocation {
                        line_number: spans.cell_card_lines.get(idx).copied(),
                        card_kind: "CellCard".to_string(),
                        summary: summarize_cell(c),
                    });
                }
            }
            (
                format!("Cell {}", id),
                refs_idx.cell_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Surface(id) => {
            for (idx, s) in input.surfaces.iter().enumerate() {
                if s.id == id {
                    definitions.push(DefinitionLocation {
                        line_number: spans.surface_card_lines.get(idx).copied(),
                        card_kind: "SurfaceCard".to_string(),
                        summary: summarize_surface(s),
                    });
                }
            }
            (
                format!("Surface {}", id),
                refs_idx.surf_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Material(id) => {
            for (idx, card) in input.data_cards.iter().enumerate() {
                let line = spans.data_card_lines.get(idx).copied();
                match card {
                    DataCard::Material {
                        id: mid,
                        nuclides,
                        options,
                    } if *mid == id => {
                        let pairs: Vec<String> = nuclides
                            .iter()
                            .map(|n| format!("{}: {}", n.zaid, n.fraction))
                            .collect();
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("M{}", id),
                            summary: format!(
                                "Material M{}: {} nuclides [{}], options={:?}",
                                id,
                                nuclides.len(),
                                pairs.join(", "),
                                options
                            ),
                        });
                    }
                    DataCard::ThermalScattering { id: mid, tables } if *mid == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("MT{}", id),
                            summary: format!("Thermal Scattering MT{}: {:?}", id, tables),
                        });
                    }
                    _ => {}
                }
            }
            (
                format!("Material M{}", id),
                refs_idx.mat_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Universe(uid) => {
            for (idx, c) in input.cells.iter().enumerate() {
                if c.params.iter().any(|p| matches!(p, CellParam::Universe(u) if *u == uid)) {
                    definitions.push(DefinitionLocation {
                        line_number: spans.cell_card_lines.get(idx).copied(),
                        card_kind: format!("Cell {} (U={})", c.id, uid),
                        summary: summarize_cell(c),
                    });
                }
            }
            (
                format!("Universe U={}", uid),
                refs_idx.univ_refs.remove(&uid).unwrap_or_default(),
            )
        }
        InspectTarget::Transform(id) => {
            for (idx, card) in input.data_cards.iter().enumerate() {
                if let DataCard::Transform {
                    id: tid,
                    is_degrees,
                    values,
                } = card
                {
                    if *tid == id {
                        definitions.push(DefinitionLocation {
                            line_number: spans.data_card_lines.get(idx).copied(),
                            card_kind: format!("TR{}", id),
                            summary: format!(
                                "Transform TR{} (degrees={}): {:?}",
                                id, is_degrees, values
                            ),
                        });
                    }
                }
            }
            (
                format!("Transform TR{}", id),
                refs_idx.tr_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Tally(id) => {
            for (idx, card) in input.data_cards.iter().enumerate() {
                let line = spans.data_card_lines.get(idx).copied();
                match card {
                    DataCard::Tally {
                        id: tid,
                        kind,
                        particles,
                        targets,
                        total_bin,
                        ..
                    } if *tid == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("F{}", id),
                            summary: format!(
                                "Tally F{} ({:?}) particles={:?} targets={:?} total_bin={}",
                                id, kind, particles, targets, total_bin
                            ),
                        });
                    }
                    DataCard::DetectorTally {
                        id: tid,
                        axis,
                        particles,
                        detectors,
                        ..
                    } if *tid == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("F{}", id),
                            summary: format!(
                                "Detector Tally F{} (axis={:?}) particles={:?} detectors={:?}",
                                id, axis, particles, detectors
                            ),
                        });
                    }
                    DataCard::Fmesh(fm) if fm.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("FMESH{}", id),
                            summary: format!(
                                "Mesh Tally FMESH{} geom={:?} origin={:?} imesh={:?} jmesh={:?} kmesh={:?}",
                                id, fm.geom, fm.origin, fm.imesh, fm.jmesh, fm.kmesh
                            ),
                        });
                    }
                    _ => {}
                }
            }
            (
                format!("Tally {}", id),
                refs_idx.tally_refs.remove(&id).unwrap_or_default(),
            )
        }
        InspectTarget::Distribution(id) => {
            for (idx, card) in input.data_cards.iter().enumerate() {
                let line = spans.data_card_lines.get(idx).copied();
                match card {
                    DataCard::Si(si) if si.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("SI{}", id),
                            summary: format!(
                                "Source Info SI{} (option={:?}, {} entries): {:?}",
                                id,
                                si.option,
                                si.entries.len(),
                                si.entries
                            ),
                        });
                    }
                    DataCard::Sp(sp) if sp.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("SP{}", id),
                            summary: format!("Source Probability SP{}: {:?}", id, sp.content),
                        });
                    }
                    DataCard::Sb(sb) if sb.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("SB{}", id),
                            summary: format!("Source Bias SB{}: {:?}", id, sb.content),
                        });
                    }
                    DataCard::Ds(ds) if ds.id == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("DS{}", id),
                            summary: format!(
                                "Dependent Source DS{} (option={:?}): entries={:?} pairs={:?}",
                                id, ds.option, ds.entries, ds.pairs
                            ),
                        });
                    }
                    DataCard::Sc { id: sid, comment } if *sid == id => {
                        definitions.push(DefinitionLocation {
                            line_number: line,
                            card_kind: format!("SC{}", id),
                            summary: format!("Source Comment SC{}: {}", id, comment),
                        });
                    }
                    _ => {}
                }
            }
            (
                format!("Distribution D{}", id),
                refs_idx.dist_refs.remove(&id).unwrap_or_default(),
            )
        }
    };

    InspectReport {
        query: query_label.to_string(),
        canonical_target,
        found: !definitions.is_empty(),
        definitions,
        references,
    }
}

fn summarize_cell(c: &CellCard) -> String {
    match &c.definition {
        CellDefinition::Normal {
            material_id,
            density,
            geometry,
        } => {
            format!(
                "Cell {}: mat={}, density={:?}, geometry={:?}, params={:?}",
                c.id, material_id, density, geometry, c.params
            )
        }
        CellDefinition::LikeBut { base_cell_id } => {
            format!(
                "Cell {}: LIKE {} BUT params={:?}",
                c.id, base_cell_id, c.params
            )
        }
    }
}

fn summarize_surface(s: &SurfaceCard) -> String {
    format!(
        "Surface {}: boundary={:?}, transform={:?}, kind={:?}, params={:?}",
        s.id, s.boundary, s.transform_id, s.kind, s.params
    )
}

/// Formats an [`InspectReport`] as human/LLM-friendly text.
pub fn format_inspect_report(rep: &InspectReport) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== Inspect: {} ===\n", rep.canonical_target));
    if rep.definitions.is_empty() {
        out.push_str("Definitions: NOT DEFINED in this input file.\n");
    } else {
        out.push_str(&format!("Definitions ({}):\n", rep.definitions.len()));
        for def in &rep.definitions {
            let line = def
                .line_number
                .map(|l| format!("Line {}", l))
                .unwrap_or_else(|| "Line ?".to_string());
            out.push_str(&format!("  - [{}] {}\n", line, def.summary));
        }
    }

    if rep.references.is_empty() {
        out.push_str("References (0): Not referenced by any card.\n");
    } else {
        out.push_str(&format!("References ({}):\n", rep.references.len()));
        for r in &rep.references {
            let line = r
                .line_number
                .map(|l| format!("Line {}", l))
                .unwrap_or_else(|| "Line ?".to_string());
            out.push_str(&format!(
                "  - [{}] {} -> {}\n",
                line, r.source_card, r.context
            ));
        }
    }
    out
}

