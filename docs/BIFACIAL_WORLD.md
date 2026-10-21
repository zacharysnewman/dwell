# Dwell — The Bifacial World: Two Faces, Gravity Toward the Middle

> **Status: [built]** — the design for implementation **Phase 13** (as built: [ADR 0023](./adr/0023-bifacial-world.md))
> ([`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md)). As it lands, the built parts move into
> [`ARCHITECTURE.md`](./ARCHITECTURE.md) (§6.3 world bounds and generation, §6.6 LOD, §7 physics,
> §9 players) and [`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md), with an ADR superseding the
> vertical bounds of ADR 0011 and Phase 12's dome.
>
> **Re-sized by the baseline rework [built, Phase 16]** (`IMPLEMENTATION_PLAN.md` Phase 16,
> `BASELINE_EVALUATION.md` §3.1, ADR 0025): a 50 km disc, the midplane at y = 0, a 10-layer stone
> core per face and a spherical build volume replace the ~4 km crust and bounds of the Phase 13
> design; §2 gives the geometry as built. Phase 12 (the domes this file mentions) is cancelled. The
> rest of the text is the Phase 13 design: its gravity, crossing, generation and rendering rules
> stand, at the new sizes.
>
> **Bedrock and portals [built, Phase 48, 2026-10-21]:** the owner reversed "no bedrock" and "no
> crossing routes": a bedrock layer now seals the faces from each other either side of the
> midplane, and a few portals per world (shafts with a ladder, sited from the seed) lead through it
> from one face's surface to the other's. §12 is the as-built design; §2, §4 and §9 note what it
> changed.

## 1. Intent (owner, 2026-10-05)

The world becomes **bifacial**: the disc has a second inhabited face on its underside, with
**gravity inverted on the other side of the halfway point**. The underside is built like the top —
the normal terrain (Phases 10–11) and a dome of floating islands (Phase 12) — so the whole world is
a **two-sided, sphere-like object**: two hemispherical domes of sky, 8,192 km in radius, whose flat
sides are the disc's two faces. It is not truly spherical; it is two copies of today's world shape,
back to back.

## 2. Geometry

**As built (Phase 16).** The disc has radius 50 km (`WORLD_RADIUS`); the **midplane** is y = 0
(`MIDPLANE_Y`, chunk row boundary 0/−1), also the origin of generation and the centre of the
**build sphere** (`bounds`: radius 50 km, ARCHITECTURE.md §6.3), whose equator is the rim. Face A is
y ≥ 0; face B's voxel at y is the face-local voxel `h = −1 − y` (`MIRROR_SUM` = −1), its chunk row
`cy` the face-local row `−1 − cy`, so face B's sea surface is y = −32 (sea level `h` = 32 on both
faces). Each face has a **core of 10 stone layers** (`h` 0–9, `CORE_LAYERS`) that generation never
carves (but for a portal's shaft, §12), so the disc is at least 20 layers thick; since Phase 48 its
first rows from the midplane are the indestructible bedrock (§12); the core anchor zone (`CORE_ANCHOR_LAYERS` = 8,
Phase 20) lies within it. The chunk rows run from −1 563 to 1 562 (the sphere's height, symmetric
under the mirror). The **flip band** stays 4 m (`FLIP_BAND`): well inside the 20-layer crust, so
the band is reached only in a dug shaft or beside the rim, as before. The rim is a cliff a few tens
of metres tall between the two seas instead of ~4 km. In the LOD every section up to level 11 lies
wholly on one face; only the root (level 12) straddles the midplane (ARCHITECTURE.md §6.6, the thin
crust).

The Phase 13 design follows. Then (after Phase 12): the disc's ground band runs from `WORLD_MIN_Y` = −2,048 to
`TERRAIN_MAX_Y` = 6,144 with sea level at 0, bedrock at the bottom, the void below, and a dome of
radius `DOME_RADIUS` = 8,192 km above.

Bifacial:

- **The midplane** — the halfway point — is the plane `y = MIDPLANE_Y` = −2,048 (today's world
  floor; chunk-aligned: chunk row −64). There is **no bedrock**: the rock at the midplane is
  ordinary and can be dug through (owner, 2026-10-05). Instead, a **core zone** of
  `CORE_ANCHOR_LAYERS` (≈ 8) on each side of the midplane anchors both faces for structural
  integrity by *position*: any solid voxel still in the zone counts as grounded (§7.1, Phase 20),
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
- **Physics** (Jolt when designed; Rapier since Phase 24, `dwell-physics` `physics.rs`): the world
  keeps one gravity vector; each body's gravity scale is set by its side of
  the midplane (−1 on face B) and scaled inside the flip band, updated when a body crosses it. The
  player controller applies its own gravity (it is a custom rigid-body controller) through the
  face sign below.

## 4. Crossing between the faces

**No dedicated crossing routes** (owner, 2026-10-05): no generated wells, ledges or portals.
*Reversed 2026-10-21 (Phase 48, §12): a few generated portals with ladders lead through the
bedrock, and the bedrock means a dug shaft stops at the midplane.* As designed, a player crossed in
one of two ways:

1. **Through the rock.** Dig down toward the midplane (gravity points there), through the core —
   nothing in it is indestructible — and keep digging into face B's crust; past the midplane "down"
   points back toward it, so the far side of the shaft is now "up" and the player builds or digs
   their way out. In the open air of a dug shaft the flip band applies (§3).
2. **Around the outside.** The disc's edge is a cliff about 4 km tall between the two seas, with
   nothing at the midplane. A player who goes over the edge falls toward the midplane, is slowed in
   the flip band and floats there beside the wall; from there they climb, dig into or build up the
   wall on the other side. (Today the rim drops into a killing void; the bifacial rim replaces
   that.) Phase 10 rings the rim with 512 km of open ocean, so reaching the edge means crossing
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
  jumping, crouch, ladders, swimming and ground snapping (about 45 places in the C++ build's `controller.cpp`,
  more in `voxel_query.cpp`; now `dwell-physics` `controller.rs` and `query.rs`). Rather than general gravity directions, it gains a **face sign**
  (±1) threaded through every vertical quantity and probe direction; the voxel queries mirror
  lookups for face B. A **mirror-equivalence suite** proves it: every controller scenario run on
  face A and its mirror image on face B gives mirrored traces bit for bit, natively and in WASM.
- **Crossing the band** (in a dug shaft, or beside the rim wall): the face sign switches at the
  midplane; **the body turns over and the view keeps pointing exactly the same way, rolling
  upright** (owner, 2026-10-21, replacing the half forward somersault of 0.8.1):

  > "Ideally I'd like the character body to flip but the camera continue pointing in the same
  > direction, so if you're climbing down a ladder that goes through the world mid point, you
  > could hold forward (while looking down) and as you pass the mid point your character's body
  > would flip, but the camera would be pointing the same direction so you could continue holding
  > forward which would now make you climb up."

  and, after a nod about the right axis was tried: "Oh I see the problem, looking straight up and
  straight down are fine, the problem is that you're upside down. Let's go with whichever method
  lets you remain looking in the same direction, so maybe that's roll?" — and "perhaps slow down
  the transition a little."

  The rule: the look **continues across** (`FaceFrame::look_across`): a face-local heading h and
  pitch p on the old face go on as h and −p on the new one. Face B being face A's mirror, that
  look's world direction is the **same at every heading and pitch** (A's (cos p sin h, sin p,
  cos p cos h) and B's face-local (cos p sin h, −sin p, cos p cos h), mirrored); only the camera's
  up (and so its right) is reversed: the view is upside down. It is drawn **rolled** back by half
  a turn about its forward axis (`face_flip::residual`), easing gently in and out to 0 over
  `crossingRollS` (0.8 s, `shared/look/motion.json`), so it starts at the old view and turns
  upright about a forward that never moves. The controls follow the look: looking down a ladder
  before the crossing (forward climbs down) is looking up it after (forward climbs up), the same
  world way; walking and strafing stay relative to the camera at rest. Targeting, the LOD camera
  and the listener use that look at rest, whose forward is the drawn one throughout the roll. The
  pitch changes on the client only (the server sees the inputs); the controller continues the look
  the same way on the crossing tick itself, whose input was sampled on the face before, so that
  tick goes on rather than back (`controller.rs` `step_input`). The eye swings across the body
  (from above its centre to below) over the same time instead of jumping (`eye.rs`), and the
  motion effects go on through the crossing (they no longer reset there, MOTION_EFFECTS.md). In the
  flip band a body can swing back and forth across the midplane, and each crossing continues the
  look again (no hysteresis), the roll going on from the view drawn then. The flip is predicted
  like any other movement; no flag or protocol change was needed.
- **Light: a sun and a moon** (owner, 2026-10-05), **static** at rest, and turnable as a unit with
  the sky (below). The sun is Phase 7's
  directional light, lighting face A; the **moon** is a second directional light pointing the
  opposite way (`moon direction = −sun direction`, "counter-angled"), lighting face B with a
  cooler, dimmer moonlight. With no shadows, each light would also reach the other face's
  ceilings, so each fragment takes only its own face's light (chosen by its side of the midplane),
  and the hemisphere/ambient light is per face too. Colours and intensities live in Phase 7's
  palette module as tunables. The sky is **one sphere** graded from day to night (a sky frame: the
  day zenith, the sun and the moon), which first turns as a whole about a horizontal axis — the
  sun's elevation changes, π swaps day and night — client-side only (`?skyrot`, `?skyspin`, a
  settings slider); the lights are weighted smoothly by their height over each face's horizon.
  *Superseded by the day/night cycle (Phase 32, [`DAY_NIGHT.md`](./DAY_NIGHT.md)): the sun moves
  with world time, the moon stays opposite it, and each face's sky and lights are keyframes at its
  own solar angle (face B's is face A's + 180°); the slider and switches are gone.*
