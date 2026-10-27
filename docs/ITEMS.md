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
the piece is centred on the aim). A break takes as much as the held item is tall: the next cell into
the block too, if it holds something breakable. `EditOutcome::also` is the second cell;
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
both blocks) and go with their block; breaking takes a piece's height. `tests/world.rs`: tall grass on
a grass or dirt slope. `dwell-game`: the palette is the items, 1 m by default. `shared/items/gen.test.mjs`:
the data's validation; an item added by data alone. Protocol vectors: the piece byte.

## 5. Not yet

The held item in the hand shows a whole block for a ½ m piece; the region tools and brushes still
work in cells; no survival inventory (creative only).
