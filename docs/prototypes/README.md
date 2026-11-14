# Prototypes

Standalone design prototypes, kept so they survive between sessions. Each is one HTML page
(three.js from jsDelivr; open the file in a browser with internet access, or use the published
link). They are not part of any build; Prettier and CI ignore `docs/`.

| Prototype | File | Published (private to the owner) | Used for |
|---|---|---|---|
| Adaptive Block Lab | [`adaptive-block-lab.html`](./adaptive-block-lab.html) | https://claude.ai/artifact/7gvrFMfUyT6GegnyKPTTqV | The adaptive-block rules on whole scenes and three voxel grids |
| Shape Playground | [`shape-playground.html`](./shape-playground.html) | https://claude.ai/artifact/Sc4Pu3UqX8TcpcrRrEYpVz | Every shape and situation, hand-shaped by the owner; the rules fitted to it |
| Half-Height Voxel Lab | [`half-height-lab.html`](./half-height-lab.html) | https://claude.ai/artifact/DGQZG6GH1GhxvZRtPgh5MN | Today's 1 m cells beside 1 × ½ × 1 m cells with 1 m double blocks: shapes, 27° and 45° slopes, plants standing on the ground, ½ m growth ([proposal](../proposals/HALF_HEIGHT_VOXELS.md)) |
| Tree Growth | [`tree-growth.html`](./tree-growth.html) | https://claude.ai/artifact/MRW4RcNKTHkYdxMyRKLLHz | Oak, pine and palm as species data, grown from a thin sapling to maturity in stages (the same silhouette, the trunk thickening in ¼ m steps), in the Leaf Lab's chosen look (option 5) with octagonal logs; seeds, compare, a forest row (2026-11-11); *Palm fronds*: leaf blocks (the game today), frond cards, or *Split* side by side (2026-11-12; **needs republishing to its link**) |
| Tree Growth: skeleton | [`tree-growth-skeleton.html`](./tree-growth-skeleton.html) | not published | The G0 page: a hand-built tree grown by revealing its skeleton, obstacles, and jitter variants (Phase 30, G0) |
| Chisel (biome editor) | [`client/chisel.html`](../../client/chisel.html) | not published (runs the client's WASM: `npm run build:wasm`, `npm run dev`, then `/dwell/chisel.html`) | The world's biomes edited with sliders and previewed live on the real generator; Export/Import the biome JSON (Phase 56 B3, ADR 0074) |
| Painterly Texture Lab | [`painterly-texture-lab.html`](./painterly-texture-lab.html) | https://claude.ai/artifact/4CTkNW4G6p8p67MkSNMUJA | The painterly materials and palette edited live with the game's generator; the JSON copied back (Phase 39). Folded into Chisel's Textures tab (0.52.42); kept for the character and sky materials |
| Model Viewer | [`model-viewer.html`](./model-viewer.html) | not published (reads the repo's GLBs: serve the repo root) | Every skinned model of `shared/models` (the Peasant and seven animals) playing the clip each role maps to (Phase 54, S1) |
| Character Lab | [`character-lab.html`](./character-lab.html) | https://claude.ai/artifact/YAieiGfJnp5f2FnoR6wQWB | The player's full body and first-person arms, the wolf and the deer, from `shared/characters` (Phase 40, C0) |
| Water Grid Lab | [`water-grid-lab.html`](./water-grid-lab.html) | https://claude.ai/artifact/NynEuDz9qjjBq1U4GRrv75 | Water as a depth per column (local-inertial): rivers, dams, diversions, a flood, chained basins; the steady solve and the water graph beside it; sleeping chunks, still water as pools, θ; place, dig, springs ([evaluation](../proposals/WATER_MODEL_EVALUATION.md), the model chosen in [ADR 0060](../adr/0060-water-surface-grid.md)) |
| Falling-sand water (2D) | [`fluid-ca-2d.html`](./fluid-ca-2d.html) | not published | The owner's cellular water and sand, made conserving and settling (swap-only moves, a step toward the nearest drop) |
| Falling-sand water (3D) | [`fluid-ca-3d.html`](./fluid-ca-3d.html) | not published | The same rules in 3D (fall, diagonal, a per-layer distance field): basins, a spring filling a valley, tanks and a pipe ([evaluation](../proposals/WATER_MODEL_EVALUATION.md) §5) |
| Leaf Lab | [`leaf-lab.html`](./leaf-lab.html) | https://claude.ai/artifact/FXhLL4srjPF1tzCts5m6cq | Ways to make 1 m leaf cells read better beside the octagonal trunks ([ADR 0072](../adr/0072-octagonal-logs.md)): canopy normals, chamfered exposed cells, painted leaf cards, their combinations and a smooth-blob reference, side by side, with a growth slider |
| Voxel models | [`vox/`](./vox/README.md) | not published (open the `.vox` files in MagicaVoxel) | The player, the wolf and the deer from `shared/characters` voxelized at their rest pose, one named model per part in a group per bone, for artists to refine (C0 follow-up) |
| Model Part Painter | [`model-part-painter.html`](./model-part-painter.html) | https://claude.ai/artifact/Jc7FsiFetXgkNX6QHnbyYS (the published copy embeds the Peasant; use Copy JSON there, downloads are blocked) | Paint which part (skin, hair, hat, shirt, …) each vertex of a skinned model's full mesh belongs to, preview a creature's colour variants by part, export the part map (`shared/models/parts/<model>.json`; WORK_QUEUE item 43, [CHARACTERS.md §11.11](../CHARACTERS.md)) |
| Music Audition | [`music-audition.html`](./music-audition.html) | https://claude.ai/artifact/Ccdtsp5eiQYF4b9uzspyYe (the tracks published beside it as .mp4) | The game's four music tracks (the owner's picks, 2026-11-12; the published link still holds the 20 candidates they were picked from): play, skip, keep or drop each, a shuffle preview with the playlist's gaps and cross-fade, the picks copied as JSON (WORK_QUEUE item 33, [AUDIO.md §2a](../AUDIO.md)); in the repo it plays `shared/music/` (serve the repo root) |

The first two feed [`ADAPTIVE_BLOCKS.md`](../ADAPTIVE_BLOCKS.md) (Phase 18, formerly 9e) and
[ADR 0024](../adr/0024-adaptive-natural-blocks.md); Tree Growth feeds
[`TREE_GROWTH.md`](../TREE_GROWTH.md) and [ADR 0033](../adr/0033-tree-growth.md). The Character Lab feeds
[`CHARACTERS.md`](../CHARACTERS.md) and [ADR 0041](../adr/0041-characters.md).

## Adaptive Block Lab

- Scenes: Basics, Terraces, Hill, Terrain (value noise), Wall & pile (natural ground against
  crafted blocks), Flat meadow. Place, remove, orbit; natural and crafted materials.
- **Playground rules** (default): the five rules of `ADAPTIVE_BLOCKS.md` §2. **Previous rules**:
  the earlier designs, for comparison — a height field with resting heights and slope limiting on
  the 1 m grid (with its Heights, Slope and Shape depth settings), and a per-voxel autotile on the
  finer grids.
- Grids: 1 m cubes, 1 × ½ × 1 m voxels, ½ m cubes (with a 1 m placement brush).
- **Copy 5³** copies a 5 × 5 × 5 block neighbourhood as JSON (blocks, column heights, each column's
  shape and surface) for discussing a spot in chat.

## Shape Playground

- 28 labelled situations (A–AB) on a 32 × 32 grid: small shapes, plateaus, a pit, steps, a cliff,
  a diagonal drop, ramps, 1-wide walls and stairs, one situation for each shape the rules can
  produce, a T and a cross.
- Tap a block to cycle its shape, rotate it, or pick from the shape panel (the game's shapes plus
  the 18 the rules add, marked "not in the game yet").
- **Rules & conflicts**: each rule's follow/break counts, outlines (green follows, red breaks,
  amber: no such shape), and a list of the blocks that disagree with the rules, with "Use the rules'
  shape".
- Edits are saved in the browser; **Your last export** restores the owner's final shapes (embedded
  in the page, 2026-10-08: all 250 blocks match the rules; extracted for the game's tests into
  `shared/blocks/playground-export.json`); export and import as JSON.
