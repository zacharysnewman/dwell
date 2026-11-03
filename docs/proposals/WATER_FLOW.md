# Proposal: water flow (basins, outlets, currents)

> **Status: proposal, not adopted.** Nothing here is built; ARCHITECTURE.md does not describe it.
> It is adopted by the owner's answers to §13, an ADR (the next, 0053) and phases in the
> implementation plan.

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
~21 m³/s) seed the chains: their sources become **springs**, their water the initial levels.

## 3. What is stored

Per basin (the world's save, a new table; sent to clients):

- `id`, its liquid, its **level** (m, f64), its **volume** (m³).
- Its **cells** are not stored: they are the connected cells below the level, found by a flood fill
  from a seed cell when the basin forms or its shape changes (an edit at its rim).
- A **level-to-volume table** (the area at each ½ m row, so volume ↔ level is a lookup), made by
  the same fill.
- Its **outlets**: each a rim cell span where the liquid can leave, with its sill height and width,
  and where it goes (another basin, or a stream).
- Its **inflows**: springs, streams and other basins' outlets that end in it.

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

1. **In:** springs + streams arriving + upstream outlets' flow (m³/s × dt).
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

Liquid is conserved: it comes in at springs (and later rain) and leaves to the sea, the world's edge
or evaporation, so a diverted river really takes the water from the old one.

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
speed, faster in a narrow fast run than on a lake. The same for players swimming (the swim layer's
drift) and, later, items. The clients get the currents with the basins, so the prediction carries a
raft or a swimmer as the server does.

## 8. On the wire and in the save

- **Save:** the basins table (id, liquid, level, volume, seed cell, outlets) and the springs. The
  liquid cells stay in the chunks as now.
- **Network:** a basin's level and currents when they change (a few bytes); the clients rewrite
  their own rim cells at row crossings, as the server does. Chunk edits from the basins are not
  sent cell by cell.

## 9. Liquids as data, and what is general

One engine, a small fixed vocabulary: basins, outlets (weir), leaks, streams, springs, sinks,
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
  and snow return it over land (as springs spread over an area), snow by the bands' snow line. Needs
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
the basins table, the protocol gains basin messages, and the generator seeds springs and levels from
the world map (the terrain a seed generates is unchanged; its water's state is new).

## 13. Open questions for the owner

1. **Lava and lahar on contact:** lava meeting water (stone? obsidian-like?), lava burning blocks,
   lahar depositing mud as it slows. Decide now, or later as data?
2. **Time scale:** at real rates a large lake drains over hours. Real rates, or a world setting that
   speeds the water up?
3. **The smallest basin:** do puddles count (a basin of a few cells), or is there a minimum size below
   which water is just static blocks?
4. **Players' own water:** does placing a water block make a basin (it spreads and finds its level), or
   is placed water static as now?
5. **Currents on players:** should a strong current carry a swimmer away, or only drift them a
   little?
6. **First phase:** I would start with water only: basins, outlets, damming, draining, diverting,
   currents on floating bodies, near players with fast-forward. Lava, lahar, jets and evaporation
   later. Agreed?
