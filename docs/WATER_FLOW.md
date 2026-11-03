# Water flow (basins, outlets, currents)

> **Status: adopted (the owner's answers, 2026-11-03; [ADR 0053](./adr/0053-water-flow.md)),
> Phase 52. W1 built (the server's basins, §15); W2–W5 planned.** First water only; lava, lahar,
> jets, evaporation and weather later (§10).

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
2. **Out:** each outlet's flow by the **weir law**, `Q = c · width · head^1.5` (`head` the level over
   the sill, `c` from the liquid's viscosity), plus leaks through blocks below the level
   (`leak × area × head`), plus evaporation (`rate × surface area`).
3. **Volume** += in − out; **level** from the table.
4. If the level crossed a row: the rim cells of that row become liquid (rising) or air (falling).
   A rise that reaches a new outlet adds it; a rise that reaches another basin at the same level
   **merges** the two; a fall below a ridge in the bed **splits** the basin in two
   ("fill, spill, merge", the hydrology standard; the world map already finds lakes by priority
   flood).
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
  in the chunks as now.
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
  surface (a river's lower pool, past its step, is a body of its own). Its inflow is what its outlets
  passed in the world before the edit (W1's stand-in for runoff, §2.1); liquid an edit put there
  brings nothing in. A body over `maxBasinCells` (65,536) is a reservoir at its surface's top.
- **The container**, row by row from its bottom: each row from the cells over the row below (and its
  own), spread sideways over open cells held up by a solid block or the row below; as many rows as
  its volume fills. Rows a basin meets another basin's cells in merge the two; a reservoir's cells
  are walls (a basin beside the sea fills toward its level through the outlets between, drowned).
- **Outlets**: an open cell beside a container cell with nothing holding it up (sill: that row's
  floor) or under one (a hole in the floor), and a block of `leaks` data beside it with an open cell
  past it. A weir passes `weir × head^1.5` per cell, drowned (`head` over the higher of the sill and
  the level it runs into), a leak `leak × ½ m² × head`; no more in a tick than the basin holds over
  its lowest way out.
- **Where it goes** (`follow`, up to 64 routes a tick): down while open, then along the floor for an
  open cell with nothing under it within `streamReach` (16); onto liquid (that body, found if new),
  or it pools there as a new basin, or out of the loaded world (lost).
- **Rows** are written when the level crosses half a row; a row the level falls below goes, and a
  container that falls apart splits, each part with the liquid over its own cells. An empty basin
  with nothing coming in goes.
- **Near players**: basins within `liveRadiusM` (160 m) of a player tick; the rest wait.
- **The day** of a new world is 20 minutes since this change (owner, 2026-11-03; DAY_NIGHT.md),
  so water time runs 72× real time × the water speed; both are the world's settings (ADR 0054), a
  new world's water speed the server's `water_speed` (`flow.json` `speed` by default).

Deviations, for W2–W3: basins aren't saved (after a restart a body is found again by the next edit
touching it, at the inflow it then has), waiting basins aren't fast-forwarded, running water between
basins isn't drawn (it arrives where it lands), and a sheet spreading wider than `maxRowCells` over a
plain is capped there (the excess lost).

Tests (`water/tests.rs`, both faces): `a_dammed_channel_fills_and_spills_over_the_dam` (rises a row,
spills at its old flow within 5 %, a pool below, the water conserved to 1e-6),
`a_cut_drains_a_lake_to_the_cuts_floor`, `a_deeper_trench_takes_the_flow_from_the_old_way` (the
notch dry, the trench within 5 %), `a_cut_from_the_sea_floods_a_hollow_to_sea_level` (a 68,600-cell
sea, its level unmoved), `the_flow_data_resolves`; `tests/water.rs`
`breaking_a_lakes_wall_drains_its_top_row_and_clients_see_it` (through the server; red with the water
tick off); `shared/fluids/gen.test.mjs`.

