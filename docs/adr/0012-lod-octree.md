# 0012. Whole-world view: a 3D level-of-detail octree, generated on the client

- Status: Accepted
- Date: 2026-09-29

## Context

The world is an 8,192 km disc, 8,192 m tall (ADR 0011): about 5 × 10¹³ full-detail chunks. The
requirement is that everything that should be visible from where the player's view is — including
high on a mountain, in a flying vehicle later, or a dev camera high in the sky — **is visible**, at
a level of detail (LOD) reduced with distance, and that other players' builds show up in it.

The reference is **Distant Horizons** (DH), a Minecraft mod (studied from its 3.3.2 release; it is
LGPL-3.0 — Dwell reuses its *concepts*, not its code). What it does:

- **A detail-level pyramid of fixed-size sections.** A section at detail level *d* is always 64 × 64
  columns whose cells are 2^*d* blocks wide, so every level costs the same per section and the
  number of sections in view grows with the log of the view distance.
- **Detail from distance.** `d = floor(log(distance / unit) / log(base))` with presets
  (base 2–2.2, unit 64–320 blocks), plus a special case for zoomed cameras.
- **No holes.** Walking a quadtree from the root, a parent stays rendered until *all* its children
  can render; only then are they swapped in. Loading goes coarsest first, then nearest first, so
  the whole view appears at once, blurry, and sharpens.
