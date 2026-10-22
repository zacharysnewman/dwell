# 0049. The world map terrain: mountains, rivers and lakes

- Status: Accepted (the owner's request, 2026-10-21)
- Date: 2026-10-21
- Supersedes: the interim one-island height field as the terrain (ADR 0025's generator; its
  noise stays only as the map's input)
- Design: [`WORLD_GENERATION.md`](../WORLD_GENERATION.md) §4; the hydrology prototype's proposal
  [`proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md`](../proposals/TERRAIN_HYDROLOGY_AND_VOLUMES.md)
  (layers 0–1 adopted); plan: Phase 49

## Context

The owner (2026-10-21): "Can you actually use that work in progress terrain generator with
mountains, elevation, rivers, etc? And use it in game? No experiment flag required. I'm not attached
to our current terrain generator." Later the same day: mountains "up to 8000ft", and snowy grass and
snowy leaves high up. Rivers and lakes are not local: a valley holds a river because of everything
uphill of it. They cannot be computed per chunk from noise.

## Decision

- **A world map per face** (`dwell-worldgen` `worldmap`): the hydrology prototype's map (the base
  noise field sampled on a 100 m grid over the disc, mountain ranges lifted by the uplift layer,
  stream-power erosion, priority-flood lakes, traced rivers; `shared/worldgen/hydrology.json`'s
  default profile `uplift`, peaks ~2,300–2,500 m) kept compactly: heights, the sea, the flow
  direction per cell (for moving water later), the lakes and the river polylines. It is a pure
  function of the face's seed; chunk generation stays a pure function of (seed, chunk) given it.
- **Every column is shaped from the map** (`terrain.rs`, generator version 21): the map's height,
  Catmull-Rom bicubic (C1); fine detail noise, faded near water; river channels from the smoothed
  polylines (bed, valley, banks never lower than the water; ½ m terraces down the river); lakes flat
  inside a smooth shore with banks round them; the sea as before; bands of snow over a noisy,
  slope-aware snow line, bare rock on steep ground, sand at the shore and under water. The tunables
  are data (`shared/worldgen/terrain.json`), as are the new blocks (`dwell:snowy_grass`,
  `dwell:snowy_leaves`) and the pines' snowy variant by height (`placements.json` `variants`).
- **Computed once, everywhere, never on a frame:** a native process computes a seed's two maps on
  two threads (~1.2 s) and keeps a copy on disk; the browser builds them in two workers, keeps them
  in IndexedDB (`dwell-world-maps`, keyed by the build's map fingerprint, a settings-hash slot and
  the face seed) and hands the bytes to every WASM instance (the page, the job workers, the local
  world), which install them. The loading screen says "Shaping the world…" meanwhile. The bytes are
  lossless with a checksum; a seed's map digest is pinned and checked natively and under WASM.
- **The vertical range needs no scaling:** the build sphere reaches 25 km up each face.

## Consequences

- A new compatibility line (0.26.0): every seed's terrain changes. Old worlds keep their build.
- Joining a terrain world costs the maps' build the first time on a device (~1.5 s in WASM, both
  faces at once), then an IndexedDB read.
- Deferred: per-world generation settings (a later line, the cache key has the slot), moving water,
  deltas' and falls' polish, caves and the volume layer (the proposal's layer 2).