- **Copy overrides for the game** (Phase 18): every block whose shape differs from the rules', as
  entries for `overrides` in `shared/blocks/adaptive.json` (`{ "around": "+==/=.-/===", "shape",
  "facing" }`); blocks with one neighbourhood and different shapes are reported as conflicts. Paste
  them in and run `node shared/blocks/gen.mjs`, which validates them and the adjacency test.

## Tree Growth

- **Species as data**: a `SPECIES` object in the page shaped like a future `shared/trees/<name>.json`
  (trunk height and width from sapling to mature with their steps, branch rules, crown shape and
  normals, leaf look, growth stages); *Show species JSON* prints it. Oak: thick trunk, two to four heavy
  branches stepping out and up, a lumpy crown with its lumps on the branch tips. Pine: tall straight
  trunk, a cone of 2 m tiers (a wide layer and a narrower one), thin whorl stubs. Palm: a leaning trunk
  of ½ m ring segments offset in ⅛ m steps, a crown heart block and long drooping fronds.
- **Palm fronds** (2026-11-12, WORK_QUEUE answer 8: "I'd like to see a comparison of the two"): *Leaf blocks* draws each
  frond as the game builds it today (the path sampled 12 times, every ½ m cell it crosses a `dwell:leaves` block,
  chamfered and carded like any leaf); *Frond cards* draws two to four long arching painted cards along each frond's
  path (by its length; the longer, the more they sag), the leaf card's painterly strokes in a frond's shape; *Split*
  shows the same palm (seed, stage) both ways from one camera, blocks left, cards right, with the player for scale.
  `?fronds=blocks|cards|split`. The edit is not on the published link yet: republish the page to it.