- **Direct coarse generation.** A world generator may produce a section at a coarse detail level
  directly instead of generating full chunks and downsampling (`REQUIRES_SPLITTING` when it can't).
- **Update propagation.** Changed sections are flagged (`ApplyToParent`) in SQLite and a background
  job re-downsamples them up the tree, nearest to the player first.
- **Multiplayer.** The client asks for a section with the timestamp of the copy it has; the server
  answers only if its copy is newer, under rate limits.

DH is 2.5D: each column holds a short list of vertical runs. Dwell is 3D (overhangs, caves,
buildings stacked vertically, a world 8 km tall), and its terrain generation is bit-identical on
server and clients (ADR 0010), which DH's is not.

## Options considered

- **DH as is: quadtree of columns of vertical runs.** Compact for terrain; but a column's run count
  is capped per level, stacked structures and cave mouths degrade badly, and it is a second voxel
  representation beside chunks.
- **Octree of cubic sections, same cell encoding as chunks.** 3D throughout; level 0 *is* a chunk;
  reuses the palette + RLE codec, meshing, and storage. Above the level where one section spans the
  world's height it degenerates to a quadtree by itself.
- **Heightmap/impostor far field.** Cheapest, but no overhangs or buildings, and a visible seam
  between the voxel near field and the far field.
- **Where distant data comes from:** server-generated and streamed (as DH does), or generated on
  the client from the seed with only modified sections sent.

## Decision

**A 3D octree of LOD sections, generated on the client, with only modified sections sent.**

1. **Grid.** Level L has cubic cells 2^L m wide; a section is **32³ cells** (32 × 2^L m). Level 0
   is the chunk grid itself. LOD coordinates `(L, i, j, k)` are relative to the corner
   (−2²³, `WORLD_MIN_Y`, −2²³), so a level-L section's cells align with level L−1's, and the whole
   disc fits in one root at `LOD_MAX_LEVEL` = 19 (a section 16 777 km wide). From level 8 upward a
   single section spans the world's 8,192 m height, so those levels have one row.
2. **Content** is a 32³ array of `u16` materials, encoded like `ChunkData Explicit` (palette + RLE).
   - **Unmodified** sections come from `GenerateLod(seed, generatorVersion, L, i, j, k)`: a pure
     function in `server/core/worldgen` evaluating the generator at cell centres, with noise
     octaves and features smaller than a cell dropped (so distant terrain does not alias). It is
     deterministic native-vs-WASM like chunk generation and checked by the same golden test.
   - **Modified** sections — any section with a modified chunk below it — are
     `Downsample(8 children)`, recursively, where unmodified children come from `GenerateLod` and
     level-0 children are chunks. Downsampling a 2×2×2 block: solid if at least 4 of 8 cells are
     solid (so a one-voxel wall or floor survives one level), else liquid if at least 4 are
     liquid, else air; the material is the most common among the qualifying cells, ties going to
     the upper cells (surface materials win).
   - Sections that the generator's column height bounds prove all air, or buried with no exposed
     face, are skipped without being generated.
3. **Server: propagation.** A chunk edit marks its level-1 section dirty. Off the tick, a budgeted
   job re-downsamples dirty sections nearest to players first, marking each parent dirty in turn,
   up to the root (at most 19 sections per edited chunk). Each written section gets a
   `lodRevision` from a server-wide counter. Sections are stored in the world database
   (`lod_sections`, §6.4) as a cache derivable from chunks; a generator version change rebuilds it.
4. **Network: index + request.**
   - The **LOD index** is the set of modified sections at `LOD_INDEX_LEVEL` = 8 (8,192 m columns,
     one row) with their revisions. The server sends it after `WorldgenCheck` (`LodIndex`) and
     broadcasts changes as propagation writes them (`LodIndexUpdate`, coalesced).
   - A section at level ≥ 8 is modified exactly when an index entry lies under it. A section at
     level < 8 whose level-8 ancestor is not in the index is unmodified. In both cases the client
     can generate unmodified sections itself.
   - Otherwise the client sends `LodRequest(L, coord, knownRevision)` on `control`; the server
     answers `LodData` as `Generated` (nothing modified here), `Explicit` (content + revision), or
     `Unchanged` (the client's revision is current). Requests are rate-limited per client.
   - LOD traffic uses its own reliable server-to-client stream, **`lod`**, with its own budget
     (`LOD_BYTES_PER_SECOND`), so it never delays chunk data or voxel deltas on `world`.
   - In full-chunk mode (the client's generator failed verification) the server also answers
     every `LodRequest` with `Explicit`; detail fills in at the rate the budget allows.
   - Everyone's builds are visible from anywhere, as the design requires.
5. **Client: selection.** The octree is walked from the root around the **camera** (not the
   player's body) each frame, refining a node while its cells' projected size exceeds
   `LOD_PIXEL_ERROR` pixels (a screen-space error: the same result as DH's distance formula at a
   given field of view and resolution, and correct for zoom and altitude without special cases).
   A parent stays drawn until all 8 children are ready (meshed, or known empty); level-0 children
   are the streamed chunks, so where the camera is far from the player's body the finest level
   drawn is 1. Generation and meshing jobs run coarsest first, then nearest, in the worldgen and
   meshing worker pools.
6. **Client: drawing.** LOD meshes are greedy-merged, one flat colour per material (the average of
   its texture) with face shading. Sections carry a one-cell apron from their neighbours so faces
   on section borders can be culled; cracks between levels are closed by keeping border faces
   against differently leveled neighbours. The depth range (5 cm to beyond 16 000 km) is split into
   **two passes** — a far pass for LOD sections, then a depth clear and a near pass for chunks and
   entities — because a logarithmic depth buffer would disable early depth testing on WebGL2.
7. **Dev camera.** A client-side free-fly camera, detached from the player's body, that can climb
   high enough to see the whole disc. It changes only what the client renders and requests; the
   server keeps streaming full-detail chunks around the body.

## Consequences

- Cost per level is constant and most 3D sections are empty or buried, so a few thousand sections
  cover the view; from high above, the disc resolves to roughly one cell per few pixels.
- Untouched terrain costs no bandwidth at any distance; builds cost bandwidth in proportion to how
  many sections they touch at the levels a client needs.
- LOD data reveals the coarse layout of built areas, including enclosed rooms at 2 m resolution,
  to any client (§11). Sealing enclosed voids server-side is a possible later mitigation.
- `GenerateLod` output can differ slightly from downsampled full-detail chunks, so a section may
  change a little when a nearby build turns it from generated to downsampled, and levels pop when
  swapped. Accepted as the normal LOD trade-off.
- Reversal: the far field could fall back to server-streamed heightmaps without touching near-field
  streaming; the index/request protocol would carry them unchanged.
