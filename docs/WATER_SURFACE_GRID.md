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
- **A tick keeps to a budget** (0.49.6, BUGS.md #32): water runs on game time, so a water tick is
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
