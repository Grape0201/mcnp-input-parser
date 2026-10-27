use serde::{Deserialize, Serialize};

/// Metadata and content for an MCNP card/input template topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateTopic {
    pub name: &'static str,
    pub summary: &'static str,
    pub content: &'static str,
}

const TEMPLATES: &[TemplateTopic] = &[
    TemplateTopic {
        name: "skeleton-sdef",
        summary: "Minimal complete 3-block MCNP input for Fixed-Source / Shielding (SDEF)",
        content: "\
Fixed-Source Shielding Skeleton
c ======================================================================
c Block 1: Cell Cards (id mat density geometry params)
c - Blank line terminates Block 1 (DO NOT put blank lines inside a block)
c - Continuation line: 5+ leading spaces OR trailing '&'
c ======================================================================
1  0          -10      imp:n,p=1.0            $ Inner void source region
2  1 -7.87     10 -20  imp:n,p=1.0            $ Iron shield shell
3  2 -0.0012   20 -90  imp:n,p=1.0            $ Air region around shield
99 0           90      imp:n,p=0.0            $ Exterior graveyard (imp=0)

c ======================================================================
c Block 2: Surface Cards (id [tr] mnemonic params)
c - Blank line terminates Block 2
c ======================================================================
10 so  5.0                                    $ Inner sphere R=5 cm
20 so  25.0                                   $ Shield outer sphere R=25 cm
90 so  100.0                                  $ Problem boundary R=100 cm

c ======================================================================
c Block 3: Data Cards
c ======================================================================
mode n p
c --- Source Definition (14.1 MeV isotropic point neutron source) ---
sdef pos=0.0 0.0 0.0 erg=d1 par=n cel=1
si1  l 14.1
sp1  d 1.0
c --- Materials (negative = weight fraction, positive = atomic fraction) ---
m1   26056.80c 1.0                            $ Iron-56
m2   7014.80c -0.78 8016.80c -0.22            $ Dry air
c --- Tallies ---
fc4  Neutron track-length flux in shield and air
f4:n 2 3
e4   1.0-3 0.1 1.0 10.0 15.0
fc5  Point detector at r=50 cm along Z axis
f5:n 0.0 0.0 50.0 1.0
c --- Control ---
nps  100000
",
    },
    TemplateTopic {
        name: "skeleton-kcode",
        summary: "Minimal complete 3-block MCNP input for Criticality (KCODE / KSRC)",
        content: "\
Criticality Calculation Skeleton (KCODE)
c ======================================================================
c Block 1: Cell Cards
c ======================================================================
1  1 -18.74   -10      imp:n=1.0 tmp=2.53-8   $ Fissile core
2  2 -1.0      10 -20  imp:n=1.0 tmp=2.53-8   $ Water reflector
99 0           20      imp:n=0.0              $ Outside void

c ======================================================================
c Block 2: Surface Cards
c ======================================================================
10 so  8.74                                   $ Core radius [cm]
20 so  28.74                                  $ Reflector outer radius [cm]

c ======================================================================
c Block 3: Data Cards
c ======================================================================
mode n
c kcode <nsrck> <rkk> <ikz> <kct>
kcode 1000 1.0 20 120
ksrc  0.0 0.0 0.0
c --- Materials ---
m1   92235.80c -0.935 92238.80c -0.065        $ HEU metal
m2   1001.80c 2.0 8016.80c 1.0                $ Light water (atomic ratio 2:1)
mt2  lwtr.20t                                 $ Thermal scattering S(a,b) for M2
c --- Tallies ---
fc4  Core and reflector neutron flux
f4:n 1 2
e4   1.0-9 1.0-6 0.1 1.0 20.0
",
    },
    TemplateTopic {
        name: "surfaces",
        summary: "Standard surface mnemonics, required parameter counts, and sense rules",
        content: "\
# MCNP Standard Surfaces Reference

Format: `[*/+]id [tr] mnemonic params...`
- Prefix `*`: Reflective boundary condition
- Prefix `+`: White boundary condition
- `tr > 0`: Coordinate transformation ID (`TRn` must exist in Block 3)
- `tr < 0`: Periodic boundary paired with surface `|tr|` (valid ONLY on planes `P`, `PX`, `PY`, `PZ`, transmissive only)

| Mnemonic | Description | Param Count | Parameters | Equation / Sense (`-s` = inside/negative, `+s` = outside/positive) |
| :--- | :--- | :---: | :--- | :--- |
| `PX` | Plane normal to X | 1 | `D` | $x - D = 0$ (`-s`: $x < D$, `+s`: $x > D$) |
| `PY` | Plane normal to Y | 1 | `D` | $y - D = 0$ (`-s`: $y < D$, `+s`: $y > D$) |
| `PZ` | Plane normal to Z | 1 | `D` | $z - D = 0$ (`-s`: $z < D$, `+s`: $z > D$) |
| `P` | General plane | 4 or 9 | `A B C D` (or 3 pts `x1 y1 z1 ...`) | $Ax + By + Cz - D = 0$ (`A,B,C` not all zero) |
| `SO` | Sphere at origin | 1 | `R` ($R > 0$) | $x^2 + y^2 + z^2 - R^2 = 0$ (`-s`: inside sphere) |
| `S` | General sphere | 4 | `x0 y0 z0 R` ($R > 0$) | $(x-x_0)^2 + (y-y_0)^2 + (z-z_0)^2 - R^2 = 0$ |
| `SX` / `SY` / `SZ` | Sphere on X/Y/Z axis | 2 | `coord R` ($R > 0$) | Center on axis at `coord` |
| `CX` / `CY` / `CZ` | Cylinder on X/Y/Z axis | 1 | `R` ($R > 0$) | Infinite cylinder along axis (`-s`: $r < R$) |
| `C/X` / `C/Y` / `C/Z` | Cylinder parallel to axis | 3 | `c1 c2 R` ($R > 0$) | `C/X`: `y0 z0 R`; `C/Y`: `x0 z0 R`; `C/Z`: `x0 y0 R` |
| `KX` / `KY` / `KZ` | Cone on X/Y/Z axis | 2 or 3 | `coord t^2 [±1]` ($t^2 > 0$) | `t = tan(theta)` squared; optional 3rd param selects single sheet (`+1` or `-1`) |
| `K/X` / `K/Y` / `K/Z` | Cone parallel to axis | 4 or 5 | `x0 y0 z0 t^2 [±1]` ($t^2 > 0$) | Apex at `(x0, y0, z0)` |
| `SQ` | Axis-aligned quadric | 10 | `A B C D E F G x0 y0 z0` | $A(x-x_0)^2 + B(y-y_0)^2 + C(z-z_0)^2 + 2D(x-x_0) + 2E(y-y_0) + 2F(z-z_0) + G = 0$ |
| `GQ` | General quadric | 10 | `A B C D E F G H J K` | $Ax^2 + By^2 + Cz^2 + Dxy + Eyz + Fzx + Gx + Hy + Jz + K = 0$ |
| `TX` / `TY` / `TZ` | Elliptical torus | 6 | `x0 y0 z0 A B C` ($A,B,C > 0$) | Major radius $A$, minor semi-axes $B$ (parallel to axis) and $C$ (perpendicular) |
| `X` / `Y` / `Z` | Axisymmetric by points | 2, 4, or 6 | `coord1 r1 [coord2 r2 ...]` ($r_i \\ge 0$) | 1, 2, or 3 `(coord, radius)` pairs |
",
    },
    TemplateTopic {
        name: "macrobodies",
        summary: "Macrobody mnemonics, parameter order, and facet numbering (.1 .. .8)",
        content: "\
# MCNP Macrobodies & Facet Numbering Reference

Macrobodies define closed 3D solids (`-id` = inside the solid, `+id` = outside the solid).
Individual facets can be referenced in geometry expressions or surface tallies as `±id.facet` (e.g. `-10.1`, `+10.3`).

| Mnemonic | Description | Param Count | Parameters | Valid Facets (`id.f`) |
| :--- | :--- | :---: | :--- | :--- |
| `RPP` | Axis-aligned box | 6 | `xmin xmax ymin ymax zmin zmax` ($x_{\\min}<x_{\\max}$, etc.) | `.1` ($x_{\\max}$), `.2` ($x_{\\min}$), `.3` ($y_{\\max}$), `.4` ($y_{\\min}$), `.5` ($z_{\\max}$), `.6` ($z_{\\min}$) |
| `BOX` | Orthogonal box | 12 | `vx vy vz a1x a1y a1z a2x a2y a2z a3x a3y a3z` (corner $\\mathbf{v}$, 3 orthogonal edge vectors) | `.1`/`.2` (normal to $\\mathbf{a}_1$), `.3`/`.4` (normal to $\\mathbf{a}_2$), `.5`/`.6` (normal to $\\mathbf{a}_3$) |
| `SPH` | Sphere macrobody | 4 | `vx vy vz R` ($R > 0$) | `.1` (spherical surface) |
| `RCC` | Right circular cylinder | 7 | `vx vy vz hx hy hz R` (base center $\\mathbf{v}$, axis height vector $\\mathbf{h} \\ne 0$, $R > 0$) | `.1` (cylindrical side), `.2` (top plane at $\\mathbf{v}+\\mathbf{h}$), `.3` (bottom plane at $\\mathbf{v}$) |
| `TRC` | Truncated right cone | 8 | `vx vy vz hx hy hz R1 R2` ($\\mathbf{h} \\ne 0, R_1 > 0, R_2 > 0$) | `.1` (conical side), `.2` (top plane $R_2$), `.3` (bottom plane $R_1$) |
| `REC` | Right elliptical cylinder | 10 or 12 | `vx vy vz hx hy hz v1x v1y v1z v2x v2y v2z` (or 10 params with scalar minor radius) | `.1` (elliptical side), `.2` (top plane), `.3` (bottom plane) |
| `RHP` / `HEX` | Right hexagonal prism | 9 or 15 | `vx vy vz hx hy hz r1x r1y r1z [s1..s3 t1..t3]` | `.1`..`.6` (6 prism sides), `.7` (top plane), `.8` (bottom plane) |
| `ELL` | Ellipsoid | 7 | `v1x v1y v1z v2x v2y v2z Rm` ($R_m \\ne 0$) | `.1` (ellipsoidal surface) |
| `WED` | Right angle wedge | 12 | `vx vy vz v1x v1y v1z v2x v2y v2z v3x v3y v3z` | `.1` (slanted face), `.2`/`.3` (rectangular sides), `.4` (top), `.5` (bottom) |
| `ARB` | Arbitrary polyhedron | 30 | 8 corners `(x,y,z)` (24 floats) + 6 four-digit face indices | `.1`..`.6` (up to 6 faces) |

### Examples
```mcnp
10 rpp -50.0 50.0  -50.0 50.0  0.0 100.0      $ Box [-50,50]x[-50,50]x[0,100]
20 rcc  0.0 0.0 0.0   0.0 0.0 80.0   15.0     $ Cylinder from z=0 to z=80, R=15 cm
```
",
    },
    TemplateTopic {
        name: "cells",
        summary: "Cell cards, Boolean geometry operators, LIKE n BUT, Universes and Lattices",
        content: "\
# MCNP Cell Cards Reference (Block 1)

## 1. Normal Cell Syntax
`<id> <mat> [<density>] <geometry_expr> [<params...>]`
- Void cell: `<mat> = 0` (DO NOT specify `<density>` when `mat = 0`!)
- Material cell: `<mat> > 0` (`<density>` REQUIRED; `< 0` for mass density [g/cm3], `> 0` for atomic density [atoms/b-cm])
- Filled cell (`FILL=u`): MUST have `<mat> = 0` (unless modeling a homogeneous background, MCNP requires void for filled cells in standard validation).

## 2. Geometry Boolean Operators (Precedence: `#` > Intersection ` ` > Union `:`)
- `-10` : Negative sense (inside macrobody/sphere/cylinder or below plane)
- `10` or `+10` : Positive sense (outside macrobody/sphere/cylinder or above plane)
- `-10.1` : Negative side of facet 1 of macrobody 10
- `A B` (space) : Intersection ($A \\cap B$)
- `A : B` (colon) : Union ($A \\cup B$)
- `#20` : Complement of Cell 20 (Note: cannot complement a cell that has `FILL`)
- `#(10 -20)` : Complement of surface expression

## 3. `LIKE n BUT` Syntax
`<id> LIKE <base_cell_id> BUT <key=val ...>`
- Base cell `<base_cell_id>` must be a Normal cell (cannot chain `LIKE` onto another `LIKE` cell).
- Common overrides: `MAT=2 RHO=-2.7 TRCL=1 IMP:n=1.0 U=2`

## 4. Universes & Lattices (`U`, `LAT`, `FILL`)
```mcnp
c --- Pin cell universe U=1 ---
10 1 -10.4  -1      u=1 imp:n=1.0             $ Fuel pin
11 2 -1.0    1      u=1 imp:n=1.0             $ Moderator around pin
c --- Rectangular lattice U=2 (LAT=1: hexahedral, LAT=2: hexagonal) ---
20 0        -10 11 -12 13 -14 15  u=2 lat=1 imp:n=1.0
     fill=-1:1 -1:1 0:0
          1 1 1
          1 1 1
          1 1 1
c --- Container cell filled with Universe 2 ---
30 0        -100    fill=2 imp:n=1.0
```
",
    },
    TemplateTopic {
        name: "source",
        summary: "Fixed-Source cards (SDEF, SI, SP, SB, DS, SC) and built-in spectra",
        content: "\
# MCNP Source Definition Reference (`SDEF`, `SI`, `SP`, `SB`, `DS`)

## Rules
1. Either `SDEF` (fixed source) or `KCODE` (criticality) must be present, never both.
2. Every distribution `Dn` referenced on `SDEF`, `SI`, or `DS` must have a matching `SIn`/`SPn` pair (or `DSn`), except when `SPn` uses a standalone analytic function (`-2`, `-3`, `-4`, `-5`, `-6`, `-7`) for `ERG` or `DIR` without `SIn`.
3. Length matching:
   - `SIn H b0 b1 ... bk` (Histogram, default: $k+1$ bin edges) requires `SPn D 0.0 p1 ... pk` ($k+1$ entries, first entry MUST be `0.0`).
   - `SIn L v1 v2 ... vk` (Discrete values: $k$ entries) requires `SPn D p1 p2 ... pk` ($k$ entries).
   - `SIn A x1 x2 ... xk` (Arbitrary PDF points: $k$ entries) requires `SPn D f1 f2 ... fk` ($k$ entries).

## Common SDEF Patterns

### 1. Isotropic Point Source with Discrete Lines (`L`)
```mcnp
sdef pos=0.0 0.0 0.0 erg=d1 par=p
si1  l  1.1732 1.3325                         $ Co-60 gamma lines [MeV]
sp1  d  0.5    0.5
```

### 2. Uniform Spherical Volume Source with Histogram Spectrum (`H`)
```mcnp
sdef pos=0.0 0.0 0.0 rad=d1 erg=d2 par=n
si1  0.0 10.0                                 $ Radius 0 to 10 cm
sp1  -21 2                                    $ Power law r^a with a=2 (uniform in sphere)
si2  h  1.0-3 0.1 1.0 10.0                    $ 3 histogram energy bins
sp2  d  0.0   0.2 0.5 0.3                     $ Leading 0.0 + 3 bin probabilities
```

### 3. Cylindrical Volume Source with Watt Fission Spectrum (`-3`)
```mcnp
sdef pos=0.0 0.0 0.0 axs=0.0 0.0 1.0 rad=d1 ext=d2 erg=d3 par=n
si1  0.0 15.0                                 $ Cylinder radius 0..15 cm
sp1  -21 1                                    $ Power law r^1 (uniform in disk)
si2  h -30.0 30.0                             $ Axial extent -30..+30 cm
sp2  d  0.0  1.0
sp3  -3 0.988 2.249                           $ U-235 thermal Watt spectrum (no SI3 needed)
```
",
    },
    TemplateTopic {
        name: "tallies",
        summary: "Tallies (F1..F8, F5), energy bins (E), dose conversion (DE/DF), multipliers (FM), FMESH",
        content: "\
# MCNP Tallies & Modifiers Reference

## 1. Standard Tally Types (`F<n>:<p>`)
The last digit of `<n>` (`n % 10`) determines the tally type and target kind:
- `F1` (Surface current) & `F2` (Surface flux): Targets MUST be defined **Surface IDs**.
- `F4` (Cell flux), `F6` (Energy deposition), `F7` (Fission energy), `F8` (Pulse height): Targets MUST be defined **Cell IDs**.
- `F5` / `FX5` / `FY5` / `FZ5` (Point / Ring detector): Targets are `(x y z R0)` or `(axis_coord radius R0)` tuples ($R_0 \\ge 0$).

## 2. Tally Modifiers (ID `<n>` must match a defined `F<n>` tally, or `0` for global `DE0`/`DF0`)
```mcnp
fc4  Neutron dose rate in Cell 2 and Cell 3
f4:n 2 3 (2 3) t                              $ Individual cells, union group (2 3), and total 't'
e4   1.0-9 1.0-6 1.0-3 0.1 1.0 10.0 20.0      $ Monotonically increasing energy bins [MeV]
de4  log 1.0-9 1.0-3 0.1 1.0 10.0 20.0        $ Dose energy table (must match DF4 length)
df4  lin 3.7-6 4.0-6 2.5-5 1.3-4 1.5-4 2.0-4  $ Dose conversion factors
fm4  (1.0 1 (102) (-6))                       $ Multiplier: C=1.0, Mat M1, reactions (n,gamma) & fission
fs4  -10 -20 t                                $ Segment tally by surfaces 10 and 20
sd4  100.0 200.0 300.0                        $ Segment volumes/areas
```

## 3. Superimposed Mesh Tally (`FMESH<n>:<p>`)
- `<n>` should end in `4` (e.g. `FMESH4:n`, `FMESH14:p`).
- Number of intervals in `IINTS`/`JINTS`/`KINTS`/`EINTS`/`TINTS` must match the number of coarse mesh points in `IMESH`/`JMESH`/`KMESH`/`EMESH`/`TMESH` (when `ORIGIN` provides the lower bound).
```mcnp
fmesh14:n geom=xyz origin=-50.0 -50.0 -50.0
     imesh=0.0 50.0 iints=10 10
     jmesh=50.0     jints=20
     kmesh=50.0     kints=20
     emesh=1.0 20.0 eints=1 1
     out=col
```
",
    },
    TemplateTopic {
        name: "variance-reduction",
        summary: "Importance (IMP) and Weight Window (WWE, WWN, WWP, WWG, MESH) cards",
        content: "\
# MCNP Variance Reduction Reference

## 1. Cell Importance (`IMP:<p>`)
- Every cell MUST have an importance defined for every particle in `MODE` (either inline on the cell card `imp:n=1.0` or via a Block 3 `imp:n ...` card).
- When using a Block 3 `IMP:<p>` card, the total number of expanded entries MUST equal `cells.len()`.
- At least one cell must have `IMP > 0`, and the exterior boundary cell must have `IMP = 0`.

## 2. Cell-Based Weight Windows (`WWE`, `WWN<g>`, `WWP`)
- For each energy/time interval group `g = 1, 2, ...`, `WWN<g>:<p>` must contain exactly `cells.len()` entries (`-1` = kill particle, `0` = use cell importance, `> 0` = lower weight bound).
```mcnp
wwe:n  1.0-3 1.0 20.0                         $ 3 energy intervals (g=1, 2, 3)
wwn1:n 0.5 0.25 0.1 0.05 -1                   $ Group 1 lower bounds (1 per cell)
wwn2:n 0.5 0.20 0.05 0.01 -1                  $ Group 2 lower bounds (1 per cell)
wwp:n  5.0 3.0 5.0 0 -1                       $ wupn=5, wsurvn=3, mxspln=5, mwhere=0(cell), switchn=-1
```

## 3. Weight Window Generator (`WWG`, `WWGE`, `MESH`)
- `WWG <tally_id> <reference_cell_id> <target_lower_bound>`
  - `<tally_id>` must reference a defined `F<n>` tally.
  - `<reference_cell_id>` must reference a defined Cell ID.
```mcnp
wwg    4 1 1.0
wwge:n 1.0-3 1.0 20.0
```
",
    },
];

/// Returns all available template topics.
pub fn list_templates() -> &'static [TemplateTopic] {
    TEMPLATES
}

/// Looks up a template topic by name (case-insensitive, supports prefix/alias matching).
pub fn get_template(topic: &str) -> Option<&'static TemplateTopic> {
    let q = topic.trim().to_lowercase();
    if let Some(exact) = TEMPLATES.iter().find(|t| t.name == q) {
        return Some(exact);
    }
    // Common aliases
    let canonical = match q.as_str() {
        "sdef" | "fixed-source" | "shielding" => "skeleton-sdef",
        "kcode" | "criticality" | "crit" => "skeleton-kcode",
        "surface" | "surf" => "surfaces",
        "macrobody" | "macro" | "facet" | "facets" => "macrobodies",
        "cell" | "geometry" | "lattice" | "universe" => "cells",
        "src" | "distribution" | "si" | "sp" => "source",
        "tally" | "fmesh" | "dose" | "fm" => "tallies",
        "vr" | "ww" | "weight-window" | "imp" | "importance" => "variance-reduction",
        _ => return None,
    };
    TEMPLATES.iter().find(|t| t.name == canonical)
}
