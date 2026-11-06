# 0057. Balanced rivers: generated water at rest by the engine's one law

- Superseded by [ADR 0060](0060-water-surface-grid.md): the basins were removed in 0.48.2, the river lips and the pool graph in 0.49.0.
- Status: Accepted (the owner, 2026-11-04: "Balanced generation", and the answers in WATER_FLOW.md
  §17.6); built (0.45.0); amends ADR 0055
- Date: 2026-11-04
- Design: [`WATER_FLOW.md`](../WATER_FLOW.md) §17

## Context

ADR 0055 made generated water "at rest as found". Its natural outlets are scaled to pass the world
map's flow, seeps carry the flow where it has no way out, and cells beside it are held like walls.
That was because the terrain lays rivers in ½ m terraces that the engine's law (ADR 0056) doesn't
balance. The result is two kinds of water: generated water behaves differently from water an edit
moves. The owner (2026-11-04): "why would water behave differently just because it existed on
generation time?"

## Options

1. **Balanced generation** (chosen by the owner): the generator solves each river pool's level with
   the engine's own law for the map's discharge over its lip; one law for all water, and ADR 0055's
   special cases removed. A new compatibility line.
2. **The adjustment on the lip**: each generated lip keeps a correction factor any water crossing it
   uses. Uniform in time but not in space, and not physics.
3. **Settle at generation**: run the water briefly when a chunk is generated. Slower generation, and
   pools at chunk edges depend on which neighbours were loaded.

## Decision

Option 1, with the owner's answers: pools rest on full half-block levels, the generator sizing each
lip (whole cells wide, whole half-blocks of water over it) and the engine's flow for each pool being
exactly what its lip carries there (the map's flow rounded to it); the waterline generated flooded, and, in the same
release, water drawn as a slope over a ½ m step between pools (drops of a block or more stay
waterfalls). Shipped as 0.45.0 on its own.

## Consequences

- Generated water, edited water and water in chunks that load later follow one law.
- The terrain each seed generates changes: a new compatibility line (0.45.0).
- The opening law and the liquids' weir coefficients are shared by the generator and the engine.
- ADR 0055's scaling of natural outlets to the map's flow is gone: a found body passes what its
  ways out carry by the one law. Kept as built (WATER_FLOW.md §17.7): `rest`/`held` for generated
  banks and risers the grid doesn't always show, seeps where a river leaves the loaded world, and
  the links between found bodies for what the unsimulated river brings.
