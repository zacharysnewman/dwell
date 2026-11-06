# 0056. One opening law: water through any opening by C·A·√(2gΔh)

- Superseded by [ADR 0060](0060-water-surface-grid.md): the basins were removed in 0.48.2.
- Status: Accepted (the owner, 2026-11-03: "Yes go ahead"); amends ADR 0053 and 0055
- Date: 2026-11-03
- Design: [`WATER_FLOW.md`](../WATER_FLOW.md) §4, §15, §16

## Context

The owner asked what a hole in a wall across a river does: it joined the two sides into one basin
at one level, so the wall held nothing back. Asked for the realistic law between two bodies of
water: the pressure difference across a submerged opening is `ρ g Δh` at every depth, so it passes
`Q = C · A · √(2 g Δh)` (Torricelli, with the contraction `C`); poured freely, each strip of it by
its own depth, which summed over a sill open above is the weir law. The engine had one law for
sills (the weir) and rules for the rest: merge at one level, split where joined only above full
rows, merge through rows both fill.

## Decision

One law for every opening and every face two basins share (`face_flow`: drowned below the far
side's level, free above it; `Floor` through a cell's floor), `C` derived from the liquid's weir
coefficient (`weir = C · ⅔ · √(2g)`, so sills pass what they did). Basins that touch exchange water
through their shared faces (contacts) by it. Whether two are one body is the law's limit: they
merge when their faces would even them out within `mergeSeconds` (data, 0.5 s) and carry what flows
through them within 2 cm; a basin narrowed by an edit to an opening that can't splits there.
Faces found bodies touched as found are held (the river's steps stay as generated), and the
generated ground never splits a body. No more in a flow step into a basin than evens the two out.

## Consequences

- A wall with holes holds a river partly back by their size and depth; two pools joined by a hole
  even out over time; a wide gap is one body; a submerged sill passes more than the old drowned weir
  (its drowned part is pushed by the whole difference).
- Pressure jets (WATER_FLOW.md §10) can take their speed from the same law, `√(2 g Δh)`.
- No saved-world, protocol or generator change: a patch on the 0.43 line (0.43.2).