- **Growth**: a slider and a time-lapse; each species has 8 stages, height in ½ m steps, trunk width in
  ¼ m steps, the crown the mature crown scaled. Oak branches and pine whorls appear from a stage on.
- **Views**: One, Seeds (four variants), Compare (the three side by side), Forest (each species at
  stages 1, 3, 5, 6 and 8); time of day; block cells (leaf, log and the palm's frond cells).

## Tree Growth: skeleton (G0)

- **Load a tree**: three built-in samples (prototype content: an oak-like tree with marked roles and
  root flares, a conifer-like one without roles, a wide spreading one with a marked `trunk` and a
  `core` knot), or paste or import a structure JSON in S1's format (`STRUCTURES.md` §3). Palette
  values may be `{"block": ..., "role": ...}` with `wood`, `leaf`, `root`, `trunk` or `core`;
  without a role, names with log or wood are wood and names with leaves are leaves.
- **Skeleton**: extracted as `TREE_GROWTH.md` §2 describes, shown as nodes and links (amber trunk,
  teal branches, red pruned, grey not grown yet); notes under the tree say where inference had to
  guess (loops, stray wood, floating leaves).
- **Age**: a slider in in-game days, play/pause at 10–150 days a second, and Seed, Sapling, Young,
  Mature stops; the trunk days per metre (30), branch ratio (1/3), world speed and tip-leaf radius
  are editable, with the real time to maturity at a 30-minute day. A badge checks that the mature
  stage equals the model voxel for voxel.
- **Obstacles**: click a face or the ground to place a stone; a node whose core runs into one is
  pruned with its subtree.
- **Variation**: a seed with Roll, and branch angle, length, droop and drop chance; the variant row
  shows the authored tree and five seeds at full age; "Grow the variant" grows the jittered tree.
- **Export**: the `growth` and `variants` sections as JSON (with the world's growth speed beside
  them), or the whole structure with them and the palette roles. The state is saved in the browser.

## Half-Height Voxel Lab

- One scene on two grids side by side, with synced cameras: today's 1 m cells with the Phase 18
  rules, and 1 × ½ × 1 m cells with the proposal's four rules (`proposals/HALF_HEIGHT_VOXELS.md`
  §2). Either view can be shown alone.
- Scenes: Meadow, Basics (lone ½ m and 1 m bumps, a ridge, ½ m and 1 m one-wide stairs, pits, ½ m
  and 1 m terraces), Hill, Terrain, Growth rows. They are given as surface heights in metres and
  realised as each grid best can.
- Blocks in the half grid are ½ m (one voxel) or 1 m double blocks (two voxels, one block). A
  block's corners drop at most to its floor, so ½ m blocks make 27° slopes and 1 m blocks 45° ramps.
  **1 m on steep ground** (default) puts a double block on top of any column 1 m or more above any
  of its 8 neighbours; **½ m blocks only** compares without them.
- Tools: Place (a ½ m block or a 1 m double block in the half grid) and Remove (the top block),
  Plant and Grow (both views). Grow all, Mow, Scatter, and grow over time.
- Grass grows only on flat tops (a full block on the ½ m grid, a level resting top on the 1 m
  grid); a plant whose ground stops being flat goes. Changing the shape set re-plants the scene's
  grass on what is flat.
- Plant footing: at the cell floor (as the game draws plants) or sunk to the shaped surface.
  Floating plants are tinted red and counted per view.
- **Slope shapes (½ m view)**, its own section at the top: **Adaptive (all shapes)** (reshaped from
  the neighbours on every edit), **Cardinal ramps + corners** (ramps facing N, E, S, W plus outer
  and inner corners, at ½ m and 1 m: 24 pieces and the full block), **Cardinal ramps only** (the 8
  straight ramps and the full block) or **No slopes** (flat ½ m blocks). The fixed sets are
  stored: picked when the scene
  is built or a block is placed, never changed by a neighbour. Dug-out ground and shapes without a
  piece are full blocks. The view counts the blocks off the rules.
