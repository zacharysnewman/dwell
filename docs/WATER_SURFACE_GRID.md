# Water as a simulated surface grid

Status: design for Phase 53, [ADR 0060](./adr/0060-water-surface-grid.md) (accepted 2026-11-05);
planned, nothing built in the game yet. Evidence: [`WATER_MODEL_EVALUATION.md`](./proposals/WATER_MODEL_EVALUATION.md)
§5 (the lab, the native benchmark on the game's terrain).

## 1. The owner's answers (2026-11-05)

| Question | Answer |
|---|---|
| The direction | Keep the map-scale erosion as the carver for now; let the water system decide where water stands, at generation and at runtime (dams, diversions, floods); carving at block scale later, as its own system |
| Where the generation fill runs | The whole world, at creation: "I think we'd need initial rivers set up, or we wouldn't have any rivers/lakes on first world start" |
| River beds until carving exists | None: "No beds, water finds its way" (the map's eroded valleys only) |
| What feeds the water | Both: the map's river flows now, rain everywhere as a later layer |
| What is data | All of it: sleep and band, flow physics, pools, sources |
| Visible noise | Anything under 10 cm reads as noise on ½ m blocks |
| World creation time | "Preferably under a minute, that would be the goal"; the creation screen shows what work is being done, "maybe even visually" (e.g. the map filling as the fill runs) |
| Rivers as sheets (no beds) | Prototype it first, before G1 ships |
| Carving at generation | "Let's try it with eroding, but only dirt and sand get eroded, not stone" (after the first bed-free fill made chains of ponds) |
| Storage | Asked for a recommendation; the one in §3 (storage): "Good to go on storage" |
| Caves and overhangs | "We'll save the caves/overhangs question for later" (§2a) |

## 2. The model (runtime)

- **Columns:** a depth (m) per 1 m ground column, over the column's ground (the top of its solid
  cells); each face between neighbours carries a discharge per metre of width (m²/s).
- **The law:** local-inertial shallow water (Bates et al. 2010) with de Almeida's θ-weighting, friction
  (Manning) from the flow's whole speed (the four perpendicular faces averaged, so diagonal flow is
  damped as straight flow is), no faster than critical, and an outflow limiter (a column never sends
  more than it holds). Each substep as long as the deepest active water allows (CFL).
- **Chunks of 16 × 16 columns,** stepped in parallel; only chunks with water exist in the water layer.
- **Sleep:** a chunk sleeps once no level in it moved more than the band (5 cm) over a calm window
  (10 s), with its faces at their recent averages; a sleeping chunk keeps its imbalance as one number
  and the water it owes. It wakes when its neighbours' flow drifts enough to move it half the band in
  10 s, when what it owes would, when a shared face jumps (an edit, a flood front), or on an edit; woken,
  it takes what it owes over 5 s, in proportion to its columns' depth. Springs count as a steady inflow
  and the sea as a held level, so their chunks sleep too.
- **Pools:** still water (≥ 64 connected columns, slower than 5 cm/s, level within 5 cm) becomes a pool:
  one level from its volume, its interior never stepped, its shore chunks computing the faces between
  pool and grid. A pool whose level moves faster than 1 cm/s, or that an edit reaches, goes back to the
  grid.
- **Conserved:** every face's flux goes to both sides; sleeping chunks and pools keep exact accounts.

## 2a. Water is its own layer (the owner: separate water and land chunks)

Water isn't blocks: it's a layer of its own over the ground (depth and flows per column, in water
chunks of 16 × 16 columns), so a water change never touches a terrain chunk:

- **Simulation:** water chunks step, sleep and wake by themselves.
- **Saving:** water deltas (§3, storage) apart from block edits.
- **Network:** only awake water chunks' changes, in their own messages; a rising river never resends
  terrain.
- **Drawing:** the water surface is its own mesh from depth; a water change re-meshes only the water,
  never the terrain chunk under it.
- **Physics:** swimming, buoyancy and currents query the water layer.
- **One way between them:** an edit to blocks changes the ground under the layer and wakes it; water
  never changes blocks.

