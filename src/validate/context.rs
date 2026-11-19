use super::types::{ErrorCode, Severity, ValidationCategory, ValidationError};
use crate::ast::{
    CellCard, DoseCard, DsCard, McnpInput, Particle, SiCard, SpSbCard, SurfaceCard, TallyKind,
};
use crate::CardLineSpans;
use std::collections::{HashMap, HashSet};

pub(super) struct CellFillRecord {
    pub cell_id: u32,
    pub line: Option<usize>,
    pub universe: Option<i32>,
    pub filled_universes: Vec<i32>,
}

pub(super) struct ValidationContext<'a> {
    pub input: &'a McnpInput,
    pub spans: &'a CardLineSpans,
    pub diagnostics: Vec<ValidationError>,
    pub cell_map: HashMap<u32, (usize, &'a CellCard)>,
    pub surface_map: HashMap<u32, (usize, &'a SurfaceCard)>,
    pub materials: HashMap<u32, usize>,
    pub mt_cards: HashMap<u32, usize>,
    pub transforms: HashMap<u32, usize>,
    pub tallies: HashMap<u32, (usize, TallyKind)>,
    pub fmeshes: HashMap<u32, usize>,
    pub si_cards: HashMap<u32, (usize, &'a SiCard)>,
    pub sp_cards: HashMap<u32, (usize, &'a SpSbCard)>,
    pub sb_cards: HashMap<u32, (usize, &'a SpSbCard)>,
    pub ds_cards: HashMap<u32, (usize, &'a DsCard)>,
    pub sc_cards: HashMap<u32, usize>,
    pub de_cards: HashMap<u32, (usize, &'a DoseCard)>,
    pub df_cards: HashMap<u32, (usize, &'a DoseCard)>,
    pub fm_cards: HashMap<u32, usize>,
    pub fs_cards: HashMap<u32, usize>,
    pub sd_cards: HashMap<u32, usize>,
    pub e_cards: HashMap<u32, usize>,
    pub t_cards: HashMap<u32, usize>,
    pub c_cards: HashMap<u32, usize>,
    pub fc_cards: HashMap<u32, usize>,
    pub mode_indices: Vec<usize>,
    pub sdef_indices: Vec<usize>,
    pub kcode_indices: Vec<usize>,
    pub ksrc_indices: Vec<usize>,
    pub nps_indices: Vec<usize>,
    pub active_particles: HashSet<Particle>,
    pub used_surfaces: HashSet<u32>,
    pub used_materials: HashSet<u32>,
    pub universe_cells: HashMap<i32, Vec<u32>>,
    pub cell_fills: Vec<CellFillRecord>,
    pub cell_has_imp: HashMap<Particle, HashSet<u32>>,
    pub cell_nonzero_imp: HashMap<Particle, bool>,
    pub referenced_dists: Vec<(u32, Option<usize>, String)>,
    pub vector_dist_ids: HashSet<u32>,
}

impl<'a> ValidationContext<'a> {
    pub fn new(input: &'a McnpInput, spans: &'a CardLineSpans) -> Self {
        Self {
            input,
            spans,
            diagnostics: Vec::new(),
            cell_map: HashMap::new(),
            surface_map: HashMap::new(),
            materials: HashMap::new(),
            mt_cards: HashMap::new(),
            transforms: HashMap::new(),
            tallies: HashMap::new(),
            fmeshes: HashMap::new(),
            si_cards: HashMap::new(),
            sp_cards: HashMap::new(),
            sb_cards: HashMap::new(),
            ds_cards: HashMap::new(),
            sc_cards: HashMap::new(),
            de_cards: HashMap::new(),
            df_cards: HashMap::new(),
            fm_cards: HashMap::new(),
            fs_cards: HashMap::new(),
            sd_cards: HashMap::new(),
            e_cards: HashMap::new(),
            t_cards: HashMap::new(),
            c_cards: HashMap::new(),
            fc_cards: HashMap::new(),
            mode_indices: Vec::new(),
            sdef_indices: Vec::new(),
            kcode_indices: Vec::new(),
            ksrc_indices: Vec::new(),
            nps_indices: Vec::new(),
            active_particles: HashSet::new(),
            used_surfaces: HashSet::new(),
            used_materials: HashSet::new(),
            universe_cells: HashMap::new(),
            cell_fills: Vec::new(),
            cell_has_imp: HashMap::new(),
            cell_nonzero_imp: HashMap::new(),
            referenced_dists: Vec::new(),
            vector_dist_ids: HashSet::new(),
        }
    }
}

pub(super) fn cell_line(spans: &CardLineSpans, idx: usize, id: u32) -> Option<usize> {
    spans
        .cell_card_lines
        .get(idx)
        .copied()
        .or_else(|| spans.cell_lines.get(&id).copied())
}

pub(super) fn surf_line(spans: &CardLineSpans, idx: usize, id: u32) -> Option<usize> {
    spans
        .surface_card_lines
        .get(idx)
        .copied()
        .or_else(|| spans.surface_lines.get(&id).copied())
}

pub(super) fn data_line(spans: &CardLineSpans, idx: usize) -> Option<usize> {
    spans.data_card_lines.get(idx).copied()
}

pub(super) fn check_particle_in_mode(
    particle: &Particle,
    active_particles: &HashSet<Particle>,
    line: Option<usize>,
    context: &str,
    diagnostics: &mut Vec<ValidationError>,
) {
    if !active_particles.is_empty() && !active_particles.contains(particle) {
        diagnostics.push(ValidationError {
            code: ErrorCode::ParticleNotInMode,
            severity: Severity::Warning,
            category: ValidationCategory::Mode,
            line_number: line,
            message: format!(
                "{} specifies particle `{}` which is not listed on the MODE card",
                context, particle.0
            ),
        });
    }
}

pub(super) fn is_strictly_increasing(vals: &[f64]) -> bool {
    vals.windows(2).all(|w| w[0] < w[1])
}
