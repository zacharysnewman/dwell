# Water as data: the pool graph

*Proposal, 2026-11-04. Not adopted: nothing here is architecture until an ADR adopts it. Asked for by
the owner after 0.47.0 ("write up the new design to replace our existing water block based design with
data based").*

## 1. Why

Today water is blocks first and data second. The generator writes river and lake water as blocks,
and the water engine (`dwell-sim` `water.rs`, ~3,900 lines) reconstructs pools by flood-filling those
blocks when an edit wakes them. From the blocks it then infers what each pool was doing as generated:
what comes in, what goes out, what holds it, where the river leaves. Almost every water bug since
0.42 has lived in that inference, and each fix added a mechanism nobody can see:

| Mechanism | Why it existed | What the owner saw |
|---|---|---|
| feeds by place | a woken pool needs its river's inflow | pools filling from nowhere (BUGS 25) |
| links between found bodies | a pool below counted what the pool above sent | double counting, rivers rising (BUGS 22) |
| seeps | a pool whose way out wasn't found passed its flow into the ground | water vanishing |
| `rest` / `held` | cells beside generated water treated as walls | water standing against air (the 2026-11-04 screenshot), ramps never flooding (BUGS 26) |
| natural scaling, `natural_grown`, `hold_grown`, `refeed`, edge sources | chunk loads, the loaded world's edge | rivers changing as you walk |
| fronts down the map's rivers (W2d), catch-up | changes through unloaded country | — |

On top of that, the generator decides each column alone, but the engine needs every pool in exact
balance with the pool above it. 0.45–0.47 bought that balance with lip plans, flow stepped between
junctions, channel widening and flat lip tops, and the strict sweep still finds new cases each time it
samples more pools (BUGS 27).

The root cause: **the engine rediscovers from blocks what the generator meant.** This proposal makes
what the generator means into data the engine loads, and makes water blocks an output only.

## 2. The idea

The world map already knows every river: its course, its surface along it, its discharge, where it
joins another, the lakes and the sea. From that, the generator derives a **pool graph** per face:

- **Nodes are pools**: a reach of a river (or a lake, or the sea) with one level. Each pool has its
  extent along the course, its level row, and its **stage–storage table**: how many cells each row
  holds, so volume ↔ level is a lookup.
- **Edges are lips**: where a pool pours into the next. Each lip has its cells (the crest), its sill
  row and the pool it pours into. A tributary's last lip is an edge into the river it joins.
- **Sources** are the rivers' heads and lake inflows (the map's discharge at the head, in whole lip
  cells). **Sinks** are the sea and the map's edge.

The graph is **the source of truth for water**:

1. **The generator carves terrain from the graph**: channel beds, lip walls and crests exactly as
   each lip record says, and writes each pool's water at its level. No column decides a lip alone.
2. **The engine loads pools from the graph**: nothing to discover, infer or hold. Each pool's inflow
   is its upstream lips and sources, and its outflow is its own lips by the one opening law
   (ADR 0056). Balance is a property of the data, checked once when the graph is built.
3. **Water blocks are written from pool levels** (fill eighths) in loaded chunks, as today.

```
 map rivers ──► pool graph (data, per face) ──► terrain (carved from it)
                      │                       └► water blocks (from levels)
                      ▼
               engine: levels by the law ◄── edits change the graph
```

## 3. The graph

### 3.1 Built from the map

For each river course (`channels.rs`, already deterministic and chunk-independent):

- Walk down the course. A pool starts at the head or at a lip and ends at the next row step of the
  surface (where 0.47's `lip_band` puts a step). Its level is that row.
- Each step gets a lip: `cells = lip_cells(q)` from the river's stepped flow (0.47's `step_flows`,
  kept). Its crest cells are chosen once, on the step's actual grid cells (0.47's `LipPlan`, moved
  from per-column caching into the graph build).
- A river joining another ends in a lip into the pool of the other river at the join point.
- A lake is one pool with its outlet river's first lip. The sea is a reservoir sink.

