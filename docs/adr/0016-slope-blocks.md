# 0016. Slope blocks: shaped block families, one baked shape table, shaped terrain

- Status: Accepted
- Date: 2026-10-06
- Resolves: Phase 9 of the implementation plan (slope shapes, collision, building, terrain, LOD)
- Builds on: [0015](0015-block-registry.md) (the block registry), [0010](0010-worldgen-noise-numerics.md)
  (strict IEEE numerics)

## Context

Terrain was cubes and half-block slabs. Phase 9 adds standard (45°) and gentle (26.57°) slope blocks
with hip and valley corners, upright and inverted, generated on the terrain, walkable with
identical collision on server and WASM client, placeable by players and drawn by the LOD
([`SLOPE_BLOCKS.md`](../SLOPE_BLOCKS.md)). It touches the registry, both meshers, terrain
collision, targeting, the edit check, the controller's probes, the generator and the LOD.

## Decision

1. **Shaped block families.** `shapeFamilies` in a namespace's block data lists the shapeable
   materials; the generator makes `dwell:<m>_slope[facing,flooded,half,shape]` (9 shapes × 4
   facings × 2 halves × 2 flooded = 144 states) and `dwell:<m>_slab[flooded,half]` per material,
   appended after the explicit blocks (earlier ids are unchanged; `stone_slab` keeps its slot by
   being declared in place with `family`/`of`). The families take density, colour and textures
   from their base block and are all `placeable` (`palette: "all"`): the hotbar keeps one slot
   per material and a shape key picks the piece.
2. **One baked shape table.** A shape is four corner heights in halves of a cell (NW, NE, SE, SW), a
   split diagonal and an inversion flag; `shared/blocks/shapes.mjs` bakes each distinct shape into
   closed outward-wound convex polygons tagged with their cell face (or the sloped surface), side
   profiles, volume, convexity, and the y extent, emitted into both `blocks.gen.h` and
   `blocks.gen.ts`. Everything reads this table: the collision mesh, the chunk mesher, targeting,
   the edit check, the probes, the LOD mesher's pieces. Face culling is exact for what it models:
   a face is dropped where the neighbour's opposite face covers it completely (side profiles under
   or over a linear height line, full floors and ceilings); partial overlaps are kept, which leaves
   some back-to-back interior faces (invisible, and the adjacency test proves there are no holes).
3. **Exact queries in `core/block_shape`.** `RayEnterShape`, `SolidSpanAt`, `FaceCovered`,
   `VerticalSegmentDistanceSq` use `+ − × /`, `min`, `max` only. Cubes and slabs go through the same
   arithmetic as the box casts they replaced, bit for bit (a differential test keeps the old box
   cast; the controller golden trace is unchanged).
4. **Flooded.** `flooded` is a state property, normalised by the server from the cell on
   placement (water: flooded, else dry), breaking leaves water, and swimming counts a flooded cell
   as water.
5. **Terrain.** Generator version 5 adds stage 5b: per-column *continuous surface* (the highest zero
   of the density, closed-form per lattice layer), cell corners = mean of four columns rounded to a
   half, piece = exact shape where one exists, else the nearest by a fixed rule
   (`PIECE_NEAREST`, generated once for both languages). No neighbour data is read, so chunks agree
   with each other and with the point queries. This is a **breaking change** (the terrain a seed
   generates): a new compatibility line, raised by the owner in `package.json`.
   **Amended (0.3.1, 0.4.1):** each column has one surface cell on solid cells. 0.3.0
   shaped every cell the corners crossed, so where they spanned a block from a half-height two
   pieces stacked with a gap under the upper one, and it left ground steeper than a block per cell
   as cubes; 0.3.1 and 0.4.1 clamp such corners into the cell holding the column's own surface, so
   steps carry a slope and nothing rests on a slope. The terrain a seed generates changes, but by
   the owner's decision (2026-10-06) it ships as patches with the same generator versions: worlds take
   the fix in chunks not yet edited; edited chunks keep the old slopes (a seam where they meet).
6. **LOD.** The client's LOD mesher derives the same pieces from column heights (generated surface
   heights or top solid cells) without changing the LOD format; walls follow the slopes' edges.
7. **Controller.** `maxSlopeAngle` 45° → 50°; the walkable face of a standard slope is exactly 45°
   and must not depend on float rounding.

## Consequences

- The registry hash changes (new states, `stone_slab` gains properties), so protocol-compatible
  clients and servers must share a build; world files keep canonical strings, so
  `dwell:stone_slab` still resolves (to its default state).
- Chunk generation is ~25 % slower with slopes; the LOD mesher emits individual triangles for
  sloped columns only.
- Phase 14 (clusters) reads each shape's convexity and volume from the table; inner corners split
  into two convex wedges there.

## Rejected

- A tetrahedral corner piece (54.7° face, unwalkable).
- A hand-made id formula for shapes (the registry replaces it).
- Reading neighbour chunks to smooth terrain (breaks "a chunk is a pure function").
