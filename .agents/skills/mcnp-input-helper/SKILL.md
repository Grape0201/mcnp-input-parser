---
name: mcnp-input-helper
description: >-
  LSP-like assistant for creating, editing, inspecting, and validating MCNP (Monte Carlo N-Particle)
  input files (.i, .inp, .mcnp). Use this skill whenever the user asks to generate a new MCNP input
  deck, modify cells/surfaces/materials/tallies/sources in an existing MCNP input file, inspect cross-references
  or available IDs, or debug MCNP syntax and semantic errors.
---

# MCNP Input Helper Skill (LSP Workflow for AI Agents)

This skill equips you with [`mcnp-helper.sh`](./scripts/mcnp-helper.sh), a fast CLI powered by `mcnp-input-parser` that acts as a **Language Server (LSP)** for MCNP input decks:
- **Document Symbols & Conflict-Free IDs** (`--symbols`)
- **Go-to-Definition / Hover / Find References** (`--inspect <TARGET>`)
- **Card & Skeleton Templates / Completion Snippets** (`--template [TOPIC]`)
- **Syntax & Semantic Diagnostics with Fix Hints** (`--validate`)

---

## Standard Workflow

### Step 1: Before Editing an Existing MCNP File (Symbol & Reference Lookup)

Never guess existing Cell, Surface, Material, Universe, or Tally IDs. Run `--symbols` first:

```bash
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --symbols path/to/input.inp
```

- Check the `=== Next Available IDs (Conflict-Free) ===` section at the bottom of the output whenever adding new Cells, Surfaces, Materials (`M<n>`), Universes (`U=<n>`), Transforms (`TR<n>`), Source Distributions (`D<n>`), or Tallies (`F<n>` / `FMESH<n>`).
- Before modifying or deleting an existing card, check everything that references it using `--inspect`:

```bash
# Target syntax: c<id> (Cell), s<id> (Surface), m<id> (Material), u<id> (Universe), f<id> (Tally), tr<id> (Transform), d<id> (Distribution)
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --inspect s10 path/to/input.inp
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --inspect m1 path/to/input.inp
```

---

### Step 2: Authoring New Files or Complex Cards (Templates & Cheatsheet)

When generating a new MCNP input file from scratch or adding macrobodies, `SDEF` distributions, `FM` multipliers, `FMESH`, or weight windows, fetch the exact template or parameter reference:

```bash
# List all available template topics:
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --template

# Print a specific template or reference table:
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --template skeleton-sdef
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --template skeleton-kcode
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --template macrobodies
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --template surfaces
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --template source
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --template tallies
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --template variance-reduction
```

For quick formatting rules (3-block blank line rules, continuation lines, density sign conventions, macrobody facet numbering), read [CARD_SYNTAX_CHEATSHEET.md](./references/CARD_SYNTAX_CHEATSHEET.md).
For detailed descriptions of all error and warning codes (`E001`–`W904`), see [VALIDATION_ERRORS.md](../../../docs/VALIDATION_ERRORS.md).

---

### Step 3: Mandatory Post-Edit Diagnostics Loop (`--validate`)

After **every** creation or modification of an MCNP input file, run `--validate`:

```bash
.agents/skills/mcnp-input-helper/scripts/mcnp-helper.sh --validate path/to/input.inp
```

1. **Zero Errors Required**: If `--validate` reports any `[SYNTAX]` or `[E...]` errors, read the `-> Hint:` attached to each diagnostic, edit the file to fix the issue, and re-run `--validate` until `0 error(s)` is achieved.
2. **Review Warnings**: Address any `[W...]` warnings (such as `W309 UnusedSurface`, `W310 UnusedMaterial`, `W503 MissingKsrcForKcode`, `W802 ParticleNotInMode`) unless the user explicitly requested unused placeholder definitions.
