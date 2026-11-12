# Known bugs

Open bugs reported by the owner, with how to reproduce them and what is known. Fix them one at a time, each with a
regression test that fails first (CLAUDE.md). When a bug is fixed, move it to **Fixed**, giving its version and the test that
pins it.

## Open

### 93. Vertical swings (axe, sword, …) play twice for one swing

Owner, 2026-11-12: "For vertical swings on axe, sword, etc, it sort of double swings." One vertical swing shows two
down-strokes (or a stroke, a snap back and a second stroke). Suspects: the vertical clip's keyframes loop or include a
second stroke; the swing restarted by a second trigger (the click and the hit event, or the server's echo of the
swing re-starting the local one); the level/vertical choice flipping mid-swing. Check with `clip_probe` and the
swing's trace, both faces.

### 92. Downhill water shows edge triangles and thin lines at its ½ m level steps

Owner, 2026-11-12 (screenshot + block dump): "I'm seeing some edge triangles and thin lines at slope transitions on
downhill water." Build 0.54.142 (f68d23af), seed 950418385, generatorVersion 8, feet (1061.8, 79.6, 12359.4), yaw 313°,
pitch −15.6°. A held, asleep river on sand (`dwell:sand[flooded=true]`, `dwell:sand_ramp[facing=south,flooded=true,
half=bottom,shape=ramp]`): the surface grid's levels step down ½ m at a time along the flow (81 → 80.5 → 80 → … → 68.5,
in diagonal bands), every column `held`, none awake. In the screenshot each band edge draws a bright vertical
triangle where a step meets another step at a corner, and thin light lines along the step edges. Suspects: the water
mesher's side faces (skirts) at a level step — drawn as full quads/triangles between neighbours whose corner heights
are averaged differently, so a corner shared by three levels leaves a sliver or wedge; or the surface's per-corner
smoothing across a step (T-junction cracks showing the sky/specular through). Expected: a downhill river reads as
one continuous sloped surface (or clean steps), with no wedges or cracks.

### 91. A 10× world sits for minutes on "Shaping the world map… 0/2" with no sign of progress (fixed natively after 0.54.143; the owner's browser check outstanding)

