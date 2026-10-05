# Dwell — The Bifacial World: Two Faces, Gravity Toward the Middle

> **Status: [planned]** — the design for implementation **Phase 13**
> ([`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md)). As it lands, the built parts move into
> [`ARCHITECTURE.md`](./ARCHITECTURE.md) (§6.3 world bounds and generation, §6.6 LOD, §7 physics,
> §9 players) and [`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md), with an ADR superseding the
> vertical bounds of ADR 0011 and Phase 12's dome.

## 1. Intent (owner, 2026-10-05)

The world becomes **bifacial**: the disc has a second inhabited face on its underside, with
**gravity inverted on the other side of the halfway point**. The underside is built like the top —
the normal terrain (Phases 10–11) and a dome of floating islands (Phase 12) — so the whole world is
a **two-sided, sphere-like object**: two hemispherical domes of sky, 8,192 km in radius, whose flat
sides are the disc's two faces. It is not truly spherical; it is two copies of today's world shape,
back to back.

## 2. Geometry

Today (after Phase 12): the disc's ground band runs from `WORLD_MIN_Y` = −2,048 to
`TERRAIN_MAX_Y` = 6,144 with sea level at 0, bedrock at the bottom, the void below, and a dome of
radius `DOME_RADIUS` = 8,192 km above.

Bifacial:

- **The midplane** — the halfway point — is the plane `y = MIDPLANE_Y` = −2,048 (today's world
  floor; chunk-aligned: chunk row −64). There is **no bedrock**: the rock at the midplane is
  ordinary and can be dug through (owner, 2026-10-05). Instead, a **core zone** of
  `CORE_ANCHOR_LAYERS` (≈ 8) on each side of the midplane anchors both faces for structural
  integrity by *position*: any solid voxel still in the zone counts as grounded (§7.1, Phase 14),
  so a dug shaft leaves the surrounding crust standing. Today's bedrock material and the void
  below the world are no longer generated.
- **Face A** (the top, today's world) is unchanged: ground band [−2,048, 6,144), sea level 0, dome
  above.
- **Face B** (the underside) is its **mirror image about the midplane**: a voxel at height `y` on
  face B corresponds to face-local height `h = 2 × MIDPLANE_Y − 1 − y` (= −4,097 − y), so face B's
  sea level is at y = −4,096, its ground band is [−10,240, −2,048), and its dome hangs below,
  reaching y ≈ −4,096 − 8,192,000. The mirror maps voxels onto voxels and **chunks onto chunks**
  (chunk row `cy` ↔ −129 − `cy`).
- **Its own terrain.** Face B is not a reflection of face A's landscape: it is generated with a
  different seed stream (its own continents, rivers, biomes, islands), in face-local coordinates,
  then mirrored into place. Generation stays a pure function of `(seed, version, chunk)`.
- The crust between the two seas is ~4 km thick (kept, owner 2026-10-05); between face A's deepest
  basins (~−540 m) and face B's, there are ~3 km of rock.
- **Fits the existing numbers:** y spans about −8,196,096 to +8,192,000. Chunk rows stay within
  `int32`; `posfix` positions (1/256 m, ±8,388,608 m) cover both domes; the LOD root (level 19,
  16,777 km) covers the whole span once its origin moves from `WORLD_MIN_Y` to −2²³ (like x and z
  already); the creative-flight ceiling (24,000 km) gains a matching floor.

## 3. Gravity

**Down is always toward the midplane.** For a point above the midplane gravity is −y (as today),
below it +y. Players on face B stand on the underside with their heads pointing −y; islands in face
B's dome hang below the disc with their tops facing −y.

- **The flip band.** In open air within `FLIP_BAND` (≈ 4 m) of the midplane, gravity fades
  linearly to zero at the midplane and a drag damps vertical motion, so a body that reaches the
  midplane in the open settles there instead of oscillating between the faces forever. A player in
  the band moves as when swimming (PLAYER_CONTROLLER.md §6.4's swim layer, without buoyancy).
- Where the midplane is solid (almost everywhere inside the disc), the band is never reached.
- **Jolt:** the world keeps one gravity vector; each body's gravity factor is set by its side of
  the midplane (−1 on face B) and scaled inside the flip band, updated when a body crosses it. The
  player controller applies its own gravity (it is a custom rigid-body controller) through the
  face sign below.

## 4. Crossing between the faces

**No dedicated crossing routes** (owner, 2026-10-05): no generated wells, ledges or portals. A
player crosses in one of two ways:

1. **Through the rock.** Dig down toward the midplane (gravity points there), through the core —
   nothing in it is indestructible — and keep digging into face B's crust; past the midplane "down"
   points back toward it, so the far side of the shaft is now "up" and the player builds or digs
   their way out. In the open air of a dug shaft the flip band applies (§3).
2. **Around the outside.** The disc's edge is a cliff about 4 km tall between the two seas, with
   nothing at the midplane. A player who goes over the edge falls toward the midplane, is slowed in
   the flip band and floats there beside the wall; from there they climb, dig into or build up the
   wall on the other side. (Today the rim drops into a killing void; the bifacial rim replaces
   that.) Phase 10 rings the rim with ~400 km of open ocean, so reaching the edge means crossing
   that ocean (or flying).

Spawn is always on face A (owner, 2026-10-05).

## 5. Generation and LOD

- Every generator stage runs in **face-local coordinates** (x, h, z) with the face's own seed
  streams; a face-B chunk is the face-local chunk flipped vertically. Point queries (`SolidAt`,
  `GroundY`, `ColumnAt`, island queries) take the face. Spawn stays on face A.
- **Air chunks:** the sky-floor test, island bounds and the dome bounds are per face.
- **LOD:** sections up to level 7 (4,096 m) are aligned to the mirror and generate like chunks;
  coarser sections straddle the midplane, and `GenerateLod` decides each cell's face by its
  centre's side of the midplane. LOD bounds per section column become the union of both faces'
  bounds. `LodIndex` already carries the row (Phase 12).
- Water fills "below sea level" in face-local terms (for face B: between the ground and y = −4,096,
  above it in world y).
- Determinism goldens gain face-B chunks and sections, including the flip of a chunk and a
  midplane-straddling LOD section.

## 6. Players, camera and rendering

- **The controller in face-local frame.** The controller assumes +y is up in its probes, step-up,
  jumping, crouch, ladders, swimming and ground snapping (about 45 places in `controller.cpp`,
  more in `voxel_query.cpp`). Rather than general gravity directions, it gains a **face sign**
  (±1) threaded through every vertical quantity and probe direction; the voxel queries mirror
  lookups for face B. A **mirror-equivalence suite** proves it: every controller scenario run on
  face A and its mirror image on face B gives mirrored traces bit for bit, natively and in WASM.
- **Crossing the band** (in a dug shaft, or beside the rim wall): the face sign switches at the midplane;
  the camera turns over smoothly (a roll through 180° over ~0.5 s) instead of snapping. The flip is
  predicted like any other movement; if the local player's state needs a flag for it, it goes in
  `PhysicsSnapshot` (a protocol bump).
- **Light: a sun and a moon** (owner, 2026-10-05), both **static** for now. The sun is Phase 7's
  directional light, lighting face A; the **moon** is a second directional light pointing the
  opposite way (`moon direction = −sun direction`, "counter-angled"), lighting face B with a
  cooler, dimmer moonlight. With no shadows, each light would also reach the other face's
  ceilings, so each fragment takes only its own face's light (chosen by its side of the midplane),
  and the hemisphere/ambient light is per face too. Colours and intensities live in Phase 7's
  palette module as tunables.
- **Rendering:** the camera's up vector follows the face; the meshers' per-face shading (Phase 7's
  tint table) treats a face-B chunk's −y faces as its tops; the sky gradient (face B: a moonlit
  night sky with the moon's glow where the sun's is on face A) and haze are evaluated in the
  viewer's face-local frame. Height fog uses the height above the viewer's face's sea level.

## 7. Physics (Phases 14–16)

Falling clusters and debris use per-body gravity by side; structural integrity anchors on the core
zone (§2) instead of a bedrock layer, so digging through the core is allowed and what remains in
the zone still holds both faces; bodies that fall off the rim settle in the flip band.

## 8. How the phase is checked

- Generation: a face-B chunk equals the vertical flip of the face-local chunk; face B's terrain
  differs from face A's; no bedrock is generated and every core voxel can be dug; goldens for
  face-B chunks and straddling LOD sections, natively and in WASM.
- Controller: the mirror-equivalence suite (all scenarios, both origins, native and WASM); walking,
  jumping, swimming and climbing on face B; crossing by a dug shaft through the core and by going
  over the rim, with the face sign and camera turning over once.
- Gravity: a body falling off the rim settles in the flip band (no endless oscillation); Jolt bodies
  on face B fall toward the midplane.
- Bounds: nothing generated outside the two domes and bands; streaming and LOD reach both domes;
  posfix round-trips positions at both extremes.
- Lighting: face A lit by the sun only and face B by the moon only (a ceiling on either face is not
  lit by the other face's light).
- Manual: walking face B, digging through and going around the rim, and views of both domes.

## 9. Decisions

Decided (owner, 2026-10-05):
- **No crossing routes:** only digging through the rock or going around the rim.
- **Light:** a static sun (face A) and a static, counter-angled moon (face B).
- **Spawn** always on face A; the **crust** stays ~4 km thick.
- **The rim ocean stays:** Phase 10's ~400 km rim ocean is kept, so reaching the edge means crossing
  open water or flying.
- **Face B reuses everything:** the same generator stages, biome table, palette and island design
  as face A, with its own seed streams; only its light (the moon) and night sky differ.

Later (not part of this phase): a day/night cycle with a moving sun and moon, and giving face B a
character of its own.
