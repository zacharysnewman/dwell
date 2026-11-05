# Evaluating water models: pools, flowing reaches, or a water surface grid

*Evaluation, 2026-11-04, for the owner ("I want you to really evaluate that proposal before we go
down that path of reaches of flowing water"). Not adopted: nothing here is architecture.*

## 1. What water has to do in Dwell

Collected from the owner's reports and answers (0.42–0.48):

| # | Requirement | Owner's words, where they came up |
|---|---|---|
| R1 | Water moves only where it is seen to; no hidden flows or walls | "Water should have to visibly connect pools" (0.47) |
| R2 | A dam raises the river behind it, which then flows **over** the dam, standing above it | "overflowing means, flowing OVER the dam" (0.48) |
| R3 | Spilled water runs downhill to the lowest place near and spreads on flats | "Always try to flow down" (0.48.1) |
| R4 | Rivers look like flowing rivers, not terraces of ponds | the rethink (0.48.1) |
| R5 | Levels read at clean heights (rounded to ½ m blocks, at least visually) | "Levels should round to the nearest half block" (0.45) |
| R6 | Placing a block somewhere unrelated changes nothing | the strict sweep (0.42–0.47) |
| R7 | Water is conserved; currents push swimmers and floating bodies | W4, ADR 0053 |
| R8 | Works server-authoritative, at world scale, across unloaded country, saved, on both faces | ARCHITECTURE |
| R9 | An extendable, data-driven system | CLAUDE.md |

## 2. The candidates

### A. Pools and lips (today, 0.48)

Every body of water is a pool with one flat level; pools pass water to each other through lips by
the weir law. Rivers are chains of pools a ½ m step apart (the pool graph, ADR 0059).

- **Good:** conserves water exactly; dams, cuts and holes behave correctly; cheap (a pool is one
  number); any 3D shape works, caves and tanks included (pools are flood-filled from cells).
- **Bad:** a river is a staircase of still ponds, so it never *flows* (R4). Balancing the
  staircase took exact lip planning, chutes, junction lips and channel widening (0.45–0.48), and
  it is still fragile (BUGS 27, 28). "One flat level" has no place for water that is moving across
  land: thin sheets spread at the pool's level (fixed in 0.48.1 with rims and reach limits, i.e.
  more special rules). Currents (R7) would have to be invented from lip flows.

### B. Flowing reaches (the idea under evaluation)