Owner, 2026-11-12 (M4 Max MacBook Pro): "I tried to start a 10x world … And it's up to 3 minutes so far on 'Shaping the
world map...' 0/2. Doesn't indicate well what it's doing." At 10× the world map runs at a 1 km cell (about 1 M cells
a face, today's count), so it should take seconds, not minutes; something scales with the radius or the uplift's
×100 counts. And the stage reports only whole faces (0/2), so minutes pass with no visible progress. Expected: a
10× map in seconds (or a measured, honest estimate), and progress inside each face's build (its steps: base field,
tectonics, hydrology, erosion, rivers, biome field).
Owner, later: "Looks like it started the server 200+ seconds in and now it's at almost 1 minute looking for a place to
stand. And still 0/2 280+ seconds in on shaping the world map." So the map takes ~200 s, the spawn search ~1 minute
more, and the map stage's counter stays at 0/2 even after the server has started (it never advances or completes).
Then: "Loaded in 300s in, but I can only see the skies of both faces, no world." / "Can't move." The world finished
loading after ~300 s but no terrain drew (only the sky of both faces, as if the player is in the gap between them or
no chunks arrived) and the player can't move (no ground chunks, so the controller waits; or the spawn point is
invalid at 10×, e.g. outside the generated area or NaN). Likely related to #76 (memory at big sizes) or the spawn
search's result at 10×.
Then: "Oh I see, 'Could not connect to 10x...'" — the empty sky is the join failing (`client/src/main.ts`'s
"Could not connect to …" status), not missing terrain: the in-browser server either died (memory, #76) or didn't
answer within the client's connect timeouts (`transport.rs` CONNECT_TIMEOUT_MS 8 s, `peer.ts` 20 s) while still busy.

**Cause and fix (2026-11-12, measured natively, BIG_MAP.md §3.1).** Two steps of a face's map grew with the
radius, not the cell count: the uplift measured every land cell against every segment of every range (300 ranges at
10×: 17 s), and the channels' index listed each ~1 km river segment in thousands of 64 m squares (21 s and a **12.4 GB**
index). One 10× face took 43.7 s natively (1×: 1.9 s) and a native 10× server 114 s at a 12.4 GB peak. In the browser
that is past the 4 GB of WebAssembly memory: the map workers never finished (so the stage stayed at 0/2: no face's
event was lost, none arrived), the local server built its own maps inside its "Finding a place to stand" stage (the spawn search itself is
at most 1.2 s natively at every size) and its worker died, and the page said "Could not
connect" over an empty sky. The local world has no connect timeout (the loopback waits for the worker's `ready`), so
no timeout was to blame. Now: the spines are indexed by where they reach and the channels' square follows the cell
(both exact: the 1× goldens are unchanged); a 10× face is 3.1 s and a 10× server starts in 6.2 s at 484 MB.
`dwell-worldgen/tests/map_build_budget.rs` pins the op counts per cell at 1× and 10× on both faces (red on the old code:
21 322 segment tests a land cell at 10× against 208 at 1×). The map builder reports its steps (`progress.rs`, the labels
`loading.json` `mapSteps`) through the map worker to the stage ("Shaping the world map — rivers (5/6), face 1 of 2", the
bar moving inside each face; `ui/mapProgress.ts`, Vitest in `mapProgress.test.ts` and `worldMaps.test.ts`); a failed map
build ends the stage saying so; a dying local world worker says it ran out of memory (`workerFailure`,
`loopback.test.ts`) and closes the session with why, also once playing. `tests/world_size.rs`
`a_10x_world_s_spawn_stands_on_its_ground` (both faces) checks the 10× spawn is on the disc with its feet on solid
ground. Not checked here (no GPU in the cloud session): the 10× world in the browser, walking there, and its memory
(#76); the 10× channel index still holds ~16 M entries.

### 90. Volumetric clouds look badly dithered and pixelated where they overlap, mostly at cloud level

Owner, 2026-11-12: "The clouds also look really badly dithered when they overlap? I think mostly when you get to their
level? Looks very pixelated." Suspects: the volume pass runs at ½ or ¼ resolution with a per-frame jitter and temporal
accumulation; near or inside the layer the reprojection fails (the camera moves through the volume, depths change
fast), so the history is rejected and the raw jittered low-res result shows, then a plain upsample makes it blocky.
Fixes to weigh: blue-noise jitter, a depth-aware (bilateral) upsample, neighbourhood clamping instead of history
rejection, more steps or full resolution when the eye is in or near the layer.

### 89. Sub-60 fps flying at High on the owner's M1 MacBook Air (measured; partial)

Reported 2026-11-12 (owner): "Im seeing on this m1 Mac that im getting sub-60FPS when flying on High. Im wondering
if the new trees and volumetric clouds are at fault!" Target (WORK_QUEUE item 30): High at 60 fps on an M1 Air.

Measured on this M1 Air (`npm run bench -- --warm --unthrottled high-fly`, docs/DEV_TOOLS.md "Flying at High on the
M1"): flying at level 4 from the starter's mixed forest toward the tall cone, `gpu.frame` 8.8 ms mean over open
ground and **13.4–15.0 mean, p95 15.6–17.3 (18.5 in one run) over the forests**; the main thread is not the limit
(`frame.cpu` p95 ≤ 10.5 ms, no long tasks). **Not the volumetric clouds**: Medium → Low (the 2D deck) for the
same flight changed nothing (15.0 → 15.7 ms, within the spread); their steps are fixed per pixel, so altitude does
not grow them. **Not streaming**: 330–400 MiB of chunk meshes uploaded per 4 s over forest, but a 1 MiB desktop
upload budget (from 4) moved the frame ≤ 1.8 ms and lagged the uploads, so it was not adopted. **The trees are, as
standing still in #78**: over forest the near pass goes 3 → 6–8 ms (the cutout leaves and tall grass) and the sun
shadows ~2 ms more. Speed effects' motion pass costs ~0.5 ms; bloom and shafts nothing measurable.

Outstanding: no cut yet (45-minute box). The levers: a Tall grass setting (a mesher flag, #78's outstanding item),
the far trees as impostors (WORK_QUEUE item 31), and High's sun shadows over forest; a vsync run of `high-fly` for
the dropped-frame count and a faster flight (level 6+) for the streaming at speed.
### 87. An NPC's raised vertical block leans the blade back over its shoulder

Found fixing #86 (2026-11-12), not reported: `clip_probe` puts the third-person `guard_vertical` pose's
sword grip at (0.50, 1.38, 0.33) and its tip at (0.38, 1.73, -0.45) m from the feet (right, up, ahead):
the blade ~66° off upright, its tip behind the body, where `biped_guard_vertical`'s comment means "the
blade upright ahead of the face" (a first guess, never fitted). The sword's `guard.stances.vertical.blade`
follows what is drawn, so a clash's sparks land on the drawn blade. Fix: fit the clip with
`clip_probe --fit` (as the level block was), then refit the data (the probe test fails until they agree).

### 85. A dev `look` with an object kicks the player: "refused the connection: Malformed message"

Found 2026-11-12 filming the sword duel. `__dwell.look({yaw: 0, pitch: -5})` (the signature is
`look(yaw, pitch)`, numbers) turns the look to NaN; the next sword swing sends its `Sweep` with a
NaN look, the server's decode refuses it (`r.check(look.iter().all(is_finite))`) and rejects the
whole session as malformed. Repro: a flat local world, `pick('dwell:sword')`, `look({yaw:0,pitch:-5})`,
`edit('break')`. Fix: the dev hook checks its numbers (as `teleport` now does, #84), and the client
never sends a non-finite look (clamp or drop at the sender). Open.

### 84. `?at=` at a far spot hung the page loading (part of #84; not reproduced)

Found 2026-11-12 reaching a wolf pack 5 km off. Natively a spawn link 5 km off (in the air, at the
ground, under it) loads in under 2 s (`game_local.rs` `a_spawn_link_five_km_off_loads`), and in the
browser a dev teleport of 400 m+ arrived and its terrain streamed. Not reproduced: needs the link
and seed that hung. The teleport half is fixed (Fixed, #84).

### 78. 26 fps at High on the owner's M1 MacBook Air ("I think cards are killing my m1 mac")

Reported 2026-11-12 (owner): "It seems like we're hiding some performance settings. I don't see 'tree' options in
our settings for graphics. I think cards are killing my m1 mac too." / "Running 26fps." / "GPU at 31ms". On the
cards: "I think the internal leaf cards may be sucking up a lot of unneeded power and I think the leaf cards on the
back sides of trees and farther out trees are rendering which is unnecessary." Target (WORK_QUEUE item 30): High at
60 fps on an M1 Air.

Measured on this M1 (8-core, 2560×1600 buffer, High = 50 % render scale, `npm run bench -- --warm --unthrottled
high-forest high-open`, `gpu.frame` mean ms, docs/DEV_TOOLS.md "High on the M1"): the starter's broadleaf spot
15.5–15.7 ms at High (55 fps unthrottled), open ground 9.2. The leaf cards cost **0.6 ms** there; the cutout meshes
as a whole (the leaves' cutout cubes, tall grass, flowers, ladders and the cards) **4.5–4.7 ms**, the full-detail
distance 256 → 128 m 2.4–2.7, sun shadows 1.4–1.7, water Medium → Low 0.8–0.9, bloom 0.4–0.6; shafts, torch
shadows and clouds ≤ 0.3. This Mac did not reproduce 31 ms at High (the owner's view was likely denser or
fuller-screen; not known).

Done (this change): leaf cards culled three ways — no cards on a face looking into a cavity inside the crown (a leaf
cell within `interiorCells` 4 straight out), the vertex shader drops a card whose plane the eye is more than
`backCullM` 0.5 m behind (the crown's far side) and every card past its fade's end (they were rasterised and
discarded per pixel), and the fade pulled in to 24–40 m (was 48–64) — card cost 0.62 → 0.29 ms; The starter's mixed forest (`high-mixed`, denser) read **17.05 ms** at High after the culls (cards 0.23, the cutout meshes 4.2), over budget, so **High's full detail went 256 → 192 m** (shared/settings/graphics-presets.json; 256 m and up are Ultra's): **13.88 ms** there (61 fps unthrottled). Leaf cards moved
to a Graphics → Trees group; an audit test (`client/src/ui/graphicsAudit.test.ts`) fails when a renderer tier
function has neither a Graphics setting nor a stated reason. Tests: `a_crowns_inner_cavity_draws_no_cards` (red
with `interiorCells` 0: 36 and 42 cards in the hollow on faces A and B), `a_leaf_cards_packed_normal_is_its_plane_on_both_faces`
(red on face B without the mirror's re-pack).

Outstanding: a Tall grass setting needs a mesher flag (`ChunkFlags`): the renderer's cutout switch hides the leaves
and ladders with the grass, so it can't be the setting; distant (LOD) shadows follow Shadows; tall grass density,
far trees, hit shards and far herds have no tier function yet to put behind a setting; the owner's 31 ms needs his
view (a dense forest spot) to reproduce.

### 76. A bigger world's page takes memory that grows with the area (2× +230 MB, 4× +1.07 GB)

Found 2026-11-11 (0.54.59) checking BIG_MAP.md M2b on the M1's warm bench (seed 0, 1280×720, `memory()`
once settled at the spawn; session scratchpad `bigworld/`). Everything but the meshes (which follow the
view), in MB:

| | core | server | each of 4 job workers | the other 4 workers | non-mesh total |
|---|---|---|---|---|---|
| 1× | 194 | 61 | 77–79 | 22–28 | 749 |
| 2× | 198 | 203 | 107 | 22–26 | 979 |
| 4× | 337 | 334 | 241 | 22–24 | 1 819 |

The world map is 1002² cells at every size (its cell doubles), and natively a 2× world peaks at 176 MB
against 1×'s 164 MB, so something in the job workers and the local server grows with the disc's
area (job workers ~+9 MB a 1× area). Suspects: a grid at a fixed cell over the disc or the build volume
(the LOD's or streamer's per-column tables, the water's held sea, the hydrology grid's
`grid_for_radius(cell, WORLD_RADIUS)` callers), or the big map's disc-level reveal state. Next: a
`memory()` per worker kind after each of `prepareWorld`, the spawn search and the first terrain, at 1×
and 4×. At 8× and 10× this decides whether the desktop test fits at all.

### 77. A bigger world's island loses its mountains and fills with small lakes

Found 2026-11-11 (0.54.59), same check: the big map's widest level at 1×, 2× and 4× (scratchpad
`bigworld/bw-1x_map-l2.png`, `bw-2x_map-l2.png`, `bw-4x_map-l2.png`). 1×'s island has two snowy ranges
and a few long rivers; 2×'s has thin ridges and its interior speckled with small lakes and a
grid-like mesh of streams; 4×'s has no snow at all and the speckle covers the island. The ground at
the spawn still plays well (`bw-2x_spawn.png`, `bw-4x_spawn.png`). Suspects: the uplift's ranges and
the island shape's absolute tunables (BIG_MAP.md §1) narrower than the map's coarser cell (200 m at 2×,
400 m at 4×), and the erosion and lake fill tuned per cell rather than per metre. A worldgen look
question for the owner (the plates are to decide the land, REALISTIC_WORLD.md), not a crash.

### 69. A dark vertical column in the far terrain before the starter world's volcano

Found 2026-11-11 (0.52.64) on the starter world's tour: `?worldgen=test:starter&fresh=1&at=8571.5,350,-4866.5`
(15 km east of the arc's first cone, the default look). A thin dark-brown column, a lighter foot,
stands from the badlands' horizon up past the top of the screen, in front of the cone (session scratchpad
`leftovers-biomes/tour_volcano_east_facing_west.jpg`). Narrowed 0.52.65: it is in the **LOD** (gone with
`?lod=0`, `seam_lod_off.jpg`); it is **not a skirt** (`?loddebug=skirts` draws the skirts magenta, the
column stays brown, `seam_skirts_debug.jpg`); and the height field has **no spike** there (every 16 m
column within 12 km of the spot, west and south, stands within 60 m of its neighbours' highest; a probe
in `tests/starter_world.rs`, not kept). With `loddebug=nostandins,nocull` it stays too (`seam_nostandins_nocull.jpg`): not a stand-in, not the per-section culling. Suspects next: a LOD section's cells with a wrong top (a column
of a coarse level whose surface or colour comes from a log, e.g. a tree folded into the LOD, or a
cell past the build volume), or the cone's crater ice dome at a coarse level. Next: a level tint switch (there is none yet in
`?loddebug=`) to see its level, then the section's cells natively at that level. Not fixed.

Narrowed further after 0.52.92 (session scratchpad `lod-bugs/`): it is the **distant shadows** (DAY_NIGHT.md
§12), not the LOD's geometry. With `?lodshadows=off` it is gone (`run2/v69ns/noshadow.png`); with
`?loddebug=levels` it darkens whatever lies behind it, across sections of different levels, keeping
their colours (`run1/v69/east-levels.png`), so it is a shade, not a surface. It stays put in the world
when the view turns (yaw −80: it moves right of the centre, `run2/v69yaw/yaw-80.png`): a streak along
the light's direction (the world's opening time; the sun low in the east), not tied to the screen. At
the spots' time 0.45 it is not there (`run3/v69t/west.png`). It is fixed in the world on the line
z ≈ −4866 (the spot's z, 15 km due east of the cone; along the low eastern sun): from 100 m north it stands right of the centre
(`side/v69m/z4766.png`), and it does not follow where the player started (`player/v69p/`). Not the
sweep's max mips (with every step reading mip 0 it stays, `nomip/v69a/west.png`). So a caster stands
east of the hill on that line, narrow across it, in the heightmaps (the chunks' and sections' tops):
suspect a thin ridge along the cone's +X ray — the arc cones' column detail (`peak_relief`, radial
cleavers: an angle seam at 0?) or a section's sliver. Next: the heightmaps' texels along z = −4866 east
of the hill (a dev readback). A native probe (seed 0, face A; not kept) finds such a line: a narrow
ridge on the cone's east ray, z ≈ −4880…−4850 from x ≈ −2200 to −1500, its crest 15–25 m over the
ground 24 m north and south and a few metres wide (a radial cleaver, `peak_relief`). Whether its
shade should reach the hill is the question: the sweep has no sun disc across the light, so a ridge a
few metres wide shades as a wall would.

### 60. On the software renderer the page draws no frame for ~6 s just after the first torch is placed

Found 2026-11-10 chasing main's e2e failure on 0.51.8: `edit.spec.ts`'s every-palette-block test failed
3/3 on CI with a look or a hotbar key not taken within 5 s (the target still the ground cell under
the placed block; slot 4 selected after Digit6), both waits for the next frame. Locally (SwiftShader,
the CPU throttled 6× by CDP) the page drew no `requestAnimationFrame` for 6.0–6.2 s right after the
torch (slot 12) was placed, in two of three runs, while `page.evaluate` still answered (the main thread
free: the frame or the GPU process stalls, not a long task). The thumbnail capture (0.51.6) was
ruled out (its drawImage/toDataURL ran at other times, ~200 ms). On CI the stall hit earlier slots,
so the trigger there is not certain. The spec now waits up to 20 s for a frame (`FRAME_STALL_MS`,
0.51.9). Suspects: a pipeline first compiled for the torch's light or flame, or the light update a
torch starts. Next: a bench run that places a torch on `?world=flat` under SwiftShader and reads the
frame phases around it.

### 56. A second water surface layer under the water, strange to be between

Owner, 2026-11-09 (on 0.50.76–0.50.78): "There's a weird underwater double surface layer, and it
looks strange when you're between them." Then: "I actually can't repro that double surface layer
issue." Parked: not reproduced by the owner or here; reopen with a screenshot if seen again. Suspects: the 0.50.20s underside texture or the waterline band drawn at a second
level (e.g. a flooded cell's top or the ½ m step's lower surface seen from below), a sea/river
cell's top plus the cell's own underside, or something from the heightfield work (0.50.69–0.50.78)
even with the switch off (the mesher's `water_tops` flag is on, so tops should be meshed as before).

### 54. Close chunks drawn in low detail while carried, and edited chunks refined only up close

*Reported by the owner, 2026-11-09,* riding a raft at walking or swimming speed on a fast desktop: "close
chunks aren't being correctly prioritized. And previously loaded chunks are not being unloaded behind
you" (low detail within 10 m ahead), and edited chunks (by water) "refusing to load in higher detail …
unless you get really close". **Not reproduced yet.** The bench's `carry` step (docs/DEV_TOOLS.md) rode
seed 0 from its spawn at 3 m/s for 45 s (135 m, 1280×800, the default preset, this M1 Air's GPU): the
chunk under the feet and the one 10 m ahead were drawn as chunks in all 2,700 frames (`probe.under`,
`probe.ahead` 0), `job.chunk.wait` p95 6.6 ms, max 25 ms, the generation queue (`pool.gen`) empty but
for single frames. Natively, `game_local.rs`'s `riding_a_raft_streams_the_terrain_ahead_and_drops_it_behind`
is green too. Next: the `carry-flood` ride (a dug channel, the probe's `target` against `control`), a
larger window and a faster CPU, and the LOD's `errorScale` (4.65 with the default 256 MB cache at
1280×800) on a bigger screen.

**How to capture it (0.51.23; owner, 2026-11-10: still seen while flying round loading a lot):** when
close chunks stay low detail or missing, press **F8** (or Menu → Debug → Copy streaming report, or the F3
overlay's Copy streaming) at that moment and paste the copied JSON into the report; where the clipboard is
refused it shows selected in a text box to copy by hand. It lists each chunk within ±3 chunks of the
player with its streamer and LOD state and, for one not drawn in full, what it waits on (`waits.reasons`:
`notLoaded`, `generating`, `unlit`, `neighbourGeneratingOrUnlit`, `dirty` with its place, `meshing`,
`lodNotShowingChunks`, …, and its job's place in the pool's queue), the pool's queues and oldest jobs,
the streamer, the server's unloads within 48 m in the last 30 s, the LOD's cache against its budget and
the last 10 s of frames (docs/DEV_TOOLS.md § Streaming report).

**Repro detail (owner, 2026-11-09):** "I had traveled a long ways down a river at or greater than
walking speed, loading all the chunks along the way at high detail" … "for several minutes at or greater than walking speed." So it shows after a LONG trip,
not at the start. Leads this points at: (1) the worker pool's FIFO queue backs up with jobs for
chunks already passed, so new near chunks wait behind stale ones; (2) the chunk cache filling to its
Distant-memory budget and the eviction keeping the trail behind (the "not unloaded behind you" half)
while refusing new near chunks; (3) memory/upload pressure growing with the trail. Next: a bench
scenario that travels several minutes down the seed-7 river at 4–6 m/s and logs the near ring's
detail, the queue depth, the cache size and evictions over time.

**Long trip natively (2026-11-10, 0.51.1):** `game_local.rs`'s `long_trip` flies the player 8 m/s in a
straight line with the LOD on (default desktop settings: 256 MB, 256 m full detail) and the workers
finishing one job a poll, and times how long each chunk of the near ring (3 × 3 around the player, the
ground's row) waits to be drawn as a chunk. Over the **flat** world it was red: from ~1.1 km out every
near chunk waited 12 s or for good, though meshed and in the scene. Cause: `flat_lod_bounds` judged a
column inside the world by `in_world` of the column's *bottom* row, where the build sphere is only a
few km across, so past ~1 km every column read as outside, its level-1 sections Empty and their chunks
never shown (the LOD drew level 2 over them). Fixed in 0.51.1 (the disc test of the column's nearest
point); tests `the_flat_worlds_ground_is_content_kilometres_from_the_axis` (dwell-worldgen) and
`a_long_trip_over_the_flat_world_keeps_the_near_ring_at_full_detail` (red: 720 frames at x ≈ 1,290 m;
green: none over 2 s in 1.4 km). Over that trip the streamer's load stayed flat (~1,480 chunks), the
generation queue drained and nothing 14 chunks behind stayed drawn: leads (1)–(3) did not show there.
**The generated terrain (the owner's river) is not affected by this cause** (its bounds come from its
columns) and stays unreproduced: the same flight on `Rig::new()`'s terrain was blocked by a hill within
50 m. Seen there at the start's load with one job a poll: a near chunk waited 15 s while 300–500 meshes
and ~90 generations were queued — the pool's generation queue is FIFO (`pool.rs`), and a chunk is not
meshed while a neighbour is still being generated, so lead (1) remains the suspect. Next: the long trip
over terrain on a fixed path (teleport along a line, or a raft on a river) and the bench's river ride.

**Reproduced over the terrain and fixed (2026-11-10, 0.51.3):** `game_local.rs`'s
`a_long_trip_over_the_terrain_keeps_the_near_ring_at_full_detail` carries the player 5 m/s in a straight
line 40 m over seed 7's ground for 90 s (450 m; the position set every frame, so no hill stops it), the
same LOD settings and one job a poll. Red on 0.51.2: a near-ring chunk waited 356 frames (~6 s) to be drawn
as a chunk, the mesh backlog growing past 200 chunks in 30 s. The near chunk was meshed; the LOD keeps a
level-1 section (2 × 2 × 2 chunks) drawn until all 8 of its chunks are, and the near ring's sections
reach chunks 2 out, which the server sends only as they come into its view (`VIEW_RADIUS_CHUNKS` 3),
just as the ring reaches them. Those then waited behind (1) the pool's FIFO generation queue: far chunks'
generations and the LOD's jobs queued first; and (2) the re-meshes of drawn chunks, which every
arriving chunk marks around it. Fix: the pool starts chunk generations within 128 m of the player
before the other generation jobs, nearest first (`Pool::set_focus`, `NEAR_JOBS_M`; off while the
respawn preload's second centre is set, whose order the streamer and the LOD keep), and the streamer
meshes chunks in the view not drawn yet before re-meshes (also not with a second centre). Green: the
worst wait 76 frames (1.3 s; the LOD's own `FORCE_CHUNKS_AFTER_MS` wait for its level-1 section is up to
1 s of it). A first try that also put chunk meshes before the LOD's section meshes kept the respawn
preload's screen from opening in its 3 frames (`with_the_lod_a_preloaded_respawn_opens_its_loading_screen_at_once`)
and was dropped. **Left:** the owner's check on the river in the browser, and the report's other half
(edited chunks refined only up close).

### 51. Reopening a world in the same tab stalls on CI: spawn's reload and settings' reload

Found 2026-11-08 getting CI green (PR #421). On CI's software renderer, two specs fail on all three
attempts, both right after `page.reload()` of a local world: `spawn.spec.ts` ("a spawn link puts the
player back …": after the reload `terrainReady` never comes within 60 s) and `settings.spec.ts` (after
its reload the page ignores clicks for the rest of the 90 s). Both pass on a real GPU and on local
SwiftShader. Suspects: the reloaded page waiting on the old page's local-world worker or storage (a
save in flight, a lock not released), or #49's stall far longer on a reopened world. Next: log the
time from reload to the local world's start, and the storage/worker events, on CI.

### 50. Flying up at the top flight speed stops the game ticking on CI

*Found 2026-11-07 (0.50.32).* `e2e/flightSpeed.spec.ts` on CI (software renderer): after the join's
stall is waited out (#49) and 60 ticks at 8 m/s fly fine, Space held at level 11 (362 m/s, straight
up in `?world=flat`) ticks 26 controller ticks in 20 s (`state().stats.ticks`) and the player climbs 0 m;
it failed the same way with a 1 s timed hold on main at 0.50.30 and 0.50.25 (#412). It passes locally,
on the M1 GPU and on SwiftShader. Unknown yet whether the page stops drawing (streaming 362 m/s of
new chunks on the software renderer) or something holds prediction (flight loading, 0.50.13; the
stream budget, 0.50.14). Next: on CI, read `telemetry().frames.interval` and the terrain counts while
held, to tell a stalled page from a held controller.

### 49. On the software renderer the page stops drawing for seconds just after the terrain is ready

*Found 2026-11-07 (0.50.31) while getting CI's e2e green.* On SwiftShader (the suite's CI adapter, and
locally with `playwright.config.ts`), `?seed=31&spawn=b` reports `terrainReady` with 9 of ~180 streamed
chunks meshed; within a second rAF stops for 3–5 s (main thread responsive, `page.evaluate` answers, no
long task) while the rest are meshed and first drawn (`upload.bytes` peaks at ~5 MB in a frame), then
the game catches up its ticks at once. 0.50.10 does the same (~3 s), so it is not a recent regression;
`?shadows=off` and `?lod=off` keep it. All 14 pipelines are created up front, so it is not a lazily
created pipeline. The suspect is SwiftShader compiling its per-state draw routines on the first draws
(it compiles when a draw first uses a state, not when the pipeline is created). A real GPU does not
stall (the same specs pass on the M1 GPU). Effects on CI: input held right after the join moves the
player 0 m (spawn.spec, gamepad.spec, flightSpeed.spec), the loading screen's fade and the menu button's
stability wait for frames (join.spec, settings.spec). The specs now wait it out (`e2e/ticking.ts`
`untilTicking`: the streamed chunks meshed, then ticking in real time) or allow for it in their timeouts.
Next: a Chrome trace of the GPU process during the stall, to see whether it is the routine compiles or
the uploads; if uploads, a per-frame upload cap on a CPU adapter.

### 41b. A reservoir filled behind a dam never falls asleep (the rest of #41; partly fixed in 0.49.44)

*Measured 2026-11-06 (0.49.39), the remaining part of #41.* At seed 0's dam sites
(`tests/water.rs` `measure_a_raised_dams_churn`, release, 170 water minutes; 0.49.39 as built):
at z 7452 13–18 water chunks awake (mean 15–17) and 21–1,269 cells written a minute (late mean 241),
the reservoir spreading again from minute 150; at z 7380 22–32 awake (mean 27) and 0–53 cells a
minute over its last 40 minutes.

**Fixed in 0.49.44: a sleeping chunk passes its flow on** (owner, 2026-11-06: "if a chunk is
sleeping, it should be treated as if all the water flows into it"). A chunk asleep was taken to
carry no flow, so asleep beside a running reach it owed the river, woke, and paid it back over 5 s
as a wave that woke its neighbours. Falling asleep it now keeps each edge face's flow into it and
its balance (`water.rs` `keep_edges`, `kept`, `drift`); asleep it owes its balance and, across an
edge toward an awake chunk, only how far the flow there drifts from what it kept (nothing between
two sleepers). So a draining bed doesn't sleep wet on its kept flow, a film out of the rest
(shallower than the sleep band) that fell more than 0.1 mm in its calm window is not calm, and a
sleeper owing half its free water (over 0.01 m³) wakes. The fixture reservoir
(`water::tests` `a_reservoir_filled_to_its_spill_falls_asleep`, both faces, no longer ignored):
red awake all of its last 5 minutes, 45 cells written; green within the test's 30 s and none.
`a_hollow_below_a_dam_keeps_its_water_still`, `a_lake_cut_open_drains_on_after_its_player_leaves`
and the seed-0 dry-reach test stay green.

**Still open: deep water's slosh, and seed 0's sites.** `measure_a_raised_dams_churn` after the
fix: z 7452 14–21 awake, 18–546 cells a minute from minute 120; z 7380 11–29 awake, 3–113 cells a
minute. Little changed: the deep reservoirs slosh past the sleep band, which is 5 mm
(`sleepBandM`), not 5 cm. The fixture `stirred_deep_water_settles` (a 40 × 8 m basin 3.5 m deep
stirred by a block column) still sloshes ±1–2 cm and is awake all of 30 water minutes (ignored).
Tried before, not kept: a drag on slow water (60 s under 5 cm/s) settled the basin in 8 minutes but
slowed a cut lake's last levelling (`a_lake_cut_open_drains_on_after_its_player_leaves`); a ±3 cm
calm test. **Next:** damp only deep water whose rest flow is ~0 and whose level is oscillating
about a steady mean (the calm window's mean, not its start), or a calm test on the window's mean
level for deep water with the slosh's volume conserved (the chunk sleeps at its mean, the swing's
volume kept in `owed`).

### 34. e2e play.spec's two-client test times out on CI since 0.49.4

*Found 2026-11-06, from CI.* "two clients on a native server see each other move": page B (on a
150 ms, 5 %-loss link, `?netsim=150,20,5`) never reports its terrain ready within 20 s. It passes
locally on a real GPU (14 s). Main passed it at #336 and #337 (each twice, a re-run today included)
and has failed it on every run since: main at #340, PRs #338, #341 and #342 (three runs), main
with #338 reverted, and main with #340 reverted (both of those include #339). Cutting the water
currents' datagrams to a quarter didn't help, and there is no water near the native server's spawn
(seed 0, (2128, 7544)). Next: re-run main at #340 and bisect #338's pieces on a branch from #336.

### 30. A held Place builds too little at a few frames a second (partly fixed in 0.49.2)

*Found 2026-11-05 (0.48.2), from CI.* Under the e2e suite's software renderer (CI ~5 fps, a Mac's
SwiftShader ~3 fps) a held-Place build in `e2e/buildDrag.spec.ts` built its first piece and no more
(towering, walking back in Line mode, bridging help, held ladders, crouch-jump towers); those tests
had failed on CI since they were written (0.38.5). Three causes. A key pressed and let go before a
tick took the input (a slow frame, or the prediction held while the server caught up) was never
sampled: the jump was lost. The server checked each piece against
its view of the player's body, which lags the client's by the inputs still queued, a whole slow
frame of them: a piece the client placed once its body was clear went into the server's body and was
refused (a region fill's cell skipped). And the client looked for the moment a jump cleared the next
piece once a frame, and at 200–300 ms frames that ~0.25 s moment fell between frames. Fixed in 0.49.2: a key tapped between ticks counts for the next tick (`input.rs` `tapped`); a
place into a player, or a region edit entirely into one, waits up to `EDIT_HOLD_TICKS` (30) checked
each tick (`server.rs`), and the held stroke is checked at every predicted tick (`game.rs`
`drag_tick`). `game_local.rs` `towering_builds_a_metre_a_jump_at_five_and_three_frames_a_second`
(red before: 0 m of 4 at 5 fps) and `a_jump_pressed_and_let_go_between_two_ticks_still_jumps` (red
before: 0 m). The e2e spec now holds each jump for 9 of the controller's ticks by its tick trace, not
150 ms of the clock, which at a few frames a second can be one tick (a hop too low to build under).
At 2 fps a tower still misses some jumps.

**Still open:** on CI after 0.49.2, walking back in Line mode and the crouch-jump tower pass, but
towering, both bridging tests (a tower first) and the held ladder still fall a piece short (34.5 or
35.5 m of 36; the ladder's first piece missing). Main's shard 1 failed six of these before. Next:
the tick trace on CI (it builds a metre a jump locally at 3 fps).

### 23. A block can't be placed against the downhill side of a ramp

*Reported 2026-11-03 (0.44.2).* "I can't place a block against the outer edge of (downhill) side of a
shallow ramp, maybe same problem with steep ramps too?" Not investigated yet: targeting a ramp's low
face, or the placement check against its shape.


## Fixed

### 88. Volumetric clouds z-fight against tall terrain (fixed after 0.54.135; the browser check left local)

Owner, 2026-11-12: "a bug with volumetric clouds z fighting against tall terrain like mountains." Where a mountain
rises into the cloud layer, the cloud volume and the terrain flickered against each other. Cause (read in the code):
the clouds pass (`clouds_fs`) marched the whole layer, not to the scene, and kept the distance to its first cloud,
jittered every frame by `cloud_jitter` by up to a step (hundreds of metres at Medium); the composite's
`upsampled_clouds` cut the clouds against the scene by a ±5 % band of that distance, so on a slope inside the layer
the cut flipped frame to frame; and the colour held the cloud behind the rock too.

Fixed (docs/CLOUDS.md §8.9): the pass reads the scene's depth under each texel (nearest and farthest of its
full-resolution pixels) and marches only to the farthest (the whole layer where any of it is sky): no cloud behind
a texel's rock. It keeps that span beside the colour instead of the jittered first cloud, and the composite and the
history's reprojection weigh the four texels round a pixel by whether their march ended at the pixel's scene, within
a band in metres of the texel's depth range (3–15 % of the distance, `volume.depthBand`/`depthBandRange`): depths
alone decide, so nothing flips with the jitter. Tests: `crates/dwell-look/src/clouds.rs` `depth_tests` (a CPU model of
the pass and composite, red before: the cut swung 0 → 0.79 over the jitter's 16 frames, samples went past the rock),
both faces; `dwell-client` `shaders.rs` pins the wiring and naga validates the module. Not checked on a GPU (cloud
session): the look at a mountain in the layer and the `gpu.clouds`/`gpu.composite` timings are for a local
`npm run bench -- --warm`.

### 86. A sword clash's sparks show ~1.5 m beside the blades, in the air (fixed after 0.54.123)

Owner's duel, filmed 2026-11-12 (WORK_QUEUE item 35, round 5): the villager blocks and strikes back,
but the clash's sparks fly about 1.5 m to the side of the blades, in the air. Cause: the server's clash
point (`ClashNet::point`) was never on the blades: a player's strike sent the middle of the striker's eye
and the creature's body (`clash_point(eye, feet, rows)`), an NPC's blow the middle of the two eyes. The
client was right: it bursts the sparks at the point it is sent (`Game::on_clash`, world, no offset).
Fixed: each guard stance's raised blade is data (shared/tools `guard.stances.*.blade`: grip and tip from
the holder's feet, right / up / ahead, fitted to the third-person raised poses), and the clash point is
the middle of the two blades' nearest points where the swing's arc (finely, between its steps) comes
nearest the raised blade; a strike on no raised blade puts it on the striking blade nearest the body.
Tests: `crates/dwell-sim/tests/weapons.rs` `a_clash_point_lies_where_the_blades_met` (a level slash on
a villager's upright counter: red before, 0.65 m from the guard's blade; both faces) and the NPC's
blocked blow in `an_npc_blocks_the_wind_up_by_counter_then_strikes_back_unhurting` (red before, 0.41 m
from the player's level blade); `clip_probe.rs` `the_raised_blades_data_is_where_the_raised_poses_draw_it`
keeps the data within 0.1 m of the clips.

### 84. A dev `teleport` did not move the player (fixed after 0.54.116)

Found 2026-11-12 reaching a wolf pack 5 km off. `__dwell.teleport(feet)` posted whatever it was
given to the worker and answered `true`; a point that was not `[x, y, z]` of finite numbers (an
object `{x, y, z}`, a string) reached `teleport_local_player` as NaN, which refused it silently.
Now `feetOf` takes `[x, y, z]` or `{x, y, z}` and anything else throws, naming the input
(`client/src/local/world.ts`, `world.test.ts` "feetOf"); a single far teleport (5 km) moves the
player and its terrain arrives (`game_local.rs` `a_single_teleport_five_km_off_…`, which passed on
the old code too: the core was right, the page's hook was not). The `?at=` hang is open (#84 above).

### 83. The sword showed the axe's hint (fixed after 0.54.116)

"Axe: level swing (right click: vertical)" while holding the sword: the hint was a constant in
`chop.rs`. Now each tool kind's hint is data (shared/tools `hints`, every kind a tool uses needs one;
`dwell_protocol::chopping::hint`), tested in `chopping.rs` `each_tool_shows_its_own_kinds_hint` and
`gen.test.mjs` (red before: no weapon hint existed). Seen in game: "Sword: level stance (tap right
click: vertical, hold: block)".

### 79. A herd grows by several animals within seconds of coming near (fixed after 0.54.101)

Seen filming the deer (2026-11-12, seed 7, herd id 3 at 3000,2200 face A): a herd that arrived as 6 head had 9 within
seconds and 12–13 within a minute. Not a double spawn: the record leaves its face's far groups once and becomes one
creature per member (checked). The cause was births: a fed, rested pair within 2 m tried every half-second decision
at the deer's 0.5 chance, so a fed herd of 6 had 6 young in its first minute. Fixed by pacing births as data (each
species' `near.mate`): each parent, and the young, rest `restS` before mating again (deer 600 s, wolves 1800 s), and a
group has at most one young every `groupEveryS` (deer 240 s, wolves 900 s), counted from when it came near, so a herd
never grows in front of a player who just arrived. Pinned by `dwell-sim` `herds.rs`
`a_fed_herd_near_a_player_has_at_most_one_young_a_minute` (red: 6 young in a minute; it also checks the herd spawned
once) and `births_follow_the_data_s_pace` (the pace by data alone), both faces.

### 80. Deer in a herd stand inside each other (fixed after 0.54.101)

Seen in the deer video (still_01 and clip 1): two does occupy the same space for seconds. Herd members had no spacing:
they spawn 1–4 m round the record, walk to a groupmate's exact place for company and mating, and young are born by a
parent. Fixed: every creature has a body radius by data (shared/creatures `bodyRadiusM`: doe 0.35 m, stag 0.4, wolf
0.3), and each tick `Creatures::separate` steers two closer than their radii plus `limits.gapM` (0.25 m) apart at up
to `limits.pushMps` (1.2 m/s) and pushes two overlapping straight out (a few passes for a crowd), each move kept to
ground the creature can stand on with its body clear. Pinned by `herds.rs` `herd_members_never_stand_inside_each_other`
(red: 0.69 m of overlap), both faces.

### 81. A wolf walks partly inside a cliff wall (fixed after 0.54.101)

Seen in the wolf clip (pack id 9 at -7800,-15600, a terraced cliff): a wolf's body overlapped the stone wall at the
cliff foot. A creature's step checked only the column under its centre, so its body went up to its radius into a
wall. Fixed: a step is taken only if the columns its body's disc reaches (`bodyRadiusM`) have nothing solid from its
`climbRows` up to its height (a step it can climb is no wall); else it slides along the wall on one axis, else
stops; knockback and separation keep to the same rule; a body already against a wall moves by its centre so it can
walk out. Pinned by `creatures.rs` `a_creature_s_body_keeps_out_of_a_wall_it_walks_into` (red: 0.297 m inside the
wall), both faces.

### 82. `?at=` or a teleport to a y inside the ground drops the player through to face B (fixed after 0.54.101)

`?at=3000,260,2200` where the ground is at 381 fell through to face B; a teleport while flying to a y inside a
mountain did the same: inside a solid cell the terrain's collision has no face to stop the capsule. Fixed on the
server, for every way in (a spawn link, a teleport, the dev tools): each tick before the controller, a player whose
capsule's centre is in a solid full cell is put, at rest, on the first row above its feet with ground under it and
its height free (`server/buried.rs`); one whose way up isn't loaded yet is held where it is until it is. Pinned by
`dwell-sim` `tests/buried.rs` `a_spawn_inside_the_ground_stands_on_the_ground_above` and
`a_teleport_into_the_ground_lifts_the_player_out` (red: the feet fell to 6 and 10 m in a 52 m world), both faces.
`tests/stream_cost.rs`'s held flight now rises over a hill on its way, as a player would.

### 75. A tree never falls to a player's swings (fixed after 0.54.25)

Found 2026-11-12 by visual pass 2 (WORK_QUEUE "Smaller items"): a one-cell log column on the flat world, 4 level
swings aimed high (pitch −3°) cut nothing visible, 4 low (−18°) a kerf, 12 more alternating high and low and 4 vertical
left a thin kerf and a slit, no notch and no fall (`fell-probe-sheet.png`, `c-alt-12.png`). Native tests felled trees
because they swung at a cell's two bands in turn. The cause: a standing log's ½ m row has two quarter-metre bands, and
a player's swings at one aim all land in the same band, hacking one kerf that binds deeper each swing (`bite / (1 +
kerf / bindM)`): 40 swept level swings at −3° or −18° from 1.5 m felled nothing (−3° and −18° hit different rows, so
alternating them split the cut over two cells). Fixed: a swing aimed at the band cut deeper than the other lands in the
other, in the open wood (`dwell_protocol::chopping::Cut::open_band`), so one aim notches; the stone axe's `biteM` 0.12
→ 0.1 makes it 11 swings (the owner's ~12, WORK_QUEUE item 7). Pinned by `dwell-sim` `tests/chopping.rs`
`the_same_level_aim_swing_after_swing_fells_a_trunk_in_about_twelve` (both faces; red: no fall in 40) and
`aiming_at_one_spot_notches_as_alternating_the_bands_does`, and `dwell-protocol`
`a_swing_at_the_deeper_band_lands_in_the_open_one`. Bench: `fell-tree`.

### 74. Damage cracks never drawn in game (fixed after 0.54.25)

Found 2026-11-12 by visual pass 2: stone after 2 and 4 pickaxe hits and dirt after 2 and 4 shovel hits showed the carve
but no crack strokes; a pixel diff against the undamaged cell changed only inside the dent (`diff-stone-4.png`,
`stone-zoom-0-2-4.png`). The native tests passed on `mesh_chunk_damaged`, which meshes no ground items. The game's
chunks do (`mesh_chunk_trees` with a `GroundJob`): the items were appended to the opaque mesh after the crack stream
was padded to the cells' vertices, so the stream fell short (712 entries for 1036 vertices in the test) and the client's
`textured_vertices` dropped a stream of the wrong length whole: no crack in any chunk with a pebble or stick. Fixed:
`ground_mesh::append` pads the stream over the items, and the client keeps a short stream's entries (0 past them).
Pinned by `dwell-mesh` `a_chunk_with_ground_items_keeps_its_cracks_for_every_vertex` (both faces; red before) and
`dwell-client` `terrain_vertices_carry_the_crack_bytes` (a short stream).

### 52. A felled trunk landing on its chopper can fling them far (fixed after 0.54.15)

Owner, 2026-11-08: "we shouldn't be capping player kick like that" — "the cap is removed". 0.50.37's
`max_body_kick` cap is gone (0.50.39), so this is open again: a 10 m trunk felled toward its
chopper throws them ~150 m at ~75 m/s (`dwell-sim` `tests/chopping.rs`
`a_felled_trunk_falling_on_the_chopper_does_not_fling_them`, ignored until fixed; both faces).
Measured (2026-11-08): the trunk's slanted underside comes down on the player standing on the
ground at ~5–9 m/s at the contact; the frictionless capsule is pinched between it and the ground,
the grounded controller cancels the downward part of the push each step, and the solver's
separation compounds sideways each step (16 → 48 m/s over 12 ticks). Later options: the tree's
mass (a non-dominant piece), contact softness or a corrective-velocity limit for player–piece
contacts, CCD so the overlap stays shallow, or the controller keeping the pinch's downward push.
Measured again 2026-11-12 (WORK_QUEUE item 24): with the log's `fellWedgeTrunk` now `open` (the gash, the
trunk's foot thin at the notch) the ignored repro flings the chopper ~155–160 m (face A 154.7 m, face B 160.0 m),
up from ~104–108 m on the `matching` wedge (103.8 m, 107.8 m) just before; still open.
Fixed 2026-11-12, at the cause, with no cap on the kick: the fling is the frictionless squeeze (a "watermelon
seed"). Pinched between a nearly flat underside (normal 14° off vertical, flattening to 0°) and the ground, a
capsule with no friction can only go sideways, at the descent over the slant's slope: 5 m/s at 14° is 20 m/s,
and as the slant flattens the speed grows without bound (−15 → −57 m/s over 10 ticks, then airborne). The
solver did what rigid frictionless bodies must; real bodies grip what they press on. So a piece's contact
whose normal toward the player points down (face-local y below −0.5) takes the piece's own friction
(`PIECE_FRICTION`, 0.8: it holds a wedge flatter than 39°), in a Rapier contact hook (`physics::PinchGrip`,
the player's collider asking for it); the capsule stays frictionless on the ground, walls and side hits.
The chopper is now shoved ~2 m/s as the trunk passes and moves 0.5 m (face A) / 0.4 m (face B)
(`a_felled_trunk_falling_on_the_chopper_does_not_fling_them_{a,b}`, un-ignored). `dwell-physics`
`tests/pieces.rs`: `a_piece_coming_down_on_a_standing_player_pins_them_not_squeezes_them_out` (red without
the grip: 1.88 m at 12.1 m/s; green 0.04 m at 0.7 m/s) and
`a_piece_moving_into_a_standing_player_throws_them_as_hard_as_physics_says` (a 900 kg log at 10 m/s throws the
player at 9.26 m/s, the inelastic momentum's 9.2, uncapped). Both faces (`DWELL_FACE=b`).

### 73. A felled trunk lying diagonally up a slope loses most of its cells when it settles (fixed after 0.54.10)

Owner, 2026-11-12: "I've also seen big chunks of a tree disappear if it settled diagonal long ways up a high, it
settles flat and most of the tree disappears into the ground." The settle snapped the body's rotation to lying flat
about its reference cell (its foot), so a trunk resting up a hill was laid level at its foot's height, inside the
hill; lifting the whole piece one or two rows didn't free it, and every carrier whose cell was taken was dropped.
Now a piece at rest tilted follows its pose column by column (`felling::follow_pose`), and a column whose carriers
are blocked rises to the first free rows (or drops a row onto the ground under it): it steps along the hill, the log
axis along the trunk, no wood lost (docs/FELLING.md §4). Pinned by `dwell-sim` `felling.rs`
`a_trunk_at_rest_up_a_slope_settles_whole_along_the_hill` (both faces; an 8 m trunk on a 1:2 hillside): red before
(3 of 16 logs settled), green with the fix.

### 72. ~23 fps on an iPhone at Low, and z-fighting where leaf cards meet tree trunks (fixed after 0.52.109; the frame rate partly)

Owner, 2026-11-11 (iPhone 17, Safari, quality Low): "I'm only getting ~23 fps on mobile, seeing lots of z-fighting on
tree trunks with the leaf cards. What might be dropping frames since the last while. Used to get 60 fps."

- **Z-fighting:** a leaf cell's cards (shared/look/leaves.json `cards`, 0.52.56) stood off every exposed face, and a face
  toward a log counts as exposed (an octagonal log covers nothing), so cards were placed through and along the trunk's bark
  beside it. Now a card with any corner reaching into a neighbouring block that is neither leaves nor empty is dropped
  (`mesher/leaves.rs` `leaf_voxel`). Pinned by `dwell-mesh` `leaf_cards_stay_out_of_the_logs_beside_them` (both faces):
  red on the unfixed mesher (a card's corner inside the log beside the leaf), green with the fix.
- **Frame rate:** the cards are now off on the phone tier by data (`cards.tiers.phone`, `render::cards_for`; a `?cards=0|1`
  dev override). Measured on the bench's `--touch` phone page (393 × 852 at 3×, Low preset, unthrottled, M-series desktop
  GPU): in the starter's broadleaf forest `gpu.frame` p95 4.76 → 4.36 ms (−8 %; `gpu.near` 3.55 → 3.18, `gpu.far`
  2.38 → 2.10) with cards off; at the default world's spawn (no near trees) no change. The cards are one cost, not the
  whole regression: the largest passes on Low are `gpu.composite` (~2.9–3.3 ms), `gpu.arms` (~2.1 ms at the spawn) and
  `gpu.water.tiles` (1.1–1.7 ms with no water in view). **Outstanding:** those three toggled one by one, and a comparison
  with a release from before the leaf, cloud and lava work: the launcher's builds (`/dwell/v/0.51.24/`) did not start
  under the bench (a 404 on a resource), and 0.52.40 is no longer published.
- **Round 2 (branch `claude/mobile-fps-2`):** the bench's `--base` opened a release as `…/v/<version>/dwell/` (the 404);
  it now opens the path as given. The **Low preset turns the leaf cards off** (a `leafCards` preset field and a Leaf
  cards setting, Settings → Graphics; `graphicsPresets.test.ts` red with Low's on, green off). Versions compared on the
  `--touch` phone page at Low, the default world's spawn, Chrome's CPU throttled 4×, M-series GPU (mean ms):

  | Build | frame.cpu | draw.render | lod | gpu.frame | gpu.near | gpu.composite | gpu.arms | gpu.water.tiles |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | 0.51.24 | 3.84 | 1.98 | 1.06 | 4.50 | 2.81 | 2.01 | 1.30 | 0.90 |
  | 0.52.77-dev.529 | 3.50 | 1.77 | 0.89 | 4.38 | 2.87 | 2.12 | 1.37 | 0.91 |
  | this branch (≈ 0.52.111) | 3.23 | 1.44 | 0.93 | 4.32 | 2.77 | 2.24 | 1.42 | 0.88 |

  No regression on the desktop at the spawn, CPU or GPU: the composite, arms and water-tile costs were already there in
  0.51.24. The local server's water worker ticks at 0.15 ms mean. So what dropped the phone to ~23 fps does not show on a
  desktop GPU at the spawn. The likeliest cause is the leaf cards on the phone's tile GPU: alpha-cut (`discard`) quads
  defeat an Apple GPU's hidden-surface removal, so their overdraw costs far more there than the desktop's 8 %. The
  cards are now off on the phone tier and on Low. 0.52.101 and newer releases still don't start under the bench (another
  404, not looked into), so 0.52.78–0.52.110 are not bisected. **Outstanding:** a real iPhone check on a build with the
  cards off; then, if it is still slow, the arms pass (a separate pass, a store and reload of the frame on a tile GPU)
  and the composite.
- **Round 3, moving (branch `claude/fps-moving`).** Owner, 2026-11-12, after the cards went off on Low: "I got 60 fps
  when standing still on Low", so the drops left come while moving. Measured on the bench's `--touch` phone page
  (393 × 852 at 3×, Low, M-series GPU) with Chrome's CPU throttled 4×, walking, sprinting and flying (level 4) +X for 8 s
  from the default world's spawn and the starter's (the bench's `phone-moving` scenario). Standing still frame.cpu is
  ~3.5 ms; moving its p95 is 13–20 ms, and every frame over 16.7 ms is **loading work on the main thread**, never the
  draw (`draw.render` ~1.4 ms): the worst frames are `mesh_dirty` 12–17 ms (the flood light of an arrived chunk, then
  a mesh start, each as its kind's "first of the frame" past the 8 ms deadline) plus `jobs.apply` 4–15 ms (2–4
  finished chunk meshes interleaved and written at once, 1.8–3.7 MB; one near chunk's mesh is 1.2–2 MB). The Chrome
  trace of the default world's flight (11 s, main thread 6.3 s busy): `padded_box` (copying a chunk's 44 × 64 × 44 sky
  volume out for its mesh job) 0.99 s, the flood light `LightMap::load` 0.84 s, `interleave` 0.41 s, `writeBuffer`
  0.28 s, `encode_job` 0.27 s, the minimap (`fillReveal`, `render_disc`, `scan_tops`) ~0.55 s, `LodSystem::update`
  0.41 s. No long task over 50 ms; LOD section uploads are small (`upload.section` p95 0).
  **Cuts:** (1) finished chunk meshes upload under a per-frame **byte budget by tier** (data:
  `shared/render/streaming.json` `uploadBytesPerFrame`, phone 512 KiB, desktop 4 MiB; `dwell-game` `upload_budget.rs`,
  oldest first, the first of a frame always): test `game_local`
  `no_frame_uploads_more_chunk_mesh_bytes_than_the_scenes_budget`, red with the budget ignored (55 of 80 frames over
  256 KiB, up to 4.66 MB), green with it; scheduler tests in `upload_budget.rs`. (2) A late frame runs **one** loading
  item past its deadline, the light's or a mesh start's in turn (`chunk_stream` `mesh_first`): test
  `a_late_frame_runs_one_item_past_its_deadline_and_the_light_and_meshing_take_turns`, red on the old rule (a mesh
  start after the light had passed the deadline), green. (3) `interleave` copies one stream at a time into its column
  (test `interleave_puts_each_vertex_streams_in_order`). (4) The bench's `record` takes a `budget`
  (`{"frame.cpu.p95": …}`), failing the scenario when over, and the `phone-moving` scenario sets one.
  Before and after (CPU 4×, one 8 s run each, ms; "dropped": frame intervals over 20 ms):

  | Route | World | Build | interval p50 / p95 / p99 | dropped (> 20 ms) | frame.cpu p50 / p95 / p99 | frames > 16.7 ms CPU | jobs.apply p95 / max | mesh_dirty p95 / max |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | walk | default | main (0.54.3) | 16.7 / 18.3 / 18.6 | 3 of 477 | 3.6 / 15.8 / 21.2 | 19 | 4.0 / 14.6 | 9.5 / 14.7 |
  | walk | default | this branch | 16.7 / 18.5 / 18.7 | 0 of 479 | 3.7 / 14.0 / 17.1 | 6 | 3.3 / 6.9 | 8.8 / 12.4 |
  | walk | starter | main (0.54.3) | 16.7 / 18.6 / 18.7 | 0 of 478 | 3.6 / 12.7 / 18.7 | 14 | 3.1 / 7.1 | 7.9 / 14.8 |
  | walk | starter | this branch | 16.7 / 18.4 / 18.6 | 1 of 478 | 3.6 / 12.1 / 17.6 | 6 | 2.4 / 5.9 | 8.0 / 13.3 |
  | sprint | default | main (0.54.3) | 16.7 / 18.6 / 31.5 | 6 of 473 | 8.2 / 16.3 / 21.4 | 22 | 5.0 / 10.9 | 11.1 / 14.7 |
  | sprint | default | this branch | 16.7 / 18.6 / 18.7 | 2 of 477 | 8.6 / 14.1 / 17.5 | 9 | 4.1 / 7.6 | 8.9 / 13.5 |
  | sprint | starter | main (0.54.3) | 16.7 / 18.5 / 18.7 | 1 of 479 | 3.7 / 14.1 / 18.7 | 14 | 3.0 / 7.8 | 8.8 / 16.2 |
  | sprint | starter | this branch | 16.7 / 18.5 / 18.6 | 0 of 478 | 4.0 / 12.5 / 17.3 | 6 | 2.4 / 4.1 | 7.6 / 14.3 |
  | fly | default | main (0.54.3) | 16.7 / 31.8 / 35.0 | 36 of 579 | 9.6 / 19.7 / 26.0 | 58 | 6.2 / 15.0 | 14.1 / 16.8 |
  | fly | default | this branch | 16.7 / 18.6 / 33.7 | 23 of 603 | 9.7 / 16.9 / 20.5 | 34 | 4.3 / 8.6 | 11.7 / 14.0 |
  | fly | starter | main (0.54.3) | 16.7 / 18.5 / 33.4 | 14 of 602 | 3.9 / 14.6 / 20.3 | 21 | 3.1 / 7.6 | 7.8 / 16.7 |
  | fly | starter | this branch | 16.7 / 18.3 / 32.8 | 8 of 618 | 4.0 / 12.5 / 18.5 | 11 | 2.3 / 4.4 | 7.7 / 13.8 |

  The cuts take the frames over 16.7 ms of CPU from 148 to 72 over the six runs and the flight's dropped frames from
  36 to 23 (default) and 14 to 8 (starter); `jobs.apply`'s worst from 15.0 to 8.6 ms and `interleave` 0.41 → 0.28 s in
  the flight's trace. Desktop is unchanged by design (4 MiB a frame is above any burst measured). At CPU 6× (this
  branch, `phone-moving`, default world) the walk drew 57.3 fps (interval p95 18.7, p99 33.9, max 51 ms) and went over
  the scenario's 4× budget (`frame.cpu` p95 18.8 against 18.5), which stopped the run there: the budgets are set for 4×.
  **Outstanding:** a single chunk's light or mesh start is still 5–14 ms at 4× (`mesh_dirty` max 12–14), and one near
  chunk's upload 5–8 ms (it is over the phone's budget alone): the next cuts are the flood light off the main thread
  (DEV_TOOLS.md "Flight loading", fix 2), the sky volume's copy (`padded_box`, the largest main-thread cost: copy it in
  the worker, or ship the chunk and its neighbours once), a chunk's upload split across frames, and the minimap's
  tile work; then a real iPhone check while moving.
- **Round 4 (branch `claude/fps-moving-2`).** The bench's `phone-moving` scenario (walk, sprint, fly 8 s each in turn,
  default world, Low) at CPU 4× and 6×, budgets left out so every route runs, against main after round 3:

  | Route | CPU | Build | interval p50 / p95 / p99 | dropped (> 20 ms) | frame.cpu p50 / p95 / p99 | frames > 16.7 ms CPU | mesh_dirty p95 / max | jobs.apply p95 / max |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | walk | 4x | main (round 3) | 16.7 / 18.5 / 18.6 | 0 of 478 | 3.7 / 13.8 / 17.5 | 7 | 8.8 / 12.3 | 3.4 / 7.2 |
  | walk | 4x | this branch | 16.7 / 18.5 / 18.7 | 0 of 478 | 3.9 / 13.4 / 17.5 | 6 | 7.9 / 13.3 | 2.2 / 3.7 |
  | sprint | 4x | main (round 3) | 16.7 / 18.5 / 18.6 | 0 of 480 | 3.1 / 10.9 / 15.3 | 2 | 6.8 / 12.4 | 3.4 / 7.4 |
  | sprint | 4x | this branch | 16.7 / 18.5 / 18.6 | 0 of 479 | 3.5 / 9.1 / 15.7 | 4 | 4.0 / 12.7 | 2.3 / 4.5 |
  | fly | 4x | main (round 3) | 16.7 / 18.3 / 18.6 | 5 of 606 | 3.6 / 13.3 / 18.4 | 13 | 7.4 / 13.1 | 3.6 / 8.5 |
  | fly | 4x | this branch | 16.7 / 18.3 / 18.6 | 5 of 609 | 3.7 / 12.4 / 17.8 | 12 | 5.8 / 13.5 | 2.4 / 5.3 |
  | walk | 6x | main (round 3) | 16.7 / 31.7 / 35.0 | 28 of 452 | 6.2 / 18.7 / 26.3 | 28 | 9.6 / 18.7 | 5.4 / 8.6 |
  | walk | 6x | this branch | 16.7 / 18.7 / 33.3 | 17 of 461 | 5.7 / 19.9 / 25.4 | 29 | 10.8 / 17.9 | 3.4 / 6.4 |
  | sprint | 6x | main (round 3) | 16.7 / 18.6 / 33.4 | 18 of 463 | 4.9 / 14.7 / 25.1 | 17 | 6.4 / 18.0 | 6.7 / 10.8 |
  | sprint | 6x | this branch | 16.7 / 18.5 / 32.8 | 11 of 470 | 5.2 / 13.0 / 23.1 | 12 | 5.0 / 18.6 | 3.4 / 7.3 |
  | fly | 6x | main (round 3) | 16.7 / 33.7 / 48.7 | 69 of 523 | 5.1 / 22.4 / 30.1 | 49 | 8.6 / 20.0 | 5.1 / 13.5 |
  | fly | 6x | this branch | 16.7 / 33.2 / 48.8 | 49 of 551 | 5.5 / 21.1 / 28.1 | 36 | 9.3 / 21.2 | 3.4 / 8.3 |

  Over the three routes at 6× the dropped frames go from 115 to 77 and `jobs.apply`'s worst from 13.5 to 8.3 ms (4×:
  8.5 → 5.3); at 4× the routes were already near 60 and move little. **Cuts:** (1) the sky volume's copy
  (`padded_box`): the fluid continuation into missing neighbours (BUGS 8) looked a neighbour up for each of the
  volume's 124k voxels at every chunk on the streamed view's edge; it now looks each up once and visits only the
  missing ones' part of the apron (`voxel.rs` `continue_fluids_into_missing`; 0.44 → 0.12 s in the 4× flight's
  trace). Tests `padded_fluids`: the blocks equal the per-voxel rule's for five sets of missing neighbours, and
  `a_chunk_on_the_streamed_edge_copies_its_sky_volume_about_as_fast_as_one_inside`, red on the old rule (edge 0.38 ms,
  inside 0.05), green. (2) A mesh job's message is written into one buffer sized up front (`jobs/codec.rs`): test
  `a_mesh_job_is_written_into_one_buffer_sized_up_front`, red on the old writer (535 KB in a 1 MB doubling buffer),
  green; `encode_job` left the trace's top. (3) A chunk mesh over the frame's upload budget is written a slice a frame
  into room reserved in the arena (`render/staged.rs`, pumped before each draw; the old mesh drawn until it is whole;
  `upload.staged_bytes` ≤ 512 KiB a frame on the phone), and no further mesh is handed over meanwhile
  (`Scene::uploads_waiting`): tests `staged` (no frame over the budget, a chunk shown only once whole, a replaced one
  dropped) and `game_local` `no_mesh_is_handed_over_while_the_scene_is_still_writing_one`, red with the wait ignored
  (288 meshes handed over), green. The sky volume is still copied on the main thread (`padded_box` 0.12 s, the copy
  itself); sending the worker the chunk and its neighbours instead would need the workers to hold the streamed world.
  **Not done:** the flood light off the main thread (`LightMap::load`, now the largest main-thread cost: `run_from`
  0.26–0.40 s self, and `mesh_dirty`'s worst frames at 6×, 18–21 ms, are one chunk's light). It is a larger change than
  this round's box: the light map is the main thread's state (edits relight it at once, meshes read its padded light,
  creatures and players their flood light), so a worker would have to hold the streamed voxels and the light map
  and answer each mesh start's padded light; a smaller step is lighting a chunk across frames (the BFS resumable
  under the stream budget). The minimap's tiles were not touched: they are under their own 2 ms budget
  (`shared/ui/map.json` `revealed.budgetMs`; `fillReveal` 0.1–0.2 s in the traces). Then a real iPhone check.
- **Round 5 (branch `claude/fps-moving-3`): a chunk's flood light across frames.** `LightMap::begin_load` and `resume`
  run the one-shot load's own steps (each channel's removal queue, the cells lit back, the brightest-first spread)
  a slice at a time, the search's state kept between frames; the streamer resumes it `LIGHT_SLICE_CELLS` (1024) cells
  between looks at the clock while the next slice fits its light budget, and the chunk stays unlit for meshing (its
  own and its neighbours') until it is done, so no mesh starts on half its light. Any other light update finishes it
  first, and so does a change to a chunk the light holds in its column or the ones beside it
  (`VoxelWorld::settle_light_near`); the chunks arriving all round while it lights, not held until their own light,
  leave it be (finishing it at each arrival put 14–19 ms into `jobs.apply` at 6× in the first try). Tests (both
  faces): the light lit 512 cells a slice ends as the one-shot light, every slice within 7 × its cells' writes
  (red with the budget ignored: one slice); an edit mid-light gives the load-then-edit light and matches a relight
  from scratch, and a new chunk arriving beside it leaves it part way (red when any nearby arrival finished it);
  the streamer's `a_big_chunks_light_runs_across_frames_within_their_budget_and_meshes_once_lit` (a 30 ms light at
  an 8 ms budget: red at once, frame 0 spent 30 ms; green, at most 4.8 ms + one slice a frame, meshed only once lit).
  `phone-moving` at CPU 4× and 6× (Low, default world, one run each, against main after round 4):

  | Route | CPU | Build | interval p50 / p95 / p99 | dropped (> 20 ms) | frame.cpu p50 / p95 / p99 | frames > 16.7 ms CPU | mesh_dirty p95 / max | jobs.apply p95 / max |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | walk | 4x | main (round 4) | 16.7 / 18.5 / 18.7 | 0 of 478 | 3.9 / 13.4 / 17.5 | 6 | 7.9 / 13.3 | 2.2 / 3.7 |
  | walk | 4x | this branch | 16.7 / 18.5 / 18.7 | 0 of 479 | 3.9 / 11.3 / 12.1 | 1 | 6.8 / 8.5 | 2.1 / 4.4 |
  | sprint | 4x | main (round 4) | 16.7 / 18.5 / 18.6 | 0 of 479 | 3.5 / 9.1 / 15.7 | 4 | 4.0 / 12.7 | 2.3 / 4.5 |
  | sprint | 4x | this branch | 16.7 / 18.4 / 18.7 | 0 of 479 | 3.4 / 10.1 / 12.0 | 0 | 4.6 / 8.4 | 2.3 / 4.4 |
  | fly | 4x | main (round 4) | 16.7 / 18.3 / 18.6 | 5 of 609 | 3.7 / 12.4 / 17.8 | 12 | 5.8 / 13.5 | 2.4 / 5.3 |
  | fly | 4x | this branch | 16.7 / 18.3 / 18.6 | 2 of 613 | 3.6 / 11.7 / 12.6 | 1 | 6.0 / 8.1 | 2.3 / 5.4 |
  | walk | 6x | main (round 4) | 16.7 / 18.7 / 33.3 | 17 of 461 | 5.7 / 19.9 / 25.4 | 29 | 10.8 / 17.9 | 3.4 / 6.4 |
  | walk | 6x | this branch | 16.7 / 18.6 / 32.0 | 8 of 471 | 5.9 / 13.2 / 17.8 | 6 | 7.1 / 8.4 | 3.3 / 5.8 |
  | sprint | 6x | main (round 4) | 16.7 / 18.5 / 32.8 | 11 of 470 | 5.2 / 13.0 / 23.1 | 12 | 5.0 / 18.6 | 3.4 / 7.3 |
  | sprint | 6x | this branch | 16.7 / 18.6 / 18.7 | 4 of 478 | 5.2 / 12.9 / 14.8 | 2 | 6.4 / 8.8 | 3.4 / 7.7 |
  | fly | 6x | main (round 4) | 16.7 / 33.2 / 48.8 | 49 of 551 | 5.5 / 21.1 / 28.1 | 36 | 9.3 / 21.2 | 3.4 / 8.3 |
  | fly | 6x | this branch | 16.7 / 32.9 / 34.5 | 43 of 562 | 5.6 / 15.0 / 23.9 | 16 | 6.7 / 8.4 | 3.5 / 8.2 |

  **The 6× flight's worst `mesh_dirty` is 8.4 ms (21.2 before; the target was under 16.7)**, every route's at 4× and 6×
  under 9.1 ms. Frames over 16.7 ms of CPU at 6×: 77 → 24 over the three routes; dropped frames at 6×: 77 → 55. The
  6× flight's worst frames now are `ticks` (13–20 ms: the prediction's ticks, the terrain collision loading while
  flying), not the loading: the next cut there. Still open: a real iPhone check while moving.
- **Round 6 (branch `claude/fps-moving-4`): the prediction's ticks.** A tick of 13–20 ms in the 6× flight is one
  tick (`ticks.count` 1), not a catch-up of several, and its time is the terrain collider (`Players::tick_terrain`):
  natively, on a streamed world like the client's, a tick that reaches new chunks takes up to 1.4 ms building them
  (`ensure_box`) and up to 1.6 ms putting cells in the voxel collider (`catch_up`: the cells in the player's reach,
  then `COLLISION_CELLS_PER_TICK`, 8192, more of new chunks a tick), ~8–10 ms each at 6×; Rapier's step is
  ~0.4 ms a tick. **Cut:** the prediction's tick puts fewer of those background cells in on a phone
  (`Players::set_collision_cells_per_tick`, data: `shared/render/streaming.json` `collisionCellsPerTick`, desktop the
  server's 8192, phone 1024); the cells within the player's reach still go in at once whatever the budget, so the
  player moves the same, and the server is unchanged. Test `dwell-physics` `collision_budget`
  `a_smaller_collision_budget_bounds_a_ticks_cells_and_moves_the_player_the_same`: red with the budget ignored (a
  tick put 8437 cells in), green (≤ 1024 + the reach's), and the run's feet equal the default budget's on every one
  of 600 ticks across a chunk border; `streaming` pins desktop at the server's budget and the phone's below it. The
  controller suites pass on both faces (`cargo test -p dwell-physics`, 263 tests, and again with `DWELL_FACE=b`). Main's #642 (the
  `PinchGrip` contact hook, contact recycling off for the player's pairs) measured with it (the last column): no
  cost on the tick here, mean and p95 within the runs' noise. `phone-moving`, ms:

  | Route | CPU | main (round 5) ticks mean / p95 / max | this branch, before #642 | this branch with #642 | frames > 16.7 ms CPU (main → branch) | dropped (> 20 ms) (main → branch) |
  | --- | --- | --- | --- | --- | --- | --- |
  | walk | 4x | 0.50 / 1.2 / 11.7 | 0.54 / 1.5 / 8.2 | 0.42 / 1.3 / 8.3 | 1 → 1 | 0 → 0 |
  | sprint | 4x | 0.34 / 1.0 / 11.1 | 0.45 / 1.4 / 7.9 | 0.41 / 1.3 / 8.2 | 0 → 0 | 0 → 0 |
  | fly | 4x | 0.62 / 1.7 / 14.1 | 0.73 / 2.0 / 11.2 | 0.67 / 2.0 / 10.8 | 1 → 1 | 2 → 2 |
  | walk | 6x | 0.83 / 1.7 / 17.5 | 0.80 / 1.9 / 11.6 | 0.68 / 1.9 / 12.6 | 6 → 2 | 8 → 5 |
  | sprint | 6x | 0.61 / 1.6 / 15.8 | 0.72 / 2.2 / 11.7 | 0.62 / 1.7 / 10.9 | 2 → 2 | 4 → 8 |
  | fly | 6x | 1.05 / 2.5 / 19.8 | 1.25 / 3.9 / 16.4 | 1.12 / 3.3 / 18.8 | 16 → 8 | 43 → 37 |

  The 6× ticks' worst: walk 17.5 → 12.6, sprint 15.8 → 10.9, fly 19.8 → 18.8 (one frame; the flight's p95 2.5 → 3.3,
  the cells within reach now going in as the player reaches them). What is left in the worst ticks is building a
  newly reached chunk's collision (`ensure_box`, ~1.4 ms natively): building the chunks ahead of a moving player
  between frames was tried and parked (on the test's generated world the builds ahead read neighbours that then
  changed, and their rebuilds landed in the ticks; it needs a streamed-world test). Still open: that, and a real
  iPhone check while moving.

### 71. "Starting the graphics" takes a long time with no elapsed time shown (fixed after 0.52.101)

Owner, 2026-11-11: "'Starting the graphics' now takes a long time without showing elapsed time." The stage is
painted before its work (`client/src/ui/startStages.ts`), but `Client.create` then makes the renderer in one
long main-thread task, so the screen can't repaint and the stage's timer stands still until it is done.

Measured 2026-11-11 (bench `idle-spawn`, M-series Mac, Chrome's Metal shader cache emptied for a cold
start; the bench now prints the start's frame gaps, long tasks and `dwell:create:*` marks): the
premise above is wrong. `create` holds the main thread ~40 ms (the atlas 25, the characters 8, the
rest ≤ 2 ms each; no long task in the stage). Cold, the stage is 1.8 s, of which the device request
is 1.5–1.75 s (warm 7–125 ms), and the page's frames stutter (gaps of 170–800 ms). Then the GPU
process compiles the renderer's pipelines for **9.0 s** with the page painting **no frame at all**
(one 8.9–9.0 s frame gap, warm: none over 400 ms), so the screen stands on its last paint, "Starting
the graphics… 1.8 s"; the join waits on the compile (5.2–11.4 s cold vs 0.1–0.2 s warm). First step
(this change): the compile is its own stage, `shaders` "Preparing the shaders", from `create`'s end
to `Client.gpuSettled` (a queue fence), beside the world's stages, so the timeline and the frozen
screen name it (test `startStages.test.ts` "shows the GPU's compile after create as its own stage",
red then green).

**Fix:** the renderer makes its pipelines lazily (`render/pipeline.rs`: `Lazy`, built on first use;
36 of the 38 that `with_samples` made, the water tiles' two left). While the loading screen is up the
frame's draw waits for `Renderer::warm_pipelines`, which builds one pipeline a frame and the next only
once the GPU has done the last (a queue fence), so the GPU process compiles them one at a time with
paints between; a tier that is off (shadows, distant shadows, clouds, light shafts) is not warmed and
its pipelines are made when it is switched on. Cold, after: the longest no-paint gap after `create`
is **683 ms** (before 9.0 s); the longest of the whole start 815 ms, in the device request; playing
**14.1 s** (before 15.1–15.6 s); warm unchanged (7.45 s, no gap over 160 ms). Tests: `render::tests::
a_renderer_makes_its_pipelines_lazily_and_warms_them_in_batches` (38 made up front before: red, then
green with 2) and `a_tier_switched_off_is_not_warmed_and_is_made_on_first_use`. Left: the device
request itself (~1.7 s cold, gaps to 815 ms), Chrome's.


### 68. LOD holes and tall walls near a 4400 m summit (fixed after 0.52.92)

**Cause:** the renderer's far pass, not the LOD. Its near plane moved out with the eye's altitude over a
fixed 1024 m "terrain top" (`TERRAIN_TOP_M`): with the eye at 5300 m it sat at 0.8 × 4276 = 3421 m, so
the cone 0.9–2 km away was clipped from the far pass, and the near pass reaches only 1024 m — the sky
showed through, the coarse sections' skirts beyond it stood as walls. It moved with the camera (from
(500, 7000, 1500) the hole was the whole near half of the cone). Now the top is the higher of 1024 m and
the highest box of the sections, stand-ins and chunks shown (`render/mod.rs` `far_pass_near`,
`drawn_top`). Test: `dwell-client` `render::tests::the_far_pass_keeps_terrain_drawn_over_the_default_top_from_above_it`
(red: near plane 3420.8 m; green after); `dwell-lod` `the_lod_covers_the_cone_with_the_player_flying_over_the_summit`
(both faces) pins the selection with the player in the air as in game (passes before and after: the
selection was never the cause). In Chrome on the GPU the summit view is whole (session scratchpad
`lod-bugs/run4/v68b/summit.png` before, `lod-bugs/after68/v68b/summit.png`, `high.png` after).

Found 2026-11-10 (0.52.27) looking at the tall peak in game: `?worldgen=test:tall_peak&seed=7&fresh=1&at=500,5000,-866`,
camera at (500, 5300, 34) looking down 45° toward −Z into the crater (bench `--compare`, the `summit`
configuration). The cone's flank shows a hole (the sky and stars through it, lower right) and tall
vertical grey walls where LOD sections at different levels meet, around the crater. From 20 km and
from the cloud deck the cone is whole. Suspects: the LOD sections' bounds or skirts at heights the
default world never reaches (~4400 m, ~8800 rows), or the selection near a camera far above the
full-detail chunks. Next: `__dwell.state().lod` at that view, and `tests/tall_peaks.rs`'s LOD levels
checked for neighbouring sections' seams.

Narrowed 0.52.29 (natively, `dwell-lod` `the_lod_covers_a_4800_m_cone_seen_from_its_summit`, both faces;
passes): the summit view over the real tall-peak terrain, generated and meshed for real with the player's
chunks streamed on the rim, settles with every column of the cone in view (3.2 km each way, 8 m apart)
covered in plan by a drawn surface, a shown skirt or drawn chunks. So the selection leaves no hole there,
and the one seam it finds — ~100 columns (face A) where the ground crosses a horizontal section boundary
between a coarse section and the finer ones over it (e.g. level 5 j 70 under level 4 j 142 at 3584 m) —
is closed by the coarse section's top skirt. Ruled out: the column bounds (the bounds and the cells come
from the same columns), the build volume (the summit is inside it), the selection. Still suspect: what
the plan view can't see — a skirt or surface drawn but facing away (culled) or clipped, the side skirts
as the "walls" (on 128 m cells a side skirt stands up to the column's surface, tens of metres on the
cone's slope, in the riser's colour), the renderer's per-section culling bounds at ~9000 rows, and a
stand-in drawn clipped.

In Chrome on the GPU (0.52.29, `?loddebug=` switches; screenshots in the session's `peaks2/run1`): at
the summit view the grey walls and the flat grey planes around the crater are all **skirts**: `skirts`
draws them magenta, and with `noskirts` the sky shows through where they were. So the ground is
missing over those areas and the skirts are only what shows in the gap. Per-section culling
(`nocull`) and stand-ins (`nostandins`) change nothing, so neither is the cause. The level tints put the
planes at level 6. The LOD's own state in game: every column within 2.4 km of the crater has drawn
sections stacked through its ground (levels 3–5), nothing is waiting, the cache is at 252 of 256 MB and
the error scale is 3.06. Natively the same view settles with every column drawn up to its ground, at
the game's options (3 px, 128 m, 256 MB) and at a 48 MB budget too. Over a 3.2 km box, levels 2–6, the
column bounds never call a section empty that has solid cells. So the selection, the bounds and the
generator agree natively, and the game differs. Next suspect: what the game's sections are made from.
The job pool is created only once the test world's maps are installed (`world_maps`, `WebJobs::new`), so
the plain race is ruled out. But any instance that builds a generator without them would compute the
seed's own ground, which has no cone. After that, the meshes as uploaded. Next: in the
page, compare a drawn section's mesh (or its column surface) at a hole column with the native one.

Checked 0.52.37 (ADR 0074 B7): an instance that already held the seed's own map ignored a test world's
installed map (`worldmap::cache::install` kept the first), and the portal sites were cached by seed
alone; both fixed (an install replaces a map whose worldgen config differs, `tests/worldgen_config.rs`
red → green; sites keyed by the faces' configs). **Not the cause:** in Chrome on the GPU, a fresh
`?worldgen=test:tall_peak&seed=7&fresh=1&at=500,5000,-866&loddebug=skirts`, camera (500, 5300, 34)
looking 45° down, still shows the stars through the crater's flank and magenta skirts around it
(session scratchpad `chisel-b7/tall-peak-summit-skirts.png`). Still open; next as above.

### 70. The starter world reloads the tab on iPhone while loading the terrain (fixed 0.52.68)

Owner, 2026-11-11 (iPhone 17, iOS Safari, 0.52.65): "I can't load the game on mobile, it restarts partway
through loading map chunks steps. Also sometimes it'll say 'a problem repeatedly occurred'"; then "it's
the starter world that's broken not the game". Safari kills a tab over its memory limit (~1–1.5 GB with
the GPU's share). Measured on the bench (`--touch`, a 393 × 852 at 3× phone page, the mobile tier, Chrome
on the M1's GPU; the F3 memory line's `memory()` at the end of loading and 30 s later):

| World | Total at load | Server | World workers (2) | Core | GPU meshes (LOD cache) |
| --- | --- | --- | --- | --- | --- |
| Default, 0.52.67 | 746 MB | 62 | 79 + 77 | 98 | 363 (240) |
| Starter, 0.52.67 | **1,467 MB** | **284** | **277 + 277** | 115 | 440 (309) |
| Starter, shared biome set | 995 MB | 114 | 114 + 114 | 139 | 442 (311) |
| Starter, 0.52.68 (both fixes) | **850 MB** | 114 | 114 + 114 | 138 | 299 (168) |
| Default, 0.52.68 | 676 MB | 59 | 77 + 77 | 97 | 295 (173) |

Two causes. (1) A many-biome world's biome set holds whole-map grids (the biome field's coast, fresh
water and rain, the erosion's drainage: ~60 MB a face natively, more in a WASM memory, which never
shrinks), and every generator built its own: a job worker makes four of the world (its terrain, the
chunk, LOD and bounds generators), each with both faces, each face's set built twice (the build's, then
the worldgen config's): 20 sets in a worker (`tests/shared_biomes.rs`, red: 20). The default world's one
biome has no grids, so only test worlds paid. Fixed: `terrain::shared_biomes` builds a map's set once per
process and every generator shares it (red 20 → green ≤ 2). (2) The LOD's error scale coarsened ×1.15 a
second from 1, so a phone's 96 MB cache held 3.4× its budget (326 MB of sections) at the end of loading
and fit only ~18 s later; it now steps by the overshoot, ×√(in use / budget) between 1.15 and 2
(`dwell-lod` `coarser_step`; `a_fresh_view_far_over_the_cache_budget_fits_it_within_seconds_…`, red:
13.5 s → green). Left: the 850 MB is not measured on the iPhone itself (Safari has no memory readout
the page can read; the F3 memory line is the game's own count); the starter's grids could shrink further
(f16 grids, the drainage's height copy of the map), and the server and the world workers still hold one
copy each.

**The grids shrunk (pending release, 2026-11-11).** Natively measured, seed 7, both faces of a
process (`tests/starter_memory.rs`, `BiomeSet::grid_bytes`; each face's map 1002 × 1002 cells, 79 % sea):

| Grid (one face) | Before | Now | How |
| --- | --- | --- | --- |
| Biome field coast | 3.83 MB | 0.98 MB | stored by its exceptions: 0 over the sea |
| Biome field fresh water | 3.83 MB | 3.83 MB | dense f32 (no common value) |
| Biome field rain | 3.83 MB | 0.98 MB | by its exceptions: the sea's flat rate |
| Drainage distance | 3.83 MB | 0.80 MB | by its exceptions: 0 on a line (the sea) |
| Drainage line height | 3.83 MB | — | read from the world map at the line's cell |
| Drainage accumulation | 3.83 MB | 0.60 MB | per cell, by its exceptions (1); read at the line's cell |
| Drainage source | 3.83 MB | 0.80 MB | by its exceptions: a line is its own source |
| **Both faces (a process)** | **53.6 MB** | **15.9 MB** | test bar 20 MB (red at 53.6) |

`grid::SparseGrid` keeps a default and the cells whose bits differ from it, so every read is the dense
grid's value exactly: the starter's terrain is pinned bit for bit on both faces
(`the_starter_worlds_terrain_is_unchanged`: the grids' reads, columns every 128 m, chunks and LOD
sections; recorded before the change) and the build's goldens are unchanged. Build scratch (the
drainage's sort order and sources, the biome field's wet and sea masks) is dropped as soon as it is
used, and face A's set now holds ~8 MB while face B's is built, so a WASM memory's high-water mark falls
too. Not done: f16 or quantized grids (they would change the terrain, a new compatibility line); lazy
grids per process (the server, the chunk worker and the LOD worker each generate the biome surface and
the erosion, so each reads every grid). Expected ~37 MB less in each of the three processes (~110 MB
of the 850 MB); the page's numbers are to be re-measured on the bench (`memory-phone --touch`).

**Re-measured, 0.53.2 (2026-11-11)**, the same method (`memory-phone --touch --compare` with a `{"worldgen":
"test:starter"}` config, the M1's GPU; MB at the end of loading, 30 s later in brackets):

| World | Total at load | Server | World workers | Core | GPU meshes |
| --- | --- | --- | --- | --- | --- |
| Default, 0.53.2 | 662 MB (583) | 60 | 77 + 77 (+ 20 + 22) | 96 | 285 (206) |
| Starter, 0.53.2 | **661 MB** (588) | 83 | 83 + 83 (+ 16 + 15) | 99 | 258 (184) |

The starter now reads the same as the default world (850 → 661 MB at load): the grids' cut took ~31 MB
from the server and each big world worker, as expected. "One world worker on phones" is not needed for
the memory: nothing in the starter dominates any more; the GPU meshes (the LOD's cache and the near
chunks) are the largest share in both worlds, then the two big world workers. (The two smaller workers,
~20 MB each, are the second pool's.) Still not measured on the iPhone itself.

### 67. The minimap fragmented, black-edged and coarse when moving fast (fixed pending release 0.52.10)

Owner, 2026-11-10: "The mini map gets all fragmented when moving fast, it should always show the full
width and height of what's around you, it's leaving black edges" and "the mini map is showing a coarse
view sometimes. It shouldn't ever be coarse." Columns the scan found no top in (not streamed yet, LOD
only, or past its reach) were drawn from the 8 m explored cells (coarse), or left transparent until
their 8 m generated tile came (black). Fixed: they are drawn from the generator's 1 m tops, the big
map's closest level's tiles, prefetched a tile ahead of the disc (`minimap::Generated`, ADR 0068's
0.52.10 amendment). Red → green: `minimap_tests.rs`
`the_disc_shows_unloaded_ground_at_one_metre_never_coarse_or_black` (both faces; failed on an explored
cell's colour with the old fill). With it, the big map open now keeps building every level's tiles
until done (`minimap.test.ts`, red → green).

### 66. Dashed gaps along a river's water steps seen from upstream (fixed pending release 0.52.6)

Owner, 2026-11-10: "Top of water slopes from up river looking down, have some 'gap' artifacts" (thin
see-through lines along the edges of the river's ½ m water steps). The step's faces met exactly, but
the pools' tops were merged into long rectangles while each pour slope and riser is one cell wide:
their corners lay inside the tops' long edges (T-junctions), and the rasterizer cracked along each
crease. Fixed: tops within two columns of a step are drawn cell by cell (`mesher.rs` `top_on_step`,
WATER_RENDERING.md §14). Red → green: `mesher_tests.rs`
`a_water_step_has_no_t_junctions_along_its_creases` (both faces, 161 → 0); also
`a_water_step_is_watertight_seen_from_upstream` (rays from upstream, green before and after: the
geometry had no real hole). Not seen in the browser: the bench's river at seed 7 showed no step in
view, so the fix is checked by the test only.

### 65. Below a dam, a dried river's chunk woke and slept for ever (fixed pending release 0.52.6)

*Found 2026-11-10: `tests/water.rs` `the_river_below_a_dam_across_seed0s_river_dries_to_the_next_inflow`
failed on main from 0.52.0 ("the dry river's chunks sleep": 1 awake), no water cell left.* Since
0.52.0's flat river beds (#62) the drained reach below seed 0's dam keeps a film of 0.1–0.5 mm on a
level bed. Cut off from its rest, it runs off by `runs_on` in proportion to its depth, so it only
thinned; its chunk fell asleep still owing that drain, woke ~90 s later to take it, stayed awake its
10 s calm window and slept again, round and round (the test caught it awake at 600 s). Fixed: a river
column out of its rest (`share` 0, a map flow, generated water) still draining under 1 mm soaks into
its bed (`water.rs` `FILM_SOAK`); a wetting front (net inflow) and still water (the basin tests'
conservation) are untouched. Red → green: that test (1 → 0 awake); the rest of `--test water` and
the grid's unit tests pass.

### 64. A frame drop (60 → 45 fps) every ~13 s while moving (fixed pending release 0.52.3)

Owner, 2026-11-10: "If I'm sprinting and hopping around I notice I get a periodic frame drop of about
15FPS from 60 to 45 about every 13 seconds or so", later "while walking and standing still too" (M1
Air, Chrome, a local world). Not distance-periodic and not in a fresh world: the bench's 58 s runs
standing, sprinting with hops and at a Retina window size on seed 7 had no frame over 18.8 ms. It
grows with the world explored: the explored map (ADR 0068) saved every 10 s while cells change by
reading the world's whole record from IndexedDB, merging every explored cell into it and writing it
back, all on the page (structured clone both ways). With 100 000 explored cells (a few hours' play)
a walk dropped frames of 134 and 151 ms, ~10 s apart (frame.cpu 2–23 ms: the stall fell between
frames, in the IndexedDB callbacks). Fixed: a save hands the store only the cells changed since the
last one, and the store merges them in its own worker (`client/src/ui/exploredStore.ts`,
`exploredWorker.ts`); the same 40 s walk on the same 100 000-cell record: max 18.8 ms, the cells
still saved. Tests: `exploredStore.test.ts` (one more cell saves one cell; saves merged one at a time
give the whole save's record; a record of another cell size keeps earlier saves under `cellsByM`, red
before the fix: it kept only the last save's cells). The stall itself is browser timing, measured with
a scratch Playwright repro that seeds the record (no bench step seeds explored cells yet).

### 63. Fallen logs settled with mismatched half rings (fixed pending release 0.52.2)

Owner, 2026-11-10: "sometimes when trees fall and settle, the rings/logs end up offset, so it's
like an upward facing half ring stacked on a downward facing half ring?" A lying log (axis x or z)
is two ½ m cells, its lower and upper half, each squared off where it joins the other. Settling
itself pairs a trunk's cells correctly at any row, odd ½ m heights included (checked natively for
both turns about x and z and odd lengths). Cause: the join rule (ADR 0072 §2) joined any two cells
of one group and axis beside each other, so a log lying one row higher or lower than its neighbour
(trunks felled side by side on uneven ground, or one lifted a row by `settle`) had its lower half
squared against the other's upper half: each octagon lost a corner and the caps' bark rings broke
into offset halves. Fixed: across a level side, a lying log's cell joins only the same half, the
half read from the log cells it joins above and below (`octagon::joined_mask`); level logs still
join full width, standing logs are unchanged. Test (both faces, red before the fix):
`felling::tests::trunks_settled_side_by_side_a_row_apart_keep_whole_octagons`.

### 62. River beds rose into dry grass humps across the channel (fixed pending release 0.52.0)

Owner, 2026-11-10 (0.51.18, seed "0"): "On rivers there's a bug where they don't go all the way
through, there's these grass sections that are a few blocks higher than the river bed, water kinda
waterfalls over them a little." At (-3845, 20041) the water grid's floor ran 397.5 m, rose to 399 m,
then stepped down a row at a time to 394.5 m. Cause: a **chute** (ADR 0059: a reach whose surface
falls faster than a row every 2.5 m, `river_rows.rs`; also a river's column two rows over another's
water beside it) was drawn as a dry bed a row under its surface, over the pool above's bed (its
river's depth, 1-3 m) and, being dry beside water, raised a row over that pool's water by the
waterline rule. Fixed: no chutes; every column of a channel is cut to its river's depth under its
own row like any reach, wet, so the bed only falls downstream (`river_rows.rs` removed). Where a
river joins one lower than it reaches, its surface steps down to the other's water a row a cell
(within two cells along an axis), as water, so no wall of water stands there either. Spawn points of
seeds 0 and 7 (both faces) unchanged; the worldgen goldens unchanged (no golden chunk holds a chute). Changes the
terrain a seed generates near steep rivers: a new compatibility line, 0.52.0. Test:
`a_rivers_bed_never_rises_downstream` (dwell-worldgen `terrain.rs`; seeds 0 and 7, both faces, the
owner's site; red: "(-3846, 20043) in river 42's channel is dry at 399").

### 61. Chickens drawn pure white

Owner, 2026-11-10: "Chickens are showing up pure white." The chicken's (and the kitty's) source mesh
had its own white vertex colours, so the importer's palette colours went into a second layer
(`COLOR_1`) and the game, which reads `COLOR_0`, drew them white. Fixed in 0.51.22: the two GLBs'
colours moved to `COLOR_0`, and `shared/models/blender_import.py` removes a mesh's own colour layers
before adding ours. Test: `every_model_is_coloured_not_plain_white` (dwell-models; red on the old
GLBs: "kitty: one colour only").


### 59. On a slow device the loading screen waited its whole 20 s timeout (fixed in 0.51.8)

Found 2026-11-10 from CI: `e2e/join.spec.ts:55` and `:113` ran into their 30 s test timeout on most main runs
since 2026-11-08 (before 0.51.3, which was first suspected), and the few that passed took 29.2-29.8 s: the
screen never opened on the terrain, only on its 20 s `timeoutS`, and the page's start and the fade at CI's
~5 fps took the rest. Natively, on the flat world with the LOD, it opened on the near ring in ~150 frames
at 60 fps but timed out at 5 fps with 168 of 179 chunks shown. Two causes: (1) above the player, the level-1
sections made on the way down to the forced chunks (`FORCE_CHUNKS_AFTER_MS`, which a slow page reaches
before the LOD's levels have loaded) sat unclassified under an empty level-2 section, and
`shows_nothing_at` stopped at them instead of the empty one above; (2) below, the ground chunks' level-1
sections waited for their partners beyond the ring to be meshed, and the streamer kept only a few meshes
in flight a frame, the workers idle most of each 200 ms frame. Fixed: `shows_nothing_at` skips sections not
yet classified, and a slow page keeps up to 8x the chunk meshes in flight (`SLOW_FRAME_MESHES_MAX`,
`pool.rs`). Pinned by `with_the_lod_the_loading_screen_opens_on_the_terrain_at_a_slow_pages_frame_rate`
(dwell-client `game_local.rs`; red: timed out at 20 s with 168/179; green: opens on the near ring).

### 58. On a slow device a walk's start or stop was corrected by up to 3 ticks of walking (fixed in 0.51.2)

Found 2026-11-10 in the bug 54 work: `a_device_at_under_two_frames_a_second_walks_and_jumps_without_snapping`
(dwell-client `game_local.rs`, 48 server ticks a frame, CI's 800 ms frames) passed by luck. With 0, 1 or 3
frames more on the ground before the slow frames its walk's correction was 0.08–0.17 m, with 2, 5 or 8 it
was 0.25 m, over the 0.2 m bar. Cause: the client held its prediction while 45 of its inputs were
unacknowledged (`UNACKED_HOLD`, for a stalled server that sends no snapshots), below a slow frame's 48
ticks, so it predicted only 45 a frame and dropped the rest. The server starved for 3 ticks every frame
and repeated the last input for them, so where the walk started and stopped among the starved ticks set
how far the server walked past the prediction. Fixed: `UNACKED_HOLD` is `MAX_TICKS_PER_FRAME` + 30 (90,
under the server's 96-input queue), so a frame's whole catch-up is predicted and sent. Pinned by
`a_slow_devices_prediction_matches_whatever_the_phase_of_its_frames` (0–8 extra frames before the slow
ones; red: a 0.25 m correction after 2, the server starving 3 ticks a frame; green: 48 ticks a frame, no
starved ticks, no correction).

### 57. iOS Safari: touch stops doing anything (no look, taps neither break nor build)

Owner, 2026-11-09: "a really random iOS Safari bug where tapping and dragging stops doing anything
at all, no looking, tapping doesn't break or build. Game is fully responsive." Cause (found in code,
not reproduced on a device): the touch controls hold one finger per zone (`TouchControls::look_down`
/ `stick_down` ignore a new finger while one is held). iOS Safari can drop a finger's pointerup or
pointercancel (a system gesture, the controls hidden under the finger), so that finger stayed held
and every later one was ignored. Fixed in 0.50.79: when the page sees no finger down (`touchend` /
`touchcancel` with no touches left), loses focus or is hidden, held fingers are released
(`all_lifted`, a lost held build ends); a zone that loses pointer capture lets its finger go.
Tests: `a_finger_whose_lift_never_arrived_does_not_block_the_next`, `a_tap_after_a_lost_lift_still_taps`,
`a_held_build_whose_lift_never_arrived_ends_when_a_new_finger_comes`,
`a_new_stick_finger_takes_over_a_lost_one` (dwell-game touch; red: the second finger was ignored).
Check on the device: if it still happens, lifting every finger once should bring touch back.


### 55. Walking animations played while standing still on a moving body (fixed in 0.50.65)

*Reported by the owner, 2026-11-09:* "For some reason walking animations play while standing still on
moving physics objects." Two causes. The controller's state was its target velocity, which includes the
platform's carry, so a player still on a raft was `Walking` (on the wire too, for everyone); and the
animator's speed (the walk's choice and its clip rate) and the footsteps' strides were the world's
velocity. Fixed: `step_state` takes the carry (`base_velocity`) off; the animator reads the velocity
over the moving ground (your own less `base_velocity`; a remote's less the velocity of the drawn body
under its feet, `Pieces::ground_velocity`); strides are walked over that ground (`MoverSample::carry`).
Walking against a raft's motion, still in the world, walks and steps. Pinned by `dwell-physics`
`standing_still_on_a_moving_platform_is_idle` and `walking_on_a_moving_platform_walks_even_against_its_motion`
(red: `Walking` / `Idle`), `dwell-client` `a_remote_still_on_a_moving_body_is_idle_and_one_walking_on_it_walks`
and `your_arms_still_on_a_moving_body_are_idle` (both faces; red: `walk`), `the_ground_velocity_is_the_moving_body_under_the_feet`,
and `dwell-game` `footsteps_are_walked_over_the_moving_ground_carrying_the_mover` (both faces; red: 0 steps against the raft).

### 53. Edited ground looked speckled with dirt from far away (fixed in 0.50.51)

*Reported by the owner, 2026-11-09:* chunks edited by water "look like they have speckled dirt instead
of grass at lower LODs". An edited chunk's level-1 section is the downsample of its cells, and where the
ground steps up by one ½ m row across a LOD cell's top, that LOD cell (three of its eight cells filled)
is air and the one under it showed the dirt under the higher columns' grass; the generated section
shows the surface's grass there. Fixed: a surface cell of a downsample (filled, air over it on its face)
takes its columns' tops with the cells of the block over it (`dwell_world::lod::downsample_octant`).
Pinned by `dwell-world` `a_downsampled_step_in_the_ground_shows_its_grass_not_the_dirt_under_it` (both
faces; red: `dwell:dirt` under the air, green: grass).

### 48. Neighbouring chunks falling asleep at different moments of a slosh made or lost water (fixed in 0.49.52)

*Found 2026-11-07 in the work on #41b (the stirred basin's slosh).* Falling asleep, a chunk keeps
the flow across each of its edges as it stands at that moment (0.49.44's `kept`). Two neighbours
that fell asleep at different moments of a swing each kept a different flow across their shared
edge, and with no one stepping that edge the difference was made or lost every second: a stirred
40 m basin 3.5 m deep gained 1.07 m³ in 900 water s (2.5 m³ in 25 minutes). Fixed: a chunk falling
asleep beside a sleeper keeps that sleeper's flow for their shared edge (its balance takes the
difference), so both sides agree (docs/WATER_SURFACE_GRID.md §2). Pinned by `water::tests`
`neighbours_falling_asleep_at_different_moments_of_a_swing_conserve_the_water` (both faces; red
1119.100 → 1120.170 m³, green within 1 L).

### 47. A river slowed right beside an edit (fixed in 0.49.43)

*Found 2026-11-06 by two agents, at the owner's creek (seed 3974502447 by 12850, 330, -3330).* A held
river column's current is the map's (`river_current`: Manning's on the course's mean fall, ~1.28
m/s there), a freed one's its own velocity (its carried flow over its depth, ~0.1 m/s), so a block
set on the bank freed the river within three columns of it and the river visibly slowed there
(flecks, foam, ripples, a swimmer's drift), speeding up again where it was still held. Fixed: a
freed column still its river's (in the rest, with a rest speed) shows the map's current, scaled by
how much of the rest's speed it still runs and blended to its own velocity as its depth leaves its
generated depth (`riverCurrentBandM`; docs/WATER_SURFACE_GRID.md §2c). A dam's pool, a drained bed and
placed water keep their own. Pinned by `water::tests` `a_freed_river_column_runs_as_its_held_neighbours`
(both faces; red 0.100 m/s beside 1.000 held, green within 20%) and
`a_dammed_reachs_pool_keeps_its_own_still_current`.

### 46. A ramp in a chunk otherwise solid under solid ground panicked its collision build (fixed in 0.49.29)

*Found 2026-11-06, writing the terrain-collision rebuild tests for #45.* A chunk all solid under a
solid chunk was built as cubes without reading its neighbours, but a shaped cell in it (a ramp
placed in a dug cell deep down) read them to cull its faces: an index out of bounds, the tick
panicking. Fixed: such a chunk counts as buried only if every cell is a full cube. Pinned by
`dwell-physics` `terrain.rs` `a_ramp_in_a_chunk_otherwise_solid_under_solid_ground_builds` (red: the
panic; green).

### 45. A collapse's falling sand stalled the server's tick for 30 s (fixed in 0.49.29)

*Found 2026-11-06 at seed 0's dam site (#350's bench):* the pit dug beside the wall dropped the
river bank's sand, and the ticks over 25 ms were its falling pieces (764 of the server's 1,118 ms in
the first 4 s; ticks 45 ms, then 27–29). Natively (`dwell-sim` `tests/collapse.rs`
`measure_the_dam_sites_pit_collapse`) 177 sand columns lay packed edge to edge against the cut's
walls; their boxes touched exactly and the solver held the whole raft by the walls' friction, so it
crept down at ~0.2 m/s and every column stayed a body for the 30 s until `maxSeconds` settled it
where it hung (4.5–6 ms a tick natively, 4 of it the physics step). Fixed: each piece cell's box is
drawn in 1 cm a face (`PIECE_SKIN`), its cells merged into fewer boxes (`merged_boxes`), a piece that
hasn't gone anywhere counts as still (`restDrift`), pieces settle after 1 s and together every
6 ticks, and a settle rebuilds only its chunk's terrain collision and the shaped chunks round it
(ARCHITECTURE.md §7.3). Now 5 s of bodies at ≤ 1.4 ms a tick natively; in Chrome the cut's seconds
read 5.1, 4.3, 1.7 ms mean, then ~1.2. Pinned by `tests/collapse.rs`
`sand_packed_in_a_well_falls_down_it_and_settles_in_seconds` (red: 256 columns still bodies at 6 s,
92,160 body-ticks, 18.6 ms mean tick; green: settled in 4.6 s, ~43,000) and
`a_blasts_pieces_land_in_a_heap_and_settle_in_seconds` (red: 108 of 513 left at 10 s; green: none at
12 s), and `terrain.rs` `a_cell_changed_rebuilds_its_chunk_not_the_full_cube_chunks_round_it` (red:
18 builds; green: 1).

### 44. A LOD section computing at the server-jobs worker when the world saved was never recomputed (fixed in 0.49.26)

*Found 2026-11-06, reading the save path for #43.* Since 0.49.14 the browser's local server computes
its modified LOD sections in the server-jobs worker. A section written since the last save whose
recompute was out at the worker when the world saved was saved with its old content and marked clean
(the save flagged only sections waiting to be dispatched), so if the page closed before the next save
the reopened world kept the old content and never computed it again. Fixed: every section still to
compute (dirty, held, or computing, at the worker too: `LodPropagation::recomputing`) is saved dirty
and recomputed on load. Pinned by `tests/lod_server.rs`
`a_section_whose_recompute_is_out_at_the_worker_is_saved_dirty_and_recomputed_on_load` (red: the
reopened world's section not dirty; green: dirty, and recomputed on load with the edit made while
its job was out).

### 43. An edited area looked terraced from far away (fixed in 0.49.26)

*Reported by the owner, 2026-11-06:* an edited area's distant view was a coarse, blocky plateau next
to smooth ground. A modified LOD section (the server's content) was meshed with no column surface
(`surface_in_cells` returned none for any section the server sent), so its columns were drawn at
their cells' tops, whole-cell terraces kilometres wide at coarse levels, beside generated neighbours
drawn at their exact heights. Fixed: a modified section asks the generator for its surface alone (a
pure function of where it is; its cells stay the server's), and the mesher draws each column its
content still tops where generation did at the exact height, an edited column at its cells. Pinned
by `dwell-lod` `a_modified_section_is_meshed_with_its_generated_surface` (red: no modified section's
mesh had a surface; green re-checked after merging main) and `dwell-mesh`
`a_column_whose_top_no_longer_holds_the_surface_is_drawn_at_its_cells`. In the browser at seed 0's
dam site (`?local=1&seed=0&fresh=1&at=2364.5,142,7443.5`, a 32 × 8 × 32 m stone block filled by
`__dwell.tool`, seen from ~300 m up) the ground round the block drew smooth, as its neighbours did.

### 41. Water never dried up below a dam (fixed in 0.49.25)

*Found 2026-11-06, measuring #40; reported by the owner the same day:* "Water never dries up below a
dam." … "Like completely." Natively at seed 0's river (`measure_a_raised_dams_churn`, ignored), a wall
raised across it kept 17–30 water chunks awake for as long as it ran, and the river below the wall kept
its water. Cause: free water changes by the difference of the two layers' flows (WATER_SURFACE_GRID.md
§2b), and the rest layer's divergence is a spring or sink at every column of a river, not only at a
drawn step's lip (the law's θ-weighting spreads each step's pour up its pool: 0.14 m/s mid-pool, 0.99 at
a lip in the fixture), and a sink at the lake a river runs into. Below a dam those springs refilled the
reach for ever and the lake's sink drained it, and a sleeping chunk beside them owed the rest's flow in
full each substep, so it woke, took what films it had and slept again. Three earlier attempts (owing only
an edge's change since sleep; carrying the chunk's rate of change asleep; the rest flow in proportion to
the water left) were dropped (they broke a filling lake, woke resting rivers, and seed 1's untouched
river). Fixed: a free column's rest divergence counts only while it is in the rest (`Water::share`,
latched: out once less than `restCut`, 0.25, of the rest's flow into it comes from columns in the rest;
back over `restBack`, 0.75; `flow.json`), so the cut runs down the rest's own flow from the dam to the
next inflow or the lake, and that reach drains by the law and the carried flow; such a column keeps its
river's flow out past the grid (`runs_on`); the carried flow is upwind (so a hollow keeps its water); a
sleeper owes the rest's flow by its column's share, and drops what it owes past the water it has; and a
column out of the rest is calm only below a tenth of its water's motion. Pinned by `water::tests`
`the_river_below_a_dam_dries_completely` (both faces; red 180 water cells below the dam, 1.05 m deep;
green none, its chunks asleep), `a_hollow_below_a_dam_keeps_its_water_still` (red: at the river's level;
green: under its lip, level, asleep, writing nothing), `a_world_saved_below_a_dam_dries_on_load` (red 180
cells), and `tests/water.rs` `the_river_below_a_dam_across_seed0s_river_dries_to_the_next_inflow` (the
wall at z 7452: red 575 water columns left from the wall to the creek's backwater at z 7526 after ten
water minutes, 8 chunks awake there; green none, none awake). In the browser at the same site (`?local=1&seed=0&fresh=1&at=2364.5,142,7443.5`, the wall by `__dwell.tool`) the 1,150 water cells below it were gone within 10 s real, the bed dry. Untouched rivers still rest exactly
(`a_block_set_in_a_generated_rivers_bed_changes_nothing_else`, seeds 0–2), a dam still backs a river up
(#38's tests) and nothing stands on a raised dam (#40's). `a_dam_across_a_drawn_river_backs_it_up` now
reads the pool below after two water minutes: its fixture's reach below is closed, and its water stays
in it now that the rest's sinks don't take it. **Remaining:** the reservoir behind the dam doesn't fall
asleep once it spills (`measure_a_raised_dams_churn`, 170 water minutes: at z 7452 6–18 chunks awake,
19–30 before, and 0–1,269 cells written a minute, 22–1,264 before, the reservoir spilling on from minute
150; at z 7380 21–32 awake, 20–30 before, and 0–991 written, 7–1,024 before, down to 1 a minute over its
last 20 minutes, 20–46 before); that is the filled reservoir's spill and its ±1–3 cm slosh past the 5 mm
sleep band, not the river below. Open as #41b.

### 42. A river stopped flowing below a fast reach, and further out (fixed in 0.49.24)

*Reported 2026-11-06* (build 0.49.22, seed 3974502447, feet (12833.97, 348.16, -3298.34), yaw
139, pitch -32): "in this spot the river stops flowing too". Every river column of the creek in
view (x 12844–12865, z -3347…-3316) held, asleep, level with its rest. Two causes, both seen in the
browser with `__dwell.waterFlowAt` along the creek
(`?local=1&seed=3974502447&fresh=1&at=12834,348,-3298`): (1) z > -3314 ran at 1.5 m/s and z -3316
to -3344 at 0.10 m/s: the map's course there is held level for 167 m where the land dips (its levels
never rise), and on a segment's own fall of 0 a held river's current was its flow over its depth
alone (0.6 m³/s over 5.4 m × 1 m); (2) past z -3345 nothing: the server sent the currents of the
chunks within 2 of the player's, so 33–48 m out the flow stopped, inside the 40 m it is drawn fully.
Fixed: a held river's current runs on the larger of its segment's fall and its course's mean over
`riverFallReachM` (256 m, `flow.json`) up and down stream (1.28 m/s there), and the currents are sent
as far as the flow is drawn (`waterflow.json` `fadeToM`: 4 chunks, 8 a send). Pinned by
`the_owners_level_creek_runs_as_the_river_round_it` (red: 0.11 m/s),
`a_level_reach_runs_on_its_courses_fall` (both faces' maps; red: 103 and 104 level reaches under
0.4 m/s), `a_level_reach_takes_its_courses_mean_fall` (a fixture course) and
`a_player_is_sent_the_currents_as_far_as_the_flow_is_drawn` (both faces; red: 35 of the 60 chunks
within 60 m never sent). No world or terrain changes: the currents are found as a chunk's water
joins the grid, so saved worlds run on load. WATER_SURFACE_GRID.md §2c.

### 40. Water on top of a dam never drained, even after it was raised (fixed in 0.49.23)

*Reported 2026-11-06, present "for many versions".* "Once water has entered the column (covered
the top of the dam with water) that water will never drain away even if you raise the height of
the dam, you end up with standing water that continues to 'flow over' into the river bed beyond";
"it just goes up and down, never drains or deletes". Screenshots: a stone wall across a river, a
pale slab of water along its raised top, water pouring off its downstream edge. Cause: the rest
layer (WATER_SURFACE_GRID.md §2b) keeps the river as generated beside the water as it is, and a
free column changes by the difference of their flows; where the drawn river's flow changes (a
step's lip, a narrowing, a junction) the rest's divergence is an implicit spring. A column built up
into a dam kept its spring, so the grid kept pouring water onto the dam's top whatever its height;
raising the dam also lifted what stood on it. Not trapped water, nor flooded blocks. Fixed: a
column whose floor is at or over its generated water's level changes by its own flows alone
(`Water::out_of_rest`); its neighbours keep their rest flows, so dams still back rivers up (#38's
tests pass). Read from the state, so saved worlds heal on load. Tests: `water::tests`
`water_on_a_raised_dam_drains_off_it` and `a_world_saved_with_water_on_a_dam_drains_it_on_load`
(both faces; a flat river whose flow begins at the dam; red: 3.9 cm standing on the raised dam, 3
cells drawn over it, and the same after a save and load; green: dry), `tests/water.rs`
`water_on_a_raised_dam_across_seed0s_river_drains` (seed 0 by z 7380; red 20 cells on the raised
top, green none). The river around a dam still never falls asleep (#41).

### 39. Triangle-shaped holes in the water at a river's steps (fixed in 0.49.20)

*Reported 2026-11-06.* "Diamond-ish shaped visual cuts appear in water slopes in rivers": from a
river pool's surface looking upstream at a step, triangles of the bank behind showed through the
water at the step. Cause: a pool's edge over a step is drawn as a slope down across the open cell
beyond it (`mesher.rs` `slope_over`), and the wedge of water under each slope had open ends. Where a
river crosses the grid on the diagonal, its steps are a staircase, so most slopes have an open
cell beside them that pours no such slope; from the lower pool's level the eye looked in through
the open end at the slope's back, and since 0.49.5 (#31) a surface's back seen from outside the
water isn't drawn, so the ground behind showed through (before 0.49.5 these were flat blue
triangles). Not the shader rule: #31's sheets were behind a drawn riser, these had nothing in front.
Fixed: the wedge is closed with a triangle at each end whose neighbour is open and doesn't pour the
same slope (`slope_over`), so the eye meets water from its front there. `mesher_tests.rs`
`a_diagonal_river_step_shows_no_slopes_back_from_the_lower_pool` (both faces: rays from the air over
the lower pool of a diagonal staircase step; red 71 of 1872 rays met a slope's back first, green
none). Checked in the browser at seed 0's river (camera at 6743.2, 94.49, 9641.1, yaw 221.86 from
`goto('water', {minCurrent: 0.05, aim: 'upstream'})` near 6744, 9643): the holes are water.

### 38. A dam didn't raise a level river, and level and shallow rivers had no current (fixed in 0.49.19)

*Reported 2026-11-06.* "Damming should raise the water and does", "I feel like I saw this when the
river was only 1 voxel deep, it didn't seem to have a flow either", then "dead rivers ... the
shallow ones missing currents". At seed 0's dam site (`?local=1&seed=0&fresh=1&at=2364.5,142,7443.5`,
a 32 × 32 × 2 stone wall across the river at z 7452) the water behind the wall rose 2 cm in ~1.7 h
of water time. Cause: since 0.49.0 (G1a, no lips) a drawn river is flat between its steps, and the
grid's rest layer (the water as generated, stepped by the law with its depths fixed) carried the
river's flow only over its steps; on a level reach it carried none, so a dam stopped nothing. The
map's current had the same gap: Manning's speed on a reach with no fall is zero, so about half of
the rivers' channel columns (seed 0: 109 of 209 sampled) were still. Fixed: each generated column
carries the map's river flow (`TerrainGenerator::river_flow`) across its faces in both the water as
generated and as it is, so an untouched river stays put and a dam stops the flow in the water as it
is alone; a held river's current is its flow over its depth where faster than Manning's
(WATER_SURFACE_GRID.md §2b, §2c). Saved worlds are fixed on load (no record change). Pinned by
`a_dam_across_a_generated_flat_reach_backs_the_river_up` (the dam site natively: red 139.50 →
139.51 m in 20 water minutes, green → 140.07), `a_dam_across_a_flat_river_backs_it_up_by_the_maps_flow`,
`a_shallow_flat_reach_runs_downstream_held_and_freed` and
`a_world_saved_before_the_rivers_flow_backs_up_when_dammed` (both faces; red with the flow off: 1.7
cm, a freed reach still, a restored reach still), `a_flowing_flat_river_rests_when_an_edit_frees_it`
(both faces: the flow moves nothing untouched) and `every_reach_of_a_river_runs_level_ones_too` (red:
a level reach still). In the browser the wall raised the water 0.58 m to the banks within 30 s real.

### 37. Drag building into water built one piece and no more (fixed in 0.49.16)

*Reported 2026-11-06.* "Make it so I can drag-build underwater, I notice it doesn't work underwater
for some reason", then "you don't have to be underwater … 2+ dragged out always failed for me." A
held Place whose pieces went into water cells, standing in the water or beside it, built the first
piece or two and then nothing. The server writes a block put into water in its flooded state
(`flooded_for`: stone becomes `stone[flooded=true]`); the client's answer to each drag fill checked
that the cells held exactly the block sent, so a flooded piece read as one the server had left out
(as it skips a cell its view of the body is in). A line then went back to that piece, found its own
block there and waited on it for good. Fixed: a piece counts as built flooded or not
(`build_drag::fill_placed`), with the answer's handling moved out of the game loop into
`BuildDrag::answered`. Pinned by `build_drag`'s `a_stroke_builds_on_into_water` (both faces: a Line
up, a Line across and a Flat into water; red before: one piece after the first for each line) and
`game_local`'s `a_line_drags_up_through_water_from_the_shore_and_from_in_it` (a pool in the
sandbox, the Line from the shore and from the bed; red before: two of the six pieces) and
`a_flat_drags_a_floor_across_a_pools_bed_from_the_shore_and_from_in_it` (a Flat stroke, which
passed before as well: a refused Flat column is not gone back to).

### 36. The bottom half of steep slopes under moving water stayed dry (fixed in 0.49.13)

*Reported 2026-11-06.* "The bottom half of steep slopes is still either not flooded or visually not
flooded, creating more water visual artifacts." BUGS.md #26 again, on the surface grid: a ramp (a
shaped block with a `flooded` state; steep slopes are two, `steep_lower` under `steep_upper`) was solid
to the grid (0.48.2), and the grid's drawing never wrote over a solid cell, so water rising over a
ramp never set its `flooded` state and its open part showed air (nor did it dry when the water went).
Fixed: the grid's copy of the world marks such blocks (`cells::HOLDS_DRY`, `HOLDS_WET`), and drawing a
column floods the ones in its rows and its neighbours' while the water beside or over them stands
half a fill step into their cell, dry otherwise (`Water::flood_near`; written as the `flooded`
property by name, `Write::flood`, checked against the world as other writes are). Pinned by
`water_risen_over_a_steep_slope_floods_its_ramps_and_dries_them_when_it_goes` (both faces; red before:
the steep slope's lower ramp stayed dry under the flooded pit).

### 35. The status line read 60 fps on a phone drawing 30 (fixed in 0.49.10)

*Reported 2026-11-06 (0.49.9).* "It's saying I'm running at 60FPS but I'm clearly running at 30 or even
lower, just moving the camera is stuttering." The meter counted every animation frame the browser
offered, but while the GPU is still behind on two frames the game skips drawing one (render/pacing.rs,
`draw.skipped`), so a phone whose GPU took ~33 ms a frame (the water close up, WATER_RENDERING.md §8
"Cost close to the water") drew every other one and still read 60. Fixed: `Client::frame` says whether it
drew, and the meter counts only those (`ui/fps.test.ts`, red: 60 for a page drawing every other of 60
frames, green: 30). The stutter itself is the GPU's: the frame rate now shows it.

### 33. Damming a river stalled the game (fixed in 0.49.7)

*Reported 2026-11-05 (0.49.5).* "Water definitely seems to be hanging my game thread … my frame rate
is fine, but I get stopped while moving … This is when I dam a river usually." The frame rate held
while the server fell behind, so the client held its prediction until it caught up. Water runs on
game time (72 × real at the default day): a water tick, every six server ticks, was 7.2 s of water,
about 45 substeps over every awake chunk. In a native server with a cut lake flooding out, those
ticks took 12–34 ms and grew as chunks woke (the grid's other work under 1 ms). Fixed: a tick does at
most `stepBudget` column-substeps (`flow.json`, 40,000; ~2.5 ms natively with 60 chunks awake) and
the rest of its water time is lost, not counted toward calm. `water::tests`
`a_water_tick_keeps_to_its_step_budget_however_much_water_is_awake` (red before: 50 substeps over
10 awake chunks).

### 32. Dead on face B, the third-person camera was upside down (fixed in 0.49.6)

*Reported 2026-11-06.* "When you die on the upside half of the world, the 3rd person camera is
flipped upside down (again)." When the player dies its body leaves the physics world and the
predictor's controller falls back to face A's frame; the death camera (and the drawn body) used
that face, so on face B the view was face A's: upside down. Fixed: the death camera takes the face
the body lies on (`FaceFrame::at_height` of the death position) and sits 4 m behind it along that
face's look and 2.5 m along its up (`game.rs` `death_camera`). `game.rs`
`the_death_camera_is_upright_on_the_face_the_body_lies_on` (both faces; face B red before).

### 31. Flat blue sheets on rivers behind their steps (fixed in 0.49.5)

*Reported 2026-11-05 (0.49.x).* "Flat darker colored blue faces" on rivers, seen looking upstream:
from below a stepped river's upper pool, the eye saw that pool's surface from underneath through
the step's riser, and the water shader drew a surface's underside as seen from inside the water
(the body's flat colour) though the eye was in the air. Not culling, and no air or unflooded
slope: the cells were water over sand and flooded ramps. Fixed: an underside seen from outside the
water isn't drawn (`underside_from_outside`, WATER_RENDERING.md); the thin risers stay until
rivers aren't drawn in steps (Phase 53 G1b). `shaders.rs`
`a_water_surfaces_underside_is_drawn_only_from_inside_the_water` (red before); checked in the
browser at seed 0's river (6744, 9643): the sheets are gone.

### 28. A junction at the top of a chute can leave a lip a cell short (gone in 0.49.0)

*Found 2026-11-04 (G1, the graph checker).* Seed 0, river 17 at (16555, 2142): a tributary joins
where river 17 drops into a chute, so the step's band is split between the two rivers and river 17's
lip has room for 3 of its 4 cells. One lip in 44,401 on seeds 0–5; the checker allows one in ten
thousand. Fix: plan a junction step's band across both rivers. Gone with the lips (Phase 53 G1a): the generator draws none.

### 27. On a steep diagonal cascade a lip can still pour into a cell too many or too few (gone in 0.49.0)

*Found 2026-11-04 (0.47.0 work, the strict sweep at eight pools a seed).* Seed 0 near (−15250,
91, 1840): pools a couple of cells long, a step every 2 m on a diagonal; a lip's band runs into
the next lip's, and a two-cell pool perched beside the river drains once found. The sweep checks
four pools a seed (it passes); at eight this site fails. Suspects: the next lip's crest not under
this lip's, bank columns lower than a crest's water beside it. Gone with the lips (Phase 53 G1a): the generator draws none.

### 29. Dirt walls stuck out into rivers at their step-downs (fixed in 0.49.0)

*Reported 2026-11-05 (0.48.1), seen "for quite a few versions".* Blocks of the bank (dirt, grass on
top) reach into the channel where a river steps down a row, standing over the water. A river
generation artifact: the step's **lip** (ADR 0058, `terrain.rs` `LipPlan`, `river_lip_rise`): the
generator raises a crest across the channel `lip_cells` wide and walls the columns behind and beside
it `river_lip_rise` rows over the water, so the basin engine's weir law held each pool exactly at its
drawn level. Since 0.48.2 the water surface grid rests the world's water as drawn whatever its shape
(its rest layer, WATER_SURFACE_GRID.md §3), so the lips do nothing. Fixed in 0.49.0 (Phase 53
G1a): the generator draws no lips; `terrain.rs` `no_dry_wall_stands_in_a_rivers_channel` (red before:
seed 0's (−6159, 3946) stood a row over its river's water).

### 26. Water risen over a steep slope left air in the bottom half of its ramps (fixed in 0.47.0)

*Reported 2026-11-04 (0.45.2).* "The bottom half of steep slopes … don't fill with water leaving
ugly air gaps visually in the water around underwater steep slopes." The ramps beside a body as
found held it like walls (`rest`) and still did when a dam's flood rose over them, so they were
never flooded. Fixed: rest cells the water rises over are held no longer (`submerge_rest`). Pinned
by `water_risen_over_a_steep_ramp_floods_both_its_cells` (both faces; red before).

### 25. Water invisibly filled nearby pools (fixed in 0.47.0, ADR 0058)

*Reported 2026-11-04 (0.45.2, seed 3695715780, a dam).* "Water is invisibly filling nearby pools,
essentially the visible water doesn't seem to match what's happening … Water should have to
visibly connect pools, or they shouldn't be connected." The pool below their dam took in the
river's whole flow at its own cells (`feeds`), the pool above none; links and seeps moved water
unseen too, and water stood in walls against the hidden `rest` cells where generated pools met.
Fixed by ADR 0058 (WATER_FLOW.md §18): a pool takes in only what is seen pouring in, no links,
seeps only at the loaded world's edge, and lips planned so each passes what the one above it does
(steps fully walled but for the crest). Pinned by `a_pool_nothing_is_seen_pouring_into_takes_
nothing_in` (red on 0.46) and the strict sweep at twelve pools.

### 22. The river below a dam rose (fixed in 0.45.1 and 0.45.2)

*Found 2026-11-03 (0.44.3, the owner's seed 0 dam, through the server natively); on 0.45.0 the river
below the dam rose 0.31 m, and the rapids further down (~15 pools a few metres apart) 0.2–0.25 m.*

- **Causes:** (1) the dam's split left the link from the pool above with the part below it (0.45.1);
  (2) each body's own inflow was taken again every tick as what its lip carries less what the found
  bodies above bring: a part split off below a dam has no entry of its own, so it passed nothing on
  in the books and the pools below counted the river as their own besides what arrived; (3) a loss
  (a lip above carrying more than a pool's own) drained a pool dry while a dam above filled, and it
  came back as another body; (4) a split left a link's source with the old part, so a fall's water
  was taken off the pool below twice.
- **Fix (0.45.2):** a body's own inflow is kept with the cells it comes in at (`Steady::feeds`): set
  when it is found (what its ways out carry less what the found bodies above bring), taken off once
  as each body above links in (`link_once`), carried by splits and summed by merges; a body with a
  way out of its own passes what that carries; a loss takes only what arrives; a split moves a
  link's source and target to the parts that hold them.
- **Tests:** `a_dam_that_splits_a_pool_leaves_the_river_below_at_its_level` (0.45.1; red: 52.72 m
  against 52.5 m), `a_pool_losing_to_the_ground_keeps_its_water_while_a_dam_above_fills` (red: the
  pool empty at 52.0 m when the spill came back). On the owner's dam every pool below it, down the
  rapids, stands at its found level; the dam's pool at 140.63 m over its 140.50 m crest.

### 24. Rising water vanished and came back higher at each step, and slopes didn't rise with it (fixed in 0.44.5)

*Reported 2026-11-04 (0.44.4, seed 0, the dam at x 2359 z 7437).* "The water definitely isn't filling
smoothly over time, it's still jumping 0.5m in height every few seconds as it rises." "The water
actually disappears and then reappears at a slightly higher level." "And slopes with water don't
correctly show this dynamic raising."

- **Causes** (found in the browser: the cells read every frame, the page recorded and its frames
  compared, the chunks meshed natively): (1) water's writes go out as `Rebake` modifications, and the
  client hid every cell a `Rebake` wrote until a settling piece's glide claimed it or half a second
  passed: each step of a rising pool drew its surface row as air, the row below showing, for half a
  second. (2) The water wrote its cells with `set_voxel`, a chunk revision per cell, then sent them
  as one modification: the client saw a gap every time and asked for the whole chunk again (1,845
  resyncs in one session). (3) A body's surface was drawn at the nearest fill, so a level hovering at
  a rounding point (the pool below the dam, fed in pulses) flipped each tick, and at a row's floor
  the row came and went. (4) A flooded ramp's water was drawn at the full row whatever the water
  beside it.
- **Fix:** only cells within a falling piece's reach wait for its glide (`Pieces::near_cell`); the
  water writes with `VoxelWorld::write_voxel` (no revision per cell); the drawn fill holds within
  `fillHysteresis` (`shared/fluids/flow.json`, ¼ step) past the half step; a flooded ramp's water is
  drawn at the fill of the water beside it in its row, none while the water is only below it.
- **Tests:** `a_rebake_far_from_any_piece_hides_nothing`,
  `the_waters_modifications_reach_a_client_in_revision_order` (red: revision 154 after 129),
  `a_level_hovering_at_a_step_is_drawn_steadily` (both faces; red: the row drawn 0, 22, 0, 22, …),
  `a_flooded_ramp_draws_its_water_at_the_fill_of_the_water_beside_it` (red: 3.875 against 3.219).
  Checked in the browser on the owner's dam: no resyncs, and the recorded pool rises a step at a time
  with no frame drawn lower or darker between.

### 21. A dam across a river didn't raise it (fixed in 0.44.3)

*Reported 2026-11-03 (0.44.2, seed 0 by the spawn, screenshots).* "I dammed that river. When I did, it
flooded that valley's bottom, but didn't continue filling, and there's no visual or effective water
flowing over the banks and down the hill?"

- **Cause:** the river's stretch there has no way out of its own (its riverbed's ramps close both ends,
  walls to the water engine), so it passes the map's flow unseen (a seep). The seep had no place: the
  dam split the stretch in two and both halves kept it, so the half behind the dam drained the river
  through it and never rose.
- **Fix:** a seep is where the river leaves the stretch: its cell farthest down the map's river (on the
  river most of its cells lie on), else farthest from where the water comes in (`Steady::exit`, saved,
  record version 4); only the part holding it has the seep (WATER_FLOW.md §2.2). A split's feed whose
  cell is in no part goes to the nearest part. Rivers through flooded ramps (the owner's preferred
  fix) were tried and collapsed the river's steps in the generated sweep; kept as a later option.
- **Test:** `a_dam_across_a_closed_stretch_holds_the_river_back` (both faces, the dam a block a tick;
  red on 0.44.2: the river behind it stayed where it was found). Checked natively through the server on
  the owner's river (seed 0, x 2359 z 7437): the river behind the dam rose and spilled over it.

### 20. A dam's flood reached only so far up the river, and an edit there lowered it (fixed in 0.43.3)

*Reported 2026-11-03 (0.43.2).* "When I dammed a river, it filled to the new level only so far back
along upriver. And then attempting to update that part of the river by making a block edit, lowered
the level of the river down to that up to the dam and stopped flowing in that direction."

- **Cause:** an unloaded cell is a wall to water, and a basin was found again only when an edit or its
  level changed it, not when the chunk beside it loaded: the flood stopped where the loaded world
  ended when it rose. The river found later beyond that edge, its water touching the flood, had no
  natural outlet, and its seep passed on everything that came in, the flood's water too: it drained
  the flood unseen.
- **Fix:** a basin keeps the unloaded chunks beside it (`edge`) and is found again when one loads; a
  seep passes the body's map flow and what found bodies' natural outlets bring it, not what an edit
  or a flood sends it, which fills it (WATER_FLOW.md §15). Saving the basins and fast-forwarding the
  ones out of reach stay W2.
- **Test:** `a_dams_flood_goes_on_up_the_river_when_its_chunks_load` (both faces; red on 0.43.2: the
  flood stopped at the chunk edge; with only the edge fix, the river found there drained it).

### 19. Water filled up to a wall across a river, not over it and down the other side (fixed in 0.43.1)

*Reported 2026-11-03 (0.43.0, screenshot of a stone wall across a river).* "Water fills up to the
wall blocking the river, not over it and down the other side. And it doesn't flood when the wall is
at the same level as the water."

- **Cause:** the pool stayed one basin over the wall (joined by the open row above it), so one level
  and no step at the crest; the spill over a wall, a few centimetres deep, was under the half row a
  row is drawn at, and running water wasn't drawn at all (W1); the pool below the wall, cut off from
  the map's flow, kept passing only its links' flow and filled with the spill.
- **Fix:** basins joined only above their full rows are separate and merge only through a row both
  fill; spills drawn (the crest as water, the way down as `dwell:running_water`); a found body passes
  what comes in each step, its links found where its natural outlets ran as found (WATER_FLOW.md §15).
- **Test:** `a_wall_across_a_river_is_spilled_over_and_the_river_below_holds` (both faces; a wall
  level with the water and one row higher; red on 0.43.0: the river above didn't rise).

### 18. Ramps beside the water stood dry, the water ending in a wall (fixed in 0.42.2 and 0.43.0)

*Reported 2026-11-03 (0.42.1, screenshot).* "Slopes next to water aren't flooded at generation time
or update: you can see the water wall edge."

- **Cause:** a ramp's open part holds water only in its `flooded` state; neither the water flow's
  writes nor the generator gave it to a ramp beside the water at its row (the generator floods a
  ramp only in a column under water).
- **Fix (update):** the water's writes flood the blocks with a `flooded` state beside the rows they
  wet, and unflood them when no liquid is beside them (`water.rs` `flood_edge`).
- **Fix (generation, 0.43.0, a new compatibility line; owner, 2026-11-03: "world gen should fill
  slopes"):** a ramp piece in a row where a neighbouring column holds water over its ground is
  generated flooded (`terrain.rs` `generate`). Treating ramps as open to water was considered and
  set aside (owner: "weird directional edge cases").
- **Test:** `ramps_at_the_waters_edge_flood_and_dry_with_it` (both faces; red on 0.42.1);
  `dwell-worldgen` `tests/worldmap.rs` `ramps_beside_a_rivers_water_hold_it` (red before: 244 of 499
  ramps beside the fixture river's water dry).

### 17. A block set at the bottom of any river flooded it without end, or drained it (fixed in 0.42.2)

*Reported 2026-11-03 (0.42.1).* "Putting a single block at the bottom of the river causes it to
flood continuously, rising higher and higher … flying around seems to delete basins?" and "just
adding a block to the bottom of any river makes it flood endlessly, or seemingly randomly drain
completely."

- **Cause:** W1 took a body's inflow from what left it just before the edit, by the basins' rules,
  which the generated water doesn't keep: plants, banks and the next step beside it counted as
  outlets (one pool 61 m³/s in, a lake 1,059 m³/s); a higher pool merged with the lower ones it met
  below its surface, the river one basin with every pool's inflow and no way out; a pool found
  downstream filled before its ways were found; 7.2 s ticks overshot small pools; basins whose
  chunks were evicted lost their water, and waiting ones filled with what live ones sent them
  (WATER_FLOW.md §16).
- **Fix:** the redesign (ADR 0055, WATER_FLOW.md §2.2): the world as found is at rest, its natural
  outlets passing the world map's flow, linked into one flow network; merges at one level; flow
  steps; ways found in rounds with water in transit; waiting basins pass on what comes in and wait
  while a chunk of them is unloaded.
- **Test:** `a_block_set_in_a_generated_rivers_bed_changes_nothing_else` (seed 1, two rivers, six
  hours of water time; red on 0.42.1: 173,921 cells changed), `a_river_found_at_rest_keeps_its_banks_dry`,
  `a_basin_waits_while_a_chunk_of_it_is_unloaded`, `a_waiting_basin_passes_on_what_comes_in` (both
  faces; red on 0.42.1).

### 16. Blocks in a riverbed flooded a stepped fall, then the river drained dry (fixed in 0.42.1)

*Reported 2026-11-03 (0.42.0, a local terrain world).* "I added a row of 3 half blocks to the bottom
of the river, just below a ~3 block high stepped water fall. The river flooded up to the full height
of the waterfall top … then it filled downstream, then it suddenly drained completely."

- **Cause:** water flow W1 (docs/WATER_FLOW.md) found the pool and the pools above it together, each
  with the inflow that left it, so the fall's water was counted twice and the pool rose; rising, it
  merged with the pool above at once, though that pool's surface was higher, and so up the falls; the
  merged river had one level, set by its lowest way out, which left the upper steps dry, their
  inflow no longer where it came in.
- **Fix:** a basin's inflow from a pool now found upstream is taken off it (`Water::credit`); basins
  merge only when their levels meet (until then the higher pool's water is a wall, and it falls into
  the lower); inflows are kept where they come in, and a receding flood leaves each step that water
  comes into its own pool (`spin_off_shelves`).
- **Test:** `blocks_set_in_a_riverbed_under_a_stepped_fall_change_nothing` (three pools falling two
  rows each, three cells set under the fall; red on 0.42.0),
  `a_pool_and_the_pool_above_it_found_together_count_the_falls_water_once`,
  `a_flood_over_a_rivers_steps_leaves_each_step_its_water_when_it_recedes` (both faces; red before).

### 15. Cliffs walled the rivers in, and water stood open beside them (fixed in 0.35.0)

*Reported 2026-10-27 (0.34.0, the cliffs world, seed 3764472973 face A near (-3794, 240, -5614)).*
"Seeing issues with rivers in the cliff environment: big edges rising up on sides of river, and
drop off edges without filling low areas."

- **Cause:** the cliffs' slope mask read a river valley's steep banks as a hillside, so it raised
  walls along the water and sank the ground beside it below the water's level (and reshaped wet
  columns themselves).
- **Fix:** water columns are never reshaped; dry ground is never lowered or carved under the sea's
  surface; the cliffs fade in 8–30 m from a river or lake (`volume.json`'s new `water` input).
- **Test:** `cliffs_keep_off_the_rivers` and `no_water_stands_open_beside_lowered_ground` (seed 7,
  both faces: 53,731 / 44,935 open water faces on 0.34.0, none on the plain terrain or now) and
  `cliffs_keep_off_the_river_at_the_report` (4,945 of 5,930 columns by the water reshaped; none now).
  Checked in Chrome at the report's spot, before and after.

### 14. A steep edge up the hills where two river valleys meet (fixed in 0.32.0)

*Reported 2026-10-27 (0.31.4, a phone; seed 1397629041, face A, feet at (-7464, 106, 8376)).* "There's
still a bug with river valley meetups … It's pushing up a steep edge at the join point of the
terrain, though not by the water, but up the hills adjacent."

- **Cause:** each column carved its valley toward the nearest river channel alone (`terrain.rs`
  `column`). Where two rivers' valleys overlapped, the nearest switched from one to the other along
  a line, and the ground jumped there by the difference in their levels, out to the valleys' reach
  (12 m, ~100 m north of the report).
- **Fix:** the valley is carved by every channel whose valley reaches the column, the lowest
  (`Channels::each_near`), which cannot step. Generator 26, a new compatibility line.
- **Test:** `no_seam_where_river_valleys_meet` (round every confluence of seed 7, both faces: 600 and
  64 steps against the slope on the old rule, none now) and
  `no_seam_at_the_reported_meeting_of_valleys` (the report's spot: 297 steps, none now). Checked in
  Chrome from the report's viewpoint, before and after.

### 13. Crossing the midplane swapped the clouds, the fog and the light (fixed in 0.31.2)

*Reported 2026-10-27 (0.31.1).* "It's still an abrupt transition between the two world sides when you
move between them. Clouds disappear from one side, appear on the other, lighting changes (I think),
and a white fog band switches sides. So a clear cloudless night sky turns into cloudy, and cloudy blue
sky turns into vague foggy white."

- **Cause:** the sky's colours, the fog's colour and density, the exposure and the cloud deck were the
  viewer's face's, swapped across the 4 m flip band: only the viewer's deck was drawn, and the other
  face's sky below the horizon was hazed in the viewer's colours.
- **Fix:** the world owns its look (DAY_NIGHT.md §13, ADR 0050): each ray takes the sky, clouds and fog
  of the air it passes through; both decks are drawn, each lit by its own face; the exposure and
  saturation ease over `eye.adaptS`.
- **Test:** `the_sky_behind_is_the_same_from_either_side_of_the_midplane` (dwell-look: every ray 0.3 m
  above and below the midplane; 0.77 apart on 0.31.1, under 0.02 now), with the shader tests in
  DAY_NIGHT.md §13. Checked in Chrome at seed 7's portal axis, before and after.

### 12. A hard dark (day) or white (night) band past the rim of the world (fixed in 0.31.1)

*Reported 2026-10-27 (0.30.x, on a phone).* The 0.28.5 fix for bug 10 "looks really bad on the
horizon … a very harsh transition between the two and you can see a black edge during the day and
a white edge during the night"; "If you can look down off the edge of the world, I don't mind if you
can see a pretty transition."

- **Cause:** below the viewer's horizon the sky behind the world switched to the other face's within
  a pixel. Flying high, the gap between the rim and the horizon showed the other face's sky at the
  opposite time of day: night's navy at noon, the day's pale horizon at midnight.
- **Fix:** as through an atmosphere round the disc, a ray just below the horizon shows the viewer's
  horizon haze (it crosses the viewer's face's air to the rim) and the other face's sky fades in as
  the view turns down, fully from ~27° (`daynight.json` `sky.behindFrom`, `behindTo`;
  `dwell_behind_share`), so a portal's shaft still shows the other face's sky.
- **Test:** `the_other_faces_sky_fades_in_below_the_horizon_not_at_it` (dwell-look: the weight is
  ~0 a degree below the horizon, 1 looking straight down, never a step; the old shader had no such
  weight). Checked in Chrome 3.6 km over seed 7, past the rim, at noon and midnight: the band gone.

### 11. River banks walled off the sea, and tall edges where rivers met (fixed in 0.31.0)

*Reported 2026-10-27 (0.30.x, on a phone).* Rivers had "a 1-2 block high, 1-2 block wide edge along
the whole thing" that blocked "off access to the ocean", and "weird tall edges … where rivers meet".

- **Cause:** two rules of the river columns (`terrain.rs`, `worldmap/channels.rs`). (1) Where the
  land beside a river was lower than its water the bank was raised ½ m over it, falling 1.5 m per m:
  a one-cell ridge along every river through low land, and, at the mouths, where the river is at sea
  level and the land under the sea, a wall of sand out into the sea (119 of 185 levees on a 16 m grid
  of seed 7). (2) A joining river reached the other's level only at the other's centreline (and was
  capped at its own level where lower), so across the other's channel its water stood up to 5 m
  above, or ½ m below, the water beside it: a wall of water.
- **Fix (generator 25):** a levee falls away gently (`rivers.leveeFall` 0.15 m per m), a broad bank,
  and none where the river is at sea level. A joining river meets the other at its level across the
  other's channel (`joinMargin` plus both half-widths), the other's water backing up into it where it
  lies lower, and falls to it at most `joinSlope` 0.25 m per m upstream.
- **Tests:** `rivers_meet_at_one_level` (178 and 34 water walls round seed 7's 15 and 9 confluences
  on faces A and B before, none after) and `river_banks_are_not_ridges` (about 1.6 % of the banks
  ridges before, under 1 in 10,000 after: a natural beach cell remains), both faces. Checked in
  Chrome: seed 7's river mouth 16 km north of spawn, sand walls into the sea before, open after.

### 10. A white patch down a portal after visiting the other face (fixed in 0.28.5)

*Reported 2026-10-26 (0.28.x, on a phone).* After falling down a portal and back up, the middle
seemed to unload: a white, see-through patch showed looking down the shaft (toward the night side).

- **Cause:** not unloaded chunks. The sky behind the world was the viewer's: below the horizon,
  face A's horizon colour (pale at noon, navy at midnight). Fresh, face B's chunks round the far
  opening weren't loaded and the shaft ended in black; once the visit had loaded them, the far
  opening framed face B's night sky drawn in face A's daytime colour, with B's grass and a tree in
  silhouette round it. Found in Chrome on seed 7's portal (x 1779, z 20690) with the LOD off
  (`?lod=off`, added for it) and at both noon and midnight.
- **Fix:** below the viewer's horizon the sky behind the world is the other face's, with its sun,
  moon and stars (`dwell_sky_behind`; the frame's `DaySky` carries the other face's sky colours).
  The fog and the reflections keep the viewer's horizon colour (DAY_NIGHT.md §5).
- **Tests:** `the_block_carries_the_other_faces_sky_for_below_the_horizon` (dwell-look; the old
  block has no such colours, out of bounds) and
  `the_sky_behind_the_world_takes_the_other_faces_below_the_horizon` (the composite uses it).
  Checked in Chrome: from face A at noon, face B's midnight sky with its stars shows down the shaft.

### 8. Dark blue rectangles in rows across lakes and the sea (fixed in 0.28.1)

*Reported 2026-10-26 (0.27.7).* Some water rendered its side solid through the water: darker
blue rectangles and triangles in rows across a lake.

- **Cause:** at the edge of the streamed view a chunk's horizontal neighbours aren't loaded, and
  the mesh apron (`VoxelWorld::padded_box`, and `padded_from_neighbours`) read them as air. So a
  chunk of water meshed its side there as a wall, and the walls of the outermost ring showed through
  the surface in rows at fixed distances (seen in Chrome over seed 7's sea). Not the 0.26.7 strip:
  the rectangles stayed with it reverted.
- **Fix:** where a horizontal neighbour is missing and the chunk's own edge column holds a fluid
  (water or a flooded block), the apron goes on with it. Everything else stays air as before (the
  ground's border, and the open surface where no chunk is above). A neighbour's arrival meshes the
  chunk again with its real voxels.
- **Tests:** `the_apron_goes_on_with_the_water_where_no_neighbour_is_loaded` (dwell-world) and
  `a_water_chunk_at_the_streamed_edge_has_no_wall_into_the_unknown` (dwell-game) failed with the
  air apron and pass now.

### 9. A white edge at the sides of the water's reflection, the mountain's cut off (fixed in 0.28.1)

*Reported 2026-10-26 (0.27.7).* A white edge on the left of the reflection, where the mountain's
reflection was cut off.

- **Cause:** since 0.27.5 a reflected ray leaving through a side of the screen took the sky, so a
  mountain whose mirror image ran off the screen ended in a hard white band (0.27.1's clamp to the
  exit pixel was removed for its streaks over open water, bug 6).
- **Fix:** past the side the march goes on with its pixel held in the edge column (the scene taken
  to go on beyond the edge as it is there), and meets only land: a scene point standing over the
  reflecting surface (`SIDE_LAND_M`, 0.25 m). The floor seen through open water lies under it and
  is never met, so no streaks. In the Rust twin and the shader.
- **Tests:** `the_sides_keep_the_scene_not_a_fade_into_the_sky` checks that land past the left side
  is reflected to the edge (failed with the sky fallback) and that a low ray over open water is
  still a miss; `the_shaders_side_margin_is_this_one` pins the shader's margin. Checked in Chrome on
  seed 7: the range is mirrored to both edges, and the open sea shows no streaks.

### 2. Sun and moon shadows leaked through hills in a grid pattern (fixed in 0.27.6)

*Reported 2026-10-26.* Light leaked through hills in a grid pattern.

- **Cause:** inside the shadow cascades (160 m on High) the factor was the cascades' alone, and
  their casters are front faces within `casterReachM` (120 m) toward the light. With a low sun, the
  light's ray to a step on a hill's shaded side passes through the hill and leaves through its
  sun-facing side, often farther than 120 m away. So no caster shaded the step: each step's top
  was lit and its riser dark, a grid down the shaded slope near the player. Past the cascades the
  same slope was in the hill's shadow (the distant-terrain layer). Seen in Chrome on seed 7's range
  at 0.82 (the sun 5° up).
- **Fix:** inside the cascades the distant terrain's shadow multiplies theirs
  (`lod_shadows::combine`, the shader's `shadow_light`), so a hill beyond the casters' reach shades
  there too.
- **Test:** `a_hill_beyond_the_casters_reach_still_shades_inside_the_cascades` (dwell-look) failed
  on the old `combine` (lit at 1.0) and passes now.

### 3. Tall mountains had a permanent shadow band (fixed in 0.27.4)

*Reported 2026-10-26.* Tall mountains had a shadow that stayed put at certain heights. Possibly
related: the shadowed tops disappeared when flying a certain height above them.

- **Cause:** the distant-terrain shadows (DAY_NIGHT.md §12) draw the terrain from above into
  heightmaps between `heights.bottomM` and `topM` (`shared/look/lodshadows.json`). `topM` was
  1,536 m, from before the world map's ranges (generator 21), which reach ~2.6 km. Everything
  above it was cut from the maps and read as shaded: a flat-edged band at one height on every tall
  mountain, the peaks in shade at noon too. Seen in Chrome on seed 7's 2.3 km range.
- **Fix:** `topM` is 4,096 m.
- **Test:** `the_maps_hold_the_highest_mountains` (render/lod_shadows.rs) samples the highest
  ground of three seeds and checks it stays 256 m under `topM`. It failed at 1,536 m (ground at
  2,632 m) and passes now.
- **To confirm:** the tops "vanishing" from high above was likely the same dark shading. Recheck it
  in play.

### 6. Water reflections faded to white at the screen's sides (fixed in 0.27.1)

*Reported 2026-10-26.* Toward the left and right of the screen, reflections faded to a white gradient
where the reflected world should show. The owner's note continued "In addition, I would expect to
see …" and was cut off (see the rest below).

- **Cause:** the screen-space reflection faded every hit near any screen edge into the sky fallback
  (`edge_weight`, `ssr_edge`). A ray leaving through a side with no hit also took the sky. Low over
  land, the sky's horizon haze is near white.
- **Fix:** hits fade only near the top and bottom edges, in both the Rust twin
  (`render/reflection.rs`) and the shader.
- **Test:** `the_sides_keep_the_scene_not_a_fade_into_the_sky` failed on the old logic and passes now.
- **Regression (0.27.1, fixed in 0.27.5):** 0.27.1 also took the scene at a side's exit pixel for a
  ray leaving through that side. Over open water that pixel shows the floor through the water
  (water writes no depth), so it drew streaks fanning along the sides. The clamp is removed: such a
  ray is a miss (the sky). The same test now also checks that a low ray leaving over open water is
  a miss. It failed with the clamp (a full-weight hit at the left edge) and passes now.
- **The rest (fixed in 0.27.3):** the cut-off note was likely that tall mountains farther off didn't
  show in the water (owner, 2026-10-26). The reflection's reach was 2 km (Medium) and 4 km (High),
  so a ray toward a farther range found nothing and took the sky. The reach is now 24 km and 48 km.
  A hit's fade is measured at the scene's depth where it hit, since the march's own depth at the end
  of a step lay far beyond it on a long reach. Test: `far_mountains_are_reflected_and_near_banks_still_are`
  failed with the 2 km reach (no hit on a range 12 km off) and passes now.

### 5. Rivers had tall banks and were always shallow (fixed in 0.27.0)

*Reported 2026-10-26.* Rivers had a tall border along their edges, even blocking the way to the
ocean, and were always very shallow.

- **Cause:** a river's surface on the map often stood above the land along its banks (the coarse
  map's heights round the channel, and flat coasts). The columns then raised a levee to water + 1 m,
  tapering 1.5 m per m, so the water never stood open beside lower ground. That made walls along the
  banks and across the mouths. The depth came straight from the map's width curve (0.5–6 m, at least
  0.5 m).
- **Fix:**
  - Each river vertex's surface now lies at least the bank height (+ 0.5 m) under the lowest dry
    ground of the map cells round it. It never rises downstream and never drops below the sea, so
    levees are rarely needed.
  - The bank is 0.5 m (was 1 m).
  - Rivers are deeper: the depth curve is 0.8, 2, 4.5 and 9 m (was 0.5, 1.2, 3 and 6), at least 1 m.
  - A new compatibility line: worldgen changed.
- **Test:** `rivers_run_below_their_banks_and_down_to_the_sea` (dwell-worldgen `worldmap`, both
  faces) failed before the fix (7,188 and 6,621 river vertices too high) and passes now. The goldens
  and pinned digests are regenerated as generator 22.

### 4. Gaps between water blocks at height transitions (fixed in 0.26.7)

*Reported 2026-10-26.* There were gaps between water surfaces where a river steps down.

- **Cause:** at a step, the upper reach's column (water over water) sits beside the lower reach's
  last cell, which has air over it and its surface drawn ⅛ of a cell below its top. The mesher
  culled the side face between them as "fluid beside the same fluid", leaving an open strip from the
  lower surface up to the upper column's next cell.
- **Fix:** beside the same fluid whose surface is drawn lower, the cell draws that strip, from the
  neighbour's drawn surface up to its own (`fluid_side` now takes a bottom as well as a top).
- **Test:** `a_river_step_down_closes_the_strip_between_the_two_surfaces` (dwell-mesh) failed
  before the fix and passes now.

### 7. Placing a block too close to yourself flickered (fixed in 0.26.6)

*Reported 2026-10-26.* The block flickered in, then disappeared when the server refused it.

- **Cause:** the client predicted edits with the server's own checks, but passed no player bodies.
  So it showed a block placed into its own body, which the server refuses ("into a player").
- **Fix:** the client's prediction passes its own predicted capsule, built as the server builds its
  players'.
- **Test:** `placing_a_block_into_your_own_body_is_not_predicted` (dwell-client `game_local`)
  failed before the fix ("a block placed into the player's own cell was shown") and passes now.

### 1. Rubber-banding after a high-speed launch or a fast flight (fixed in 0.26.4)

*Reported 2026-10-26.* The most common case was a sudden fast speedup in the air.

- **Repro:** in Chrome, hover high up, then hold Shift+W at flight speed 10 or 12. It snapped back
  about 1 km, and about 10 km at speed 12.
- **Cause:** the server falls behind real time while it generates terrain for a fast flyer (its
  fixed step drops the backlog). The client kept predicting, so its inputs piled up in the server's
  queue. The server then threw inputs away in three places: its latency trim (the queue above 6 for
  a second), its queue cap (96, oldest dropped) and its input rate limit. Each skipped input is
  movement the client predicted but the server never simulated, so every snapshot snapped the
  player back. "Loading terrain…" showed because the client had outrun the streamed terrain.
- **Fix:** the client holds its prediction while the server reports 30 or more inputs queued
  (resuming at 6) or while 45 are unacknowledged. The server's trim became a backstop above 60, and
  it no longer trims a draining queue. Tests: `a_stalled_server_catches_up_without_dropping_the_players_inputs`
  (dwell-client `game_local`) failed before the fix (183 inputs skipped, then a 10.7 m snap) and
  passes now; `a_slow_device_walks_without_snapping` still passes.