- **Rendering:** the camera's up vector follows the face; the meshers' per-face shading (Phase 7's
  tint table) treats a face-B chunk's −y faces as its tops; the sky gradient (face B: a moonlit
  night sky with the moon's glow where the sun's is on face A) and haze are evaluated in the
  viewer's face-local frame. Height fog uses the height above the viewer's face's sea level.

## 7. Physics (Phases 20–22)

Falling clusters and debris use per-body gravity by side; structural integrity anchors on the core
zone (§2) instead of a bedrock layer, so digging through the core is allowed and what remains in
the zone still holds both faces; bodies that fall off the rim settle in the flip band.

## 8. How the phase is checked

- Generation: a face-B chunk equals the vertical flip of the face-local chunk; face B's terrain
  differs from face A's; no bedrock is generated and every core voxel can be dug (*since Phase 48 the
  bedrock is generated and indestructible, §12*); goldens for
  face-B chunks and straddling LOD sections, natively and in WASM.
- Controller: the mirror-equivalence suite (all scenarios, both origins, native and WASM); walking,
  jumping, swimming and climbing on face B; crossing by a dug shaft through the core and by going
  over the rim, with the face sign switching and the camera continuing once.
- Gravity: a body falling off the rim is accelerated across the midplane by the flip band; rigid bodies
  on face B fall toward the midplane.
