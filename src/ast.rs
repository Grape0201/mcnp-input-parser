use serde::{Deserialize, Serialize};

/// Complete AST of an MCNP input file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McnpInput {
    /// Optional message block at the very beginning of the file (`message:` ... blank line).
    pub message: Option<String>,
    /// One-line problem title card.
    pub title: String,
    /// Block 1: Cell cards.
    pub cells: Vec<CellCard>,
    /// Block 2: Surface cards.
    pub surfaces: Vec<SurfaceCard>,
    /// Block 3: Data cards.
    pub data_cards: Vec<DataCard>,
}

/// A value in a numeric array that may be either a concrete float or a jump (`J`) default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Entry {
    Value(f64),
    Jump,
}

impl Entry {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Entry::Value(v) => Some(*v),
            Entry::Jump => None,
        }
    }
}

/// Particle designator (e.g. `n`, `p`, `e`, `h`, `a`, `q`, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Particle(pub String);

impl Particle {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into().to_lowercase())
    }
}

// ============================================================================
// Block 1: Cell Cards
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CellCard {
    pub id: u32,
    pub definition: CellDefinition,
    pub params: Vec<CellParam>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CellDefinition {
    Normal {
        /// Material number (`0` for void).
        material_id: u32,
        /// Material density (`None` if `material_id == 0`, positive = atomic density [atoms/b-cm], negative = mass density [g/cm3]).
        density: Option<f64>,
        /// Boolean geometry expression of surfaces and cells.
        geometry: GeometryExpr,
    },
    LikeBut {
        /// Reference cell ID in `LIKE n BUT ...`.
        base_cell_id: u32,
    },
}

/// Boolean geometry expression in a cell card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GeometryExpr {
    /// Signed surface number (e.g. `-10` for negative sense, `+10` or `10` for positive sense).
    Surface(i32),
    /// Signed macrobody facet (e.g. `-10.1` -> `surface: -10, facet: 1`).
    Facet { surface: i32, facet: u32 },
    /// Complement of another cell (`#20`).
    ComplementCell(u32),
    /// Complement of a parenthesized sub-expression (`#( ... )`).
    Complement(Box<GeometryExpr>),
    /// Intersection of expressions (whitespace-separated).
    Intersection(Vec<GeometryExpr>),
    /// Union of expressions (`:`-separated).
    Union(Vec<GeometryExpr>),
}

/// Cell parameters specified either on a cell card or in a `LIKE n BUT` clause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CellParam {
    /// `IMP:n,p=1.0`
    Importance {
        particles: Vec<Particle>,
        value: f64,
    },
    /// `VOL=10.5`
    Volume(f64),
    /// `U=1` (Universe ID)
    Universe(i32),
    /// `FILL=...` (Simple universe ID or array fill tokens)
    Fill {
        transform_is_degrees: bool,
        values: Vec<String>,
    },
    /// `LAT=1` or `LAT=2`
    Lattice(u32),
    /// `TMP=2.53e-8`
    Temperature(Vec<Entry>),
    /// `RHO=-1.0` (used in `LIKE n BUT`)
    Density(f64),
    /// `MAT=2` (used in `LIKE n BUT`)
    Material(u32),
    /// `TRCL=...` or `*TRCL=...`
    Transformation {
        is_degrees: bool,
        values: Vec<Entry>,
    },
    /// Generic fallback for any other cell parameter (`KEY[:p]=VALUES`).
    Other {
        key: String,
        particles: Vec<Particle>,
        values: Vec<String>,
    },
}