Measured on seeds 0–2: 32–45 rivers, up to 3 lakes, 366–388 km of river and **8,300–12,100 pools a
face** (½ m steps).

### 3.2 Geometry

A pool's cells are the channel columns of its reach (the same column function as today), so its
stage–storage table comes from evaluating those columns' heights: no chunk needs to be loaded.
*Estimate:* a reach of a few hundred columns is well under a millisecond, and the whole face is
~10⁶ channel columns. So the graph is built lazily per region and cached, or built once at world
creation and saved.

### 3.3 Balance by construction

The same flow passes every lip of a river down to a junction, where the tributary's lip adds its own.
A checker walks the graph and asserts that each pool's inflow at its level equals its lips' outflow.
It runs on every seed in CI, so balance is proven rather than sampled by the strict sweep.

## 4. The engine

### 4.1 Pools, not basins

The engine holds a state per pool, which is just volume, and starts every pool at its generated level.
Because the graph knows every pool, the **whole network is simulated everywhere, all the time**:

- ~10⁴ pools a face; each tick, one law evaluation per lip and one volume update per pool.
  *Estimate:* well under a millisecond a tick for both faces.
- There's no "near players" condition and no unloaded edges: no edge sources or sinks, no seeps, no
  waiting pools to catch up, no fronts carried through unloaded country. A dam's effect travels
  down the graph at the speed the law gives it.
- Pools at rest stay at rest exactly: their inflow equals their outflow by construction.

### 4.2 Blocks out

When a pool's drawn level (fill steps, hysteresis as today) changes, the engine writes its cells in
loaded chunks. When a chunk loads, its water comes from the generator (generated level) plus the
pools' current levels for any pool that has moved. Unloaded pools still simulate, and their blocks
are simply written on load.

### 4.3 Edits

Edits change the graph, not the blocks the engine reads:

- **A block in a pool** (not on a lip): the pool's stage–storage loses that cell. The level rises a
  hair, and nothing else changes.
- **A block on a lip's crest** (a dam): the lip's cells or sill change, so it passes less and the
  pool rises.
- **A wall across a pool** splits the node into two pools joined by a new lip, whose cells and sill
  are the wall's lowest gap or its top.
