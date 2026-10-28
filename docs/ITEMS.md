# Items and placement rules **[built, 0.29.0]**

Owner (2026-10-26): "I want to build in 1m blocks by default in creative, but we're going to be able
to build in half blocks too, it's just a different piece (item). We'll also want to setup rules for
block overrides like placing a block on tall grass removes it, blocks can't be placed on top of
slopes, ladders can only be placed attaching to a block face, destroying the attaching block destroys
the ladder too. … some blocks attach (grass downward, ladders sideways), some are only placeable on
some blocks, some can't be placed on slopes (likely plants are going to be the only thing that can
be). If you're using the same natural moldable materials (dirt, stone(?), sand) as the one you're
building on, it will replace any slope with the block you're building, not rejecting your build."

Chosen with the owner: items in a data file; a 1 m piece lines up against the face aimed at; the ½ m
pieces are their own hotbar slots; the moldable groups are dirt (dirt, grass and snowy grass — "really
all just modified dirt") and sand.

## 1. Items (data)

`shared/items/items.json` → `shared/items/gen.mjs` → `dwell-game` `items.gen.rs` (validated against the
block registry, CI stale check). An item names the block it places and its height in ½ m cells
(`rows`: 2 a 1 m block, 1 a ½ m one). The hotbar is the items in order (`block_interaction::palette`);
a ½ m piece of a full block shows its swatch's lower half. A block that brings its own other cells
(a torch, `extends`) is a 1-cell item. Adding a piece is a new entry.

## 2. Pieces on the wire

`BlockEditRequest` carries the piece (protocol 19): `rows` (1–`MAX_PIECE_ROWS` 2) and `down`. A place
fills `rows` cells from the placed cell: away from a top or bottom face; against a side, up toward the
face's sky, or down with `down` (the client sets it when the aim is in the lower half of the cell, so
the piece is centred on the aim). A break takes the one cell aimed at, whatever the held item (owner,
2026-10-28: the 1 m break, which took a 1 m piece's second cell too, is gone; the server ignores a
break's `rows`). `EditOutcome::also` is a place's second cell;
`EditOutcome::writes` (`edit_writes_all`) makes every write, on the server and in the client's
prediction alike.

## 3. Placement rules (block data, `shared/blocks/dwell.json`)

| Field | Meaning |
|---|---|
| `replaceable` | Placing into or against it replaces it (tall grass) — already built |
| `placeOn` | The blocks it may stand on — already built (tall grass: grass, dirt and their ramps) |
| `needsSupport` | `below`: stands on the cell under it and goes with it (tall grass, torch); `behind` (new): hangs on the cell opposite its facing, a full solid block, placed only against that block's side, and goes with it (ladder; each cell of a 1 m ladder needs its block) |
| `onSlopes` (new) | It may be placed on a slope's top (tall grass); nothing else may (`EditCheck::OnSlope`) |
| `moldable` (new) | Its natural material group (`dirt`: dirt, grass, snowy grass; `sand`); a ramp takes its block's. Building with a block of the slope's group onto the slope replaces the slope with the block, not refused |

The engine is `dwell_world::edit::check_block_edit`; the registry sync carries the new fields
(`flag::ON_SLOPES`, `tag::MOLDABLE`, support value 2 `behind`), so a client maps them by name.

## 4. Tests

`dwell-world` `tests/placement.rs` (both faces): pieces of one or two cells against the face, up or
down a side, refused when the second cell is taken, written together; slopes take plants only and are
molded by their own group; ladders hang on a block's side (not a top, not a ramp; a 1 m ladder needs
both blocks) and go with their block; a break takes one cell whatever the piece. `tests/world.rs`: tall grass on
a grass or dirt slope. `dwell-game`: the palette is the items, 1 m by default. `shared/items/gen.test.mjs`:
the data's validation; an item added by data alone. Protocol vectors: the piece byte.

## 5. Drag building **[built, 0.38.4]**

Owner (2026-10-28): "When building you can build quickly in straight lines, if you place and hold
place on any side and then move or visually drag, it'll keep building in that direction (sort of
locked in that direction). Perhaps have a toggle to a flat surface version of that too where it's
locked on the Y axis, only building at that level (easy floors or ceilings)." Chosen with the owner:
the line runs straight out of the face clicked; flat paints the cells under the crosshair; a key, a
controller button and a touch button switch the modes; mouse, controller and touch all build on.

- **Data** (`shared/items/items.json` `buildModes`, the same generator: `items.gen.rs`
  `BUILD_MODES`, `BUILD_MODE_KEY`, and `client/src/ui/buildModes.gen.ts` for the controls list and
  the HUD): the key that cycles the modes (`KeyL`) and the modes in order, the first the default,
  each a name, a label and a kind. Adding a mode of a known kind is a new entry.
- **Kinds** (the engine's vocabulary, `dwell-game` `build_drag.rs`): `line` — from the first piece,
  straight along the clicked face's direction, up to the point of that line nearest the aim (the
  nearest points of the line and the view ray), a piece at a time (a cell across, the piece's height
  up or down), stopping at the first piece that doesn't fit; `level` — the cells where the view ray
  crosses the first piece's mid-height plane, joined to the last frame's by a 4-connected walk so a
  fast sweep leaves no gap, each filled once. Within `REACH_DISTANCE` of the eye; only free cells
  (air, a replaceable plant, water, not the player's own body), so nothing is overwritten.
- **Holding**: the first block is the ordinary place; a stroke starts from the cells it filled and
  lasts until whoever started it lets go: the right button (`mouseUp`, or the window losing focus or
  the pointer lock), the controller's place button (`Gamepad::down`), or, on touch in Place mode, a
  still press on the view held `HOLD_MS` (350 ms), which then turns the view as it drags and is no tap
  when it lifts. Switching: `L`, the controller's `buildMode` (D-pad right, `shared/input/gamepad.json`),
  the touch Build button; the mode shows beside the block's name over the hotbar.
- **On the wire**: the pieces after the first are `RegionEdit` Fills of one piece each (no protocol
  change). A `BlockEditRequest` is refused against a face the eye can't see (line of sight), which a
  floor reaching away or a line past the eye needs; region edits are checked for reach and the world
  only. They go out at half the server's region queue a tick (`MAX_QUEUED` 4 → 2 a tick), predicted
  at once. Only plain blocks drag (`build_drag::drags`): a region fill runs no place rules, so a
  ladder (facing, support), a torch (other cells) or tall grass (`placeOn`) is placed a click at a time.
- **Tests**: `build_drag.rs` (both faces: a line up and across, a level's swept cells around a block,
  each once; the pacing; plain blocks only; the holder), `input.rs` (the key while playing, on no
  other key), `touch.rs` (the hold), `gen.test.mjs` (the data, a mode added by data alone), Vitest
  (the controls list), `e2e/buildDrag.spec.ts` (a line up a top face, stopping when let go; a flat
  sweep at the level only).

## 6. Not yet

The held item in the hand shows a whole block for a ½ m piece; the region tools and brushes still
work in cells; no survival inventory (creative only).