// ============================================================================
// Block 2: Surface Cards
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundaryCondition {
    /// Default transmissive boundary.
    Transmissive,
    /// `*` prefix: Reflective boundary.
    Reflective,
    /// `+` prefix: White boundary.
    White,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceCard {
    pub id: u32,
    pub boundary: BoundaryCondition,
    /// Optional coordinate transformation ID (`TRn`) or periodic boundary negative surface ID.
    pub transform_id: Option<i32>,
    pub kind: SurfaceKind,
    pub params: Vec<f64>,
}

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceKind {
    // Planes
    P,
    PX,
    PY,
    PZ,
    // Spheres
    SO,
    S,
    SX,
    SY,
    SZ,
    // Cylinders
    C_X,
    C_Y,
    C_Z,
    CX,
    CY,
    CZ,
    // Cones
    K_X,
    K_Y,
    K_Z,
    KX,
    KY,
    KZ,
    // Quadrics & Tori
    SQ,
    GQ,
    TX,
    TY,
    TZ,
    // Axisymmetric points-defined surfaces
    X,
    Y,
    Z,
    // Macrobodies
    BOX,
    RPP,
    SPH,
    RCC,
    RHP,
    HEX,
    REC,
    TRC,
    ELL,
    WED,
    ARB,
    /// Fallback for custom/unrecognized surface mnemonics.
    Custom(String),
}

impl SurfaceKind {
    pub fn from_mnemonic(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "P" => Self::P,
            "PX" => Self::PX,
            "PY" => Self::PY,
            "PZ" => Self::PZ,
            "SO" => Self::SO,
            "S" => Self::S,
            "SX" => Self::SX,
            "SY" => Self::SY,
            "SZ" => Self::SZ,
            "C/X" => Self::C_X,
            "C/Y" => Self::C_Y,
            "C/Z" => Self::C_Z,
            "CX" => Self::CX,
            "CY" => Self::CY,
            "CZ" => Self::CZ,
            "K/X" => Self::K_X,
            "K/Y" => Self::K_Y,
            "K/Z" => Self::K_Z,
            "KX" => Self::KX,
            "KY" => Self::KY,
            "KZ" => Self::KZ,
            "SQ" => Self::SQ,
            "GQ" => Self::GQ,
            "TX" => Self::TX,
            "TY" => Self::TY,
            "TZ" => Self::TZ,
            "X" => Self::X,
            "Y" => Self::Y,
            "Z" => Self::Z,
            "BOX" => Self::BOX,
            "RPP" => Self::RPP,
            "SPH" => Self::SPH,
            "RCC" => Self::RCC,
            "RHP" => Self::RHP,
            "HEX" => Self::HEX,
            "REC" => Self::REC,
            "TRC" => Self::TRC,
            "ELL" => Self::ELL,
            "WED" => Self::WED,
            "ARB" => Self::ARB,
            other => Self::Custom(other.to_string()),
        }
    }
}

// ============================================================================
// Block 3: Source Definition AST (SDEF, SI, SP, SB, DS, SC)
// ============================================================================

/// Known SDEF parameter variables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SdefVariable {
    Cel,
    Sur,
    Erg,
    Tme,
    Dir,
    Vec,
    Nrm,
    Pos,
    Rad,
    Ext,
    Axs,
    X,
    Y,
    Z,
    Ccc,
    Ara,
    Wgt,
    Tr,
    Eff,
    Par,
    Dat,
    Loc,
    Bem,
    Bap,
    Custom(String),
}