- Bounds: nothing generated outside the two domes and bands; streaming and LOD reach both domes;
  posfix round-trips positions at both extremes.
- Lighting: face A lit by the sun only and face B by the moon only (a ceiling on either face is not
  lit by the other face's light).
- Manual: walking face B, digging through and going around the rim, and views of both domes.

## 9. Decisions

Decided (owner, 2026-10-05):
- **No crossing routes:** only digging through the rock or going around the rim. *Reversed
  2026-10-21: portals through a bedrock layer (§12).*
- **Light:** a static sun (face A) and a static, counter-angled moon (face B).
- **Spawn** always on face A; the **crust** stays ~4 km thick.
- **The rim ocean stays:** Phase 10's 512 km rim ocean is kept, so reaching the edge means crossing
  open water or flying.
- **Face B reuses everything:** the same generator stages, biome table, palette and island design
  as face A, with its own seed streams; only its light (the moon) and night sky differ.

Later (not part of this phase): a day/night cycle with a moving sun and moon, and giving face B a
character of its own.

## 10. As built — where the implementation differs from the design above

- **Midplane alignment.** Sections up to level **6** (2,048 m) never straddle the midplane (§5 said
  7): −2,048 is a multiple of 2,048, not of 4,096. The LOD origin is −2²³ in all three axes; a
  level-8 column of the world is three rows, so `LodIndex` entries carry the row `j` (protocol v12).
- **The band accelerates instead of settling** (owner, 2026-10-07). The design had drag and a fade to
  zero so a body settled at the midplane; as built, a body moving toward the midplane in the band is
  accelerated (2 g extra) and carried across, and moving away the pull fades to zero at it; nothing
  brakes it (no approach cushion, no drag). Gravity points toward the midplane on both sides, so a
  body that crosses falls back: it swings across the midplane rather than hovering in the band.
  Players and Tier 1 bodies alike.
- **The controller** runs in a face-local mirror frame rather than threading a sign through 45
  places (PLAYER_CONTROLLER.md §11); the mirror-equivalence suite is `--dwell-face=b` (in Rust,
  `DWELL_FACE=b cargo nextest run -p dwell-physics`), equal in every
  decision and to contact-solver precision in positions (not bit for bit).
- **Rendering.** Face-B chunks and LOD sections are meshed as their mirror image and turned back, so
  the face toward the player's up takes the top texture and shading. Lights are picked per fragment
  by its side of the midplane (`three/heightFog.ts` then; `own` in the WGSL of `dwell-client`
  `render/shaders.rs` since Phase 24); the sky and haze are evaluated in the viewer's
  face-local frame with a night gradient and the moon's glow. Straddling LOD sections (levels ≥ 7) mesh
  both faces from one grid: face B's part is at cell resolution, shaded as undersides.
  The LOD does not load or draw the other face (beyond 512 m, except near the rim), and a straddling
  section is meshed for the viewer's face alone (the other face's rows become stone, the tints are the
  viewer's face's).
