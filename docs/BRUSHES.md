# Brushes: painting and sculpting in the sandbox

Status: **built** (Phase 34, [ADR 0037](./adr/0037-brushes.md)). The owner answered §5 on
2026-10-19; the follow-ups (the rest of VoxelSniper's brushes, §7) are built. The owner hasn't tried
the brushes yet.
Architecture: [`ARCHITECTURE.md`](./ARCHITECTURE.md) §6.5 (edits), §10.5 (additions within a line).

The owner's brief (2026-10-15): *"add painting tooling to our playground like Voxel Sniper tools"*
([VoxelSniper Reimagined](https://github.com/KevinDaGame/VoxelSniper-Reimagined)). The playground is
the `sandbox` world type (`?new=1&type=sandbox`, 0.11.0), where structures are built
(`STRUCTURES.md`). VoxelSniper served only as a reference for its vocabulary. None of its code was
used: it is LGPL, and Dwell's engine is different.

## 1. The model

VoxelSniper splits a tool into a **brush** and a **performer**. Dwell keeps that split:

- A **brush** is the set of cells a stroke covers, centred on a cell. Each brush has a size, which is
  a radius in cells (1–16).

  | Brush | Cells |
  |---|---|
  | `ball` | Within size + ½ of the centre |
  | `disc` | A horizontal disc one cell thick |
  | `cylinder` | A disc `height` cells tall, standing on the centre |
  | `cube` | Side 2·size + 1 |
  | `ellipsoid` | A ball stretched by `stretch` [x, y, z] |
  | `splatter` | A ball with a seeded `density` of its cells (the `seed` and the centre pick them, so the preview is the result); ½ when its preset gives no density |
  | `line` | From the previous click to this one, size cells thick; it chains |
  | `square` | A horizontal square one cell thick (VoxelSniper's voxel disc) |
  | `disc_face` | A disc across the aimed-at face: flat on a floor, upright on a wall |
  | `square_face` | A square across the aimed-at face (voxel disc face) |
  | `ellipse` | An ellipse across the aimed-at face, stretched by `stretch` along the face's two axes |
  | `ring` | A horizontal ring `width` cells wide at the disc's edge |
  | `triangle` | A filled triangle through the last two clicks and this one, size cells thick |
  | `jagged_line` | A line from the previous click that wanders by up to `jitter` cells (seeded); it chains |
  | `blob` | A lump grown from the centre within the ball, each of size steps taking a neighbour with chance `growth` (seeded) |
  | `dome` | The upper half of an ellipsoid (`stretch`), standing on the centre |
  | `snipe` | The aimed-at cell alone |
  | `stencil` | A structure from `shared/structures` (STRUCTURES.md), its anchor on the centre: one of the preset's `structures`, turned and varied by its own variants, picked by the seed and the centre (VoxelSniper's stencil and stencil list) |
  | `tree` | A procedural tree of the `wood` and `leaves` blocks: a trunk `height` tall and `thickness` wide, `branches` limbs `branchLength` long rising from its upper half, a ragged crown of size radius at the top and smaller ones at the limbs' ends, and `roots` roots (seeded) |

  Any brush also takes `density` (a seeded share of its cells, VoxelSniper's splatter variants) and
  `checker` (only the cells of a 3D checkerboard, VoxelSniper's checker voxel disc). The stencil
  and the tree bring their own blocks; the shape key (Z) skips them, since only their preset's data
  names those blocks.

- A **performer** is what happens to the brush's cells. Each performer reads the world inside the
  brush and next to it.

  | Performer | What it does |
  |---|---|
  | `set` | Puts the hotbar's block in every cell the mask lets through |
  | `replace` | Puts the hotbar's block in cells that hold the replace target |
  | `overlay` | Paints the top `depth` layers of solid ground, the cells with open space above |
  | `erase` | Turns cells to air |
  | `smooth` | A majority filter, `iterations` times: a cell becomes solid when 14 or more of its 26 neighbours are, and air when 12 or fewer are |
  | `erode` | VoxelSniper's erosion. Solid cells with at least `erodeFaces` open faces become air (`erodeIterations` times). Then open cells with at least `fillFaces` solid faces fill (`fillIterations` times). Erode, melt and fill/inflate are presets of it |
  | `flatten` | Cuts the columns under the brush to the target height and fills them up to it, `height` cells either way |
  | `raise` | Grows the columns by `height` cells of their top block; with `falloff`, less toward the edge, which makes a mound |
  | `lower` | Cuts the columns down by `height` cells |
  | `blend` | VoxelSniper's blend: each cell takes the commonest state of its 26 neighbours (the 8 in its plane for a flat brush: disc, square, ring) when one state leads, `iterations` times. Air and liquids count, unless `excludeAir` / `excludeWater` |
  | `linear_blend` | A blend that fades out toward the edge: a cell is blended with a chance of 1 − its distance from the centre over the radius (seeded) |
  | `underlay` | Paints the bottom `depth` layers of solid ground, the cells with open space below |
  | `hollow` | VoxelSniper's shell brushes: solid cells further than `depth` cells from open space become air, leaving a shell `depth` thick |
  | `fill_down` | Fills each column from the brush down to the first solid block, at most `height` cells |
  | `extrude` | Pushes solid cells out along the aimed-at face's normal: up to `height` copies of each cell's block in the open cells in front of it |
  | `stamp` | Puts the brush's own blocks (a stencil's, a tree's) where the mask lets them |

  `overlay` with `onTop` puts `depth` new layers on the ground, in the open cells above it, rather
  than painting its top (VoxelSniper's overlay that adds rather than replaces).

  Sculpting never puts down a block that is unbreakable or has no palette slot: it uses the
  commonest such neighbour, or failing that the hotbar's block.

- A **mask**, set with the `mask` parameter of set, erase, overlay, underlay, the blends and stamp,
  limits which cells change. It can be `none`, `air`, `solid`, `target` (the replace target;
  VoxelSniper's include), `not_target` (all but the replace target; its exclude) or `liquid` (an
  erase with it is VoxelSniper's drain). Paired with a material, a mask gives VoxelSniper's
  "combo". A stamp with the `air` mask is a stencil's fill mode, with `solid` its replace mode.

Brushes and performers are a small, fixed vocabulary in code (`dwell-world` `brush.rs`). A stroke is
a pure function of the brush, the aim and a grid of the world's states: `brush_cells`, then
`perform`, then `paste_edits`. Unbreakable cells are never changed.

## 2. Data: `shared/brushes/brushes.json`

The **presets** players pick, and which **world types** allow brushes, are data:

```json
{
  "formatVersion": 1,
  "sizes": { "min": 1, "max": 16 },
  "worldTypes": { "sandbox": { "brushes": true, "reach": 128 } },
  "presets": [
    { "name": "smooth", "label": "Smooth", "brush": "ball", "performer": "smooth", "size": 4,
      "key": "KeyO", "params": { "iterations": 2 } }
  ]
}
```

- `shared/brushes/gen.mjs` validates the file and writes two outputs:
  - `crates/dwell-world/src/brushes.gen.rs`, the presets and the world policies;
  - `client/src/ui/brushes.gen.ts`, for the controls table and the HUD.

  CI regenerates both and fails on a diff, in the protocol job.
- A bad entry fails with a message naming it. The checks cover:
  - an unknown brush, performer, field or parameter;
  - a parameter the preset's brush and performer don't take;
  - an out-of-range value;
  - a duplicate name;
  - a key that isn't free, or that two presets share;
  - a world type that doesn't exist;
  - a reach outside 1–256 m.
- `client/src/ui/brushesGen.test.ts` runs the generator over fixture data, adds a preset by data
  alone and checks each validation. In `dwell-world` `brush.rs`, a fixture preset strokes, and
  every shipped preset strokes.
- The current presets cover the brief's tools and the rest of VoxelSniper's (§7):
  - shapes: ball, disc, cylinder, cube, ellipsoid, splatter, line, square, disc and square on the
    face, ellipse, ring, triangle, jagged line, blob, dome, snipe, splatter disc, checker square;
  - painting: ball (air only), erase, eraser (a cube), drain, replace, set but not the target,
    paint surface, cover surface, splatter surface, paint underside;
  - sculpting: smooth, erode, melt, fill/inflate, flatten, raise, lower, blend (ball, disc, cube,
    square, and one that never blends to air), linear blend, hollow (shell), fill down, extrude;
  - stamps: a tree stencil, a stencil list (a rock or a tree), a generated tree. The stencils use
    two small prototype structures added for them, `dwell:proto_tree` and `dwell:proto_rock`.
- Parameters that name things are checked against the data they name: a stencil's `structures`
  against `shared/structures/*.json`, a tree's `wood` and `leaves` against the block registry
  (`shared/blocks/vectors.txt`, through the structures' `format.mjs`). A stamp needs a stencil or a
  tree, and a stencil needs structures.

  They are prototype content (`ARCHITECTURE.md` §6.1) for the owner to tune.

## 3. In the game

- **Where.** A world type whose policy has `"brushes": true` gets brushes; today that is only
  `sandbox`. The server announces them with the `brushes` Welcome flag. The client offers the tool
  only then: V does nothing in other worlds.
- **Aim.** The brush aims along the view ray up to the world type's `reach` (128 m in the sandbox),
  not just the 5 m block reach.
- **Preview** (owner, 2026-10-19: the exact cells, in place of the wireframe). At the target, a
  see-through **ghost of the exact cells the stroke would change** shows: the performer is run on
  the client's world, so a splatter shows its real cells, a smooth only the cells it would move,
  and an erase only the blocks there are. Its colour says what a cell becomes: orange for a block
  put down, red for a block taken away, pale yellow for one block painted over another. It is drawn
  over the composite, at a third of its opacity where the terrain is in front of it, so cells inside
  the ground still show. A line or triangle also shows its clicks so far joined to the aim.
  - **Shown by a toggle**, `\`, on by default. Off, the box of the brush's cells shows instead (in
    orange lines, over everything). A held key was the alternative; a toggle was chosen because
    the ghost is what you aim by, and holding a key while clicking and steering is awkward.
  - **Bounded cost.** The ghost has only the changed cells' outer faces (a face between two cells
    of the same colour is left out), a little proud of the cells, at most `GHOST_FACE_CAP` (16,384)
    of them; a stroke whose ghost would have more shows the box of its cells. A size-16 ball's ghost
    is about 7,000 faces; a dense size-16 splatter's is over the cap. It is rebuilt when the aim,
    the brush or the blocks change, and otherwise every `PREVIEW_REFRESH_MS` (250 ms) for changes in
    the world under it; the renderer uploads it only when the picture changed (its `version`).
- **Clicks**, after VoxelSniper's arrow and gunpowder:
  - Left click centres the stroke on the aimed-at block. A line's first click sets its start; a
    triangle takes two clicks before the one that strokes, then starts again.
  - Right click centres it on the cell in front of the aimed-at face.
  - A middle click or Alt (⌥) click picks the aimed-at block as the replace target. Without a pick,
    replace uses the aimed-at block.
- **Keys** (the controls table's *Brushes* group). These keys are live only while the tool is on,
  except V. None of them was bound before.

  | Key | Action |
  |---|---|
  | V | Tool on or off |
  | Q / E | Previous / next preset |
  | Z / Shift+Z | Next / previous brush shape |
  | P / Shift+P | Next / previous performer |
  | [ / ] or Alt+wheel | Size |
  | The data's `key` (U, I, J, K, O, L, Y, `;`, `'`, `,`) | Pick that preset |

  Gen limits a preset's `key` to a list of free keys.
- **HUD.** A panel above the hotbar, in the glass theme with an orange edge, shows:
  - the preset;
  - brush, performer, size, the block and, when it is used, the replace target;
  - the last message, or the stroke's progress ("Sending 3 of 8 edits");
  - the keys.
- **Undo.** A stroke is one step in the region tools' undo/redo stack (Ctrl/⌘+Z), however many edits
  it took.

## 4. On the wire: no new message, one additive extension

A stroke goes to the server as `RegionEdit` **pastes** of at most 32³ cells. `paste_edits` splits the
stroke's changed cells into 32-cell tiles and trims each tile to its cells. Every other cell in a
tile is **`KEEP`** (`region_edit::KEEP`, 0xFFFF): the server leaves that cell as it is.

The S3 paste couldn't express "leave this cell". A paste of the client's current states would have
been an alternative, but it fails twice:

- It puts down the sandbox's unbreakable floor, and the server rejects any paste that does.
- It would overwrite a change another player made in the meantime.

`KEEP` is a cell value in the existing layout, so the wire format doesn't change. The vector
`region_edit_paste_keep` pins it. Building on that:

- **Older servers.** An older server, with region edits but no brushes, rejects `KEEP` as an unknown
  state, so the paste fails safe. Clients send `KEEP` only to a server whose Welcome sets
  **`welcome_flags::BRUSHES`** (4). Decoders ignore unknown flag bits, as with `regionEdits`
  (§10.5), so 0.11 builds stay compatible both ways.
- **Ranged reach.** The server's region-edit reach becomes the world type's brush reach plus the
  usual 1 m slack: 129 m in the sandbox, 65 m elsewhere (`region_reach`). Brush pastes and
  region tools share it. It is data, per world type, so a dedicated server gets the policy of its
  generator.
- **Unchanged.** Validation is otherwise the same:
  - box size;
  - unbreakable cells;
  - players' capsules;
  - the build volume;
  - the edit permission;
  - the cell-rate bucket (65,536 cells/s, a burst of 131,072).

  The client mirrors the bucket and queues a stroke's edits, sending them as the rate allows. A new
  edit waits until the queue is empty. Nothing is predicted: the change shows when the
  VoxelModification arrives.
- **Undo over unbreakable cells.** With `KEEP` available, undo keeps unbreakable cells as `KEEP`.
  This also fixes S3's undo of a box over the sandbox floor, which the server used to reject.

## 5. Defaults taken, and questions for the owner

The design questions (CLAUDE.md, "ask before designing") weren't asked before building. These
defaults were taken instead (coordinator, 2026-10-15):

- **What gets added later:** more presets (data), and occasionally a new brush or performer (code
  plus a vocabulary entry in `gen.mjs`).
- **Who adds them:** the owner, by editing `brushes.json`. No editor page yet.
- **Fixed in code:** the vocabulary of brushes, performers and masks. **Data:** the presets, their
  parameters and keys, and which world types allow brushes and how far they reach.
- **Scope:** per build. The world policy follows the world type.

Questions for the owner:

1. Should players eventually save their own presets, per player in browser storage? Or is
   `brushes.json` enough? **Answered (owner, 2026-10-19): the built-in presets are enough.** No
   player presets.
2. Should brushes come to other world types, such as creative terrain worlds? If so, with what
   permission (ops only?) and what reach? **Answered (owner, 2026-10-19): yes, eventually, in
   creative.** Dwell has no game modes yet; adventure and creative modes are wanted (`FUTURE.md`,
   "Game modes"). Brushes come to terrain worlds in creative mode only. If excluding them by
   finer permission (ops only) is logically complicated, allowing them for everyone in creative is
   fine. Until game modes exist, brushes stay in the sandbox.
3. Is 128 m the right reach, and is 16 the right maximum radius? A size-16 stroke is about 36,000
   cells, half a second of the server's rate. **Answered (owner, 2026-10-19): yes, both are fine.**
4. VoxelSniper offers more brushes: blend, linear blend, overlay variants, stencils and tree
   brushes. Which of them, if any, next? **Answered (owner, 2026-10-19): support all of them.**
5. Should strokes be predicted, shown before the server's echo? Today a large stroke appears about
   a round trip later. **Answered (owner, 2026-10-19): yes.** Normal break/place edits aren't
   predicted either (ARCHITECTURE.md §5: the change shows when the server's delta arrives); the
   owner asked whether they are, so predicting them is a candidate alongside.
6. Should touch screens get brushes? They have none today. **Answered (owner, 2026-10-19): yes,
   but later and at low priority:** the game targets desktop for now, until it is heavily optimised.
7. Should a ghost of the actual cells replace the wireframe preview? It costs more lines; splatter
   would show its exact cells. **Answered (owner, 2026-10-19): yes, a preview of the exact cells**,
   shown by a toggle or while a key is held (to decide in its design).

## 6. Touch screens

Not now (owner, 2026-10-19: later and at low priority, since the game targets desktop until it is
heavily optimised). Touch screens have no brush tool: V, the clicks and the keys are desktop-only.
When it comes, it needs a way to toggle the tool, aim (the crosshair already works), pick a preset
and size, and tell arrow from gunpowder; none of the engine changes.

## 7. VoxelSniper's brushes and performers in Dwell

The owner asked for all of VoxelSniper's brushes (§5, question 4). Dwell's vocabulary was checked
against VoxelSniper Reimagined's brush and performer classes (its `brush` package, read for names
and behaviour only; no code was used). Added on 2026-10-19:

| VoxelSniper | In Dwell |
|---|---|
| Voxel disc, disc face, voxel disc face | `square`, `disc_face`, `square_face` |
| Ellipse | `ellipse` (filled, across the aimed-at face) |
| Ring | `ring` |
| Triangle | `triangle` (three clicks) |
| Jagged line | `jagged_line` |
| Blob | `blob` |
| Dome | `dome` |
| Snipe | `snipe` |
| Splatter disc, voxel, voxel disc, overlay | `density` on any brush (presets: splatter disc, splatter surface) |
| Checker voxel disc | `checker` on any brush (preset: checker square) |
| Shell ball, voxel, set | `hollow` on any brush |
| Blend ball, disc, voxel, voxel disc | `blend` (presets for each shape; flat brushes blend in their plane) |
| Linear blend | `linear_blend`. VoxelSniper Reimagined has no brush of that name; Dwell's is a blend that fades out linearly toward the edge |
| Overlay variants | `overlay` with `onTop` (cover), `density` (splatter overlay), `underlay` (reverse overlay) |
| Underlay | `underlay` |
| Fill down | `fill_down` |
| Extrude | `extrude` (outward only; erase takes cells back) |
| Eraser | the `eraser` preset (a cube of `erase`) |
| Drain | the `liquid` mask (preset: drain) |
| Stencil, stencil list (schematic) | `stencil` over the structures format, with the structure's own variants for random rotation and mirroring, a list picked by the seed, and full / fill / replace modes as the stamp's mask |
| Generate tree | `tree` |
| Tree snipe | a `stencil` of a tree structure (preset: tree stencil) |
| Performers: material, combo, material-material, include, exclude | `set` (a Dwell state is VoxelSniper's material and data together, its combo), `replace`, the masks `target` and `not_target` |

The tree brush doesn't reuse the tree-growth work (`TREE_GROWTH.md`): only its G0 prototype page
exists, with no Rust skeleton or stages to call. A grown-tree brush (a structure tree at a chosen
age or variant) can come with G1, as a new brush kind over `stage_voxels`.

Deliberately not added:

| VoxelSniper | Why not |
|---|---|
| Ink and no-physics performers, and the "list" include/exclude performers | Dwell's states have no separate data value (ink), and edits have no block-update physics to turn off; the server's support check is part of every edit. Lists of targets would be a new parameter naming blocks; the single picked target covers the common case |
| Set, copy-pasta, clone stamp, stamp, move, 2D and 3D rotation | The region tools (`ARCHITECTURE.md` §6.5) already select, copy, paste, rotate, mirror and move boxes |
| Pull, canyon, canyon selection, ocean, flat ocean, regenerate chunk, block reset | Raise, lower and flatten cover pulling terrain. The others work on whole chunks or put back generated terrain; the sandbox has only its flat floor to put back, and a stroke would need the generator's states. They can come with brushes in creative terrain worlds (§5, question 2) |
| Spline, three-point circle | Curves through a list of control points need a point-editing UI; the line, jagged line and triangle cover paths and planes. Each would be a new brush kind with the same points list |
| Biome, entity, entity removal, painting, sign overwrite, clean snow, Redstone, jockey, lightning, comet, warp | Minecraft features Dwell doesn't have (biomes as data, entities, paintings, signs, snow layers, redstone, mobs, weather, projectiles, teleport) |
| Ruler, scanner, voltmeter | Measuring tools, not edits; the F3 overlay shows positions |

