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
  floor; chunk-aligned: chunk row −64). The **bedrock layer moves to straddle it** (`BEDROCK_LAYERS`
  on each side), so one indestructible core anchors both faces for structural integrity (§7.1,
  Phase 14).
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
- The crust between the two seas is ~4 km thick; between face A's deepest basins (~−540 m) and
  face B's, there are ~3 km of rock and the bedrock core.
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

Because down points toward the midplane from both sides, going "over the edge" means falling
*to* the midplane, then climbing *away* from it on the other side. Recommended routes (to confirm
with the owner, §9):

1. **Wells through the crust.** Rare generated shafts (a hashed cell grid, ~50–200 km apart, a
   few metres wide) that pierce the bedrock core, with ladders or climbable walls. Climb down
   (gravity toward the midplane), pass the midplane — the view turns over (§6) — and climb out the
   other side, which is now "up". The bedrock around a well stays indestructible.
2. **The rim.** The disc's edge is a cliff about 4 km tall between the two seas. At the midplane
   the rim carries a **ledge** (a ring shelf of bedrock a few metres wide), the floor of both
   halves of the rim wall: from either face, a climb or fall down the wall ends on it. Stepping off
   its outer edge leaves you floating in the flip band, from where you can climb the other half of
   the wall. (Today the rim drops into a killing void; the bifacial rim replaces that.)
3. Digging through bedrock stays impossible.

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
- **Crossing the band** (in a well or off the rim ledge): the face sign switches at the midplane;
  the camera turns over smoothly (a roll through 180° over ~0.5 s) instead of snapping. The flip is
  predicted like any other movement; if the local player's state needs a flag for it, it goes in
  `PhysicsSnapshot` (a protocol bump).
- **Rendering:** the camera's up vector follows the face; the meshers' per-face shading (Phase 7's
  tint table) treats a face-B chunk's −y faces as its tops; the sky gradient, sun direction and
  haze are evaluated in the viewer's face-local frame, so each face has its own sky (§9: one sun
  or two). Height fog uses the height above the viewer's face's sea level.

## 7. Physics (Phases 14–16)

Falling clusters and debris use per-body gravity by side; integrity anchors include the midplane
bedrock (which anchors both faces); bodies that fall off the rim settle in the flip band.

## 8. How the phase is checked

- Generation: a face-B chunk equals the vertical flip of the face-local chunk; face B's terrain
  differs from face A's; bedrock straddles the midplane; goldens for face-B chunks and straddling
  LOD sections, natively and in WASM.
- Controller: the mirror-equivalence suite (all scenarios, both origins, native and WASM); walking,
  jumping, swimming and climbing on face B; crossing a well and the rim ledge, with the face sign
  and camera turning over once.
- Gravity: a body falling off the rim settles in the flip band (no endless oscillation); Jolt bodies
  on face B fall toward the midplane.
- Bounds: nothing generated outside the two domes and bands; streaming and LOD reach both domes;
  posfix round-trips positions at both extremes.
- Manual: walking face B, going through a well and around the rim, and views of both domes.

## 9. Open questions

1. **Crossing routes:** wells, the rim ledge, both, or something else (e.g. portals)?
2. **Light:** one sun lighting both faces in turn (needs a day/night cycle), two suns, or each face
   lit in its own frame without a physical sun?
3. **Face identity:** should face B differ in character (darker, stranger, another biome palette),
   or be "more of the same"?
4. **Spawn and worlds:** always on face A, or a choice per world?
5. **Crust thickness:** keep today's band (≈ 4 km between the seas) or thicken it.
