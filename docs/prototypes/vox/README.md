# Voxel models (prototype)

The player, the wolf and the deer from [`shared/characters/characters.json`](../../../shared/characters/characters.json)
as MagicaVoxel `.vox` files, for the owner and artists to open, look at and refine. A prototype: nothing
in the game reads these files yet ([`CHARACTERS.md`](../../CHARACTERS.md) and ADR 0041 still draw the
characters as smooth parts).

| File | Voxels / m | Parts | Voxels | Preview |
|---|---|---|---|---|
| `player_16.vox` | 16 | 53 | 973 | [`player_16.png`](./player_16.png) |
| `player_32.vox` | 32 | 55 | 7474 | [`player_32.png`](./player_32.png) |
| `wolf_16.vox` | 16 | 31 | 528 | [`wolf_16.png`](./wolf_16.png) |
| `wolf_32.vox` | 32 | 33 | 4316 | [`wolf_32.png`](./wolf_32.png) |
| `deer_16.vox` | 16 | 42 | 477 | [`deer_16.png`](./deer_16.png) |
| `deer_32.vox` | 32 | 42 | 3920 | [`deer_32.png`](./deer_32.png) |

Each preview is the model from the front right and from the back left (an isometric software render
of the file as read back, not MagicaVoxel's renderer). [`manifest.json`](./manifest.json) lists every
file's parts (with bone and material) and its palette (index → material and ramp stop).

## Making them

```sh
node docs/prototypes/vox/vox.mjs            # writes the .vox, .png and manifest.json here
node docs/prototypes/vox/vox.mjs . /tmp/x   # also copies the previews to /tmp/x
```

[`vox.mjs`](./vox.mjs) has no dependencies. It uses the rig module (`shared/characters/rig.mjs`):
`validate`, `compose` (the character's default look: the player's hat, backpack, bedroll and gloves;
the deer's antlers), and `pivots` for the rest pose (every bone at its rest offset, no turn). Then:

1. **Voxelize.** Every part's shape (`box`, `round_box` with its corners rounded by `round` × the
   smallest side, `ellipsoid`, `cylinder`, `cone` with its `tip`, `capsule`), as the Character Lab builds
   them, is tested at each voxel's centre. A part thinner than a voxel is grown to one voxel on that side.
   First-person-only parts (`view: "arms"`) are left out.
2. **One owner per voxel.** Where parts overlap, the smaller part wins the voxel, so details (eyes,
   straps, the buckle) carve the bulk they sit on rather than hiding inside it. A part left with no
   voxel takes back the one at its centre, unless that empties a part listed before it.
3. **Colour.** The palette is built from the materials used: each material's ramp from
   `shared/textures/palette.json` (via the materials file) as three entries, shadow, base and light
   (a swatch or a single ramp stop is one entry). A voxel open to the air above takes the light, one open
   below the shadow, the rest the base: a simple lit top and darker underside, for an artist to repaint.
   27 colours for the player, 8 for the wolf, 13 for the deer.
4. **Write** MagicaVoxel format 150 (what MagicaVoxel 0.99.x writes): a `SIZE` + `XYZI` model per part,
   the scene graph (`nTRN`/`nGRP`/`nSHP`), one layer `body` (`LAYR`) and the palette (`RGBA`).
5. **Read back and check.** The script parses each file it wrote with its own reader (`readVox`, the part
   a future importer needs) and fails unless every part comes back by name, on its bone, with the same
   voxels and colours. All six files also load with the Rust [`dot_vox`](https://crates.io/crates/dot_vox)
   crate (5.x, checked by hand in a scratch project: version 150, the models, the scene nodes, the layer
   and the names all read).

## How the files map to the rig

- **Axes.** The game is +y up with the character facing +z (its left at +x). MagicaVoxel is +z up, so a
  game point (x, y, z) is the voxel point (x, −z, y) × voxels per metre: the character faces −y, its
  feet on z = 0, centred on x = 0.
- **Bones are groups.** The scene follows the rig's bone tree: each bone is a transform named by the bone
  (`_name` = `hips`, `head`, `front_upper_l`, …) at its rest pivot relative to its parent bone, over a group
  holding its parts and its child bones. Bones with no parts anywhere below are left out.
- **Parts are models.** Each part is its own model under a transform named by the part's `id` (`head`,
  `eye_l`, `brim`, `antler_beam_r`, …), so each is selectable and movable in MagicaVoxel. Mirrored parts
  are written as both sides (`_l` and `_r`). A bone and a part can share a name (the bone `head` and the
  part `head`): a transform over a group is a bone, over a shape a part.
- **Placement.** MagicaVoxel puts a model's voxel i at its transform + i − floor(size / 2); the writer
  sets each part's transform so its voxels land where the part is, relative to its bone's pivot.

## Resolution: 16 or 32 voxels per metre

- **16 / m (6.25 cm voxels)** is chunky and fits a 1 m block world's scale (a block is 16 voxels). Faces
  are a few voxels: the player's eyes are one voxel each, and the brows share that voxel and are lost
  (53 of 55 parts); the wolf's legs are two voxels thick. It reads as a toy, and is quick to paint.
- **32 / m (3.1 cm voxels)** keeps every part (the player's brows, the deer's antler tines), the round
  corners and the hat's brim read as curves, and the faces have room for expression. About 8× the voxels
  (7.5k for the player against 1k), still tiny for a GPU or a file.
- A middle ground (24 / m) or 32 / m for heads and hands only is a choice for the art direction, not the
  format: the script takes any resolution.

## How a future importer would read edited files back

An artist may repaint, reshape, add or delete voxels inside a part's model, and move a part. The
importer (not built) would:

1. Walk the scene from the root as `readVox` does, keeping the bone name from each group's transform and
   each part's name from its shape's transform.
2. Match **part names to part ids** in `characters.json` (or a new character's list), and **group names
   to bones** of the rig. An unknown name or a part under the wrong bone fails with a message naming it,
   as the rig's validation does; a part the artist deleted is reported, not silently dropped.
3. Take each part's voxels relative to its bone's pivot (÷ voxels per metre, back through the axis turn)
   as the part's shape: a voxel shape kind beside `round_box` and the rest, stored in a data file the
   generator emits for the game, so the rig, its clips and its sockets keep working unchanged.
4. Map palette entries back to materials (the manifest records which index is which material's stop) or
   keep the artist's colours as the part's own palette.

MagicaVoxel turns models only by 90°, so the rest pose is the pose to edit in; animation stays the rig's
clips. Moving a group in MagicaVoxel moves a bone's pivot, which the importer would treat as a change to
the rig's rest offsets and should report rather than apply silently.

## What looks good and what doesn't

- Good: all three read at once as the Character Lab's characters. At 32 / m the player's hat, backpack,
  bedroll and boots and the deer's antlers and white tail come through well; the parts' outlines stay
  clean because each voxel has one owner.
- Rough: at 16 / m the faces barely exist (one-voxel eyes, no brows), and thin pieces (legs, the wolf's
  ears, antler tines) become one- or two-voxel sticks that an artist would thicken. The automatic lit top
  and dark underside make terraced stripes on curved parts (the wolf's back, the hat's crown); an artist
  would repaint the shading. Overlaps the smooth parts hide (the deer's belly between the legs) show as
  odd seams in voxels.