- **Not done here:** Tier 1 bodies' gravity scales are built and tested (`dwell-physics`
  `tests/tier1_bifacial.rs`), but clusters arrive in Phase 20;
  the F4 terrain map and the debug overlays show face A.

## 11. Face parity: both faces by construction [built, 2026-10-20]

Face B kept growing bugs of its own (owner, 2026-10-20: "consider how we can handle both world sides
consistently so we don't have bugs that only affect one side or the other"): a plant's selection box
not turned over, a half-height top aimed at as a whole cell, brushes and pastes working upside down,
mirrored targets. Each came from code that was written for face A and then patched for face B in
place, with its own sign or `if y < MIDPLANE_Y`, and tested on face A only.

**The rule.** Write face-dependent code once, in **face-local coordinates** (+y up, the ground below,
as on face A), and cross into the world only through **`dwell_world::FaceFrame`**. The test harness
runs it on both faces.

- `FaceFrame` (`dwell-world` `face.rs`) is the one transform: `FaceFrame::A` is the identity,
  `FaceFrame::B` the mirror in the midplane, each map its own inverse, so the same call goes world →
  local and back. It maps heights (`y`), voxel rows (`row`), chunk rows and chunks (`chunk_y`,
  `chunk`), cells, points and vectors (`cell`, `point`, `vector`), block states and shapes turned
  over (`state`, `shape`), spans within a cell (`in_cell`), cell-face indices (`cell_face`: ±Y swap),
  boxes and their cells in layer order (`box_min`, `box_cells`, `box_flags`), and says when a mirror
  re-winds triangles (`flips_winding`). The frame of a place is `FaceFrame::at_row`, `at_height`,
  `at_chunk_y` or `of_box`. `dwell_world`'s older helpers (`up_of_y`, `mirror_y`, `mirror_chunk_y`,
  `face_local_chunk`) are shorthands for it, and `dwell-physics`' controller `Frame` is its physics
  view (it maps through it).
- **Mirroring happens at a few boundaries**, each listed in the table below as *boundary*: generation
  (a face-B chunk or LOD section is the face-local one turned over), the meshers (a face-B chunk or
  section is meshed as its mirror image and the geometry turned back), the controller (it runs in the
  mirror image of the world), and the tools (a brush stroke and a paste run face-local). Inside a
  boundary nothing knows which face it is on.
