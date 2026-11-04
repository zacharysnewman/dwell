# Water flow (basins, outlets, currents)

> **Status: adopted (the owner's answers, 2026-11-03; [ADR 0053](./adr/0053-water-flow.md),
> amended by [ADR 0055](./adr/0055-water-at-rest.md) and [ADR 0056](./adr/0056-one-opening-law.md)), Phase 52. W1 built (the server's basins, §15),
> redesigned in 0.42.2 after the review (§16): the world as found is at rest (§2.2), its flow the
> world map's. W2's saved basins and fast-forward built (0.43.4); live from load (pending the owner) and W3–W5 planned.** First water
> only; lava, lahar, jets, evaporation and weather later (§10).

The owner's request (2026-11-03): *"There's some systems I want to model, it's water flow, input and
output in any body of water in the game, but my focus is on rivers. So if you dammed a river, it
would fill, eventually flooding, real life dams let some water through, so that's like a variable
output in voxels … How can we model this in data (like per voxel) or other ways so we don't need to
update every voxel in real time."* Then: *"Draining lakes, diverting rivers, are part of this
model"*, and *"We'd also want physics applied to objects in the water based on flow direction."*

## 1. The owner's answers (2026-11-03)

| Question | Answer |
|---|---|
| What is added later? | More liquids: lava, lahar (mudflow). Evaporation, consistently at a low rate, that eventually fuels rain and snow. |
| Who adds it, and with what? | Data values: "these are new modeling concepts that are a core component". Generalize the systems where we can; the shape will be clearer as we develop. |
| A dam bursting | Draining only at first. Later, "pressure": water shooting out a distance by its head. |
| How far does it reach? | Simulated near players; the rest fast-forwarded. |
| In scope | Damming (filling, flooding over), draining lakes, diverting rivers, currents pushing floating objects. |
| Lava and lahar on contact | Later. |
| Time scale | A world setting, by default fairly fast, so changes can be seen; and scaled by the game's time, as the time of day is (§4.1). |
| The smallest basin | Whatever it takes for water to behave consistently: no minimum (§3). |
| Placed water | A small basin: it spreads and finds its level. A placeable water block is added to the hotbar. |
| Currents on swimmers | The current carries a swimmer at its speed, less what they can swim against it (§7). |
| First phase | Water only. |
| Changes downstream (2026-11-03) | A dam or a release must reach a player far downstream across unloaded country, at the river's speed; water filling or draining keeps flowing (its chunks loaded) until it settles, wherever the players are. |
| Do rivers run dry? | Not on their own while their feed is steady: runoff (§2.1). Later, glaciers' melt and weather vary it, so rivers rise and fall and small streams can run dry. |

## 2. The idea: simulate bodies of water, draw voxels from them

Water is not simulated per voxel. The simulation holds a few **basins**, each a connected body of
liquid with **one level**, joined by **flows**; the liquid cells are a picture of the levels. A
basin's level changes every tick by a single sum; its cells change only when the level crosses a ½ m
row, and then only on its rim. Cost follows what changes, not how much water there is.

- **A lake** is one basin.
- **A river** is a chain of small basins (pools) each spilling into the next over its step, the
  steps the ½ m rows the river falls by. Lakes and rivers are one thing, so a dug channel can take a
  river's water, and a dam turns a pool into a reservoir, with no special case.
- **A stream** is liquid running down over dry ground from an outlet to the next low spot: a path of
  cells, a shallow line of liquid whose depth comes from its flow.

The world map's rivers (WORLD_GENERATION.md §4: a flow direction per 100 m cell, a discharge up to
~21 m³/s) seed the chains and their initial levels.

### 2.1 Where the water comes from: runoff

