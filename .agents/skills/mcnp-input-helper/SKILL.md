---
name: mcnp-input-helper
description: >-
  LSP-like assistant for creating, editing, inspecting, and validating MCNP (Monte Carlo N-Particle)
  input files (.i, .inp, .mcnp). Use this skill whenever the user asks to generate a new MCNP input
  deck, modify cells/surfaces/materials/tallies/sources in an existing MCNP input file, inspect cross-references
  or available IDs, or debug MCNP syntax and semantic errors.
---

# MCNP Input Helper Skill (Working-Example-First Workflow)

This skill uses the `mcnp-input-parser` CLI (available in `PATH`) as a **Language Server (LSP)** and **Working-Example Generator** for MCNP input decks:
- **Bootstrap Working Examples** (`--template skeleton-sdef | skeleton-kcode > file.inp`)
- **Syntax & Semantic Diagnostics with Fix Hints** (`--validate`)
- **Document Symbols & Conflict-Free IDs** (`--symbols`)
- **Go-to-Definition / Hover / Find References** (`--inspect <TARGET>`)
- **Card Reference Tables** (`--template [TOPIC]`)

---

## Golden Rule: NEVER Write an MCNP File from Scratch!

Writing an MCNP input file from scratch in memory almost always fails due to misplaced blank lines or card syntax hallucinations.
Instead, **ALWAYS bootstrap from a validated Working Example via the CLI** and incrementally edit each block while keeping the 2 block-delimiter blank lines intact.

### Essential MCNP Structure Rules
1. **Exactly 2 Blank Lines Total**:
   - Line 1: **Title Line** (**NEVER put a blank line after the Title line!**)
   - **Block 1: Cell Cards** (`<id> <mat> [<density>] <geom_expr> <params>`)
   - *(1st single blank line — terminates Block 1)*
   - **Block 2: Surface & Macrobody Cards** (`<numeric_id> [<tr>] <mnemonic> <params>`)
   - *(2nd single blank line — terminates Block 2)*
   - **Block 3: Data Cards** (`mode`, `sdef`/`kcode`, `m<n>`, `f<n>`, `nps`, etc.)
2. **NO Blank Lines Inside a Block**:
   - Never put blank lines inside Block 1, Block 2, or Block 3 (e.g., between `mode`, `sdef`, `m1`, and `f4`/`f6`). Any extra blank line shifts subsequent cards into the wrong block or truncates the file (`[E004]`).
   - Use `c` comment lines (`c --- Section ---`) to visually separate sections inside a block.
3. **Avoid the Top 3 Syntax Traps**:
   - **Numeric Surface IDs Only**: In Block 2, write `10 so 10.0` or `10 rcc ...`, **NEVER** `s10 so 10.0` (`s10` is only for `--inspect s10` CLI queries).
   - **Macrobodies Belong in Block 2**: Define `rcc`, `rpp`, `sph`, `box` in **Block 2 (Surface Cards)**, then reference them in Block 1 cells as `-10` (inside) or `10` (outside). Never put `rpp` or `rcc` on a Cell card in Block 1.
   - **Material Fractions Sign**: On an `m<n>` card, use all negative values for weight fractions (`-0.25`) OR all positive values for atomic fractions (`0.25`), never both on the same `m<n>` card (`[E702]`). Thermal scattering goes on a separate `mt<n>` card (e.g. `mt1 lwtr.20t`), never on `m<n>`.

---

## Workflow A: Creating a New MCNP Input File

### Step 1: Bootstrap a Valid Working Example to Disk via Shell
Choose the skeleton template for your calculation type and redirect it **directly into your target `.inp` file** using a shell command:

- **Fixed-Source / Shielding / Dose Calculation (`SDEF`)**:
  ```bash
  mcnp-input-parser --template skeleton-sdef > input.inp
  ```
- **Criticality Calculation (`KCODE` + `KSRC`)**:
  ```bash
  mcnp-input-parser --template skeleton-kcode > input.inp
  ```

Verify that the bootstrapped file starts with `0 error(s)`:
```bash
mcnp-input-parser --validate input.inp
```

### Step 2: Check Reference Tables for Specific Cards (If Needed)
Read `input.inp` to see the 3 blocks (`Block 1`, `Block 2`, `Block 3`) and the 2 blank lines separating them.
Before replacing a surface with a macrobody (`rcc`, `rpp`), adding volume source distributions (`pos`/`axs`/`rad`/`ext`), or changing tallies (`f4` flux vs `f6` absorbed dose), check the exact syntax from `--template`:
```bash
mcnp-input-parser --template macrobodies
mcnp-input-parser --template surfaces
mcnp-input-parser --template source
mcnp-input-parser --template tallies
mcnp-input-parser --template cells
mcnp-input-parser --template variance-reduction
```
For detailed formatting rules, see [CARD_SYNTAX_CHEATSHEET.md](./references/CARD_SYNTAX_CHEATSHEET.md).

### Step 3: Edit Block-by-Block and Validate After Every Edit
Modify the cards inside Block 1, Block 2, and Block 3 of `input.inp` to match the user's geometry, materials, source, and tallies **without adding or removing any blank lines**.
Immediately validate after editing:
```bash
mcnp-input-parser --validate input.inp
```

---

## Workflow B: Editing an Existing MCNP File

### Step 1: Symbol & Cross-Reference Lookup
Never guess existing Cell, Surface, Material, Universe, or Tally IDs. Run `--symbols` first:
```bash
mcnp-input-parser --symbols path/to/input.inp
```
- Check `=== Next Available IDs (Conflict-Free) ===` at the bottom whenever adding new Cells, Surfaces, Materials (`M<n>`), Universes (`U=<n>`), Transforms (`TR<n>`), Source Distributions (`D<n>`), or Tallies (`F<n>`).
- Before modifying or deleting an existing card, check everything that references it using `--inspect`:
  ```bash
  # Target syntax for CLI only: c<id> (Cell), s<id> (Surface), m<id> (Material), u<id> (Universe), f<id> (Tally)
  mcnp-input-parser --inspect s10 path/to/input.inp
  mcnp-input-parser --inspect m1 path/to/input.inp
  ```

### Step 2: Edit and Run the Mandatory Diagnostics Loop (`--validate`)
After **every** edit to an MCNP input file, run:
```bash
mcnp-input-parser --validate path/to/input.inp
```
1. **Zero Errors Required**: If `--validate` reports any `[SYNTAX]` or `[E...]` errors, read the `-> Hint:` attached to each diagnostic, fix the issue, and re-run `--validate` until `0 error(s)` is achieved.
2. **Review Warnings**: Address any `[W...]` warnings (such as `W309 UnusedSurface`, `W310 UnusedMaterial`, `W503 MissingKsrcForKcode`, `W802 ParticleNotInMode`) unless the user explicitly requested placeholder definitions.
