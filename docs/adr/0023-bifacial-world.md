# 0023. The bifacial world: two faces, gravity toward the midplane

- Status: Accepted
- Date: 2026-10-07
- Supersedes: [0011](0011-planet-scale-world.md) decision on the vertical bounds (the world floor, the
  bedrock layer and the void below it); Phase 12's single dome (the dome of each face is now its own)
- Builds on: [0017](0017-continents-from-voronoi-plates.md), [0018](0018-drainage-consistent-terrain.md),
  [0022](0022-climate-biome-table-vegetation.md) (the terrain each face repeats)

## Context

The owner's intent (2026-10-05, `BIFACIAL_WORLD.md`): the disc gets a second inhabited face on its
underside, with gravity inverted on the other side of the halfway point, so that the world is two
copies of today's shape back to back — a two-sided, sphere-like object. Today the world ends at
`WORLD_MIN_Y` = −2,048 in a bedrock layer with the void below it, and a body that leaves the disc falls
forever and is killed.

## Decision

1. **The midplane** is `y = MIDPLANE_Y` = −2,048, a chunk boundary (rows −65 and −64). There is **no
   bedrock and no void**: the rock at the midplane is ordinary, diggable stone. `WORLD_MIN_Y` becomes
   `WORLD_BOTTOM_Y` = −10,240, the lowest row of face B. A **core zone** of `CORE_ANCHOR_LAYERS` (8)
   either side of the midplane will anchor both faces for structural integrity *by position*
   (Phase 14): any solid voxel in it counts as grounded, so a dug shaft leaves the crust standing.
2. **Face B is the mirror image of face A** about the midplane. A voxel at height `y` on face B is the
   face-local voxel `h = −4,097 − y` (`MirrorY`); a chunk row `cy` is face-local row `−129 − cy`
   (`MirrorChunkY`). Voxels map onto voxels and chunks onto chunks. Face B's sea level is `y = −4,096`
   (its top water layer; face-local −1), its ground band is [−10,240, −2,048) and its dome hangs below
   the disc.
3. **Face B has its own terrain.** It is a second `TerrainGenerator`, seeded by `FaceSeed(seed, B)` (a
   hash of the world seed through streams no stage uses), running unchanged in face-local coordinates.
   Face A's generator keeps the world seed, so face A's terrain is what it was (minus the bedrock). A
   chunk of face B is the face-local chunk flipped vertically, with slabs and slopes turned over
   (`MirrorMaterial`: the same state with `half` swapped, an involution; every other state is its own
   mirror). Generation stays a pure function of `(seed, chunk)`. Spawn is always on face A.
4. **Gravity points toward the midplane**: −y on face A, +y on face B. In open air within `FLIP_BAND`
   (4 m) of the midplane gravity fades linearly to zero at the midplane and a drag damps vertical
   motion, so a body reaching the midplane in the open settles there instead of oscillating (§3 of
   `BIFACIAL_WORLD.md`). The disc's rim, a ~4 km cliff between the two seas, therefore no longer
   kills: there is no void to fall into.
5. **The controller takes a face sign** (±1) threaded through every vertical quantity and probe
   rather than a general gravity vector, with mirrored voxel queries; a mirror-equivalence suite proves
   that every scenario on face A and its image on face B give mirrored traces bit for bit.
6. **Crossing**: no dedicated routes. A player digs down through the core and out the far side, or goes
   over the rim and climbs the wall on the other side, through the flip band. The face sign switches at
   the midplane and the camera turns over smoothly (~0.5 s).
7. **Light**: a static sun lights face A and a static, counter-angled, cooler moon lights face B; each
   fragment takes only its own face's light and ambient (chosen by its side of the midplane), so a
   ceiling on one face is not lit by the other face's light. The sky and haze are evaluated in the
   viewer's face-local frame.
8. **The level of detail** counts from the corner (−2²³, −2²³, −2²³) in all three axes, so it covers
   both domes; sections up to level 6 (2,048 m) never straddle the midplane, coarser ones decide each
   cell's face by its centre's side of it.

## Consequences

- The terrain a seed generates changes (no bedrock; a second face below): a **breaking change to the
  public API** — a new compatibility line, to be raised by the owner in `package.json`. Worlds saved on
  an earlier line are not loaded by this one (`RELEASES.md`).
- `kMinChunkY` drops from −64 to −320, and the world holds twice the chunk rows; streaming, edits and
  the air test handle both faces. `kBedrockLayers` survives only in the flat and playground test
  worlds.
- Phases 14–16 anchor on the core zone and apply gravity by each body's side of the midplane.
- To reverse: restore `WORLD_MIN_Y` and the bedrock layer, drop the face-B generator (face A's output
  is otherwise unchanged).