- Grass blocks' sides are drawn as the game's grass side texture: dirt with a ragged grass skirt
  (4–9 px of a 32 px tile, 0.12–0.28 m) hanging from the face's top edge, so it follows shaped tops.
- Show cells outlines the top ground block (both voxels of a double block) and the cells a plant
  occupies. The cell inspector lists what each world stores for the
  column under the pointer (`tall_grass {height: 1.5 m}` + `{part: upper}` against three plant
  voxels).
## Painterly Texture Lab

- **Folded into Chisel** (0.52.42, ADR 0074 B6): Chisel's Textures tab edits the block materials and
  the palette with the same painter and tints its 3D section. This page is **kept** (still generated and
  tested) for the character and sky materials until the owner retires it.
- Feeds [`PAINTERLY_TEXTURES.md`](../PAINTERLY_TEXTURES.md) and [`ART_DIRECTION.md`](../ART_DIRECTION.md)
  ([ADR 0040](../adr/0040-painterly-textures.md)).
- **Generated in part**: `node shared/textures/gen.mjs` rewrites the block between its `BEGIN
  GENERATED` and `END GENERATED` markers. The block holds `paint.mjs` (the game's painter) and the
  repo's palette and materials, and CI fails if it is stale. Edit the rest of the page by hand,
  regenerate, then republish.
- Materials by use (blocks, characters, sky accents). Each shows its tile, a 3 × 3 repeat, a block lit
  from above at the game's face tints, and its colour from afar (the LOD's flat colour).
- Edit the material's fields (use, ramp, overlay, wrap, alpha, `like`, `over`) and each layer: its kind,
  palette colours as chips, numbers as `4` or `4, 7` for a [min, max] range, and `at` as JSON. Add,
  move and delete layers. Edit the palette's ramps with colour pickers. Bad data shows the
  generator's own validation message.
- **Copy back**: one material, the whole `materials.json` or `palette.json`, ready to paste into
  `shared/textures/`. Edits are kept in the browser; **Reset** returns to the repo's data.

## Model Part Painter