impl SdefVariable {
    pub fn from_name(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "cel" => Self::Cel,
            "sur" => Self::Sur,
            "erg" => Self::Erg,
            "tme" => Self::Tme,
            "dir" => Self::Dir,
            "vec" => Self::Vec,
            "nrm" => Self::Nrm,
            "pos" => Self::Pos,
            "rad" => Self::Rad,
            "ext" => Self::Ext,
            "axs" => Self::Axs,
            "x" => Self::X,
            "y" => Self::Y,
            "z" => Self::Z,
            "ccc" => Self::Ccc,
            "ara" => Self::Ara,
            "wgt" => Self::Wgt,
            "tr" => Self::Tr,
            "eff" => Self::Eff,
            "par" => Self::Par,
            "dat" => Self::Dat,
            "loc" => Self::Loc,
            "bem" => Self::Bem,
            "bap" => Self::Bap,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// Typed value of an `SDEF` parameter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SdefValue {
    /// Single constant number (e.g. `ERG=14.1`, `WGT=1.0`, `CEL=10`).
    Scalar(f64),
    /// 3D vector (e.g. `POS=0 0 0`, `AXS=0 0 1`, `VEC=1 0 0`).
    Vector([f64; 3]),
    /// Reference to distribution `Dn` (e.g. `ERG=D1`, `RAD=d2`).
    Distribution(u32),
    /// Dependent distribution function `F<var> D<n>` (e.g. `ERG=FPOS D2`, `POS=FCEL D10`).
    Function {
        independent_var: SdefVariable,
        dist_id: u32,
    },
    /// Numeric list (e.g. `DAT=1 20 2026`).
    List(Vec<Entry>),
    /// Symbolic value such as particle type or cell path (e.g. `PAR=n`, `CEL=1:2:3`).
    Symbol(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SdefParam {
    pub variable: SdefVariable,
    pub value: SdefValue,
}

/// Option on an `SI<n>` card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiOption {
    /// `H`: Histogram bin boundaries (default).
    Histogram,
    /// `L`: Discrete values.
    Discrete,
    /// `A`: Points where a probability density distribution is defined.
    ArbitraryPdf,
    /// `S`: Distribution numbers for second-level sampling.
    DistributionList,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SiEntry {
    Value(f64),
    Jump,
    Distribution(u32),
    Symbol(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SiCard {
    pub id: u32,
    pub option: SiOption,
    pub entries: Vec<SiEntry>,
}

/// Option on an `SP<n>` or `SB<n>` card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpOption {
    /// `D`: Bin probabilities for `H`/`L`/`S` or density values for `A` (default).
    Default,
    /// `C`: Cumulative probabilities.
    Cumulative,
    /// `V`: Cell volume proportional probabilities.
    Volume,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SpSbContent {
    /// Explicit probability or bias array.
    Probabilities {
        option: SpOption,
        values: Vec<Entry>,
    },
    /// Built-in analytic source distribution function (`-2` Maxwell, `-3` Watt, `-21` power law, etc.).
    AnalyticFunction {
        function_id: i32,
        params: Vec<Entry>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpSbCard {
    pub id: u32,
    /// `false` for `SPn`, `true` for `SBn`.
    pub is_bias: bool,
    pub content: SpSbContent,
}

/// Option on a `DS<n>` Dependent Source card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DsOption {
    /// `H`: Continuous bin values (default).
    Histogram,
    /// `L`: Discrete values.
    Discrete,
    /// `S`: Dependent distribution numbers.
    DistributionList,
    /// `T`: Pairs of `(independent_value, dependent_dist_id)`.
    ValueDistPairs,
    /// `Q`: Pairs of `(upper_bound_value, dependent_dist_id)`.
    IntervalDistPairs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DsCard {
    pub id: u32,
    pub option: DsOption,
    pub entries: Vec<SiEntry>,
    pub pairs: Vec<(f64, u32)>,
}

// ============================================================================
// Block 3: Tally AST (F, F5, DE, DF, FM, FS, SD, E, T, C, FC, FMESH)
// ============================================================================

/// MCNP standard tally type determined by `id % 10`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TallyKind {
    /// `F1`: Surface current.
    F1SurfaceCurrent,
    /// `F2`: Surface flux.
    F2SurfaceFlux,
    /// `F4`: Track-length estimate of cell flux.
    F4CellFlux,
    /// `F5` / `FX5` / `FY5` / `FZ5`: Point or ring detector flux.
    F5DetectorFlux,
    /// `F6`: Track-length estimate of energy deposition.
    F6EnergyDeposition,
    /// `F7`: Track-length estimate of fission energy deposition.
    F7FissionEnergyDeposition,
    /// `F8`: Pulse height tally.
    F8PulseHeight,
    /// Other/custom tally digit.
    Other(u32),
}

impl TallyKind {
    pub fn from_id(id: u32) -> Self {
        match id % 10 {
            1 => Self::F1SurfaceCurrent,
            2 => Self::F2SurfaceFlux,
            4 => Self::F4CellFlux,
            5 => Self::F5DetectorFlux,
            6 => Self::F6EnergyDeposition,
            7 => Self::F7FissionEnergyDeposition,
            8 => Self::F8PulseHeight,
            digit => Self::Other(digit),
        }
    }
}

/// Target bin in a cell or surface tally (`F1`, `F2`, `F4`, `F6`, `F7`, `F8`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TallyTarget {
    /// Single cell or surface ID (e.g. `10`).
    Single(u32),
    /// Union group of cells or surfaces (e.g. `(10 11 12)`).
    Group(Vec<u32>),
    /// Complex lattice/universe expression (e.g. `(1 < 2[0:1 0:1 0])`).
    ComplexGroup(String),
}

/// Detector entry on an `F5`, `FX5`, `FY5`, or `FZ5` tally card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DetectorEntry {
    /// Point detector: `(x, y, z, r0)`.
    Point { x: f64, y: f64, z: f64, r0: f64 },
    /// Ring detector along X, Y, or Z axis: `(axis, axis_coord, radius, r0)`.
    Ring {
        axis: char,
        axis_coord: f64,
        radius: f64,
        r0: f64,
    },
}

/// Interpolation mode on `DE` / `DF` dose conversion cards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InterpMode {
    Log,
    Lin,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoseCard {
    /// Tally ID (`0` if `DE0` / `DF0` global default).
    pub id: u32,
    /// `false` for `DEn`, `true` for `DFn`.
    pub is_function: bool,
    pub interpolation: Option<InterpMode>,
    pub values: Vec<f64>,
    /// Optional keyword arguments on `DFn` (e.g. `IC=99`, `IU=2`, `FAC=-3`).
    pub options: Vec<(String, String)>,
}

/// Multiplier bin on an `FM<n>` card.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FmBin {
    /// Constant multiplier only (`C`).
    Constant(f64),
    /// Standard multiplier bin `(C m (rx_list) (attenuator))` or unparenthesized `C m ...`.
    MultiplierSet {
        constant: f64,
        material_id: i32,
        reaction_groups: Vec<String>,
    },
    /// Fallback raw parenthesized group.
    RawGroup(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmCard {
    pub id: u32,
    pub modifier: Option<char>,
    pub bins: Vec<FmBin>,
}

/// Superimposed mesh tally card (`FMESH<n>:<p>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FmeshCard {
    pub id: u32,
    pub particles: Vec<Particle>,
    pub geom: Option<String>,
    pub origin: Option<[f64; 3]>,
    pub axs: Option<[f64; 3]>,
    pub vec: Option<[f64; 3]>,
    pub imesh: Vec<f64>,
    pub iints: Vec<u32>,
    pub jmesh: Vec<f64>,
    pub jints: Vec<u32>,
    pub kmesh: Vec<f64>,
    pub kints: Vec<u32>,
    pub emesh: Vec<f64>,
    pub eints: Vec<u32>,
    pub tmesh: Vec<f64>,
    pub tints: Vec<u32>,
    pub factor: Option<f64>,
    pub out: Option<String>,
    pub extra_options: Vec<(String, Vec<String>)>,
}

// ============================================================================
// Block 3: Data Cards
// ============================================================================

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NuclideFraction {
    /// ZAID identifier (e.g. `"1001.80c"`, `"92235"`).
    pub zaid: String,
    /// Atomic fraction (positive) or weight fraction (negative).
    pub fraction: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataCard {
    // --- Materials & Mode ---
    /// Material card (`m1 1001.80c 2.0 8016.80c 1.0 gas=0 ...`)
    Material {
        id: u32,
        nuclides: Vec<NuclideFraction>,
        options: Vec<(String, String)>,
    },
    /// Thermal scattering $S(\alpha, \beta)$ card (`mt1 lwtr.10t ...`)
    ThermalScattering {
        id: u32,
        tables: Vec<String>,
    },
    /// Particle transport mode (`mode n p e`)
    Mode {
        particles: Vec<Particle>,
    },

    // --- Fixed Source & Distributions (Shielding) ---
    /// General source definition (`sdef pos=0 0 0 erg=d1 par=n ...`)
    Sdef {
        params: Vec<SdefParam>,
    },
    /// Source information card (`si1 h 0 1 5 10`)
    Si(SiCard),
    /// Source probability or bias card (`sp1 d ...` or `sb1 ...`)
    SpSb(SpSbCard),
    /// Dependent source distribution card (`ds1 s 10 11`)
    Ds(DsCard),
    /// Source comment card (`sc1 Fission neutron spectrum`)
    Sc {
        id: u32,
        comment: String,
    },

    // --- Tallies ---
    /// Standard cell/surface/pulse-height tally (`f1`, `f2`, `f4`, `f6`, `f7`, `f8`)
    Tally {
        id: u32,
        kind: TallyKind,
        particles: Vec<Particle>,
        modifier: Option<char>,
        targets: Vec<TallyTarget>,
        total_bin: bool,
        no_direct: bool,
    },
    /// Detector tally (`f5`, `fx5`, `fy5`, `fz5`)
    DetectorTally {
        id: u32,
        axis: Option<char>, // None for Point ('f5'), Some('x'|'y'|'z') for Ring ('fx5'|'fy5'|'fz5')
        particles: Vec<Particle>,
        modifier: Option<char>,
        detectors: Vec<DetectorEntry>,
        no_direct: bool,
    },
    /// Dose energy / Dose function card (`de4`, `df4`)
    Dose(DoseCard),
    /// Tally multiplier card (`fm4`)
    TallyMultiplier(FmCard),
    /// Tally segment card (`fs4 -10 -20 t`)
    TallySegment {
        id: u32,
        surfaces: Vec<i32>,
        total_bin: bool,
    },
    /// Segment divisor card (`sd4 100.0 200.0`)
    SegmentDivisor {
        id: u32,
        values: Vec<Entry>,
    },
    /// Tally energy bins (`e4 1e-3 10i 10.0 nt c`)
    TallyEnergy {
        id: u32,
        modifier: Option<char>,
        bins: Vec<Entry>,
        no_total: bool,
        cumulative: bool,
    },
    /// Tally time bins (`t4 0 10i 100 nt c`)
    TallyTime {
        id: u32,
        bins: Vec<Entry>,
        no_total: bool,
        cumulative: bool,
    },
    /// Tally cosine/angle bins (`c4 -1 0 1 t c`)
    TallyCosine {
        id: u32,
        bins: Vec<Entry>,
        total_bin: bool,
        cumulative: bool,
    },
    /// Tally comment card (`fc4 Neutron dose rate [rem/hr]`)
    TallyComment {
        id: u32,
        comment: String,
    },
    /// Superimposed mesh tally (`fmesh4:n geom=xyz ...`)
    Fmesh(FmeshCard),

    // --- Variance Reduction (Importance & Weight Windows) ---
    /// Cell importance data card (`imp:n 1 3r 0`)
    Importance {
        particles: Vec<Particle>,
        values: Vec<Entry>,
    },
    /// Weight window energy/time intervals (`wwe:n 1e-3 1.0 20.0`)
    WeightWindowEnergy {
        particles: Vec<Particle>,
        bounds: Vec<Entry>,
    },
    /// Cell/mesh weight window lower bounds (`wwn1:n 0.5 3r -1`)
    WeightWindowBounds {
        group_id: u32,
        particles: Vec<Particle>,
        lower_bounds: Vec<Entry>,
    },
    /// Weight window parameters (`wwp:n wupn wsurvn mxspln mwhere switchn mtime vn etsplit wu`)
    WeightWindowParams {
        particles: Vec<Particle>,
        params: Vec<Entry>,
    },
    /// Weight window generator (`wwg itally icell wlower ...`)
    WeightWindowGenerator {
        tally_id: u32,
        reference_cell: u32,
        target_lower_bound: f64,
        extra: Vec<Entry>,
    },
    /// Weight window generator energy/time bins (`wwge:n 1e-3 1.0 20.0`)
    WeightWindowGeneratorEnergy {
        particles: Vec<Particle>,
        bounds: Vec<Entry>,
    },
    /// Superimposed weight window mesh (`mesh geom=xyz ref=0 0 0 ...`)
    WeightWindowMesh {
        params: Vec<(String, Vec<String>)>,
    },

    // --- Physics, Cutoffs, Transformations & Control ---
    /// Physics card (`phys:n emax emcnf iunr ...`)
    Physics {
        particles: Vec<Particle>,
        values: Vec<Entry>,
    },
    /// Cutoff card (`cut:n tco elpt wc1 wc2 swtm`)
    Cutoff {
        particles: Vec<Particle>,
        tco: Option<f64>,
        elpt: Option<f64>,
        wc1: Option<f64>,
        wc2: Option<f64>,
        swtm: Option<f64>,
    },
    /// Coordinate transformation card (`tr1 ...` or `*tr1 ...`)
    Transform {
        id: u32,
        is_degrees: bool,
        values: Vec<Entry>,
    },
    /// History cutoff (`nps 100000`)
    Nps {
        count: u64,
        mgopt: Option<u64>,
    },

    // --- Criticality ---
    /// Criticality calculation card (`kcode nsrck rkk ikz kct msrk knrm mrkp kc8`)
    Kcode {
        nsrck: Option<u32>,
        rkk: Option<f64>,
        ikz: Option<u32>,
        kct: Option<u32>,
        msrk: Option<u32>,
        knrm: Option<u32>,
        mrkp: Option<u32>,
        kc8: Option<u32>,
    },
    /// Initial fission source points (`ksrc x1 y1 z1 x2 y2 z2 ...`)
    Ksrc {
        points: Vec<[f64; 3]>,
    },

    // --- Fallback ---
    /// Generic fallback for any other data card.
    Generic {
        name: String,
        particles: Vec<Particle>,
        tokens: Vec<String>,
    },
}
