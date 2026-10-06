# 0018. Drainage-consistent terrain: rivers as noise contours, terraced static water above sea level

- Status: Accepted
- Date: 2026-10-06
- Resolves: Open Decision 17 (water above sea level); Phase 11a of the implementation plan (height
  model, rivers, lakes, water above sea level)
- Builds on: [0010](0010-worldgen-noise-numerics.md) (strict IEEE numerics),
  [0011](0011-planet-scale-world.md) (the disc), [0012](0012-lod-octree.md) (the level of detail),
  [0017](0017-continents-from-voronoi-plates.md) (the coast distance and plate edges)

## Context

Water was only sea: open space below `y = 0` filled with water, nothing above. The terrain's relief
(hills, ridged ranges, massifs) was independent of any drainage, so no river could exist, and a lake
was only a hollow that happened to fall below sea level. Phase 11 ([`WORLD_GENERATION.md`](../WORLD_GENERATION.md)
§3) asks for terrain that reads as drained: rivers that run along valley floors and reach the sea,
mountains that stand away from rivers, lakes, and water that sits above sea level inland with
waterfalls where the floor steps. All of it has to stay inside the generator's rules: a pure function
of (seed, world coordinates), no neighbour reads, no simulation, bit-identical natively and in WASM,
identical in chunks, point queries and the level of detail.

## Options considered

1. **Hydrology (flow accumulation over a heightmap).** Real drainage, but it needs the whole
   catchment: neighbour reads and a global pass, which the generator forbids (chunks in any order,
   on any thread). Rejected.
2. **Rivers carved by a path/graph generator (splines from springs to the sea).** Needs a global
   or hierarchical structure and distance-to-spline queries per column. Doable, but costly per
   column and hard to keep identical in chunks and the level of detail. Rejected for now.
3. **Rivers as zero contours of single-octave noise, the relief damped by distance from them
   (Epic Terrain's technique), the water static at a terraced surface below the valley floor.**
   Chosen.
4. **Static water at a single level per river (no terraces).** A river that climbs hundreds of metres
   inland cannot hold one level; it would need dams. Terraces give steps instead.
5. **Flowing water** is out of scope (the water rules of edits are unchanged: water is static).

## Decision

1. **Three tiers of rivers** (`worldgen/rivers.h`): the zero contours of three Perlin noises
   (wavelengths 200 km, 6 km and 1.5 km: great river, river, stream). The two small tiers are sampled
   at a position displaced by a gentle vector noise (meanders). Each tier's lattice is shifted by a
   hashed offset, because Perlin noise is exactly zero at its lattice points: unshifted, every tier
   would cross at the origin — the spawn — in every world (a regression test pins it). A tier's
   `core`, `bank` and `full` thresholds on |noise| set the bed, the banks and the valley width
   (§3.3's widths follow from them and the noise's gradient); its channel is carved with a
   `(1 − s)⁴` profile so the water, a metre below the banks, keeps to the bed and the lower banks.
   The depth fades with the *undamped relief* above the valley floor: streams become dry gullies
   high up, rivers start at springs.
2. **Valley floor and relief.** On land `height = V + D × relief − carve`. `V` (the valley floor,
   `Column::valley`) is smooth by construction: the layout's own coast distance (the macro lattice,
   not the local octaves), the coast's lowland, a slow saturating rise inland
   `(s / (s + 150 km))² × 250 m`, uplift belts along convergent plate edges, and 12 % of the
   ranges' relief — all faded in from 5 to 30 km inland, so `V` is the shore's 2 m for the first
   5 km and **great rivers meet the sea at sea level**. `relief` is the old hills (now never
   below zero, so the ground is never below `V`), mountains and ranges, plus belt relief. `D` is the
   product over the tiers of a smoothstep of |noise|: 0 in a channel's bed, 1 away from every
   channel — *ridges stand between rivers, and every river runs along a valley floor*.
3. **Water above sea level.** `Column::water` is the level below which open voxels are water: sea
   level, or a river's or lake's surface. A river's surface is the **terrace** of
   `V − 1 m` (`TerraceSurface`: boundaries every 4 m, each displaced by a hashed −1, 0 or +1 m, so a
   step is 2–6 m; sea level below the first); terraces are computed per column from the
   *interpolated* `V` (never interpolated themselves), so a step is a crisp vertical face: where a
   channel crosses a boundary, the upper pool ends in a **waterfall** over the lower one. Banks are
   at least `V` and the water at most `V − 1`, so a river never spills sideways; the only
   horizontal contacts between water and air are those steps (a test counts them and requires every
   one to be a step). The water is static; the integrity and water behaviour of edits are unchanged.
