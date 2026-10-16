# Brushes: painting and sculpting in the sandbox

Status: **built** (Phase 34, [ADR 0037](./adr/0037-brushes.md)). The owner hasn't tried it yet.
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
  | `splatter` | A ball with a seeded `density` of its cells (the `seed` and the centre pick them, so the preview is the result) |
  | `line` | From the previous click to this one, size cells thick; it chains |

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

  Sculpting never puts down a block that is unbreakable or has no palette slot: it uses the
  commonest such neighbour, or failing that the hotbar's block.

- A **mask**, set with the `mask` parameter of set, erase and overlay, limits which cells change.
  It can be `none`, `air`, `solid` or `target` (the replace target). Paired with a material, a mask
  gives VoxelSniper's "combo".

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
- The current presets cover the brief's tools:
  - shapes: ball, disc, cylinder, cube, ellipsoid, splatter, line;
  - painting: ball (air only), erase, replace, paint surface;
  - sculpting: smooth, erode, melt, fill/inflate, flatten, raise, lower.

  They are prototype content (`ARCHITECTURE.md` §6.1) for the owner to tune.

## 3. In the game

- **Where.** A world type whose policy has `"brushes": true` gets brushes; today that is only
  `sandbox`. The server announces them with the `brushes` Welcome flag. The client offers the tool
  only then: V does nothing in other worlds.
- **Aim.** The brush aims along the view ray up to the world type's `reach` (128 m in the sandbox),
  not just the 5 m block reach. A wireframe shows at the target, drawn over everything like the
  region boxes, in orange:
  - ball and splatter: three great circles;
  - ellipsoid: its three ellipses;
  - disc and cylinder: two circles and four uprights;
  - cube: a box;
  - line: the segment and its ends.
- **Clicks**, after VoxelSniper's arrow and gunpowder:
  - Left click centres the stroke on the aimed-at block.
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
  | The data's `key` (U, I, J, K, O, L, Y) | Pick that preset |

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
   `brushes.json` enough?
2. Should brushes come to other world types, such as creative terrain worlds? If so, with what
   permission (ops only?) and what reach?
3. Is 128 m the right reach, and is 16 the right maximum radius? A size-16 stroke is about 36,000
   cells, half a second of the server's rate.
4. VoxelSniper offers more brushes: blend, linear blend, overlay variants, stencils and tree
   brushes. Which of them, if any, next?
5. Should strokes be predicted, shown before the server's echo? Today a large stroke appears about
   a round trip later.
6. Should touch screens get brushes? They have none today.
7. Should a ghost of the actual cells replace the wireframe preview? It costs more lines; splatter
   would show its exact cells.
