# Proposals

Plans for work that has been investigated but **not adopted**. Nothing here is architecture:
`docs/ARCHITECTURE.md` does not describe it and `docs/IMPLEMENTATION_PLAN.md` has no phase for it.
A proposal is adopted by answering its owner questions, writing an ADR and adding phases to the
implementation plan; the proposal is then folded into the design doc or deleted.

There are no open proposals.

Adopted proposals, and where they went:

| Proposal | Adopted | Now |
|---|---|---|
| Ray marching (`RAY_MARCHING.md`): a ray-marched renderer compared side by side with the mesher | 2026-10-14, [ADR 0029](../adr/0029-ray-marched-renderer.md), Phase 25; dropped at A7 (2026-10-20, [ADR 0044](../adr/0044-mesher-over-ray-marching.md)) | [`docs/RAY_MARCHED_RENDERER.md`](../RAY_MARCHED_RENDERER.md) (a note; the design is in the `archive/ray-marcher` branch) |
| Water (`WATER.md`): fluid looks as data, refraction, underwater, reflections | 2026-10-14, [ADR 0030](../adr/0030-fluid-looks-and-water-rendering.md), Phase 26 | [`docs/WATER_RENDERING.md`](../WATER_RENDERING.md) |
| Structures (`STRUCTURES.md`): a structure sandbox and a data-driven structure system | 2026-10-14, [ADR 0031](../adr/0031-data-driven-structures.md), Phase 27 | [`docs/STRUCTURES.md`](../STRUCTURES.md) |
| Tree growth (`TREE_GROWTH.md`): growing a hand-built tree from its extracted skeleton, and variations of it | 2026-10-14, [ADR 0033](../adr/0033-tree-growth.md), Phase 30 | [`docs/TREE_GROWTH.md`](../TREE_GROWTH.md) |

Numbers marked *estimate* in a proposal are extrapolated from published figures or arithmetic, not
measured on Dwell. Measure them with the bench (`docs/DEV_TOOLS.md`) before relying on them.
