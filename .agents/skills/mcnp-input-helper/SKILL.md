---
name: mcnp-input-helper
description: >-
  LSP-like assistant for creating, editing, inspecting, and validating MCNP (Monte Carlo N-Particle)
  input files (.i, .inp, .mcnp). Use this skill whenever the user asks to generate a new MCNP input
  deck, modify cells/surfaces/materials/tallies/sources in an existing MCNP input file, inspect cross-references
  or available IDs, or debug MCNP syntax and semantic errors.
---

# MCNP Input Helper Skill (LSP Workflow for AI Agents)

This skill uses the `mcnp-input-parser` CLI (assumed to be available in `PATH`) as a **Language Server (LSP)** for MCNP input decks:
- **Document Symbols & Conflict-Free IDs** (`--symbols`)
- **Go-to-Definition / Hover / Find References** (`--inspect <TARGET>`)
- **Card & Skeleton Templates / Completion Snippets** (`--template [TOPIC]`)
- **Syntax & Semantic Diagnostics with Fix Hints** (`--validate`)

---

## Essential MCNP Structure Rule: Blank Lines Are Block Delimiters

In MCNP, a blank line is **not** visual whitespace—it is a **syntax delimiter that terminates the current block**.
- An MCNP input file consists of a **Title line** followed by **3 blocks** separated by **exactly 2 single blank lines** in total:
  1. **Title Line** (1st line of the file — **no blank line after the title**)
  2. **Block 1: Cell Cards**
  3. *(Single blank line)*
  4. **Block 2: Surface Cards**
  5. *(Single blank line)*
  6. **Block 3: Data Cards** (`mode`, `sdef`/`kcode`, `m<n>`, `f<n>`, `nps`, etc.)
- **Never put blank lines inside a block** (e.g., between `mode`, `sdef`, `m1`, and `f4`) or consecutive blank lines between blocks. Any extra blank line shifts subsequent cards into the wrong block or silently truncates the rest of the file.
- Use `c` comment lines (`c --- Materials ---`) to visually separate sections within a block.

---

## Standard Workflow

### Step 1: Before Editing an Existing MCNP File (Symbol & Reference Lookup)

Never guess existing Cell, Surface, Material, Universe, or Tally IDs. Run `--symbols` first:

```bash
mcnp-input-parser --symbols path/to/input.inp
```

- Check the `=== Next Available IDs (Conflict-Free) ===` section at the bottom of the output whenever adding new Cells, Surfaces, Materials (`M<n>`), Universes (`U=<n>`), Transforms (`TR<n>`), Source Distributions (`D<n>`), or Tallies (`F<n>` / `FMESH<n>`).
- Before modifying or deleting an existing card, check everything that references it using `--inspect`:

```bash
# Target syntax: c<id> (Cell), s<id> (Surface), m<id> (Material), u<id> (Universe), f<id> (Tally), tr<id> (Transform), d<id> (Distribution)
mcnp-input-parser --inspect s10 path/to/input.inp
mcnp-input-parser --inspect m1 path/to/input.inp
```

---

### Step 2: Authoring New Files or Complex Cards (Templates & Cheatsheet)

When generating a new MCNP input file from scratch or adding macrobodies, `SDEF` distributions, `FM` multipliers, `FMESH`, or weight windows, fetch the exact template or parameter reference:

```bash
# List all available template topics:
mcnp-input-parser --template

# Print a specific template or reference table:
mcnp-input-parser --template skeleton-sdef
mcnp-input-parser --template skeleton-kcode
mcnp-input-parser --template macrobodies
mcnp-input-parser --template surfaces
mcnp-input-parser --template source
mcnp-input-parser --template tallies
mcnp-input-parser --template variance-reduction
```

For quick formatting rules (3-block blank line rules, continuation lines, density sign conventions, macrobody facet numbering), read [CARD_SYNTAX_CHEATSHEET.md](./references/CARD_SYNTAX_CHEATSHEET.md).

---

### Step 3: Mandatory Post-Edit Diagnostics Loop (`--validate`)

After **every** creation or modification of an MCNP input file, run `--validate`:

```bash
mcnp-input-parser --validate path/to/input.inp
```

1. **Zero Errors Required**: If `--validate` reports any `[SYNTAX]` or `[E...]` errors, read the `-> Hint:` attached to each diagnostic, edit the file to fix the issue, and re-run `--validate` until `0 error(s)` is achieved.
2. **Review Warnings**: Address any `[W...]` warnings (such as `W309 UnusedSurface`, `W310 UnusedMaterial`, `W503 MissingKsrcForKcode`, `W802 ParticleNotInMode`) unless the user explicitly requested unused placeholder definitions.