Today water is blocks inside terrain chunks (ADR 0059's "water was blocks first"); this replaces that.

**Open:** a surface grid has one water surface per column, so water under an overhang, in a cave below
a lake or under an arch (the terrain volume's cliffs and arches) can't be held. Accept dry caves for
now, or later allow a few stacked water layers in the columns that have overhangs?

## 2b. As built: the runtime grid, G2a (0.48.2)

`dwell-sim` `water.rs` replaced the basins with the grid, before G1, on the generator's drawn rivers:

- **Generated water is held, not stepped.** A loaded chunk's water joins at the level its blocks show,
  held: it keeps its level and trades with free water beside it, at no cost. An edit frees the water
  within 3 columns; a free column 2.5 cm off its rest frees its held neighbours, so a change spreads
  exactly as far as it reaches. This is the prototype's "sleeping chunk" taken one step further: water
  nobody changed was never awake.
- **The rest layer.** The drawn rivers are flat pools stepping down a row, which no steady flow
  matches: freed as drawn, every step poured and the release ran down the whole river (a stone in a
  seed-0 river's bed emptied 844 cells). Each chunk keeps the water as generated beside the water as
  it is, stepped by the same law with its depths fixed, and a column changes by the difference of
  their flows. The generated state is then exactly at rest whatever its shape, while a dam, a cut or
  a flood still move water by the law: a dammed river backs up by the flow it was drawn with (in the
  browser, seed 0's river rose 0.5 m above a dam and fell 0.25 m below it). The rest layer's
  divergence is an implicit spring per column; after G1 those springs are the map's river sources.
- **A block set over water** traps what is below it (left as blocks, out of the grid); one set in it
  lifts its water by its volume.
- **A tick keeps to a budget** (0.49.7, BUGS.md #33): water runs on game time, so a water tick is
  seconds of water; with much water awake (a dammed river) its substeps took 50 ms of the server's
  thread a tick and the game stopped and started. A tick now takes at most `stepBudget`
  column-substeps (`flow.json`); past it the water runs slower than game time, and its own clock
  slows with it so it isn't taken for calm.
- **Water spreads only into loaded world**; the chunks of moving water and those beside it are kept
  loaded, so it goes on with no player near.
- **Not yet:** pools and the held sea (G2b), currents in held rivers (G3b), data (G4). Settings are
  constants in `water.rs` until G4.

## 2c. The owner's answers on G1 and G3 (2026-11-05), and currents as built (G3a)

The owner approved these recommendations (2026-11-05):

- **Clients keep water as blocks for now** (a deviation from §2a's own layer on clients): the server
  already draws the grid as water blocks in eighths, so clients see water move without a new mesh or
  message. The water's own layer on clients (its mesh, LOD and wire) waits until something needs it,
  such as smoother surfaces than eighths.
- **G3 starts with currents**: the flow per column for swimmers and floating bodies, in a small
  optional datagram for awake chunks, so no new compatibility line.
- **Caves and overhangs stay dry** (§2a's open question): one surface per column.
- **Rivers keep their dug beds (now without lips) until erosion**: G1b ships with G1c, not before
  (rivers without beds and without erosion are sheets over the valley floors).

**As built (G3a):**

- **The current table.** `dwell-world` `currents.rs`: each water chunk's columns' velocity (m/s along
  x and z), per face; the faces mirror only up and down, so a current is the same vector in the world
  and face-locally. The server's grid writes its awake chunks' (`Water::publish_currents`: a column's
  faces' discharges averaged over its depth; none under `currentStillMs` 2 cm/s or `currentDepthM` 5 cm, `flow.json`); a
  sleeping chunk keeps the currents it fell asleep with, as it keeps its flows.
- **Swimmers** swim in the water's frame: the swim layer's wish velocity has the current added, so an
  idle swimmer drifts at the current's speed and swimming adds to it (PLAYER_CONTROLLER.md §6.4).
- **Floating bodies**: water's drag acts on the velocity relative to the current, so a raft let be
  comes up to it (slowly, by the area it shows and the speed squared).
- **On the wire**: `WaterCurrents` datagrams (ARCHITECTURE.md §8.3), the chunks within 2 of a player's,
  each sent when its currents change or 3 s after it was last sent, 4 at most a time and none in a
  session's first 5 s, a chunk gone still once as zeros; a client forgets one not sent again within
  8 s. The client's prediction and stand-ins read
  the same table, so a swimmer in a current predicts as the server moves it.
- **Untouched rivers (G3b):** held water's current is the world map's river's
  (`TerrainGenerator::river_current`, found once as the chunk's water joins the grid): downstream as
  the column sees the channel (the map's direction through the meander warp's inverse), at the speed
  Manning's law gives the column's depth on the river's fall with the liquid's roughness (the grid's
  own law), at most `riverCurrentMaxMs` (1.5 m/s: a steep chute's would be a torrent, and a swimmer at 3 m/s still makes headway); a level reach
  and a lake are still. It goes to clients as any other chunk's currents.
- **The flow shown (G3d):** from the client's current table, so generated and player-moved water
  look alike: ripples carried by the current (a flow map), foam streaks along it, flecks drifting
  on it, each a setting and per liquid as data (`shared/look/waterflow.json`; ARCHITECTURE.md §6.7).
  Seen from above seed 0's river at (6745, 9644), the streaks run downstream and the flecks drift.

**Showing the flow (owner, 2026-11-05):** all three of: ripples that move with the current, foam
streaks along it, and particles drifting on it; each can be turned off in Settings. All water
behaves and looks the same by the same law whether the generator or a player put it there (a held
river's current is the map's, a freed one's the law's; both are drawn alike from the one current
table). As built: G3d below.

## 2d. The water's own thread (ADR 0062, the owner 2026-11-06; built, G5)

The owner: slowing the water whenever much of it moves "is not really acceptable"; "use message passing
for now, make a note about future possibly using shared memory threads"; the water stays saved.

- **A water host** (`dwell-sim` `water/host.rs`): the grid behind one interface, placed **inline** (the
  calling thread: tests, the world-creation fill, a fallback), on a **native thread** (the dedicated
  server and native tools, `std::thread` and channels), or in a **browser worker** (a second worker the
  local-mode worker starts, by `postMessage` with transferred buffers).
- **What the grid reads becomes its own** (`water/cells.rs`): a byte a cell, solid, open, or water and its
  fill in eighths, per loaded chunk (a uniform chunk as one value). The grid's reads (`cell_kind`,
  `floor_at`, `found`, the scan for water) go through it instead of the `VoxelWorld`.
- **In** (the server, each tick, batched): chunks loaded (their cells) and unloaded; cells edited (their
  new kinds; the edit's columns touched, as `touch` does now); the players' positions; at a water tick,
  the water time to run; asks for the record (before a save) and for a report (`waterReport`).
- **Out** (read at the server's next tick start): water cells written (each with the kind it replaced),
  the awake chunks' currents, the chunks it wants loaded, the record, reports. A write whose cell the
  world has since made solid, or anything but open or water, is dropped: the edit that did it is already
  on its way in. The server broadcasts the writes as one modification, as now.
- **Falling behind instead of slowing physics:** a water tick not yet run when the next is due adds its
  time to it; the worker runs what it is owed as fast as it can, with the per-step guard (`stepBudget`)
  raised so a tick still ends; past `maxOwedS` (data) the rest is dropped and the water's clock slows, as
  now.
- **Saving:** the server asks for the record a moment before each save and writes the newest it has;
  closing a world (and a native server's shutdown) waits for a fresh one, at most a second.
- **Later** (not built): shared memory (WASM threads; needs cross-origin isolation, which GitHub Pages
  can't set, a service-worker shim there, native in Electron and Capacitor), and the grid split across
  threads by chunk if one thread isn't enough.

**As built (G5):**

- **The copy** (`water/cells.rs`): `Cells`, a `ChunkCells` per loaded chunk (`Uniform(byte)` or a
  32 KiB `Mixed` array); bytes 0 solid, 1 open, 2–9 liquid in eighths, 10 and 11 a block holding
  water in its open part (a ramp: shaped, with a `flooded` property), dry or flooded (`byte_of` from
  the registry's `liquid`, `fill`, `replaceable`, `solid`, shape and properties). A ramp is solid to
  the flow; drawing a column floods the ramps in its rows and its neighbours' while the water beside
  or over them stands half a fill step into their cell, and dries them when it goes (BUGS.md #36). The grid's reads (`floor_at`, `found`, the scan, what
  an edit trapped, `draw`) all go through it; `draw` writes its own copy as it writes water. On the
  wire a mixed chunk is runs (a byte and a u16 length), a few hundred bytes for a terrain chunk.
- **What changed** (`cells::Mirror`): each loaded chunk's `Chunk::stamp` and revision as last told.
  At each water tick, chunks new to it go as fresh loads (their water read, as `scan` did), chunks
  stored again or changed outside a modification go whole (not read again), gone ones as unloads.
  Edits and the water's own writes bump a chunk's revision in `broadcast_changes`, where the mirror
  is told (`told`), so they don't send the chunk again: an edit goes as its cells (`In::Edit`).
- **The host** (`water/host.rs`): `WaterHost` with `Placement::{Inline, Thread, Remote}`
  (`ServerConfig::water`; the dedicated server's default is `Thread`, tests' and the fallback
  `Inline`); `WaterCore` the water's side (receive a batch, `work`: run what is owed, answer); batches
  `WI1`/`WO1` with a sequence number, the answer carrying the last one seen (so the server knows what
  is still queued); malformed batches refused. Inline keeps 0.49.7's behaviour (`stepBudget`, the
  rest lost); the thread and the worker run `threadStepBudget` a run and owe the rest up to
  `maxOwedS` (`flow.json`); the native thread merges batches that queued while it ran (their time
  owed together).
- **The browser** (`client/src/local/waterWorker.ts`, `web/water_worker.rs`): the local-mode worker
  starts the water worker (a nested module worker; the world's maps passed to it so its rivers need
  not compute them) and carries the batches both ways, transferred; the first batch is `In::Init`
  (the world's generator and seed). Without nested workers, or with `?water=inline` (the bench's
  comparison), the water runs inline. A save (the page hidden or closed) asks for the record, carries
  it back and waits for it, a second at most, before it commits; autosaves ask 30 ticks ahead.
- **Telemetry:** `Server::water_stats` (the host's `HostStats`: batches unanswered, runs, the last
  run's water time run, owed and dropped and its wall time, the longest run, the water's clock, awake
  chunks, the copy's bytes; writes dropped as overtaken); in the browser `__dwell.serverStats()` (and
  the bench's `server` step) with the server's tick time a second.
- **Tests:** `water::tests` `the_water_on_its_own_thread_runs_as_inline` (both faces: the same cells
  and record through the inline and threaded hosts, each tick waited on), `the_record_survives_the_host`,
  `a_threaded_water_owes_what_it_hasnt_run_and_drops_past_the_cap`; `host::tests` (batches round
  trip, malformed refused); `cells::tests`; `tests/water_thread.rs`: the same water from a server
  inline and threaded, `a_water_write_overtaken_by_an_edit_is_dropped` (red without the check: the
  write turned the placed stone to water), `the_physics_tick_does_not_wait_for_the_water` (a host
  held 250 ms a run: inline a tick takes it, threaded none over 125 ms),
  `the_waters_record_is_saved_and_restored_across_its_thread`; the measurement below
  (`measure_a_flood_on_and_off_the_tick_thread`, ignored).

**Measured (2026-11-06, an M-series Mac):**

| | Server tick p50 / p95 / max | The water's clock over 20 s (1,440 water s given) |
| --- | --- | --- |
| Native, a 61 × 57 lake cut 20 wide on the flat world, 60 Hz paced: inline, no budget (before 0.49.7) | 2.3 / 44.8 / 146.7 ms | 1,440 s |
| … inline, `stepBudget` (0.49.7–G5) | 2.7 / 14.0 / 18.8 ms | 80 s (1,360 dropped) |
| … its own thread (G5) | 2.1 / 3.5 / 4.9 ms | 1,437 s (2.7 owed; runs up to 29 ms on the water thread) |

In the browser (the bench, `?local=1&seed=0&fresh=1&at=2364.5,142,7443.5`, a stone wall 31 m across
the river and a pit 31 × 29 m dug beside it, 40 s, `--unthrottled`): with the water worker the local
server's ticks averaged 7.0 ms, its worst `advance` 111 ms in the first 3 s (the edits' own chunk
work) and 15–20 ms after, and the water ran 2,808 of 2,880 water s (owed none, dropped none; its runs
8–18 ms); inline (`?water=inline`) the ticks averaged 9.4 ms, worst 237–252 ms over the first 9 s,
and the water ran 916 s (1,859 dropped). The copy of the world held 1.7–1.8 MB there. What is left on
the server's thread during a flood is mostly the LOD's: each chunk the water rewrites is downsampled
again (`update_lod`, on the tick in the browser and the tests' servers, `lod_threads` 0), a cost of
the water's writes, not its simulation.

**The flood's LOD and terrain off the tick (the follow-up, owner 2026-11-06):** with the water on its
own thread, what was left on the browser server's tick in a flood was the LOD: every water tick
rewrote the flooding chunks, and each one's LOD section was computed again (the generator's 2–10 ms a
section natively, more in WASM), on the tick (`update_lod`; a native profile of the dam site had 77 %
of the tick there, 57 % in `generate_lod`). Weighed: a per-tick budget alone (a section is one job:
a tick still takes a whole one), coalescing alone (the first computation of each section is still
the generator's, on the tick), and moving the work off the tick. Built, all three kinds:

- **Coalesced:** a chunk changed by the water's writes holds its section 2 s from the first such
  change (`LodPropagation::mark_chunk_settling`, `WATER_LOD_HOLD_TICKS`), so a section the water
  keeps rewriting is computed once a hold, not ten times a second; an edit computes at once.
- **Kept bases:** a modified section's generated cells are kept (`MAX_BASES`), so computing it again
  is its downsample only. Over a 10 s flood on the flat world, 51 sections ran the generator 51
  times (2,740 times before) (`a_floods_lod_is_computed_once_a_hold_from_kept_bases`, red before).
- **Off the tick in the browser:** the local server's LOD sections and the chunks it wants generated
  (the water's wanted chunks among them, a chunk's generation 10 ms natively) are jobs for the
  **server-jobs worker** (`jobs.rs`, `ServerConfig::jobs_remote`), a third worker beside the water's;
  `?serverjobs=inline` keeps them on the tick for comparison.
- **The same LOD:** after a flood the sections, on the tick or from the worker, are those computed
  from scratch from the world (`the_lod_after_a_flood_matches_a_recompute_on_the_tick_and_in_its_worker`).

Measured at seed 0's dam site, natively (`water_lod.rs` `measure_the_dam_sites_flood_ticks`, the
wall and pit, 60 Hz paced): the LOD on the tick, ticks up to 30 ms in the flood's first 3 s (the
edits' own tick 26 ms) and ~5 ms on average before the hold and kept bases, 0.2–0.4 ms after; with the
jobs off the tick, no tick over 3.9 ms (the edits' own). In the browser (the bench, `--unthrottled`):

| | Ticks: mean, worst (first seconds; then) |
| --- | --- |
| The wall alone (`dam-only`, 30 s), 0.49.12 (#348) | 4.2 ms, 58 ms (first second; 8–15 after) |
| … the jobs on the tick, held, kept bases (`?serverjobs=inline`) | 0.46 ms, 54, 26, 31 ms (first 4 s; ≤ 7 after) |
| … the server-jobs worker | 0.30 ms, 7 ms (13 once at 5 s; ≤ 6 after) |
| The wall and the pit (`dam-flood`, 40 s), 0.49.12 (#348) | 7.5 ms, 130, 40, 42, 36 ms (first 4 s; 15–26 after) |
| … the server-jobs worker | 1.5 ms, 45 ms (first second), 27, 29 (2–7 s; ≤ 19 after) |

What is left over 25 ms with the pit is the pit's own collapse: its cut undercuts the bank and the
pieces that fall are bodies stepped on the tick (a trace of the first 4 s: of the server's 1,118 ms,
764 ms in the physics step, the water and LOD none of it). Without the pit no tick passes 13 ms.

**Deviations:** the browser's water worker is started by the local-mode worker (nested), not by
`connect.ts`, so a friend world's host carries it too; the water's report is asynchronous there
(`takeWaterReport`). The inline host keeps the step budget and loses the rest, as before (a fallback,
and what the tests' servers run). The players' positions are sent but not used yet. `Water::tick` on a
world stays for tests and tools, its copy kept by a mirror of that world.

## 3. Generation

- **Kept:** the world map (ADR 0049): uplift, stream-power erosion (the valleys), fill-and-spill lakes,
  rain routed and accumulated, rivers traced with their discharge.
- **Removed from the block-scale generator:** river beds cut from the map's lines, drawn water levels,
  lips, chutes, the pool graph, lake shores forced over their level.
- **The whole-world fill, at world creation:** (1) fill-and-spill on the block-scale ground: a priority
  flood from the sea and the world's edge fills every depression to its spill level; the sources'
  discharge is routed down the flood's tree; (2) a short grid settle from that state, so the stored
  water is at rest under the runtime law; (3) the water layer stored with the world. Both faces.
- **Erosion at generation (G0 prototype, `waterbench` `nobeds`):** after the fill, the same grid runs
  and the water carves its own channels: each column's water carries a load of sediment (more the
  faster it runs), picks up soil where it carries less than it can (the top block and the dirt under
  it, 3.5 m; never stone, nothing where the band is rock), drops it where it carries more; the load moves
  with the water, so pits fill as channels cut. Sped up: 10 water minutes carve years.
- **Valleys kept (G0 finding):** with no valleys either, the map's coarse ground puts saddles 15–47 m
  high across a river's course, so fill-and-spill makes deep ponds that 3.5 m of soil can't drain. With
  the generator's valleys kept (from the map's courses) and only the bed, lips and drawn water dropped
  (`TerrainGenerator::ground_with_valleys`), fill-and-spill and erosion make a river in a channel it
  carved (2 km square: fill 0.4 s, 10 water minutes of erosion 37 s on one core). Shown to the owner;
  their verdict sets G1.
- **The river at the depth its flow needs (G0 finding):** a fill that put the river in as a film left
  it half an hour from reaching its lower reach (a 40 m-wide, nearly flat valley floor stores ~20,000 m³
  before 11 m³/s passes on): the "broken" stretches. The fill now routes each spring's flow down its
  way and, across each point of the way, sets the level whose wet section carries that flow by
  Manning on the local slope; the river runs its whole length from the start. Open: below a steep drop
  the river runs deep but carries less than the map's flow (sediment at the drop's foot ponding it, or
  the measuring cut off the meandered course); a cut across a steep valley side sets water too high
  for a moment.
- **Sources:** springs where the map's rivers rise and where their discharge grows; the sea a held
  level. Rain everywhere later.
- **No long spin-up** (the owner asked whether creation must run far faster than real time): the
  fill-and-spill doesn't step time; it puts the water where it stands in one pass (seconds), so rivers
  and lakes exist the moment it ends. Only the settle steps time, and from a good start it only irons
  out centimetres. Measured from other starts on one face (everything awake, M1 laptop): the
  generator's drawn levels needed ~1,500 water s to balance (~25 min), Manning normal depth ~600 s
  (~10 min); sleeping runs 3–22× faster. So creation stores the fill, play starts at once, and the
  runtime law (the same law) finishes the settle as it runs, near players first; optionally a short
  settle around the spawn while the world loads. To measure in G0: the settle from a fill-and-spill
  start.
- **Under a minute, shown:** creation's target is under a minute (the owner), natively and in the
  browser, with its stages shown as they run (the map's hydrology, the fill per region, the settle
  around the spawn), visually if possible: the world map with its water spreading as the fill runs.
- **Storage (recommended):** the generated water (the fill: depths, and each face's flow from the
  routed discharge) is a pure function of the seed, so it isn't saved: it's recomputed on load, as
  untouched terrain is. The water settles away from the fill wherever the simulation runs (the owner:
  "doesn't it settle though?"), so a chunk is saved only when its water differs from the fill by more
  than the band (5 cm) or was edited (dams, floods, player water): a settle that moves water by cm
  re-happens invisibly after a load, and a saved chunk keeps depth and flows both (a flowing river
  saved without its flows restarts from rest and surges), compressed (depth to the mm, flows as half
  floats, per chunk). This holds only if the fill is close to steady, which G0 measures; if the settle
  moves water more than the band across wide areas, improve the fill until it is within the band, or
  settle once at creation and keep the result as a per-seed cache (compressed, tens of MB a world,
  within the minute).

## 4. Data (`shared/water/`)

```json
{
  "sleep": { "band": 0.05, "calmWindow": 10, "wakeDrift": 10, "release": 5, "faceJump": 0.1 },
  "flow": { "manning": 0.03, "theta": 0.95, "cfl": 0.7, "vectorFriction": true },
  "pools": { "minColumns": 64, "maxSpeed": 0.05, "levelBand": 0.05, "maxRate": 0.01, "check": 10 },
  "sources": { "riverSprings": true, "rain": false, "seaHeld": true }
}
```

One generator emits the Rust constants and the TS shell's copy; a bad entry fails with a message; a
test adds a fixture by data alone. Per-world values travel in WorldSettings (ADR 0054).

**As built (G4a, 0.49.1):** the runtime's settings live in `shared/fluids/flow.json` (the file the
basins used, reshaped, so its generator and CI check carry on): `speed`, `tickEvery`, `theta`, `cfl`,
`maxSubsteps`, `sleepBandM`, `wakeBandM`, `calmS`, `releaseS`, `jumpM2s`, `releaseM`, `editReach`,
`fillHysteresis`, and `liquids` (each liquid's block, Manning's roughness, weir for the generator's
chute streams, running block; the first is the one drawn). `shared/fluids/gen.mjs` validates it
(ranges, registry blocks, the wake band at least the sleep band) and emits `flow.gen.rs`;
`dwell_sim::water::Settings::of` turns it into the engine's settings and `Water::with_settings` runs
any (`a_liquid_given_by_data_alone_runs_by_its_own_roughness`). The TS shell reads none of it. Pools
and sources join the file with G2b and G1b; per world, only the speed so far (WorldSettings).

## 5. Phases (a draft for the implementation plan)

1. **G0, the prototype finished:** the whole face settles (the awake count stops climbing), no column
   over 10 cm against stepping everything, the pools' passes parallel; measured in the scratch
   benchmark on the game's terrain (a river-heavy window, then a whole face). Then the fill-and-spill
   start on the game's ground with no beds: its time, the settle after it, and rivers as sheets shown
   to the owner (a prototype, before G1). Creation's budget: under a minute.
2. **G1, the fill at generation:** the steady solve on block-scale ground plus the settle,
   deterministic natively and in WASM; the block-scale generator stops drawing beds, levels and lips;
   a new compatibility line.
3. **G2, the runtime grid in `dwell-sim`:** the water layer per chunk, sleeping, pools, held outlets,
   saving; edits (dams, diversions, floods) as ordinary block edits; both faces; replaces basins and the
   pool graph.
4. **G3, clients:** awake chunks' water over the network, the surface drawn from depth (the water
   renderer, LOD), currents on bodies and swimmers.
5. **G4, data:** `shared/water/*.json`, generator, validation, WorldSettings.
6. **Later:** rain as a source layer; block-scale carving by the same flow.

## 6. Open questions for the owner

- **Caves and overhangs** (§2a): deferred by the owner; until then a column holds one water surface.
