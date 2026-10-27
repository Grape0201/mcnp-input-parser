# MCNP Input File Syntax & Pitfalls Cheatsheet

This concise reference summarizes the exact formatting rules and common pitfalls that cause MCNP input files to fail parsing or validation.

---

## 1. File Structure & Line Rules (Block 1 / Block 2 / Block 3)

1. **Optional Message Block**: Starts at line 1 with `MESSAGE:` (columns 1–5) and ends at the first blank line.
2. **Title Card**: Single line immediately preceding Block 1 (even a comment-like line is treated as the Title if it is the first line!).
3. **3 Blocks Separated by Single Blank Lines**:
   - **Block 1**: Cell Cards (`<id> <mat> [<density>] <geom> [<params>]`)
   - *(Exactly one blank line)*
   - **Block 2**: Surface Cards (`[*/+]<id> [<tr>] <mnemonic> <params>`)
   - *(Exactly one blank line)*
   - **Block 3**: Data Cards (`mode`, `sdef`/`kcode`, `m<n>`, `f<n>`, `imp`, `nps`, etc.)
4. **NEVER Put Blank Lines Inside a Block**:
   - A blank line **always** terminates the current block in MCNP!
   - To visually separate sections within a block, use `c` comment lines (`c --- Section ---`), never blank lines.
5. **Comments**:
   - Full-line comment: `c` or `C` in columns 1–5 followed by a space (or end-of-line).
   - Inline comment: `$` anywhere on a line strips everything after `$`. Always put `$` before trailing descriptions on cards (e.g., `99 0 90 imp:n=0 $ Exterior void`).
6. **Continuation Lines**:
   - Either indent the continuation line with **at least 5 leading spaces**, OR end the previous line with `&`.

---

## 2. Block 1: Cell Card Rules

- **Void Cells (`mat = 0`)**:
  - Format: `<id> 0 <geom_expr> <params...>`
  - **Do NOT provide a density value** when `mat = 0` (`E407`).
- **Material Cells (`mat > 0`)**:
  - Format: `<id> <mat> <density> <geom_expr> <params...>`
  - `<density>` is mandatory and non-zero (`E401`):
    - **Negative (`< 0`)**: Mass density in $\text{g/cm}^3$ (e.g. `-7.87` for iron, `-1.0` for water).
    - **Positive (`> 0`)**: Atomic density in $\text{atoms/(b}\cdot\text{cm)}$ (`10^24 atoms/cm3`).
- **Filled Cells (`FILL=u`)**:
  - Cells with `FILL=...` must be Void (`mat = 0`, no density) (`E408`).
  - Never apply the cell complement operator `#n` to a cell `n` that has `FILL` (`E410`).
- **Cell Importance (`IMP:<p>`)**:
  - Every cell must have importance defined for every particle in `MODE` (`E404`).
  - At least one cell must have `IMP > 0`, and the exterior world/graveyard cell must have `IMP = 0`.

---

## 3. Block 2: Surface & Macrobody Quick Table

### Standard Surfaces
- `PX D`, `PY D`, `PZ D` (1 param)
- `P A B C D` (4 params) or 9 params (3 points)
- `SO R` (1 param, $R > 0$)
- `S x0 y0 z0 R` (4 params, $R > 0$)
- `SX d R`, `SY d R`, `SZ d R` (2 params, $R > 0$)
- `CX R`, `CY R`, `CZ R` (1 param, $R > 0$)
- `C/X y0 z0 R`, `C/Y x0 z0 R`, `C/Z x0 y0 R` (3 params, $R > 0$)
- `KX d t2 [±1]`, `KY d t2 [±1]`, `KZ d t2 [±1]` (2 or 3 params, $t^2 > 0$)
- `K/X x0 y0 z0 t2 [±1]`, `K/Y ...`, `K/Z ...` (4 or 5 params, $t^2 > 0$)
- `TX`, `TY`, `TZ` `x0 y0 z0 A B C` (6 params, $A, B, C > 0$)

### Macrobodies & Facet Indices (`±id.facet`)
- `RPP xmin xmax ymin ymax zmin zmax` (6 params: $x_{\min}<x_{\max}, y_{\min}<y_{\max}, z_{\min}<z_{\max}$)
  - Facets: `.1` ($x_{\max}$), `.2` ($x_{\min}$), `.3` ($y_{\max}$), `.4` ($y_{\min}$), `.5` ($z_{\max}$), `.6` ($z_{\min}$)
- `RCC vx vy vz hx hy hz R` (7 params: base center $\mathbf{v}$, axis vector $\mathbf{h} \ne \mathbf{0}$, $R > 0$)
  - Facets: `.1` (cylindrical side), `.2` (top plane at $\mathbf{v}+\mathbf{h}$), `.3` (bottom plane at $\mathbf{v}$)
- `SPH vx vy vz R` (4 params: $R > 0$, facet `.1`)
- `BOX vx vy vz a1x a1y a1z a2x a2y a2z a3x a3y a3z` (12 params, facets `.1`..`.6`)
- `TRC vx vy vz hx hy hz R1 R2` (8 params: $R_1 > 0, R_2 > 0$, facets `.1`..`.3`)
- `RHP` / `HEX` (9 or 15 params, facets `.1`..`.8`)

---

## 4. Block 3: Data Card Rules

- **`MODE`**: Default is `mode n`. Any particle designator (`:n`, `:p`, `:e`, `PAR=p`, etc.) used anywhere in the file must be listed in `MODE` (`W802`).
- **`M<n>` (Material Card)**:
  - Format: `m<n> <zaid1> <frac1> <zaid2> <frac2> ...`
  - Do NOT mix positive (atomic) and negative (weight) fractions inside the same `M<n>` card (`E702`).
- **`SDEF` & `SI`/`SP` Distributions**:
  - `SIn H b0 b1 ... bk` (Histogram, default option: $k+1$ bin boundaries) requires `SPn D 0.0 p1 ... pk` ($k+1$ entries starting with `0.0`) (`E506`).
  - `SIn L v1 ... vk` (Discrete: $k$ values) requires `SPn D p1 ... pk` ($k$ values).
  - Vector variables (`POS`, `AXS`, `VEC`, `LOC`) cannot use `H`, `L`, `A` scalar options directly unless sampling distribution lists `S` (`E505`).
- **Tallies (`F<n>`)**:
  - `F1`, `F2` target **Surface IDs**; `F4`, `F6`, `F7`, `F8` target **Cell IDs** (`E602`).
  - `DE<n>` and `DF<n>` must both be present with the exact same number of entries (`E603`).