The world map has no river sources: a river's discharge is the rain-weighted drainage area above it
(`dwell-worldgen` `hydrology/erosion.rs`), so a creek is small and grows downstream. The flow model
keeps that rule: every 100 m map cell adds its **runoff** (the map's rain there × a rate in data) to
the basin or stream its water drains into, without simulating the rain on the ground. A river is fed
along its whole length, its flow downstream what its channel was carved for. While the rate is
steady, rivers never run dry on their own; only dams and diversions change them. Later the rate
comes from glaciers' melt (a store of ice released by temperature) and weather (rain from the
evaporation budget), so rivers rise in the melt season and small streams can run dry (§10).

### 2.2 The world as found is at rest

*(ADR 0055, 0.42.2.)* The generated world's water is not laid down by the basins' rules: the terrain
places it by the map's river surfaces, ½ m terraces every few metres, with plants, banks and the
next step's water beside it. Measured by the basins' rules it is not in balance (§16), so a body
of water is **at rest as it is found**, and flows only as edits change it:

- **What holds it.** An open cell beside a found body that it didn't fill (a plant's cell, a notch
  in the bank) and a leaking block it stands against hold it like walls (`rest`), until an edit
  touches them.
- **Its natural outlets.** The ways out it was found with (an open cell onto air, a fall, or onto
  the next pool's water, a step) together pass **the world map's flow** through it (the river's
  discharge, a lake's outflow; §2.1) at the level it was found at. With none (the river goes on out
  of the loaded world, or into ground), that flow leaves unseen (a **seep**): its map flow and what
  found bodies' natural outlets bring it, no more (what an edit or a flood beside it sends it fills
  it). A natural outlet an
  edit blocks still counts (so a dam holds back the river), one inside the body after a merge
  doesn't.
- **The flow network.** A natural outlet found running into another found body **links** them.
  Each tick, upstream first, a found body passes the more of its map flow and what its links bring,
  and takes in its map flow less what they bring, so a river of fifty pools passes its flow once,
  and a dam above takes it from the pools below.

So a found river with a block in its bed is the same river (the block's ½ m³ raises it a hair), and
damming, draining and diverting change it as in §5.

## 3. What is stored

Per basin (the world's save, a new table; sent to clients). There is no minimum size (the owner:
whatever water needs to behave consistently): a placed water block is a basin of one cell, which
spreads over the floor it lands on and settles at its level, a thin film merging into whatever it
reaches.

- `id`, its liquid, its **level** (m, f64), its **volume** (m³).
- Its **cells** are not stored: they are the connected cells below the level, found by a flood fill
  from a seed cell when the basin forms or its shape changes (an edit at its rim).
- A **level-to-volume table** (the area at each ½ m row, so volume ↔ level is a lookup), made by
  the same fill.
- Its **outlets**: each a rim cell span where the liquid can leave, with its sill height and width,
  and where it goes (another basin, or a stream).
- Its **inflows**: runoff (§2.1), streams and other basins' outlets that end in it.

Per cell, nothing new is stored. The liquid cells stay the registry's liquid states (`dwell:water`),
rewritten at row crossings. A **current** per cell for drawing and physics (§7) is derived, not stored.

Per block (registry data, so a dam is made of ordinary blocks):

- `leak` (m³/s per m² of face per m of head, 0 = sealed): how much liquid passes through a block of
  it. Stone 0, gravel or planks a little, a gate a block whose open state leaks freely. This is the
  owner's "variable output in voxels".

Per liquid (a new data file, `shared/fluids/flow.json`, beside the looks in the registry):

- `density` (kg/m³, already a block's), `viscosity` (how fast it flows through an opening, how
  strongly its current drags), `evaporation` (m of level a day, by area), and, later, what it does
  on contact (§12).

## 4. A tick near players

For each live basin, in order downstream:

1. **In:** runoff (§2.1) + streams arriving + upstream outlets' flow (m³/s × dt), with `dt` in
   **game time** (§4.1).
2. **Out:** each opening's flow by **the opening law** (*0.43.2, ADR 0056*): water at depth `d`
   presses with `ρ g d`, so across a submerged opening between two levels the difference is
   `ρ g Δh` at every depth, and it passes `Q = C · area · √(2 g Δh)`; where the far side's water is
   below part of the opening that part pours freely, each strip by its own depth under the
   surface, which summed up an opening open above is the **weir law**, `weir · width · head^1.5`
   (`weir = C · ⅔ · √(2g)`, so `C` comes from the liquid's weir coefficient). One law for a sill,
   a hole in a wall, a gap and the faces two pools share; plus leaks through blocks below the level
   (`leak × area × head`), plus evaporation (`rate × surface area`).
3. **Volume** += in − out; **level** from the table.
4. If the level crossed a row: the rim cells of that row become liquid (rising) or air (falling).
   A rise that reaches a new outlet adds it. Two basins that touch exchange water through the
   faces they share by the same law; they **merge** when those faces would even them out at once
   (a wide gap: one body), and a basin narrowed by an edit to an opening that can't (a wall with a
   hole) **splits** at it ("fill, spill, merge", the hydrology standard; the world map already
   finds lakes by priority flood).
5. Outlet flow out over dry ground runs downhill cell by cell (the steepest drop) as a stream until
   a low spot, where a new basin forms and fills.

### 4.1 Time

The rates are real (m³/s, the weir law, runoff), counted in game time, not real time (owner,
2026-11-03: "also scaled by game time scale, what time of day uses"). The day/night clock's day is
`daynight.json`'s day length (20 minutes, owner 2026-11-03, so game time runs 72× real time;
DAY_NIGHT.md §2), and
water's tick is a real tick × `86,400 s ÷ the day length` × the world's **water speed** setting (a
multiplier, by default fairly fast so changes can be seen). Both are the world's settings (ADR
0054: sent at join, saved with the world), so a world keeps its own. A longer day slows the water
with the sun; a faster water setting speeds the water alone. Fast-forward (§6) counts the same game time.