- **A cut** through a bank or lip makes a new lip, and its water goes where `follow_path` says
  (today's routing, kept): into another pool, or into a new **dynamic pool**.
- **Dynamic pools** are pools that edits create: a puddle where a spill lands, a pond the player dug
  and flooded. They are found by today's flood-fill, kept as graph nodes, and saved.
- **A pool rising over its banks** extends its stage–storage onto the land by flood-fill at the
  rows it reaches (the one place geometry is still searched for), and may meet another pool, which
  merges with it.

So today's flood-fill code shrinks to the dynamic cases, and generated water is never inferred.

### 4.4 Currents and clients

Each lip's flow gives a velocity along the reach (flow ÷ cross-section). That's W4's currents for
swimmers and floating bodies, and it's what W3 sends clients.

## 5. What stays, what goes

**Stays:** the opening law and flow data (`dwell_world::flow`, `shared/fluids/flow.json`); 0.47's
stepped flows, channel widths and lip planning (moved into the graph build); the mesher's slopes and
falls; running water drawn down made spills; fill eighths; `follow_path` routing; the block dump
(with the graph near the player); the edit log.

**Goes:** discovery of generated water, feeds, links, seeps, `rest`/`held`, natural scaling,
`hold_grown`/`natural_grown`/`refeed`, edge sources, fronts down unloaded rivers (`water/rivers.rs`),
catch-up, `foundSnapM`, and the live radius for river pools.

## 6. Extendability (CLAUDE.md)

- **Data:** the pool graph is generated data, deterministic per seed. Its rules (how a reach becomes
  pools, lip sizing, widths) live in `terrain.json` / `flow.json` with validation. A graph checker
  test fails on any unbalanced pool.
- **A tool:** a graph inspector page (`docs/prototypes/`): the face's rivers as pools and lips,
  levels and flows, click a pool to see its stage–storage. Also `__dwell.waterGraph()` in the game.
- **New kinds of water** (lava, springs, rain) become new source and sink kinds in the graph, not new
  inference.

## 7. Compatibility

The terrain a seed generates changes (lips carved from the graph): a new compatibility line. Saved
water state from earlier worlds is not read; those worlds open in their own version as today. The
network protocol is unchanged until W3.

## 8. Phases (proposed)

| Phase | What | Exit |
|---|---|---|
| G1 | The graph from the map: pools, lips, sources, sinks, stage–storage; the balance checker; the inspector page | every pool balanced on seeds 0–9 |
| G2 | The terrain carved from the graph (lips, beds, water levels) | goldens; the leak scan clean; the strict sweep at 20 pools a seed |
| G3 | The engine on graph pools: the whole network simulated, blocks written from levels; discovery for generated water removed | a dam's effect down the river with no player near; conservation |
| G4 | Edits as graph changes: blocks in pools, dams, walls splitting pools, cuts and dynamic pools | the existing dam, wall, cut and trench scene tests ported |
| G5 | Floods over banks; dynamic pools merging; saved graph deltas | flood and recede scenes; save and load |
| G6 | Currents from lip flows (W4) and pool levels to clients (W3) | the raft and swimmer exit criteria |

*Estimate:* G1–G3 about a week at this session's pace, G4–G5 about another week. The old engine stays
on `main` until G3 replaces it in one release.

## 9. Risks

- **Edits are open-ended.** Players can build anything. Walls, cuts and floods still need local
  geometry (§4.3). The win is that this applies only to what players change, never to what the
  generator made.
- **Generated geometry the graph didn't plan**: banks lower than a pool's level at bends, caves under
  a river. The graph build must check banks against levels (the leak scan as a build-time check);
  where they disagree, the graph either raises the bank (a terrain rule) or adds a lip there (a
  visible leak stream, as the owner chose for type-2 gaps).
- **Lakes and the sea** are big. The sea stays a reservoir. A lake is one pool with a large
  stage–storage, which is cheap.
- **Two faces:** one graph per face, both built face-locally (`FaceFrame`), with the both-faces
  parity test.

## 10. The owner's answers (2026-11-04)

- **Go ahead with G1–G3.**
- **Scope: all water is the graph** — "We don't want legacy water behavior, player made water would
  just be another graph or part of the graph … Wouldn't it need to be to be consistent?" Generated
  pools come from the map; pools edits make (a puddle where a spill lands, a pond dug and flooded, a
  lake behind a dam) are added to the same graph as nodes, and a dam or wall splits a node, a cut
  adds a lip. Flood-filling blocks only measures a new node's shape; one engine, one model.
- **Park the stopgap** (`claude/no-hidden-walls`; its junction lips taken into G2).
- Taken as recommended (not answered): pools at rest need no simulation, every pool that moved
  is simulated wherever it is; a river's graph built from the seed on demand (not saved); a leak the
  build finds a visible stream.

## 11. Open questions for the owner (asked before the answers above)

1. **Scope:** generated rivers and lakes only (player water stays today's flood-fill basins, as dynamic
   pools), or every body of water a graph node, including a bucket's puddle?
2. **Simulate everywhere:** is the whole river network always simulated, so a dam far away drains
   rivers you can't see, or only near players?
3. **The graph's life:** built once at world creation and saved with the world, or rebuilt from the
   seed on load (smaller saves, but the build cost on every load)?
4. **Leaks the graph finds** (a bank lower than its pool): raise the bank, or a visible stream out as
   chosen for 0.47's type-2 gaps?
5. **The current branch** (`claude/no-hidden-walls`: no hidden walls, junction lips; one sweep site
   left): finish and ship it as a stopgap, or park it and go straight to G1?
