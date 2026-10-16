# 0036. Render modes as block data, a cross geometry, and placement rules as data

- Status: Proposed (built on `claude/transparent-blocks` with the defaults below; the owner's
  answers to [`TRANSPARENT_BLOCKS.md`](../TRANSPARENT_BLOCKS.md) §8 are outstanding)
- Date: 2026-10-15
- Builds on: [0015](0015-block-registry.md) (blocks as data, the registry generator),
  [0016](0016-slope-blocks.md) (the shape mechanism: collision is separate from the look),
  [0027](0027-webgpu-renderer.md) and [0029](0029-ray-marched-renderer.md) (the renderer and the
  shared frame), [0014](0014-versioned-releases.md) (compatibility lines)
- Design: [`TRANSPARENT_BLOCKS.md`](../TRANSPARENT_BLOCKS.md)

## Context

The owner (2026-10-15): "semitransparent block handling with blades of grass billboards as the
starter block", and then: ladders draw a black background between their rails and rungs and should
be see-through. The renderer knows two ways of drawing a block: opaque cubes (and the ladder's plate,
drawn opaque with dark painted gaps) and water, blended and sorted, hardcoded by look name. A blade
of grass needs a third: pixels either fully there or not (alpha-tested), writing depth, in any order,
and a geometry that is not a cube face. Placing grass also needs rules the edit check doesn't have:
only on grass or dirt, replaced by whatever is placed into it, gone when what it stands on goes.

CLAUDE.md's design questions were not put to the owner before building; the coordinator gave
defaults, recorded in the design doc with the open questions.

## Options considered

- **How a block says how it is drawn**: keep inferring it from the look name and opacity (water is
  "translucent" because it is water), or **a `render` field from a fixed vocabulary**
  (`opaque`, `cutout`, `translucent`) that any look can take (chosen: glass, ice and leaves are data
  later; the mesher sorts geometry into one mesh per mode, the renderer has one pipeline per mode).
- **Grass geometry**: camera-facing billboards (rotate every frame; break under the bifacial world's
  turned-over camera and look flat from above), or **fixed crossed quads** (chosen: static geometry
  meshed once in the workers like every chunk; reads well from above and from the side).
- **Edges**: a plain alpha test (aliased, shimmering), alpha blending (needs sorting per blade), or
  **alpha-to-coverage** under the 4× MSAA already on (chosen; a plain test where MSAA is off),
  with the atlas mips' alpha rescaled to keep each tile's coverage so blades don't thin out with
  distance.
- **Placement rules**: code per block, or **generic data fields** in the block data —
  `placeOn` (a list of block ids), `replaceable`, `needsSupport` (`"below"`) — checked by the
  server's edit validation (chosen).

## Decision

- Block data gains `render` (`opaque` default; `opacity < 1` requires `translucent`; `cutout`
  requires a texture), validated by `gen.mjs` and emitted as `StateInfo::render`. Water declares
  `translucent`; ladders `cutout`.
- A **`cross` look**: 2–4 vertical quads through the cell centre, evenly turned, double-sided,
  with data options for the height and per-cell jitter (offset, turn, height scale) seeded by the
  cell's world position; `wind` (strength, speed) sways the top vertices in the vertex shader;
  `fade` (start, end, inside the minimum full-detail distance) fades it out before the LOD. Crosses
  have shape `empty`; the LOD treats empty-shaped cutout blocks as air.
- The chunk mesher writes a third mesh, `cutout`, with one extra per-vertex vec4 (sway, wind speed,
  fade start and end); the renderer draws it with its own pipeline (depth write, no culling,
  alpha-to-coverage) after the opaque draws and before the translucent ones.
- `selectionBox` gives a block a smaller box for targeting, line of sight and the outline.
- `placeOn`, `replaceable` and `needsSupport` are checked by `check_block_edit`; support removal
  is computed by `dwell_world::edit::unsupported_after` and sent in the same `VoxelModification`.
- The starter block is `dwell:tall_grass` (prototype content), appended to the block list.

## Consequences

- Adding a block changes the registry hash, so `Welcome` rejects older builds: **the grass needs a
  new compatibility line (0.12)**, the owner's decision; it must not ship as a 0.11 patch. The
  ladder's change is look-only (no state strings change, the hash stays) and can ship alone.
- Terrain hashes don't change (the grass is appended, so earlier ids hold, and world generation
  doesn't place it).
- New see-through blocks are data: a `cutout` cube is a leaf block, a `translucent` cube is glass.
  A new geometry is a new look name in the mesher's small vocabulary.
- The cutout vertex is 19 floats against the terrain's 15; only cutout geometry pays it.
- Reversal: drop the `render` field (everything opaque but water again) and the cross look.