Liquid is conserved: it comes in as runoff (and later melt and rain) and leaves to the sea, the world's edge
or evaporation, so a diverted river really takes the water from the old one.

## 4.2 The sea

The sea is not simulated: each face's sea is one **reservoir basin** at sea level, of unlimited
volume, whose level never moves. Its cells stay the static water blocks they are now.

- **A sink:** every river and outlet that reaches it ends there; what flows in is gone.
- **A source:** a channel cut from the sea into land below sea level floods it, by the weir law at
  the sea's level (a hollow behind the shore fills to sea level and stays). A dam across a river's
  mouth makes the river's last pool a basin of its own, filling and spilling into the sea.
- **No currents or tides** at first. Tides would be the reservoir's level following the clock, and
  currents a field of the sea's own; both later, as data.
- **The water cycle, later:** the sea is where most evaporation comes from (§10), so the rain and
  snow that refill the rivers come from it, and the cycle closes: water is conserved over the world,
  the sea the store that balances it.
- The two faces' seas are separate reservoirs; the disc's rim (BIFACIAL_WORLD.md) is a cliff between
  them, so nothing spills from one to the other.

## 5. Player edits

The only trigger besides time. An edit (break, place, region, explosion) touching a basin's rim, an
outlet or a stream path recomputes that basin's outlets and, if its shape changed below the level,
its fill. Nothing else is touched.

- **Damming:** blocks across a river's pool raise its outlet's sill; the pool fills at its inflow less
  the dam's leaks, and floods over the lowest rim when it reaches it (that spill a new outlet).
- **Draining:** a channel cut through a rim below the level is a new, lower outlet; the lake drains
  by the weir law (a deeper or wider cut drains faster), the shore drying a row at a time, down to
  the cut's floor; dips in the bed are left as ponds.
- **Diverting:** a trench from a pool to lower ground is a second outlet; the flow splits by what each
  can carry, and a deeper trench takes it all. The old bed's pools lose their inflow and drain to
  their own outlets. A gate block switches between two.

## 6. Far from players: fast-forwarded

A basin with no player near freezes. When it is next live (a player near, or its chunk loads), it
is brought forward by the time it was frozen: in a chain the inflows are steady, so each basin's
level at a later time is a small integration of in − out(level), cheap at large steps, upstream
first. A long-frozen chain comes back at its steady state: each basin at the level where out equals in.

