# Proposals

Plans for work that has been investigated but **not adopted**. Nothing here is architecture:
`docs/ARCHITECTURE.md` does not describe it and `docs/IMPLEMENTATION_PLAN.md` has no phase for it.
A proposal is adopted by answering its owner questions, writing an ADR and adding phases to the
implementation plan; the proposal is then folded into the design doc or deleted.

| Proposal | What | Status |
|---|---|---|
| [RAY_MARCHING.md](RAY_MARCHING.md) | GPU ray marching over a sparse voxel structure: a hybrid with the raster renderer first, a full port only if the bench shows it pays | Investigated 2026-10-14; owner questions open |
| [WATER.md](WATER.md) | A water shader with reflections, refraction and underwater rendering, on the raster renderer and under ray marching | Investigated 2026-10-14; owner questions open |
| [STRUCTURES.md](STRUCTURES.md) | A flat white sandbox for building structures (trees, rocks), saved locally, exported to a data-driven structure system | Investigated 2026-10-14; owner questions open |

The three share one prerequisite, the frame restructure (`RAY_MARCHING.md` step R1): an offscreen
HDR target, a stored and sampleable depth buffer, a composite pass, and GPU pass timing in the
telemetry. Water's refraction and the ray-marched lighting both need it, so it is built once.

Numbers marked *estimate* are extrapolated from published figures or arithmetic, not measured on
Dwell. Measure them with the bench (`docs/DEV_TOOLS.md`) before relying on them.
