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