**As built (0.43.4–0.43.5):** a basin is **active** while it fills or drains: its level moved
faster than `settleMmPerHour` (10 mm an hour of water time) last tick, it owes time, or an edit
just touched it. An active basin flows wherever the players are (owner, 2026-11-03: a dam must fill,
and the river below it change, whether or not anyone is near), and water asks the server to load and
keep its chunks and those beside it (`Water::wanted_chunks`: read or generated like the players',
never evicted) until it settles; a settled one out of reach (beyond `liveRadiusM`) waits, holding
its level and passing on what comes in, which is what it would do. A basin that waits with a chunk
unloaded counts the water time it owes; an active one passes nothing meanwhile and takes no inflow
(what other bodies send it is kept). When it is live again `Water::catch_up` brings it forward
alone: flow steps no longer than half the time its outflow takes to balance a change, than moves
its level a quarter row, or than its outflow takes to run what stands over its lowest way out; its
container found again as it fills or drains; once settled, it is caught up. A water tick spends at
most `catchUpSteps` (512) such steps, the rest the next tick. Time doesn't pass while the server is
off (the world's clock stops), so a restart owes nothing.

**Planned (W2d):** what a change sends down a river reaches the river far below through country
nobody has loaded: the difference from the river's flow travels the world map's river path at the
river's speed (owner, 2026-11-03), through lakes and confluences, to the bodies there.


## 7. Currents and objects in the water

Every liquid cell gets a **current**: the velocity of its liquid, derived from its basin's flows.

- In a **stream** or an outlet's run: along the stream's path, speed = flow ÷ the cross-section.
- In a **basin**: from each inflow to each outlet, weighted by their flows. A slow drift across a
  lake, a fast pull near a dam's gap or a drained lake's cut. Kept coarse: one vector per basin
  region near each opening plus the basin's mean, not solved per cell.

Objects take it through the water drag already built (0.41.6, BUILT_BODIES.md §6): the drag acts on
the body's velocity **relative to the current**, so a raft at rest is carried toward the current's
speed, faster in a narrow fast run than on a lake. **Swimmers** (owner, 2026-11-03) are carried at
the current's speed less what they swim against it: the swim layer's velocity is the input's wish
plus the current, so a swimmer at the swim speed (3 m/s) holds still in a 3 m/s current and goes
backward in a faster one, and a slow lake drift is barely felt. Items later. The clients get the currents with the basins, so the prediction carries a
raft or a swimmer as the server does.

## 8. On the wire and in the save

- **Save:** the basins table (id, liquid, level, volume, seed cell, outlets). The liquid cells stay
  in the chunks as now. **As built (0.43.4):** the world file's optional `water` table, one record:
  every basin but the reservoirs, whole (its container's rows rather than a seed cell, outlets, what
  holds it at rest, its steady flow, the time it owes), the flow network, water in transit, the ways
  drawn running, and the revision of each chunk a basin lies in, written in the autosave's
  transaction with the chunks. On load a basin whose chunks are at other revisions (a build on the
  line that doesn't save the water edited them) is left out with what pointed at it, and found
  again, at rest as it then is, by the next edit that touches it; a reservoir is found again when an
  edit meets it. Not a new compatibility line: an older build leaves the table, as today.
- **Network:** a basin's level and currents when they change (a few bytes); the clients rewrite
  their own rim cells at row crossings, as the server does. Chunk edits from the basins are not
  sent cell by cell.

## 9. Liquids as data, and what is general

One engine, a small fixed vocabulary: basins, outlets (weir), leaks, streams, runoff, sinks,
evaporation, currents. Each liquid is a data entry:

| Liquid | density | viscosity | evaporation | contact (later) |
|---|---|---|---|---|
| water | 1000 | low | low, always | — |
| lava | ~2600 | very high: slow streams, narrow reach | none | sets things alight; meeting water → stone (owner to decide) |
| lahar | ~2000 | high | none | deposits mud as it slows (owner to decide) |

What is not yet clear (the owner, "we may get a better idea as we develop") is left as data hooks:
a liquid's contact rules and deposition are later additions to the vocabulary.

## 10. Later

- **Pressure jets** (the owner's "shoot out a distance"): an opening below the level jets at
  `v = √(2 g · head)`; the jet's reach is a projectile from the opening, drawn as particles and
  landing as a stream where it falls. The outlet law already knows the head.
- **Evaporation → rain and snow:** evaporated volume goes into an atmosphere budget per region; rain
  and snow return it over land (as runoff), snow by the bands' snow line. Needs
  a weather system (none today: the clouds are only drawn).
- **Flood waves** (a dam breaking): a short-lived per-cell shallow-water solve near the break,
  handing back to the basins when it settles. Not planned.

## 11. Cost (*estimates*)

- A live basin a tick: a few dozen floating-point operations; hundreds of basins near players are
  far under a millisecond.
- A row crossing: the rim cells of that row (a 100 × 100 m lake's shore: ~400 cells) written once,
  and the chunks' remesh and relight, the same work as a player's edit of that size.
- A flood fill when a basin forms or changes shape: bounded by its cells; a large lake (10⁶ cells) is
  the expensive case, so fills are budgeted per tick as the felling's support search is.

Measure with the bench before relying on these.

## 12. Compatibility

A new compatibility line (pre-1.0, the next MINOR, which the owner raises): the saved world gains
the basins table, the protocol gains basin messages, and the generator seeds runoff and levels from
the world map (the terrain a seed generates is unchanged; its water's state is new).

## 13. Phases (IMPLEMENTATION_PLAN.md Phase 52)

- **W1 — Basins.** The liquid data (`shared/fluids/flow.json`), the blocks' `leak`, basins found from
  the liquid cells, their level ↔ volume, outlets by the weir law, leaks, row crossings at the rim,
  merge and split, streams over dry ground; the sea as a fixed reservoir; game time × the water speed setting (§4.1). Server only, tested
  natively (a dammed pool fills and spills, a cut drains a lake to its floor, a trench takes a river).
- **W2 — Runoff and rivers.** The world map's runoff into basins (§2.1), rivers as chains of pools
  at their map levels, the basins saved with the world, fast-forward far from players. A new
  compatibility line (0.42.0, the owner raises it): the saved world and the generator's water state.
- **W3 — Clients.** Basin levels and currents on the wire; clients rewrite their own rim cells.
  Protocol change (on the 0.42 line).
- **W4 — Currents.** Water drag relative to the current on bodies (the server and the prediction's
  stand-ins) and swimmers carried by it.
- **W5 — Placed water.** A water block in the hotbar (`shared/items/items.json`), placed as a small
  basin that spreads and finds its level.

## 14. Open questions

None blocking. Lava and lahar contact rules, jets, evaporation and weather are later (§10).

## 15. W1 as built

`dwell-sim` `water.rs` (`Water`, `Basin`, `Outlet`), data `shared/fluids/flow.json` → `flow.gen.rs`
(`shared/fluids/gen.mjs`, validated, CI on stale output). The server's `TickChanges` records each
changed cell's state before its first change; `broadcast_changes` hands them to water (not water's
own writes). Every `tickEvery` (6) ticks `Server::flow_water` runs a water tick of
`6 / 60 s × 86,400 / the world's day length × its water speed` (7.2 s of water time at a 20-minute day) and
sends its writes as one `Rebake` modification.

- **Found by edits.** An edit beside or on a liquid cell nobody has found finds its body: up the
  column to its surface, then every cell of that liquid at or below it whose column reaches the
  surface (a river's lower pool, past its step, is a body of its own). It is found at rest (§2.2):
  what holds it, its natural outlets passing the world map's flow (`Water::with_runoff`: the
  server's from `dwell_worldgen::runoff_for`, `TerrainGenerator::discharge_at`; none for the flat
  world), a seep with none. Liquid an edit put there is not at rest and brings nothing in. A body
  over `maxBasinCells` (65,536) is a reservoir at its surface's top.
- **The container**, row by row from its bottom: each row from the cells over the row below (and its
  own), spread sideways over open cells held up by a solid block or the row below; as many rows as
  its volume fills, never into another basin's container (met there: a contact); a reservoir's
  cells are walls (a basin beside the sea fills toward its level through the outlets between,
  drowned). A body's own liquid cells stay in its rows whatever is under them.
- **Openings** (the opening law, 0.43.2): an open cell beside a container cell with nothing holding
  it up (a face from that row's floor, open above, or capped by a block over it: a hole in a wall),
  an open cell under one (a hole in its floor, `C × 1 m² × √(2 g head)`), a face shared with
  another basin's container (a **contact**, both ways by the two levels; none where found bodies
  touched as found, `held`, a river's steps under their surfaces), and a block of `leaks` data
  beside it with an open cell past it (`leak × ½ m² × head`); no more in a tick than the basin holds
  over its lowest way out, and no more into another basin in a flow step than evens the two out.
- **One body or two** (0.43.2): two basins sharing faces within `MERGE_GAP` (2 cm) of each other
  merge when the faces would even them out within `mergeSeconds` (0.5 s, about the shortest flow
  step) and carry what flows through them (inflow and arrivals) within `MERGE_GAP`; a basin whose
  full rows (the surface row once half full) are joined only through a narrowing an edit made (the
  cells beside a block an edit placed don't join parts) is as many basins as fail that test, its
  liquid shared by its cells. A wide gap is one body; a hole in a wall, or a sheet over a crest, an
  opening between two (a river behind a wall with one cell open at its foot rises ~2 m before the
  hole carries its 1.8 m³/s). The generated ground never splits a body.
- **Where it goes** (`follow`, up to 1,024 routes a tick, in rounds, so the bodies a round finds have
  their ways found the same tick): down while open, then along the floor for an open cell with
  nothing under it within `streamReach` (16); onto liquid (that body, found if new), or it pools
  there as a new basin, or out of the loaded world (lost). What leaves by a way not found yet
  waits in transit (`pending`) and arrives when it is.
- **Flow steps.** A water tick (7.2 s of water time by default) is taken in up to 64 steps, short
  enough that no basin moves more than half way to its balance in one (its surface over how fast its
  outflow grows with its level).
- **Rows** are written when the level crosses half a row; a row the level falls below goes, and a
  container that falls apart splits, each part with the liquid over its own cells. An empty basin
  with nothing coming in goes.
- **Near players**: basins within `liveRadiusM` (160 m) of a player, all of whose chunks are loaded,
  tick; the rest wait: hold their level and pass on what comes in, shared as their ways out would
  share it (an unloaded cell is a wall to water, so a basin flowing on beside one would lose what
  stands there). A basin whose cells another's level wets or dries finds its ways out again. A
  basin keeps the unloaded chunks beside it (`edge`, walls to it then) and is found again when one
  loads (0.43.3): a dam's flood goes on up the river as the player reaches it.
- **The edge**: a block with a `flooded` state (a ramp) beside a row the water writes takes it, and
  loses it when no liquid is beside it (`flood_edge`).
- **Running water** (0.43.1): an outlet an edit made (not a natural one) passing at least
  `streamMinFlow` (0.05 m³/s a cell, a sheet ~7 cm deep) is drawn: its crest (the surface row's
  cells beside it, however thin the sheet) as the liquid, and the cells its water runs through to
  where it lands (`follow_path`: the fall, the run along the floor) as the liquid's `running` block
  (`flow.json`; `dwell:running_water`, the same fluid), taken away when it stops. The basins treat a
  running block as open: none is found in it, and a basin rising fills over it.
- **Crests** (0.43.1, by the opening law since 0.43.2): a wall level with a river's water splits the
  pool (the sheet over its top too thin to even the sides out), and the river above rises until it
  passes its flow over the wall.
- **What a found body passes** (0.43.1): each flow step, its own map flow and what arrived the step
  before (over a dam, from the pools above), by its natural outlets at the level it was found at;
  its links are found from where its natural outlets ran as found (an edit may block them now), so
  a pool below a wall never counts the river's flow as its own.
- **The day** of a new world is 20 minutes since this change (owner, 2026-11-03; DAY_NIGHT.md),
  so water time runs 72× real time × the water speed; both are the world's settings (ADR 0054), a
  new world's water speed the server's `water_speed` (`flow.json` `speed` by default).

**Playtest fix (0.42.1; owner, 2026-11-03, docs/BUGS.md 16).** Blocks set in a riverbed under a
stepped fall flooded it to the fall's top and then drained the river. Now: a pool's inflow comes in
at a cell (its farthest from its outlets; the map's flow by place since 0.42.2, `Steady::feeds`);
basins merge only when their levels meet (`MERGE_GAP` 2 cm), and only rising (a basin met below
another's level records it as a neighbour, merged when it reaches it); a receding basin
whose surface row is about to dry leaves the parts of that row on a floor of their own that water
comes into as basins of their own (`spin_off_shelves`), so a flood over a river's steps leaves each
step its pool.

**Redesign (0.42.2; owner, 2026-11-03, docs/BUGS.md 17–18).** After the review (§16): bodies are
found at rest and fed by the world map (§2.2), replacing the inflow measured off what left them
(and 0.42.1's `credit`, which counted again at whatever level a pool had); merges only at one level;
the flow steps; ways found in rounds, water in transit while they are; waiting basins pass on what
comes in, and wait while a chunk of them is unloaded; the edge's ramps flood.

Deviations, for W3: a natural outlet's water isn't drawn
running (the river's own steps stay as generated), and a sheet spreading wider than `maxRowCells` over a
plain is capped there (the excess lost).

Tests (`water/tests.rs`, both faces): `a_dam_out_of_reach_fills_on_until_it_settles`, `a_dammed_river_unloaded_comes_back_as_if_watched` (0.43.5; `tests/water.rs` `a_lake_cut_open_drains_on_after_its_player_leaves`),
`a_basin_saved_with_the_world_goes_on_as_it_was`, `a_saved_basin_whose_chunk_changed_since_is_found_again`
(0.43.4; `tests/persistence.rs` `the_waters_basins_survive_a_server_restart`, `dwell-storage`
`the_waters_basins_are_saved_with_the_world_and_replaced_on_each_save`);
`a_dams_flood_goes_on_up_the_river_when_its_chunks_load`
(0.43.3, red on 0.43.2); `a_hole_in_a_wall_passes_what_its_size_and_depth_drive` (the
rise within 5 % of the law), `a_gap_wide_enough_is_one_body_a_hole_two` (0.43.2, red on 0.43.1);
`a_wall_across_a_river_is_spilled_over_and_the_river_below_holds` (0.43.1);
`a_river_found_at_rest_keeps_its_banks_dry`,
`a_basin_waits_while_a_chunk_of_it_is_unloaded`, `ramps_at_the_waters_edge_flood_and_dry_with_it`,
`a_waiting_basin_passes_on_what_comes_in` (0.42.2, red on 0.42.1); `tests/water.rs`
`a_block_set_in_a_generated_rivers_bed_changes_nothing_else` (seed 1, two rivers, six hours of water
time: no other cell changes; red on 0.42.1, 173,921 cells); `dwell-worldgen` `tests/worldmap.rs`
`a_rivers_flow_is_read_where_its_water_lies`; `a_dammed_channel_fills_and_spills_over_the_dam` (rises a row,
spills at its old flow within 5 %, a pool below, the water conserved to 1e-6),
`a_cut_drains_a_lake_to_the_cuts_floor`, `a_deeper_trench_takes_the_flow_from_the_old_way` (the
notch dry, the trench within 5 %), `a_cut_from_the_sea_floods_a_hollow_to_sea_level` (a 68,600-cell
sea, its level unmoved), `the_flow_data_resolves`; `tests/water.rs`
`breaking_a_lakes_wall_drains_its_top_row_and_clients_see_it` (through the server; red with the water
tick off); `shared/fluids/gen.test.mjs`.

## 16. Review (2026-11-03)

The owner, after 0.42.1: a block set at the bottom of any river flooded it without end or drained
it, and flying away seemed to delete basins; then *"Do an evaluation on the system as a whole and
look for flaws in design or spec."* Reproduced in a generated world (seed 1): one block in a river's
bed found a pool with an inflow of 61 m³/s and no outflow, and a lake beside it with 3,978 outlets
and 1,059 m³/s. What the review found, and where each stands:

| Flaw | Status |
|---|---|
| **Inflow measured off the generated water.** W1's inflow (what left a body just before the edit) assumed the water was in balance by the basins' rules; the generated world's isn't (plants, banks and the next step beside it are outlets by those rules), so every body found became a spring as large as its shore's gaps. | Replaced (0.42.2): the world as found is at rest, fed by the world map (§2.2). |
| **Rivers aren't clean steps.** A generated river falls a row every few metres, its pools joined below their surfaces; merging a higher pool into a lower one it met collapsed the river into one basin with every pool's inflow. | Fixed (0.42.2): merges only at one level; the steps' water passes by their natural outlets. |
| **Generated water and the containers disagree.** A body's container found again spread into cells it never filled, or dropped cells over a plant, so its level jumped as soon as it was rebuilt. | Fixed (0.42.2): what holds a found body is the world as found; its own cells stay in its rows. |
| **Discovery depends on order.** 64 ways a tick, a way not found yet keeping its water: a pool found downstream filled 1.4 m in one tick before its ways were found. | Fixed (0.42.2): ways found in rounds, up to 1,024 a tick; water in transit while they aren't. |
| **No integration scheme.** A tick of 7.2 s of water time on a pool of a few dozen cells overshot (flows swinging to 666 m³/s). | Fixed (0.42.2): flow steps by the fastest basin. |
| **Live radius and loaded chunks.** Basins were live to 160 m but unmodified chunks are evicted past 3 chunks; an unloaded cell is a wall, so a basin rebuilt beside one lost its water; a waiting basin took what a live one sent it and let nothing out. | Fixed (0.42.2): a basin waits while a chunk of it is unloaded; a waiting basin passes on what comes in. 0.43.3: a basin is found again when a chunk beside it loads (a flood stopped at the loaded edge), and a seep no longer passes on what a flood sends it. |
| **Silent losses.** A sheet capped at `maxRowCells` loses its excess; water running out of the loaded world is lost. | Counted in `lost` (the conservation test sees them); kept, as W1's deviations. |
| **The water's edge.** A ramp beside the water at its row stood dry, the water ending in a wall (the owner's screenshot). | Fixed: the water's writes flood the ramps beside them (0.42.2), and the generator floods its ramps beside its water (0.43.0, a new compatibility line). |
| **Nothing saved.** After a restart the basins are gone; the water stays where it was left. | Fixed (0.43.4): the basins saved with the world (§8), and a basin that waited brought forward when it is live again (§6). |
| **No step at a crest** (owner, 2026-11-03: a wall across a river filled the water up to it, not over it and down the other side; a wall level with the water didn't raise it). One basin over a wall had one level; the spill wasn't drawn; the pool below, cut off from the map's flow, filled with the spill. | Fixed (0.43.1): crests split basins, spills drawn as running water, found bodies pass what comes in. |
| **A hole below the water in a wall** joined the two sides into one basin at one level, so the wall held nothing back. | Fixed (0.43.2, ADR 0056): one opening law, `C·A·√(2gΔh)`, for every opening and every face two basins share; one body or two by whether the faces even them out at once. Jets (§10) later from the same law. |