Rivers become a 1-D model along the pool graph: each short reach has a water depth set by its
flow and slope (Manning's equation, or the diffusive-wave form of the shallow-water equations);
water moves between reaches by the slope of the surface. Lakes and dammed water stay pools;
overland water uses the downhill rule.

- **Good:** the surface slopes smoothly (R4); depth follows from flow; a dam's backwater spreads
  upstream on its own; currents fall out of the flow (R7); very cheap, and good for distant
  rivers.
- **Bad, and decisive:** it is a **second model of the same water**. Every place they meet is a
  seam: where a dammed river becomes a pool, where a flood leaves the channel, where a player's
  spill enters the river, at junctions, lakes and the sea. The seams are exactly where every bug
  of 0.42–0.48 lived (feeds, links, seeps, hidden walls). Worse, **every block edit must be
  translated into 1-D terms**: a dam that blocks half the channel, a bridge pier, a cut through a
  bank, a block in the bed. Each needs a rule mapping voxels to a cross-section or a weir, and
  those rules are where fragility would move. A flood out of the channel cannot be 1-D at all.
- **Verdict:** wrong as the main model. **Right as the far-field model**: rivers nobody is near,
  where nothing is edited, the river is just a channel, and the pool graph's virtual pools are
  already a crude version of it.

### C. A water surface grid (columns with virtual pipes)

Each ground column holds a water depth; each column connects to its four neighbours by "virtual
pipes" whose flow accelerates with the difference in water surface height. This is the
shallow-water model of O'Brien & Hodgins (1995) as simplified by Mei, Decaudin & Hu (2007). It is
what Timberborn ships: each tile has a depth, links to its four neighbours by virtual pipes, and
the current is computed from how much water moved between ticks. Its dams, levees and floodgates
work on exactly that.

- **Good:** **one model for all surface water**: rivers flow on slopes and draw as slopes (R4);
  dams hold water back, which rises and then flows over the lowest point of the dam *or* the banks,
  as real water does (R2); spills run downhill and spread on flats with no special path-finding
  (R3); floods leave channels and return naturally; currents come straight from the pipe flows
  (R7). No lips, rims, sheet limits, chutes or junction rules: the behaviour emerges. It degrades
  gracefully: a small change makes a small difference, instead of an integer lip off by one cell.
  It's proven in a shipping game whose core is dams.
- **Bad:**
  1. **One surface per column.** Water in a cave under a hill, an aqueduct on pillars, a tank
     above a pond: a column can hold more than one body of water. Fix: *layered columns*
     (each column a short list of water spans between solids). This is the most real cost.
  2. **Scale.** A face has ~370 km of river and the sea. It cannot all be simulated. Fix: simulate
     only **active** columns (near players, or not yet settled), sleep settled ones, and let the
     river graph (B's far-field) carry rivers out of sight, as virtual pools do today.
  3. **Rivers at rest as generated (R6).** The generator must lay the water at a depth the model
     holds steady, or rivers twitch when woken. Fix: the generator computes each river's steady
     profile with the same 1-D equation as the far-field (B) and lays the water at it; the grid
     then starts within millimetres of steady. Small drift stays under the drawing snap.
  4. **Thin water and rounding (R5).** Depth is continuous; the drawing still needs rules: a
     minimum depth drawn (or none), and levels rounded to ½ m blocks where the owner wants it. That's
     the same visual question as today, and independent of the model.
  5. **Time step.** An explicit pipe model needs short steps on deep water (about 0.1 s at 10 m
     deep with 1 m columns). Today's water tick is 7.2 s of water time, so ~70 sub-steps over the
     active columns; deep still lakes sleep.
  6. **A rewrite.** The pool engine (`water.rs`, ~3,900 lines) would be replaced, not extended. The
     graph, the generator's river work, the mesher's water drawing and the dev tools stay.

### D. Minecraft-style flowing blocks (for comparison)

Source blocks, flowing blocks spreading 7 cells and seeking drops within 4. Simple and readable,
but it does not conserve water, has no real levels, and dams don't raise rivers. That fails R2 and
R7. Worth copying for its *look* (smooth corners, flow-scrolled textures) and its "seek the nearest
drop" rule (0.48.1), not its model.

## 3. Cost estimates (*estimates*, not measured on Dwell)

- **Grid, near field:** a 256 m river stretch at 10 m wide is ~2,600 columns; a flood over a 100 ×
  100 m floodplain is 10,000. At ~4 pipe updates per column per step and 70 steps a water tick,
  that is about 3 million updates a tick for the flood: a few milliseconds natively, a few times
  that in the browser's WASM. Timberborn runs a full map on its tick every couple hundred ms.
- **Far field:** ~10,000 reaches a face, simulated only when off steady (as virtual pools are).
- **Memory:** a depth and four flows per active column. Sleeping columns are just their depth
  (the drawn blocks already store it).

## 4. Recommendation

**Don't build B as the main model.** It would bring back the seams the last week removed and move
the fragility into "voxel edits → 1-D parameters".

**Prototype C before deciding**, in a sandbox scene (no world integration):

1. A layered column grid with virtual pipes over a test valley: a river with a source, a dam level
   with its banks, a dam lower than its banks, a spill onto terraces, a flood over a floodplain.
2. Drawn with the existing water rendering (depth → fills) and corner-averaged surfaces.
3. Measured: does each case look and behave right, is it stable (no waves or oscillation at dams,
   which Timberborn reports it had to tune), and what does a tick cost natively and in WASM?

If C passes, the plan is: grid near players, B as the far field and as the generator's steady
river profile, the pool graph's topology kept. Pools and lips would retire, not get patched
further. If C fails the prototype, A with 0.48.1's rules stays, and the effort goes into the look
(sloped running water) instead.

## 5. Prototype results (2026-11-04)

The owner: the "only near players" scale assumption is unmeasured, prototype it. Built:
`dwell-sim` `watergrid.rs` (the model, five tests, a cost and a stability bench) and the
[Water Grid Lab](../prototypes/water-grid-lab.html) (the same model in the browser, five scenes,
camera and edit modes, tools to place, dig, add springs and remove water).

**Two versions.** The first was the virtual-pipe model as Timberborn describes it (a pipe's flow
accelerated by the difference in surface height). Its flaw showed at once: a pipe carries flow
whatever the depth, so a 1 cm film carried the whole river over a dam, and nothing could be seen
pouring over it (the owner: "that model still doesn't show water pouring over the top of the
dam"). The second, now in both, is the **local-inertial** form of the shallow-water equations
(Bates, Horritt & Fewtrell 2010; LISFLOOD-FP, used for flood mapping): each face carries a
discharge per metre of width through the depth standing over the higher of its two beds, with
Manning friction and no faster than critical flow. Flow needs depth: water over a crest stands at
least critical depth (a test). **Springs** hold their column at a depth (an infinite source, so
the river's flow is what its channel takes), and water reaching the scene's edge is removed: the
water between is finite and conserved (the owner: "finite water, with infinite source, and delete
at the bottom").

**Behaviour** (tests and the lab):
- Water is conserved (to 1e-7 of what came in).
- A river settles in its channel, passing at its foot what its springs bring (within 5 %), banks dry.
- A dam below the banks: the river backs up behind it, stands 0.29 m over its top, and falls into
  the river below; banks dry.
- A dam level with the banks: the river rises over the dam and the banks beside it (0.14 m over
  the dam), and the banks' water falls back into the channel along its edge.
- A spill from a pond through a gap in its rim runs down a terraced hillside into the river.
- Drawn: a surface per column, corners averaged only with neighbours at about its level, and an
  unlit **curtain** of falling water wherever a column pours into a lower one (over a dam's face,
  off a bank into the channel, down a terrace step).

**Cost, measured** (native, release, one thread, Apple silicon; `cargo test -p dwell-sim --release
--lib watergrid -- --ignored --nocapture`):

| | |
|---|---|
| Virtual pipes, per wet column per step | 8 ns idle (38 ns with the machine at load 5) |
| Local-inertial, per wet column per step | 65–80 ns at load 5, about twice the pipes beside it: **~15–20 ns idle (estimated, to measure quiet)** |
| Longest stable step | **0.15 s at 2 m deep** (0.7 × cell / √(g × depth): deeper water, shorter steps; the lab sub-steps by the deepest column) |
| So, per column per second of water time | ~130 ns |
| One core, real time | **~7 million wet columns** |
| The lab's JavaScript | ~20–35 ns per column per substep |

**What that means for scale** (the sizes are estimates from the world map, not measured):
- Every river on a face, 1 m columns: ~370 km × ~15 m ≈ 5.5 million columns ≈ **most of one
  core**, natively, all the time, no "near players" needed; or a fraction of a core shared across
  threads (the step is the same work per column, easy to split by rows). Lakes add by their area,
  and deep still lakes would sleep (they force short steps for nothing).
- Every column of a face, dry ones included (π × 25 km² of land ≈ 2 billion), is out of reach and
  unnecessary: dry ground with no water beside it is skipped.
- The sea is a boundary at a fixed level, not simulated.
- **The limits are memory, the browser and deep water**, more than CPU on a server: 5.5 million
  columns at ~24 bytes is ~130 MB, too much for a browser-hosted friend world; a sparse store (only
  tiles with water) or a coarser grid away from players would bring it down.

So the earlier line ("it would simulate only active columns near players") was wrong as a
requirement on a dedicated server; in a browser-hosted world some such limit is likely still
needed. Not yet measured: layered columns (caves), the cost in WASM, the memory of a whole face's
rivers, and a quiet-machine cost of the local-inertial step.

### 5.1 Cellular water (3D), beside it

The owner asked about particles a voxel in size; at that size SPH stops working (a river 1–2
particles deep has no pressure field, and ~10 million particles at ~0.01 s steps is hundreds of
cores), so the lab instead runs its cellular counterpart: each voxel (1 × ½ × 1 m) holds a fraction
of water, passed down, sideways (evened with its four neighbours) and, when pressed over full, up
(after w-shadow's cellular-automata fluid, in 3D). Springs hold a cell full; sinks empty theirs.
Four scenes: a cave under a pond (a shaft drains the pond into the cave, which fills, then the pond
overflows), connected tanks (a U-bend), a dam on a river, an aqueduct on pillars pouring 7 m to the
ground. A cutaway shows inside.

Measured in the lab (JavaScript, 48 × 28 × 32 = 43,008 cells):
- **Fully 3D:** the cave fills through its shaft, the aqueduct's water runs along its trough and
  falls as streams to the ground: none of that fits one water surface per column.
- **Conserved:** the tanks hold exactly 1,760 cells of water after 100,000 steps.
- **Cheap per step:** 0.24–0.28 ms for all 43,008 cells, ~6 ns a cell (JS; solid and dry cells
  are skipped).
- **Pressure is very slow:** the tanks, 19 cells full against empty, stand at 15/5 after 20,000
  steps and 12/8 after 100,000 (level is about 12/12). Each cell can be pressed only 2 % over full,
  so a level difference passes through a pipe a little a step: the known weakness (Dwarf Fortress
  adds a separate pressure rule).
- **Slow, thin flow:** water moves about a cell a step; a stream off the aqueduct carries 0.005 of
  a cell per cell, and a dam's overflow is 6 cm. Rivers have no momentum.

So cellular water answers the grid's one-surface limit (caves, pipes, overhangs, falls) and the grid
answers cellular water's slow pressure and flow. The hybrid in §4 (grid for open water, cells only
where the water isn't one surface) stays the strongest option; the cellular part would need a
pressure rule.

### 5.2 The steady solve (2026-11-05)

The lab's **Steady solve** button answers each surface scene without stepping water through time:
priority-flood from the sinks (each column's spill level and its way out), discharge summed downhill
(shared between lower neighbours by drop), a lake standing at its crest by the weir law over its
whole rim, a stream at the depth that carries its discharge (critical at a brink, a standard-step
backwater above it). Compared with the grid simulation settled (8,000 steps, 800 s of water time):

| Scene | Mean depth error | Notes |
|---|---|---|
| River | 0.4 cm | volume 100 vs 97 m³ |
| Dam level with banks | 3.4 cm | pours over the dam and the bank tops (9 crest columns) |
| Dam below banks | 2.3 cm | |
| Flood over a floodplain | 5.7 cm away from the outlet | 17 cm counting the last 20 columns, where the simulation's sink draws the water down |
| Spill on terraces | 12 cm | the simulation's spring holds its pond full; the steady pond drains |
| Chain of basins | 30 cm, lakes 0.4–0.7 m high | open: chained lakes still count more inflow than the spring gives |

A solve costs 2–15 ms for the whole 4,608 columns in JS (the simulation needs 0.4–1.1 s to settle
these scenes), and nothing at rest. A pit dug beside a river stays dry; joined to the river by a
ditch, it fills to the ditch's level.

### 5.3 The diversion test (2026-11-05)

Three scenes: a deep river (8.5 m³/s, ~0.95 m deep, banks 1½ m) with a trench dug off its side to
its own outlet, the trench's lip ½ m below the river's bed, level with it, or ½ m above it (above the
bed, below the water). The share of the river the trench takes:

| Trench lip | Grid simulation | Steady solve | Steady + re-routing by the surface |
|---|---|---|---|
| ½ m below the bed | 43.6 % | 66.7 % | swings 0 % ↔ 75 %, never settles |
| Level with the bed | 38.7 % | 0 % | 0 %, then (flat bodies) swings 0 % ↔ 87 % |
| ½ m above the bed | 13.5 % | 0 % | 0 %, then (flat bodies) loses water |

- **The steady solve can't divide a flow.** It routes by the ground, so a trench level with the bed
  or above it gets nothing, and one below it gets too much. How a flow divides depends on the water
  levels in both branches at once, which a top-down pass can't know.
- **Re-routing it by the water surface failed.** Rounds of routing by the last solve's surface, then
  damped and with flat water treated as one body, never settled and, in the last form, lost water.
  It is off in the lab (`ROUTE_ITERS = 0`), kept for the record. More rules here would repeat the
  pool engine's history (§2A).
- **The grid divides it quickly, and the same way from any start:** within 20–110 s of water time
  (a few hundred steps over the whole area) from dry or from the steady answer, to the same share.

So the steady solve is for water that **doesn't divide**: rivers, lakes, dams, spills, floods over
a plain. Where flow divides (a diversion, a channel round an island, a braided river), the grid
solves the region, and its settled answer (the share through each way out) is what the world keeps.
That is the plan's "the grid where water changes" with one addition: a division the grid has
solved is stored, not re-derived, until an edit or a change of flow upstream wakes the region again.
Not tested yet: buckets (stored volume per basin), and the handover from grid to steady and back.

### 5.4 The water graph: pools and falls (2026-11-05)

The owner's model: water is a graph of **pools** (nodes holding a volume) joined by **flowing water**
(edges); flowing water is water over air or over water, so a flow is a fall from a pool's lip to the
next pool, and water at a pool's level is that pool. The lab's **Water graph** solver:

- **Pools:** the ground cut into flats, connected columns with one fill level (priority-flood): a
  lake with its flat rim, a river's stretch between two ½ m steps, a bank. Each holds a volume; its
  level follows from the volume and its columns' ground.
- **Edges:** one per pair of neighbouring pools, a crest at each face between them (the higher
  ground). Water crosses by the weir law on the levels each side: a **fall** when the lower pool is
  below the crest, a **drowned crest** (Villemonte's reduction) when both stand over it, so two pools
  merge as their levels meet, and split as they fall. Each transfer is limited so it never more than
  evens the two levels, and no pool gives more than it holds.
- **Only volumes are stepped**; every transfer takes what it gives, so water is conserved exactly
  (balance within 10⁻⁵ m³ over 8,000 steps in every scene). Toggles draw pools and falls separately.

Measured against the grid simulation, settled:

| Scene | Pools | Mean depth error | Notes |
|---|---|---|---|
| River | 102 | 10 cm | the same 285 columns wet; 126 vs 97 m³ (pools are flat: no friction slope) |
| Dam level with banks | 99 | 10 cm | 442 extra columns wet: 4 cm films over the bank tops |
| Dam below banks | 102 | 10 cm | |
| Spill on terraces | 161 | 9 cm | 831 extra columns, films 4–12 cm |
| Flood over a floodplain | 90 | 11 cm | |
| Chain of basins | 102 | 18 cm | 1,856 extra columns, films ~7 cm |

**The diversion test passes**, where the steady solve failed: the trench takes 39.4 %, 38.5 % and
20.0 % (lip below, level with, above the bed) against the grid's 43.6 %, 38.7 % and 13.5 %. The flow
divides at the river's pool, by its level over each lip.

- **Cost:** 0.01–0.07 ms a step for all the pools (90–160 of them for 4,608 columns), against the
  grid's ~0.06 ms in JS for the same area — a pool a flat, not a column.
- **Timing:** the river and dams reach 90 % of their water in 78–82 s against the grid's 58–76 s;
  the basins in 136 s against 688 s (water crosses a flat at once).
- **Weak spots:** a pool is level across its whole flat, so (1) a river stretch has no friction
  slope (10 cm too deep, more in a deep fast river), and (2) water crossing a wide flat covers all of
  it as a thin film instead of running across it in a strip. Cutting flats into 4 m or 2 m tiles made
  both much worse (a weir between tiles of one flat is the wrong law for a channel: rivers 29–41 cm
  too deep, diversion splits off by up to 38 points); it is off.

So the graph gets right what matters most for play and was hardest (where water goes: dams, spills,
diversions, merging and splitting pools, exact volumes, buckets by construction), and gets wrong how
water looks inside a pool (its slope, how far a thin flow spreads). Those are drawing questions
inside a pool, which the steady solve's profile (§5.2) already answers for a pool with water passing
through: a candidate split is **the graph for the state, the steady profile for the drawn surface**.

### 5.5 The graph with steady shapes (2026-11-05)

The graph holds the water; the steady solve gives each pool its shape: which of its columns are wet,
and their depths relative to each other. A pool's volume sets one offset for its whole shape, so the
water drawn is exactly its volume; edges read the surface at the faces they cross. Where a stream
pours over a brink the steady surface is critical depth, so those faces pass q = √g·h^1.5; a pool
holding lake columns keeps the weir's 1.7·h^1.5 (its surface is the lake's head over the crest). A
pool the steady solve leaves dry stays flat. On by default in the lab (the "shape pools" checkbox).

| Scene | Graph alone | Graph + steady shapes |
|---|---|---|
| River | 10 cm | **0.4 cm**, 98 vs 97 m³ |
| Dam level with banks | 10.4 cm, 442 extra wet columns | **2.8 cm, none extra** (66 the simulation wets aren't) |
| Dam below banks | 10.1 cm | **2.3 cm** |
| Flood over a floodplain | 10.9 cm, 338 extra | **6.8 cm**, 144 extra |
| Spill on terraces | 8.7 cm, 831 extra | 8.9 cm, 115 extra, 293 missing |
| Chain of basins | 17.5 cm, 1,856 extra | 14.8 cm, 561 extra, 350 missing |

Water still balances exactly (terraces' −54 m³ in both is its pond's starting water, never counted
as brought). The diversions are mixed:

| Trench lip | Grid | Graph alone | Graph + shapes |
|---|---|---|---|
| ½ m below the bed | 43.6 % | 39.4 % | **54.0 %** (worse) |
| Level with the bed | 38.7 % | 38.5 % | 36.0 % |
| ½ m above the bed | 13.5 % | 20.0 % | 16.2 % |

The deep trench gets worse because the shapes come from the steady solve's own routing, which sends
67 % down that trench (§5.3): the shape inherits the wrong split and leans the graph toward it. The
fix to try next: build the shapes from the graph's own settled flows (each column's discharge from
the graph's face flows, then the steady solve's level step), not from the steady routing.

### 5.6 The grid's face law on the graph (2026-11-05)

What makes the grid accurate is its face law: each face keeps a discharge, pushed by the surface's
slope across it, slowed by Manning friction, through the water over the face's higher bed, capped at
critical flow. The graph's edges now use the same law (each face between two pools keeps its own
discharge; the slope is the two pools' levels over the distance between their centres), and pools
can be flats cut into tiles. With every column its own pool the graph **is** the grid: it reproduces
it to 0.01 cm.

The cap over a crest matters: the weir's 1.7·h^1.5 for pools bigger than a column (on the idea that
a big pool's level is its head upstream of the brink) held river pools 10–11 cm deep at every pool
size; the grid's critical-flow cap (√g·h^1.5) for every pool fixed it. With it:

| Pools | Mean depth error (river, dams, terraces, flood) | Diversion (below / level / above the bed) |
|---|---|---|
| Whole flats (90–160) | 4.7–19 cm | 40.0 / 40.0 / 12.2 % |
| Flats cut 4 m (~970) | **1.0–2.0 cm**, volumes within 1–3 % | **40.0 / 38.7 / 13.2 %** |
| Flats cut 2 m (~2,000) | 0.5–0.9 cm | 40.0 / 38.3 / 13.2 % |
| The grid (4,608 columns) | (reference) | 43.6 / 38.7 / 13.5 % |

Cost, in JS, ms of compute per 100 s of water, both at 1 s steps (each subdivided to its stable step;
the graph's stable step uses the distance between pool centres):

| Scene | Grid | Graph, 4 m pools | Graph, whole flats |
|---|---|---|---|
| River | 15.4 | 27.4 (1.2 cm off) | 9.9 (4.9 cm off) |
| Flood | 90.8 (7.4 cm off its own 0.1 s run) | 44.4 (2.1 cm off) | 14.2 (19 cm off) |
| Diversion, level | 29.4 | 33.8 (1.7 cm off) | 8.3 (10 cm off) |

So the face law makes the graph and the grid **one model at different resolutions**, and at 4 m
pools it is as accurate as the grid, diversions included, with exact conservation. It is not cheaper
per step in this lab: half the faces, but more work per face in JS. Savings would have to come from
resolution chosen by place (one pool for a lake or a still flat, small pools only where water runs:
whole flats cost 2–6× less), from pools that stop changing being skipped, and from a native
implementation on flat arrays; none of that is measured yet.

### 5.7 Sleeping chunks: the grid steps only what is changing (2026-11-05)

The owner's idea: cache what isn't changing, and update it only in cascades from an edit upstream or
downstream. A steadily flowing river is not changing (its depths and face discharges hold still while
its water moves), so it can sleep like a lake. The lab's grid ("Simulate", checkbox on by default):

- **Chunks** of 8 × 8 columns (72 in the lab). One sleeps after 20 substeps in which no face's
  discharge changed by more than 10⁻⁴ m²/s and no column's level moved faster than 10⁻⁵ m/s (so a
  lake still filling, more in than out, stays awake). Awake chunks are outlined in orange.
- **Asleep:** its faces keep their discharges (the face law is skipped); every column still adds up
  what flows in and out, so water is conserved exactly.
- **Waking:** an edit wakes its chunk and the eight around it; a sleeping chunk wakes when its own
  water starts to rise or fall, or when an awake neighbour computes a discharge across their shared
  faces that differs from the frozen one, so a change cascades upstream and downstream and stops
  where it no longer changes anything.

Measured (8,000 steps settling, against the same grid always awake):

| Scene | Awake at the end | Depth difference | Water |
|---|---|---|---|
| River | 4 / 72 | 0.08 cm (max 0.5) | equal |
| Dam below banks | 6 / 72 | 0.10 cm | equal |
| Diversion, level | 22 / 72 | 0.03 cm | equal |
| Spill on terraces | 29 / 72 | 0.00 cm | equal |
| Flood over a floodplain | 41 / 72 | 0.01 cm | equal |

**The cascade works.** On the settled river (4 awake), a dam built mid-valley woke ~20 chunks (24 at
the peak), which went back to sleep as the change settled (16 at 125 s, 8 at 200 s, 6 at 350 s); the
result is within 0.08 cm (max 0.8) of the always-awake grid. Springs' and outlets' chunks always stay
awake (their columns are reset every step).

**Where it can't sleep:** deep, fast water. In the diversion, terraces and flood, the chunks from mid
valley to the outlet never settle: their discharges keep swinging by a few per cent (up to 0.02 m²/s)
— not the lab's outlets (a free outfall changed nothing), but the local-inertial scheme's known
oscillation at low friction and high speed, and in the flood real waves over the stepped floodplain
(levels moving 1–4 cm a second). de Almeida et al.'s θ-weighting (θ = 0.9) lets the diversion settle
(22 → 5 awake, 0.45 cm from the plain scheme) and the terraces (29 → 5), but moves the terraces'
answer 3.75 cm and 12 % of its water, and makes the flood worse (41 → 48). θ is left at 1.

**Cost:** in the lab the saving is modest (settled river 30 vs 45 ms per 1,000 steps; the dam's
cascade 161 vs 182 ms), because the bookkeeping loops still visit every column; a native version
would skip sleeping chunks entirely. The open question for the worst case (every river near a
player) is the big fast rivers that never settle.

**Skipping sleeping chunks entirely (same day).** The bookkeeping that still visited every column (the
stability scan, the outflow limiter and its scaling, the water balance, the buffer copies) now runs
for awake chunks only. A sleeping chunk keeps its **imbalance rate** (its columns' net flow when it
fell asleep, updated whenever an awake neighbour changes a face they share) and the **volume owed**
that rate adds up to; it costs one addition a step, wakes if the rate would move its level faster
than the sleep threshold, and on waking spreads the owed volume back over its columns. Measured (ms
per 1,000 steps, best of three):

| Scene, settled | Always awake | Sleeping, visiting every column | **Sleeping, skipped** |
|---|---|---|---|
| River (4 / 72 awake) | 36.7 | 31.0 | **8.4** (4.4× less) |
| Dam below banks (6 / 72) | 45.9 | 30.7 | **9.0** (5.1× less) |
| Diversion, level (22 / 72) | 52.7 | 58.2 | **37.8** (1.4× less) |

Skipped chunks hold perfectly still, so the result now matches the always-awake grid to under
0.01 mm settled and 0.02 cm after the dam (better than visiting them: 0.08 cm). The ledger (in − out
− stored − owed) closes to ~0.001 m³ in every mode, always-awake included: float rounding on ~1,500
m³ of throughput, not a leak.

**A slosh the edit leaves behind, and θ.** Building the dam into a settled river (4/72 chunks awake,
the springs' and the outlet's) woke 18 chunks that stayed awake past 1,500 s, though the physics was
done in under a minute: the lake rose 92 mm/s in the first 5 s, the discharge past the dam and at the
outlet was back to 1.50 m³/s by 40 s, and the lake was within 1 mm of its final level by 90 s. What
kept them awake was a cross-channel slosh the edit starts: the channel's side columns rock against
each other, ±0.3 mm with a 3.3 s period, from the lake to the outlet, and at θ = 1 it never fades.
Its levels move up to 0.6 mm/s, 60× the 0.01 mm/s sleep test. (The dam scene loaded from scratch
doesn't start it, which is why it settles to 6/72.)

Two fixes were tried:

- **A looser sleep test** (levels within ±1 mm and discharges within 0.005 m²/s over a 10 s window,
  sleeping with each face at its window average): worse. A chunk frozen beside a sloshing neighbour
  is a wall to it; the slosh reflects, the neighbour's shared faces change, and the frozen chunk
  wakes (616 wakes in 600 s, the settled river never settled, levels up to 15 mm off). Removed.
- **Damping the slosh, θ = 0.95** (de Almeida's weighting, §5.7's earlier θ = 0.9 trial): works.
  The slosh dies; the river settles in 130 s (θ = 1: 530 s); after the dam it is back to 4 awake
  chunks in 85 s (θ = 1: never), and the 600 s after the dam cost 12.9 ms per 1,000 steps (θ = 1:
  39.9; the full grid 38.0). Skipping matches always-awake at θ = 0.95 to 0.04 mm; the ledger closes
  to under 0.01 m³. Every scene now settles to 4–6 awake chunks (the flood 46 → 6, terraces 12 → 5,
  basins 22 → 5, the diversions 13 → 5), 9–25 ms per 1,000 steps against 37–122 for the full grid.

The cost of θ = 0.95: on the river and the low dam it settles within 0.01 mm of θ = 1, but where the
flow is steep and non-uniform it settles elsewhere, compared on time-averaged levels (θ = 1 swings
only 1–5 mm there, so the difference isn't its oscillation): terraces up to 69 mm (1,030 of 1,135 wet
columns over 5 mm), the flood 240 mm at its spring (133 of 2,553 columns), the diversions 5–23 mm,
the level dam 15 mm. Which is closer to real water is not measured. Basins' chained lakes still swing
31 mm at θ = 0.95, and skipping is 49 mm off always-awake there. The lab has a θ selector (default 1).

**Two places that won't sleep, with or without θ.** (The lab's `lab.whyAwake()` and `lab.wakeLog()`
show, for each awake chunk, its worst face and fastest-moving column, and why sleeping chunks woke.)

- **A dead-end hollow (basins).** A sheet 1–8 mm deep runs down the slope into it at a steady 4.9 L/s;
  nothing leaves, so its level rises 0.11 mm/s, unchanging, for as long as it was run (4,000 s,
  +44 cm). The sleep test forbids a level that moves, so a lake filling slowly never sleeps.
- **The floodplain while it fills or drains.** It changes for ~15 minutes; chunks fall asleep in a lull,
  and the next front wakes them in a wave across the flat (wakes 0.1 s apart, chunk by chunk): at
  θ = 0.95 the awake count goes 48 → 14 → 48 filling and 48 → 19 → 46 draining before it settles (~850
  s and ~600 s); at θ = 1 it never fully settles. A flat sheet is one body of water: a change anywhere
  reaches all of it, so chunks are the wrong unit there.

Tried and removed: letting a chunk sleep while its wet columns rise or fall together (held 0.4 s,
then 10 s), and giving a sleeping chunk its owed water in 0.1 mm drips instead of all at once on
waking. Both made the floodplain worse (518–682 wakes against 228, never settling, costlier than the
full grid): a plain filling looks steady for seconds while its inflow keeps changing, so chunks slept
and were woken again. What would fit both: treat a still, flat body (a lake, a filled hollow, a
ponded plain) as one pool, its level set from its volume, as the water graph does (§5.4), while the
grid steps only the water that flows; a pool filling from a trickle then costs one number.

### 5.8 Still water as pools in the grid (2026-11-05)

A new pattern beside sleeping chunks (the lab's "still water as pools", on by default, drawn
green-teal): a flat, slow body of water becomes a pool, as in the water graph (§5.4), and its
columns stop being stepped. A pool keeps a volume; its level comes from that volume over its
columns' grounds (sorted, with prefix sums: a binary search a substep). The faces between a pool and
the grid's water (or another pool) are the grid's face law against the pool's level, every substep,
whether their chunks sleep or not, with the grid's outflow limit; its own faces carry nothing.
A check every second (over awake chunks only) forms a pool where ≥ 16 connected columns ≥ 2 cm deep
move slower than 3 cm/s with levels within 2 mm, and grows a pool by neighbours matching its level;
an edit within a column of it, or its level moving faster than 1 cm/s, gives it back to the grid.
Water stays exact (each face flux goes to both sides; the ledger closes to ~0.003 m³).

Measured (θ = 0.95 unless said, ms per 1,000 steps settled):

| Scene | Full grid | Sleeping chunks | + pools |
|---|---|---|---|
| Basins (the dead-end hollow) | 107 (72 awake) | 19.3 (5) | **13.7 (3)**: the hollow is one 44-column pool, its chunks asleep while it fills |
| Basins, θ = 1 | 104 | 69.8 (38) | **47.9 (37)** |
| River, dams, terraces, diversions | 37–79 | 8.7–15.5 | the same, levels identical: no water there is still |
| Flood (filling and draining) | 122 | as §5.7 | the same: no pool forms |

- **The dead end is fixed.** The hollow pools and sleeps; its level tracks the sleeping-chunk run to
  0.6 mm. Both fill ~5% slower than the full grid (29 mm behind after 3,000 s): the trickle feeding it
  runs through sleeping chunks, an error of chunk sleep, not of the pool. Digging beside it dissolved
  the pool, and it formed again once the water was still.
- **The floodplain is not a pond.** Its water is a sheet running over the stepped plain to the outlet
  (faster than 3 cm/s, not level), so no pool forms there; what keeps it awake while it fills and
  drains is the flow itself changing. A pool there would be wrong physics (one flat level over
  flowing water).
- **Lakes a river runs through** don't pool either (the basins' chain carries 3.3 m³/s through them);
  only still water does.
- **Skip mode's own cost.** With most chunks awake, stepping only awake chunks costs ~1.7–2.7× the plain
  grid per column in this JS (the flood at θ = 1: 216 against 124 ms, the same 1,000 substeps): the
  per-face chunk lookups and branches. Native code would lay chunks out to avoid it; the lab doesn't.

**Pools for lakes a river runs through** (2026-11-05): loosening the still test (speed and level
band) does pool them, but buys nothing and costs accuracy. Settled, their chunks already sleep, so
pools only add upkeep (basins 13.5 → 18.9–26.2 ms per 1,000 steps); while basins fills from empty,
3 cm/s and 2 mm (the default) gives the sleeping-chunk result, 30 cm/s and 1 cm is 14% cheaper but 29 mm
off on ~130 columns, 1 m/s and 2 cm is dearer and 186 mm off on ~900. The lake behind a new dam
never pooled even at 1 m/s (it's a fast reach). The default stays 3 cm/s and 2 mm (`lab.setPoolParams`).

**Trend sleep** (tried and removed): a sleeping chunk kept each face's discharge and its rate of
change, measured over a 6 s calm window that had to be straight enough to hold 30 s, and its columns'
levels followed those lines in closed form (plus the real flux from awake neighbours), so neighbours
saw it rise; it woke when a real edge face left its line, after 30 s, or on drying. It failed on
every tuning: the river woke 220–228 times (the old sleep: 22), the floodplain never went below 36
awake chunks (the old: 6), at 3–4× the old cost. A woken chunk's predicted state is never quite the
equations' own, and in this scheme the difference is a wave that wakes the next chunk.

### 5.9 The grid at world scale, natively (2026-11-05)

The lab's grid with sleeping chunks (skipped entirely), written in Rust for speed and run on the world
generator's real river map (`dwell_worldgen::worldmap`: each river's course, width, depth, surface and
discharge), one face, seed 42, on an Apple M1 (8 cores, 16 GB). The code is a scratch crate, not in
the repo. Sparse 16 × 16 chunks (only where river, bank or lake is), SoA `f32` fields, rayon over the
awake chunks and the sleeping edges they touch, a sleeping chunk's imbalance settled only when it
changes, each substep sized from the deepest active water, springs counted in the balance so a steady
one sleeps. Beds are the map's (surface − depth) on ½ m steps, banks ½ m over the surface; inflow at
heads and as the map's discharge grows; rivers end at the sea in outlets.

**The world.** A face has ~33 rivers, ~360 km of them, mostly 6–8 m wide (up to 26 m), up to 28 m³/s:
2.3–2.9 M wet 1 m columns of river, and 0–2.7 M of lake (the map's 100 m lake cells). Allocated in
chunks: 11 M columns (rivers) or 14 M (with lakes), 312–393 MB.

**Cost, everything awake.** 31 ms a substep for the whole face with lakes (2.2 ns a column on 8 cores,
8.9 ns on one); at the 8–14 substeps a water second the depths need, **2–4× real time for one face**,
so about real time for both on this laptop. A branch-free, vectorised face law (fast cube root) was
slower (38–50 ms): rivers fill a fraction of each chunk row, and skipping each dry face beats
computing whole rows.

**It doesn't settle, so sleep doesn't help yet.** Run from three starts (the map's levels; every pool
filled to its lip + 0.15 m; Manning normal depth for the map's flow), 35,000 of the 43,600 chunks stay
awake. With the lab's settings (θ = 0.95, n = 0.03) the real rivers surge: over two minutes the median
wet column swings 61 mm, one in ten 0.8 m, one in a hundred 2 m (columns going dry to 8 m deep and
back), worse with slope (> 3%: 0.67 m mean) and width (> 15 m: 0.57 m), mid-course, not at the
outlets. A wide river crossing the grid diagonally on a flat bed pumps between 200 and 700 m³ every
14 s. Ported correctly (the lab's River scene in the native code settles to 1 awake chunk, 1.500 m³/s
out of 1.5 in, the same levels with and without sleep), so it's the geometry the lab never had:
steep, wide, diagonal rivers on ½ m steps, flow the local-inertial scheme (no advection term) handles
badly. Shorter substeps don't help; damping does, partly: θ = 0.7 with n = 0.05 cuts the swings by
~7× (median 12 mm, p90 75 mm, p99 0.3 m; slopes > 3% still 0.11 m), but after 1,200 s only 53 of
35,435 wet chunks pass the sleep test (2,571 would at ten times looser).

**Lakes** from the map sit above their outlet channels (cut at the river's depth below the surface)
and drain for most of a water day: a reason for pools (§5.8), or for lake levels and outlets made
consistent at generation.

**Corrected the same day: it isn't steepness.** Clean synthetic rivers in the same native code (straight,
on ½ m steps, banks ½ m over the surface, 1,800 s) all settle when they run along the grid, at 0.4–4%,
3–25 m wide, 1.5–25 m³/s: 2 awake chunks (the spring's and the outlet's), 0.0 mm of swing. Run
diagonally, they don't, for two reasons:

- **The evaluation (fixed).** Diagonal flow zigzags between x and z faces, and neighbouring columns
  trade micrometres every substep. The level test, the fastest change in any one substep, tripped on
  that (2–3.5 × 10⁻⁵ m/s against 10⁻⁵) with the levels standing still. Testing how far each level moved
  over the calm window instead lets them sleep: the 1%, 8 m diagonal river 46 → 7 awake chunks.
- **The scheme on diagonal flow (real).** Each face takes its friction from its own discharge only,
  so diagonal flow, split between faces, is under-damped. Friction from the whole speed (the four
  perpendicular faces averaged) helps (the 1%, 8 m diagonal: 1 awake chunk; 0.4%, 25 m: swings 14–44
  → 1–12 mm), but the steep, wide diagonal (4%, 25 m, 25 m³/s) still surges 0.4–2.3 m where the same
  river along x is still. Capping the whole speed at critical made it worse.

With both fixes the world still swings as much (median 52 mm, p99 2.1 m; flat reaches as much as
steep ones), so the converted map adds something the clean rivers don't have: bends, junctions, or
the banks between segments (a segment's bank is kept where the next, higher segment's channel would
run). Not isolated yet.

**On the game's own terrain** (`TerrainGenerator::column_at` with the terrain volume, face A, seed 42:
every chunk near a river or lake, the game's ground and water levels; sea columns as outlets; the
window test and whole-speed friction on). 105,575 chunks (27 M columns, 757 MB), 5.7 M wet (2.5 M
river, 3.1 M lake, 0.5 M sea), generated in 12 s; 1.2–1.3× real time with ~40,000 chunks awake, 13–14
substeps a water second. The game's starting water drained like a store over a sill (115,000 m³ too
much out in the first 150 s, halving more slowly each time) and balanced its 52.5 m³/s by ~1,500 s.
Far calmer than the converted map: over two minutes the median wet column swings 8 mm (converted:
52–61), p90 38 mm, p99 0.18 m, a few spots 2–3 m. But still 39,700 chunks awake (34,500 of 36,075 river
chunks; 4,800 of 12,690 lake): the rivers keep moving by millimetres to centimetres, more than the
sleep test allows (0.01 mm/s over the window).

**What keeps them moving, and a band.** Six river columns followed for 60 s on the game's terrain
(ranked by their swing): the median moves 0.3 cm, p75–p90 0.3–0.4 cm (periods 15–17 s), p99 0.6 cm, p99.9
3 cm, the worst in the world 4.8 cm (a 4.3 m²/s run between the rocks of a lip, ~9 s eddies).
Nothing like the 10 cm that reads as noise on ½ m blocks (owner): what kept them awake was the test
(0.01 mm/s, a thousand times finer than visible). So a band: a chunk sleeps once no level in it moved
more than the band over a calm window of ≥ 10 s (and no face jumped more than a limit a substep);
asleep it wakes when what it owes would move its wet columns half the band, or a shared face
jumps; woken, the owed water goes evenly over its wet columns; it sleeps with its faces at their ~5 s
averages, not a wobble's instant. Short runs (60 water s from the settled state, against the same 60 s
with everything stepped):

| Band, face limit | Speed | Awake (of 105,575) | Level difference p99 / p99.9 / max | Over 10 cm |
|---|---|---|---|---|
| none (all stepped) | 1.0× | 105,575 | — | — |
| 1 cm, 0.001 m²/s | 1.4× | ~28,000 | 0.33 / 0.6 / 21 cm | 5–7 columns |
| 1 cm, 0.005 | 1.6× | ~22,000 | 0.6 / 0.9 / 21 cm | 2 |
| 5 cm, 0.02–0.05 | 2.2–2.4× | ~10,000 | 2.2–2.6 / 3.4–4.0 / 59–72 cm | 65–72 |

(A first try that limited owed water per chunk rather than per wet column ran 3.2–4.7× but left
55–70 cm errors in small pools: a whole chunk's allowance poured into a few columns. Fixed.) What
holds the speed down is churn, ~40,000–60,000 wakes a minute, mostly shared faces jumping: each wake
is a jolt (owed water in at once, faces restarting) that wakes its neighbours. The errors over 10 cm
are mostly shallow edge columns, wetting or drying.

**Smooth waking.** A woken chunk takes the water it owes over 5 s, a little each substep, not at once;
a sleeping chunk wakes when the flow its awake neighbours trade with it has drifted enough to move
its wet columns half the band in 10 s (or what it owes would), or a shared face jumps 0.1 m²/s in a
substep (an edit, a flood front), no longer on a face's wobble. Same short runs:

| Band | Speed | Awake | Level difference p99 / p99.9 / max | Over 10 cm |
|---|---|---|---|---|
| 1 cm, smooth | 1.9× | 18,000–25,000 | 0.9 / 1.3 / 9.8 cm | 0 |
| 5 cm, smooth | 2.8× | 6,600–11,000 | 1.8 / 2.8 / 19 cm | 2 |

The errors over 10 cm are gone at 1 cm and nearly so at 5 cm. Wakes are now mostly the neighbours'
drift (54,000 a minute at 1 cm, 24,000 at 5 cm); averaging that drift over 10 s only moved them to
the owed water (same speed, 5 cm less accurate), so the instant drift stays.

**Which chunks keep waking.** With the 5 cm band and smooth waking, 60 s after 20 s to settle in,
against the same 60 s with everything stepped (each chunk's real swing): of 49,465 wet chunks, the
top 10% make 98% of the wakes, and 2,271 are awake more than half the time. 189 of those really swing
more than the band (116 lake, 49 river): they should be awake. 2,082 don't (real swing 3.3 cm on
average): 1,827 lake chunks, 239 chunks holding sea outlets (awake by design: an outlet empties its
columns every substep, so it never reads as steady; it could count as a steady outflow, as springs
now count as a steady inflow), ~250 river. The lake ones are big lakes, every column wet, water at
2–3 cm/s, the whole lake swaying 3–5 cm: a seiche left from the start that deep water's friction
barely damps. On the band's edge, they sleep, are tripped by the sway or a neighbour waking, and each
waking costs at least the 10 s calm window. A still lake is what pools (§5.8) are for: one level, no
sway, no chunks.

**Outlets held, lakes as pools, on a window of the real world.** Tests now run on a window cut from
the game's terrain (8 × 4.3 km around the big lake, seed 42, face A: 10,756 chunks, 1.9 M wet columns,
1.8 M of them lake): inside, the game's own ground and water; rivers entering get their map
discharge at the edge, rivers leaving are held at the generator's level at the edge; settled in 2
minutes (900 water s at 7× real time), each test 10–30 s. Two changes. Outlets hold the sea's level
(what runs in leaves the world) instead of emptying, so an outlet chunk reads as steady and sleeps,
its outflow kept as one number while it does. Lakes become pools (the lab's §5.8, natively): still
water (≥ 64 columns slower than 5 cm/s, level within 5 cm) is one level from its volume; a chunk
wholly inside pools is never stepped; the chunks along a pool's shore stay awake and compute the
faces between pool and grid; the faces inside carry nothing, so the lake's sway is gone. 180 water s
from the settled window, 5 cm band, smooth waking, against everything stepped:

| | Speed | Awake (of 10,756) | Wakes | p99.9 / max difference | Over 10 cm |
|---|---|---|---|---|---|
| everything stepped | 5.8× | 10,756 | — | — | — |
| band, no pools | 10.0× | ~4,500 | 67,000 | 3.5 / 30 cm | 5 |
| band + pools | **22.1×** | ~1,750 | 12,000 | 2.7 / 12.6 cm | 3 |

The pools' upkeep first cost more than they saved (4.8×): copying the lake's level to every chunk
edge, re-scanning shore chunks to zero the faces inside, and re-scanning the whole lake every 2 s;
now only the members beside other water are kept in step, the faces inside are a list, and pools
grow from their edges every 10 s. What stays awake is the lakes' shores (~1,000 chunks) and the
rivers (~750).

**The last columns over 10 cm** were water at the waterline: a puddle perched on a bank 0.5 m over
the river, and a river-side pocket drained dry. Smooth waking gave a chunk's owed water to its wet
columns in equal depths, so a bank's film got as much as the river, wake after wake (and a small
pocket gave as much). Given in proportion to each column's depth instead (the river takes nearly
all of it): 180 s on the window, 21.8× real time, ~1,800 chunks awake, p99.9 2.7 cm, max 8.0 cm, none
over 10 cm. (A harness fault found on the way: the stepped reference, computed in the same process
with pools on, formed pools itself and passed them to the run after it; the reference now steps
everything with no pools, and each run starts with none.) None of this is in the game: it is the
scratch benchmark; the game's water is still the basin model (`dwell-sim` `water.rs`, docs/WATER_FLOW.md),
and `watergrid.rs` is the earlier grid prototype, not wired in.

**The whole face at the window's settings** (300 water s from the sea-held state, against the same
with everything stepped): 2.4× real time against 0.8× (3×, not the window's 3.8×), 653 pools (3.6 M
columns), but the awake count climbs through the run (14,200 → 22,500, most wakes the neighbours'
drift: 57,000), 330 columns over 10 cm (max 18 cm, p99.9 4.4 cm), and the pools' per-substep passes
(serial, over 201,000 faces) take ~19 s of the 123. The window, picked around a lake, flattered it;
the climbing count is not explained yet (the sea-held state had settled only 600 s).

What it means: the grid's cost at world scale is affordable even with everything awake (about real
time for both faces on a laptop). Rivers along the grid settle and sleep at any slope; diagonal ones
need the window test and whole-speed friction, and steep, wide diagonal ones still surge; the
converted world has a further fault not yet found. Until rivers settle, the sleeping that makes the
grid cheap doesn't engage.

## 6. Questions for the owner

1. **Prototype first?** A sandbox prototype of C (a few days), judged by you in the browser, before
   any decision.
2. **Caves and structures:** must water inside caves and on built structures (aqueducts, towers)
   work from the start, or can the first version be surface water only?
3. **Thin water:** in a continuous model, how thin may visible water be? Round every surface to ½ m
   blocks, or draw true depth with a minimum (e.g. a ¼ block)?

Sources: [Game Developer: Deep dive into Timberborn's water mechanics](https://www.gamedeveloper.com/design/deep-dive-timberborn-s-water-mechanics);
[Mei, Decaudin & Hu, Fast Hydraulic Erosion Simulation and Visualization on GPU (PG '07)](https://core.ac.uk/download/52631892.pdf), [citation](https://hgpu.org/?p=2256);
[Štava et al. 2008, Interactive Terrain Modeling Using Hydraulic Erosion](https://www.cs.purdue.edu/cgvlab/www/resources/papers/Stava-2008-Interactive_Terrain_Modeling_Using_Hydraulic_Erosion.pdf).
