# The ray-marched renderer (Phase 25): dropped at A7

**Status:** Dropped by the owner at Phase 25's A7, 2026-10-20 ([ADR 0044](./adr/0044-mesher-over-ray-marching.md),
superseding [ADR 0029](./adr/0029-ray-marched-renderer.md)): *"Ray marching is massively slower. We can
roll it back… Mesher / raster is definitely our way forward."* The mesher is Dwell's terrain renderer.

Phase 25 built a second, complete terrain renderer beside the mesher: GPU ray marching over a
brickmap (clipmap levels of 64-trees, raw and palette bricks, column min-max grids) with a cone LOD,
a beam prepass, a temporal start, kept unchanged frames, render scales with FSR 1 or a bilateral
upscale, a renderer switch and an in-game compare mode (split, blink, difference), in local builds
only. At its best (A4c), marching every frame cost 1.5–3× raster's whole GPU frame at the spawn, the
horizon and flying high; only a still, unchanged view or a view straight down beat raster. The
figures are in ADR 0044.

**Where it lives:** the tag and the branch `archive/ray-marcher` (commit `8ed481f`, main at 0.20.3)
hold the marcher's last state: `crates/dwell-march`, `crates/dwell-client/src/render/march`,
compare mode, the bench's march configurations, and this document in full (the design, the plan,
each step "as built" with its measurements, the risks and the owner's answers).

**What stayed in main:** A1, the shared frame (linear colour and view depth drawn offscreen, the
composite; also water's W2) and GPU pass timing, and A2's `bench --compare` for raster
configurations (DEV_TOOLS.md).