- **Tests run on both faces.** `dwell_world::both_faces!(fn name(f: FaceFrame) { … })` declares a
  test as a module with a test per face (`name::a`, `name::b`), so a face-dependent test is never
  face A's alone; `FaceFrame::under_test()` gives the faces for a loop, and `DWELL_FACE=a|b` narrows
  either to one face. The controller suites keep their whole-suite mirror run (`DWELL_FACE=b cargo
  nextest run -p dwell-physics`, CI's "on face B" step); the `both_faces!` tests need no second run:
  `cargo nextest run --workspace` runs both faces.
- **Parity tests compare whole systems across the faces**, in face-local terms, rather than spot
  checks of face B. The **mirror world** (`BifacialTerrain::mirrored(seed)`, a test tool: face B
  generated from face A's seed) makes face B face A's exact mirror image, so anything generated,
  drawn or edited on one face can be compared with the other cell for cell:
  - generation: every chunk, LOD section and column surface of face B is face A's turned over
    (`dwell-worldgen` `tests/lod.rs` `the_mirror_worlds_face_b_is_face_as_mirror_image`);
  - the chunk mesher: a chunk copied onto face B and gathered as the client gathers it meshes to
    face A's mesh turned over — shapes, skirts, plants (their jitter), trees, water, shores
    (`dwell-mesh` `a_chunk_meshes_the_same_face_local_on_either_face`);
  - the LOD: a camera on face B of the mirror world selects, generates and meshes the mirror image of
    what it does on face A, section for section (`dwell-lod` `the_lod_draws_the_same_face_local_on_either_face`),
    and asks for the mirrored chunks (`asks_for_the_chunks_around_the_camera_on_either_face`);
  - targeting: a half-height top's box and ray hit (`dwell-world` `tests/world.rs`);
  - tools: every brush preset's stroke (`dwell-game` `brush_tool.rs`), and a copy pasted on either
    face from either face (`region_tools.rs`);
  - the controller: every scenario mirrored (`DWELL_FACE=b`, PLAYER_CONTROLLER.md §11);
  - the eye, the camera's axes, the sounds of a whole session, the cloud deck, and the shaders'
    face-dependent pieces (`dwell-client` `render/face_shading.rs`: plant sway, water ripples and
    sparkles, by their Rust twins).
- **The shaders' face frame.** WGSL carries a frame as its sign (`face: f32`, ±1);
  `FaceFrame::wgsl()` gives the shaders `face_at(y)`, `face_y(face, y)`, `face_up(face)`,
  `face_vector(face, v)` and `face_rise(face, v)`, the frame's maps, and every shader goes through
  them rather than `MIDPLANE_Y`. A face-dependent piece of shading is a function in
  `render/face_shading.rs` with a Rust twin tested on both faces.
- **Playing face B.** The dev link `?spawn=b` starts a local world with the player on face B's
  spawn (`BifacialTerrain::spawn_point_b`, a world file of its own, `…-faceb`), and a bench scenario's
  `"face": "b"` does the same (`docs/DEV_TOOLS.md`). Real worlds still spawn on face A (§9).

### 11.1 The audit (2026-10-20, completed in 0.21.x)

Every place that branches on the face or mirrors coordinates, and how it stands. *Boundary*: one of
the documented mirroring boundaries. *Frame*: goes through `FaceFrame` (or a shorthand of it, or the
shaders' WGSL frame). Since the second pass (`claude/face-gaps`) no system carries a face sign of its
own: the renderer, the netcode, the eye and camera, the sounds and the bodies went onto `FaceFrame`,
and the check of §11.2 keeps it so.

| Area | Where | How it handles face B | Status |
|---|---|---|---|
| Face of a place | `dwell-world` `voxel.rs` `face_of_y`, `face_of_chunk_y`, `up_of_y`, `mirror_y`, `mirror_chunk_y`, `face_local_chunk` | shorthands | Frame |
| Worldgen chunks | `dwell-worldgen` `bifacial.rs` `generate`, `solid_at`, air test, sky floor | face-local generator, chunk turned over | Boundary; parity test |
| Worldgen LOD | `bifacial.rs` `generate_lod` (rows mirrored, `surface_b` face-local), `lod_bounds_at`; `dwell-world` `lod.rs` `mirror_row_bottom`, `row_face`, `first_face_a_row` | face-local section, rows reversed | Boundary; parity test |
| Trees, pines, structures, decorations | `terrain.rs` `lod_trees`, `placements.rs` | placed by the face-local generator | inside the boundary |
| Spawn | `spawn_point` (face A, §9), `spawn_point_b` (dev link) | — | Frame |
| Chunk mesher | `dwell-mesh` `mesher.rs` `mesh_chunk_with(mirror)`: `mirror_padded`, `flip_arrays_y`; the job's flag from `face_of_chunk_y` (`dwell-game` `chunk_stream.rs`) | meshed as the mirror image, turned back | Boundary; parity test. **Fixed:** plants were jittered by the mirror image's world corner (a face-B plant did not look like its face-A twin) |
| Shapes, the dirt skirt, plants, fluids, flooded shores | `mesher.rs` (`side_rule`, `plant_sink`, `fluid_side`) | inside the mesher's boundary (a face-B state is stored turned over) | parity test |
| LOD meshing | `lod_mesher.rs` `mesh_section(mirror)`: `mirror_rows`, `flip_mesh_y`, ±Y skirts swapped | mirror image, turned back | Boundary; parity test |
| LOD selection | `dwell-lod` `system.rs` (`view_face`, the other face culled beyond 512 m, `surface_b` for face-B sections, `mirror_section_origin`), `grid.rs` (`is_face_b_section`) | by the camera's face | parity test; straddling sections (levels ≥ 7) are meshed for the viewer's face by design (§10) |
| Controller | `dwell-physics` `frame.rs` (`Frame`, `FaceQuery`), `controller.rs` (face switch, ladder rows) | runs in the mirror image | Boundary (via FaceFrame); the mirror suite |
| Gravity, flip band | `physics.rs` (a body's face by `FaceFrame::at_height`, its depth and motion face-local), `config.rs` | the face's up, faded across the band | Frame; `tests/bifacial.rs` |
| Terrain collision | `physics` `terrain.rs` (`up_of_y` for derived shapes) | the face's up | Frame |
| Netcode | `dwell-physics` `net.rs` `face_of_flags`, `face_bit` (the only readers and writers of the `FACE_B` bits); the controller's state carries a `FaceFrame`; `predictor.rs` (local reset, remote proxies take a `FaceFrame`) | a bit on the wire, a frame everywhere else | Boundary (the wire); round-trip test |
| Editing | `dwell-world` `edit.rs` `below`, `unsupported_after`, `plant_sink_at`, `target_box`, the raycast's plant-above | the face's up | Frame. **Fixed:** `target_box` outlined and aimed at a face-B half-height top as a whole cell |
| Targeting, the camera | `dwell-game` `block_interaction.rs` `view_direction` and `dwell-client` `render/camera.rs` `aim` (both `FaceFrame::look`; the duplicate `view_forward` removed), the audio listener (the camera's axes), the LOD camera's pitch (`FaceFrame::pitch`), mouse and touch turns (`FaceFrame::handed`) | a look toward the face's up | Frame; `aim` tested on both faces |
| Brushes and the ghost | `dwell-game` `brush_tool.rs` `stroke_at`, `covered_at` | stroke run face-local | Boundary (via FaceFrame). **Fixed:** strokes ran in world terms on face B (trees grew into the ground, overlay painted the buried side, flatten/raise/lower inverted). Brushes are only enabled in the sandbox (face A) today |
| Copy and paste | `region_tools.rs` (clipboard face-local, `paste_box`) | face-local clipboard | Boundary (via FaceFrame). **Fixed:** a face-B paste ran from the aimed-at cell into the ground, and a copy from the other face pasted upside down |
| Eye, camera across the midplane | `dwell-game` `eye.rs` (face-local heights, the eye's swing across), `face_flip.rs` (a `FaceFrame` per tick, the look continued by `FaceFrame::look_across`, the roll upright); `dwell-look` `motion.rs` (face-independent) | smoothed face-local | Frame; tests on both faces |
| Sounds | `dwell-game` `sound.rs` (`MoverSample.face`: speeds along the face's up); the client's ground and body probes (`mover_sample`) | face-local | Frame; a whole session sounds the same on either face (`both_faces!`) |
| Remote players, your body and arms | `remotes.rs` `RemoteView::face`; `dwell-client` `bodies.rs` (the animator gets face-local velocities: `dwell-characters` never knows the face), `render/characters.rs` `body_frame` (the frame's up; a proper rotation, not a mirror), the dead view's camera | drawn upside down | Frame; `body_frame` on both faces |
| Puppets (dev tools) | `game.rs` `Puppets` (a `FaceFrame`; the JS API's sign converted at `web/app.rs`) | on either face | Frame |
| Fluids under the eye | `dwell-world` `fluid.rs` `eye_fluid` | the face's up | Frame |
| Light, fog, sky, clouds, water | `dwell-client` `render/shaders.rs` and `render/characters.rs` (`own`, `lambert`, `key_light`, `fog_amount`, the underwater fog, reflections, caustics, the ink over the sea: all through the WGSL frame), `render/face_shading.rs` (plant sway, ripple and sparkle normals), `dwell-look` `lib.rs` (`sky_color`, `light_weight` take a `FaceFrame`), `sky.wgsl`, `clouds.rs` (`deck_height`, `cloud_uniforms`) | per fragment by its face; the sky, fog and clouds in the viewer's frame | Frame; the deck and face_shading on both faces. **Fixed:** a face-B plant swaying in the wind stretched away from its ground instead of bending toward it, and face-B water's ripples and sparkle facets were not face A's mirror image. Face B's clouds are another part of the noise (by design) |
| Outline, edge blur, motion | `shaders.rs` (`ink_above_sea` through the frame; the outline and blur are screen-space), `dwell-look` `outline.rs`, `motion.rs` | face-independent but for the sea test | Frame |
| Spyglass | `dwell-look` `spyglass.rs` | face-independent (zoom) | — |
| The LOD's row maths | `dwell-world` `lod.rs` `mirror_row_bottom`, `dwell-lod` `grid.rs` `mirror_section_origin` (`FaceFrame::span`), `system.rs` `view_face` | spans mirrored | Frame |

**"Stray dark slabs" (deferred bug, 2026-10-15): the camera was inside face B's rock.** Reported by
the day/night work from its `day-night-face-b` scenario, whose camera was written as "50 m over face
B's sea" at y = −4,150 when the midplane was at −2,048. Seed 7's face B stands 402 m high at the origin
(there is no sea there), and after Phase 16 moved the midplane to 0 the camera was still in the ground:
what it drew is the inside of the terrain — chunk and section walls seen from within (dark under face
B's dawn light), leaves and grass floating against the sky. Face A drawn from inside its rock looks the
same (grey walls by day); no face-B-only geometry is involved, which the parity tests above confirm
for generation, the chunk mesher and the LOD. Fix: the scenario's camera is now 50 m over face B's
ground (y = −452). Shots of both (in rock, and over the ground) are in the PR.

### 11.2 The both-faces check [built, 2026-10-20]

A plain `#[test]` can quietly cover face A alone, and new code can mirror by hand again. A test
scanning the sources keeps both out: `dwell-world` `tests/both_faces.rs`, run by every
`cargo nextest run --workspace` (CI's test job). It reads the face-sensitive crates (`dwell-worldgen`,
`dwell-world`, `dwell-mesh`, `dwell-lod`, `dwell-physics`, `dwell-game`, `dwell-look`,
`dwell-characters`, `dwell-sim`, `dwell-client`) and fails, naming the file and line, when:

1. **code mirrors by hand outside the frame**: the mirror's sums (`MIRROR_SUM`, `MIRROR_CHUNK_SUM`,
   `FaceFrame::MIRROR`), `2 × MIDPLANE_Y`, a face found by comparing with `MIDPLANE_Y`, a WGSL face
   sign made with `select(-1.0, 1.0, …)`, or the wire's `FACE_B` bit used outside `net.rs`'s
   `face_of_flags` / `face_bit`. `face.rs` and `voxel.rs` (the frame itself) are exempt; any other
   line is allowed only as a documented boundary, marked `// face-boundary: <reason>` on it or next
   to it;
2. **a face-dependent test runs on one face**: a plain `#[test]` whose code names face B, the
   midplane or a mirror (`FaceFrame::B`, `Face::B`, `FACE_B`, `MIDPLANE…`, `MIRROR…`, `mirrored(`,
   `spawn_point_b`, `face_b`, `.is_b()`, …) must be a `both_faces!` test, loop over
   `FaceFrame::under_test()`, or carry `// face-a-only: <reason>`, `// face-b-only: <reason>` or
   `// both-faces: <reason>` (it compares or crosses the faces itself). The tests that named a face
   when the check arrived are listed with their reasons in the check (`KNOWN`); the list only
   shrinks. `dwell-physics/tests` is exempt: CI runs that whole suite again on face B.

The check is a heuristic, not a proof: a test that builds only face-A fixtures without naming any
face is invisible to it. That is why face-dependent systems also get a parity test over the mirror
world (§11), and why new face-dependent code is written face-local through `FaceFrame`, which the
first rule enforces. Its own test (`the_check_catches_what_it_is_for`) pins what each rule catches.

## 12. Bedrock and portals **[built, Phase 48]**

Owner (2026-10-21): "Perhaps for now, let's add a few random portals to the other side and add
bedrock back in between the two faces. So the "portals" are holes through the bed rock leading up
to the surface on both sides."

**Data** (`shared/worldgen/midplane.json`, validated and generated by `shared/worldgen/gen.mjs`
into `dwell-worldgen` `midplane.gen.rs`; CI fails on stale output) and **engine**
(`dwell-worldgen` `portals.rs`, a small fixed vocabulary: a round shaft, a rim, a ladder up one
wall, clear ground):

| Key | Value | Meaning |
|---|---|---|
| `bedrock.block`, `thickness` | `dwell:bedrock`, 1 m | The block in each face's first rows from the midplane (2 rows of ½ m: 2 m in all); must be indestructible and inside the core |
| `portals.count` | [4, 8] | The seed picks how many (`portals::wanted`) |
| `spacing`, `within` | 3 km, 26 km | The least distance between portals; the disc they are searched in (the island is 20–30 km in radius) |
| `attempts`, `inland`, `maxHeight` | 8 000, 150 m, 40 m | Seeded points tried; a site needs land on both faces over its footprint, ≥ 150 m from both coasts, its ground ≤ 40 m above sea level on both (so a shaft is at most ~144 m long, ~48 s of climbing) |
| `radius` | 2.5 m | Columns whose centres lie within it are open: 21 columns, 5 m across |
| `rim` | stone, 1 m wide, ½ m high | A ring round each opening, on flat ground |
| `ladder` | `dwell:ladder`, wall south | The ladder in the wall-side column, the whole length, to the rim's top |
| `clear` | 4 m | No tree, structure or decoration within this of the rim, nor the spawn's patch |

**Sites.** Integer-hashed points uniform over the `within` disc, tried in order: a point is kept
when both faces have suitable ground over its whole footprint and it is `spacing` from the sites
kept before, until the seed's count. A pure function of the seed and both faces' height fields
(the generator's deterministic arithmetic), searched once per seed and rules and shared by the
faces' generators (and cached in the process: some tools build a generator per chunk); ~1–5 ms
natively. Each chunk asks which sites reach it and what each of its columns is (shaft, rim, clear
ground or none); a ladder's top comes from its wall column's `column_at`. So chunks generate in any
order, natively or in WASM, and agree.

**Shape.** In a shaft column every cell from the face's ground down to the midplane is open (the
bedrock and core cut through); the ladder column holds the ladder from the midplane to the top of
the rim it climbs out onto, on both faces, so it runs unbroken from face B's opening to face A's.
The rim columns are flat ground with the rim on top; the shaft's wall keeps its bedrock. On face B
it is all mirrored, as everything is.

**Getting through.** Gravity points toward the midplane on both sides and nothing brakes a body in
the flip band (§10), so a player who falls into a shaft swings across the midplane and rises out of
the other opening nearly as high as they fell from (or back again), and the landing can hurt. The
ladder is the way through: step off the rim over the ladder (it takes hold), look down and hold
forward; across the midplane the look continues (§6) and forward climbs up the far side to face B's
rim. Alternatives the owner may prefer: a water column in the shaft (swimming slows the fall), or
drag in the flip band inside a shaft.

**Integration.** The sky test leaves room for the rims; the LOD cuts the shafts at levels whose
cells are no wider than the radius (levels 0–1) and draws the ground over them coarser; the
hydrology prototype ignores them (they don't change the height field). Edits, region tools and
brushes leave the bedrock (`indestructible`; the client predicts with the server's check). Dev
aids: `?spawn=portal` (`portal-b`) spawns a new local world's player on the rim over the ladder of
the portal nearest the centre, on face A (B); the bench's `portal-a` and `portal-b` scenarios.

**Compatibility.** Generator version 20 changes the terrain every seed generates (the bedrock, the
portals) and the block registry gains a block: a new compatibility line (RELEASES.md §3).