- WORK_QUEUE item 43 (owner, 2026-11-12: "You should have tooling for debugging this and setting up a model so it
  can be retextured."), after bug #96. Feeds [`CHARACTERS.md`](../CHARACTERS.md) §11.11's part maps.
- **Loads** a GLB (Open files, drag-drop, or **Load the Peasant**, which fetches its GLB, part map, `models.json`,
  the palette and `creatures.json` when the page is served from the repo's root: `python3 -m http.server`, then
  `/docs/prototypes/model-part-painter.html`), a part map (refused with a message if its vertex count is not the
  GLB's `detail_0`), `palette.json`, `creatures.json` and `models.json` (its `palette.exclude`), each recognised
  by its content. Or starts a **new empty map** or one **seeded from colours** (each vertex its nearest palette
  ramp, the six commonest ramps a part each).
- **Views:** flat-shaded; **Parts** (a colour a part), **Real
  colours** (the GLB's), **Variant** (a creature's `colours` entries, each `from → to` picked or cycled, recoloured
  by part with `dwell_models::recolour`'s luminance-ratio rule) and **Weights** (a joint's skin weight as heat;
  Alt-click picks the vertex's heaviest joint). Vertex dots and edges. The **Level** select shows `detail_1…`
  with their parts mapped from `detail_0` as `parts.mjs` `levelParts` does (read-only). Hovering shows the
  vertex's index, part, island, colour and nearest ramp, height and weights.
- **Paint:** a brush (radius in screen pixels, `[` `]`; **front faces only** skips vertices hidden behind the
  mesh), **Fill island** (the welded, connected island under the cursor, as `parts.mjs` `islands`), **Fill same
  colour** (the vertices in the brush of the colour under the cursor). The part list adds and renames up to six
  parts (a rename or delete carries to the `seed` rules), shows vertex counts and a display colour; `0`–`6` pick
  a part, `0` erases. Undo and redo.
- **Export** downloads or copies the part map exactly as `parts.mjs` writes it (`$comment` and `seed` kept);
  loading the exported file reproduces it. Put it at `shared/models/parts/` and run `node shared/models/gen.mjs`.
- **Camera** (owner, 2026-11-14): left-drag orbits (while painting: middle-drag, hold Space, or the Orbit tool),
  right- or shift-drag pans, the wheel or a pinch zooms toward the point under the cursor, double-click focuses
  there; the pitch stops short of the poles (no flips) and the distance is clamped to the model's size. Views:
  Front, Back, Left, Right (the model's own sides), Top, ¾, Reset (`F`), eased. Touch: one finger orbits (a tap
  selects), two pan and pinch. The help box folds on a click.
- **Select faces** (the Edit switch; owner, 2026-11-14: "copy those Face IDs so I can paste them here and talk about
  them"): click toggles a face, shift-click adds, ctrl-click removes; alt-drag or the **Box** tool selects the
  faces whose centre is in the box (plain replaces, shift adds, ctrl removes; with "only faces I can see",
  front-facing and not hidden behind the mesh); **connected same colour** makes a click take the joined faces of
  its colour; Select none (`Esc`), Invert. Selected faces are tinted pink with a yellow outline; the hovered face is
  outlined cyan and the corner shows its id, part, colour, vertices, height and radius. **Numbering:** a face id is
  the triangle's index in the level's GLB primitive (`detail_k`, primitive 0: indices 3t..3t+2), so each level
  (100/60/35/20 %, the Level picker) numbers its own faces from 0 and keeps its own selection.
- **Copy face ids** (`Ctrl+C`) writes `peasant @60% faces: 1203,1204,1210–1215 (8) — parts: shirt 6, belt 2 —
  colours: wool_green 6, leather 2` (runs of three or more as ranges, the count, the faces' parts and palette
  ramps) to the clipboard and the text box under it (select it by hand when the clipboard is blocked, as on the
  published link). **Load selection** reads that text back (or a bare list like `4, 6-8`), switching to its
  level. **Band from selection** prints a decor `band` entry (`models.json` `decor`) whose `y`, `r` and `span`
  cover the selection; decor picks facets by position, so check the render.
- **Tested:** the page's pure core (`<script id="core">`: the GLB reader, islands, rows, the check, the export,
  the level mapping, the recolour, and the face ids' ranges, the copied text's round trip, the box select's
  centre and winding test, the hidden test, the connected-colour fill and the band) is run by
  `node --test shared/models/painter.test.mjs` against `parts.mjs` on the Peasant (the export byte for byte).
  `window.painter` drives it from a script (`setEdit("faces")`, `selectFaces`, `boxSelect`, `selectionText`,
  `loadSelection`, the camera's `zoomAt`, `panBy`, `orbitBy`, `focusAt`).

## Updating them

Edit the file here and republish it to the same link, so the URL stays the same (in Claude Code:
"publish `docs/prototypes/<file>` to <link>"; the Artifact tool updates an existing artifact by its
URL after reading it). Commit the file with the change.

## Character Lab

- **Generated, not hand-edited, in part:** `node shared/characters/gen.mjs` writes the character data
  (`shared/characters/characters.json`), the shared palette and materials, the rig module
  (`rig.mjs`) and the painter (`shared/textures/paint.mjs`) into the page between its generated
  markers; CI fails if the page is stale. Edit the rest of the page by hand.
- **Subjects:** the player, first person (a small world, the hotbar, the arms drawn in their own pass
  and FOV), the wolf, the deer, and a group to scale with 1 m blocks. Orbit, front, back, side and ¾
  views, a turntable.
- **Animation:** the clips by role (the player's idle, walk, sprint, crouch, crouch-walk, slide, jump,
  fall, land, climb, swim, fly; the arms' idle, walk, sprint, slide, jump, fall, land; the wolf's trot;
  the deer's graze), play, scrub and speed; break and place swings (<kbd>B</kbd>, <kbd>P</kbd>) layered
  over the base and the hold. **Transitions:** presets (walk → sprint → slide; jump, fall, land; walk →
  crouch; climb, swim, fly) or a sequence built clip by clip, cross-faded by each clip's `blend`, the
  jump's rise drawn as the controller's 1.25 m arc.
- **Looks:** variants (wanderer, ranger, tinker; stag, doe), accessories by place (one per place),
  colour slots from the shared materials, the held item (block or brush) and its block.
- **Parts:** the list of the character's, its accessories' and the held item's parts; click a part
  in the view to select it; edit every field live, duplicate, delete, add. Validated by `rig.mjs` as
  you go.
- **View options:** the silhouette check (flat ink), brush strokes on or off, the ink outline,
  bones and sockets, the game's tone mapping and exposure.
- **Copy characters.json** copies the whole file with the edits (or one character), to paste over
  the data file and run the generator. `window.lab` drives it from a script (subject, angle,
  variant, held item, clip time, a swing, a sequence, the camera) for screenshots.
