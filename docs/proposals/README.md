# Proposals

Plans for work that has been investigated but **not adopted**. Nothing here is architecture:
`docs/ARCHITECTURE.md` does not describe it and `docs/IMPLEMENTATION_PLAN.md` has no phase for it.
A proposal is adopted by answering its owner questions, writing an ADR and adding phases to the
implementation plan; the proposal is then folded into the design doc or deleted.

Open proposals:

| Proposal | What | Status |
|---|---|---|
| [TREE_GROWTH.md](TREE_GROWTH.md) | Growing a hand-built tree over time from its extracted skeleton, and variations of it | Investigated 2026-10-14; owner questions answered except grafting |

Adopted proposals, and where they went:

| Proposal | Adopted | Now |
|---|---|---|
| Ray marching (`RAY_MARCHING.md`): a ray-marched renderer compared side by side with the mesher | 2026-10-14, [ADR 0029](../adr/0029-ray-marched-renderer.md), Phase 25 | [`docs/RAY_MARCHED_RENDERER.md`](../RAY_MARCHED_RENDERER.md) |
| Water (`WATER.md`): fluid looks as data, refraction, underwater, reflections | 2026-10-14, [ADR 0030](../adr/0030-fluid-looks-and-water-rendering.md), Phase 26 | [`docs/WATER_RENDERING.md`](../WATER_RENDERING.md) |
| Structures (`STRUCTURES.md`): a structure sandbox and a data-driven structure system | 2026-10-14, [ADR 0031](../adr/0031-data-driven-structures.md), Phase 27 | [`docs/STRUCTURES.md`](../STRUCTURES.md) |

Numbers marked *estimate* in a proposal are extrapolated from published figures or arithmetic, not
measured on Dwell. Measure them with the bench (`docs/DEV_TOOLS.md`) before relying on them.