4. **Lakes.** A jittered grid of 12.3 km cells, a lake in 35 % of them, at least 20 km from the sea,
   a radius of 0.7–2.0 km, a squared-distance mask with a shoreline wobble, a bowl-shaped bed 4–12 m
   deep and a low berm (1.5 m) around the shore, so no lake spills over a lower shore. The surface is
   the terrace of the valley floor *at the lake's centre* (the terrain's own `V`, evaluated there)
   less 3 m. Rivers stop at a lake's shore, ending in a step down into the lake. Cells are 12.3 km,
   not the 3–10 km of the design: with the berm, lakes of up to 4 km across need cells that keep
   neighbouring lakes' domains apart (their sites ≥ 6 km apart). Sites lie in the middle half of a
   cell and a lake's reach is under 2.7 km, so a point only ever looks at its own cell's lake. A
   lake's surface needs the terrain at its centre, so it is cached per thread (a pure function of the
   seed and the lake cell: speed, never values).
5. **Caves** start 12 m deeper under any column with a channel (banks included), a lake, or a
   shallow sea floor (shallower than 40 m), so none drains or breaches water. **Overhangs** shrink
   to 1–2.5 m on most land (up from 17 m), more on mountain faces, and vanish in channels.
6. **Everything that decided "the sea" reads `Column::water`**: the classifier, surface materials
   (river and lake beds: gravel where deep, sand where shallow), the slope stage's flooding, the
   stability pass, features (no trees in water), `SkyFloor`/`IsAirChunk` and `LodBoundsOf` (water
   above sea level counts as terrain), and the spawn (dry ground; near water when some lies within
   ~300 m of the origin).
7. **Level of detail.** `GenerateLod` samples the same fields at cell centres: a tier narrower than
   a cell *widens* to one cell (so great rivers and lakes remain from altitude), and tiers drop out
   at cell widths of 16 m (streams) and 128 m (rivers), their distance factor taking its mean. From
   4,096 m cells (level 12) the lakes and the great river's windings, smaller than a cell, are
   dropped too. The great river's windings and the spring noise, smooth over 256 m, are evaluated
   at the corners of a 256 m lattice (cached per thread) and interpolated, below 256 m cells.
8. **Generator version 7.** Goldens regenerated, with a lake, a stream, a river, a great river and
   a waterfall per seed added to the chunk goldens and the LOD goldens.

## Consequences

- **A breaking change to the terrain a seed generates** (the public API, `RELEASES.md` §3): a new
  compatibility line (0.5.0). The owner raises `package.json`; worlds of the 0.4 line keep opening in
  0.4 builds.
- **Closed loops.** A zero contour of Perlin noise is mostly a *loop* of about a wavelength, so the
  small tiers' rivers circle rather than run from a spring to the sea (the great river runs on and
  reaches the coast). That is the technique's character, as in Epic Terrain. A flow-aware network
  would need option 2 (a hierarchy of river sources); it is a candidate follow-up, not part of this
  phase.
- **The lake's surface and `V`.** A lake's level is quantised from `V` at its centre, so a lake in
  a steeply sloping valley meets its berm on the uphill side only as high as the berm: the berm's
  height is a tunable (`kBermHeight`), and the contact test (spills only at steps) guards it.
- **Cost** (`dwell_worldgen_inspect <seed> bench`, Release, native, near each version's spawn, best
  of five; the 11a sampler is ~0.2 µs per column, about five Perlin evaluations): a chunk
  1.07 → 0.92 ms (the terrain around the spawn is smoother and has less overhang), LOD sections
  level 1 1.03 → 1.25 ms (+21 %), level 3 1.49 → 1.4 ms, level 5 1.77 → 1.45 ms, level 8
  1.5 → 1.65 ms (+10 %), level 10 0.8 → 0.94 ms (+15 %), level 12 0.6–0.86 → 0.65 ms. Within the
  plan's +25 %; WASM not re-measured here.
- **Reversing.** Water above sea level is a generation rule only: reverting `Column::water` to
  `kSeaLevel` and `D` to 1 restores the old shape without touching stored worlds' format.
