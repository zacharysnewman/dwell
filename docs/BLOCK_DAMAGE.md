# Block damage: pickaxe shards, shovel scoops

**Status:** adopted ([ADR 0078](adr/0078-block-damage.md), 2026-11-10); D1 and D2 built (0.52.28, §5–6),
the marks carved into the mesh and the pieces shaped as them (§6.2),
D3 (tuning) to come. The owner's direction and answers are §1; §2 was the proposal's sketch,
§5 is what D1 built.

## 1. The owner's direction

- "Tools will do damage to blocks, blocks will retain damage until destroyed or repaired (repair not
  planned for soon)."
- "I want pickaxes to break shards off of the struck stone. Shovels will scoop roundish chunks out of
  dirt/sand/etc."
- Answers (2026-11-10):
  1. **Look only:** the shards and scoops are drawn on the block; its collision stays the full block
     until it breaks (unlike the axe's carved log notches).
  2. **Drops:** for wood, stone and dirt, each hit drops some of the resource, and the destroy drops the
     other half ("drop some of the resource on [hit], on destroy drop the other half of the chunks").
  3. **Hits: tiered**, as data: the right tool at stone tier about 5 hits, metal about 3, the wrong tool or
     bare hands about 10–12; bare hands can't break stone.
  4. **Same in both modes:** creative damages blocks over several hits too. Revised (owner,
     2026-11-11): "I should be able to break any block while holding anything or nothing with the
     exception of tools": in creative only a tool's hits count; anything else breaks in one hit (§7).

## 2. Design sketch

- **Per block, per tool, with a default** (owner, 2026-11-10: "Blocks will need per tool and default
  values"): each block (in its block data, `shared/blocks/*.json`, or a material it names) carries a
  `damage` entry: `default` (hits to destroy with bare hands or any tool not listed; `null` = can't)
  and per tool (by tool class or by item: `pickaxe`, `shovel`, `axe`, or `dwell:metal_pickaxe`) its
  hits, an item entry overriding its class's. The tier speeds in `tools.json` only fill in where a
  block gives a class but not the item. Validation: an unknown tool or class, a bad count, a block
  with no default. A tool for the block or item to come (a mod's) adds by data alone.

- **Data** (`shared/tools/tools.json` `breaking`, 0.52.21): per block material a toughness (hits at
  1×), the tool class that suits it and each tier's speed; per material a **damage look** (`shards`
  for stone, `scoops` for soils, `chips` for wood, …) and its **hit drops** (what a hit flings, and the
  share of the block's drops: half over the hits, half on the destroy).
- **State:** a block's damage is a small per-cell record: hits taken (or 0..1 health) and up to N hit
  marks (the struck face and point, for the look). Held by the server, sent to clients in the chunk
  and as an edit, saved with the world (a new optional chunk section: worlds without it load
  undamaged; older builds on the line ignore it if it can be an optional tag, otherwise a new
  compatibility line; to be decided in the ADR).
- **Look:** at each hit mark the face is drawn with a shard knocked off (stone: an angular facet dent
  with a fresher, lighter rock colour) or a roundish scoop (soils: a shallow bowl), in the mesher as
  extra geometry or in the shader as a dent/normal decal; collision unchanged.
- **Feedback:** the hit's particles (the material's chips) and its recorded sound (0.52.15); the drop
  flies out as a spinning ground item that auto-picks up (0.50.68).
- **Tools:** pickaxe on stone, shovel on dirt/sand/gravel/snow, axe on wood (the axe's felling keeps
  its notches on logs; other wood, e.g. planks, takes chips).

## 3. Phases (≤ 45 min each)

- **D1:** damage state + hits to destroy by tool class and tier (data), saved and synced; hit drops
  and the destroy's half; creative and adventure.
- **D2:** the looks: stone shards, soil scoops, wood chips at the struck points.
- **D3:** tuning with the owner; repair later.

## 4. Open

- The exact hit counts per material (tuning).
- ~~Whether bare hands can break stone at all.~~ Answered (owner, 2026-11-10): **bare hands can't break
  stone** (stone's `default` is `null`; it needs a pickaxe).

## 5. As built (D1, 0.52.28)

- **Data** (owner, 2026-11-10: "Blocks will need per tool and default values"): a block's `damage` in
  its block data (`shared/blocks/*.json`), `{ "default": hits | null, "<class or item>": hits, … }`.
  `default` is the hits with bare hands or any tool not listed (`null`: it can't be broken so); a key is
  a tool class of `shared/tools/tools.json` `breaking.classes` (`pickaxe`, `shovel`, `axe`) or an item
  (`dwell:metal_pickaxe`). Resolution (`dwell_protocol::damage::hits_to_break`): the held item's own
  entry; else its class's entry divided by the item's tier speed (`breaking.tiers`: stone 1.5, metal
  2.5), rounded up; else `default`. A block without `damage` breaks in one hit, as before.
  Prototype values: stone and its ramp `{ default: null, pickaxe: 7 }` (stone pickaxe 5, metal 3, by
  hand or another tool: can't); dirt, grass, snowy grass, sand and their ramps `{ default: 11, shovel: 7 }`
  (shovel 5, metal 3, by hand or another tool 11). Logs: *superseded by §11* (they take damage
  like the others since WORK_QUEUE item 51).
- **Generator and validation:** `shared/tools/gen.mjs` reads every block's `damage` (as it does
  `breakWith`), emits `BLOCK_DAMAGE` into `dwell_protocol::chopping`'s table and `client/src/ui/tools.gen.ts`,
  and fails on a missing `default`, a count outside 1..255, an unknown class or item, or a non-object
  (`gen.test.mjs`, with a fixture block by data alone; `damage.rs` tests a fixture block and tool table).
- **State** (`dwell_protocol::damage::Damage`): per cell hit and not yet broken, the state it was hit
  as (a cell holding another state is undamaged), the hits taken, and up to `MAX_HIT_MARKS` (4) hit
  marks (the struck face and a point, for D2). The point is the face's centre until the edit request
  carries the struck point (D2). A changed cell's damage is cleared (`broadcast_changes`, with its cuts).
- **Hits** (server, `tick_edits`; creative without a tool: §7): a Break of a block needing more than one hit by the held item
  (adventure: the held inventory slot; creative: the hotbar's tool, told by `ChopAction::Held`, which
  now carries pickaxes and shovels too) records a hit and leaves the block; the edit is acked as
  refused, so a predicted break gives way. The hit that reaches the count breaks it. A block that
  can't be broken so (stone by hand) is refused. Each hit costs one edit credit (the same rate limit,
  10 a second). Same in creative and adventure (owner).
- **Drops** (where blocks are not unlimited): of the block's certain drops (rules with chance 1 and one
  item, at their least count; a block without `drops` drops itself), half (rounded down) is spread over
  the hits before the last (`hit_share`: hit k of n gives ⌊k·h/(n−1)⌋ − ⌊(k−1)·h/(n−1)⌋), the rest and
  every chance drop on the destroy (`destroy_share`), so the hits and the destroy total the block's
  drops exactly. Today's blocks drop one item, so a hit drops nothing yet: the per-hit drops show once a
  block drops more (D3 tuning, the owner's call).
- **Client:** a Break the data (and the mode, §7) says takes one hit is predicted at once as before; one that takes more
  is not predicted (the server's hits so far aren't the client's), and its last hit shows when the
  server's change arrives.
- **Saved** with the world in its own optional record (`block_damage` table, `Damage::to_bytes`
  version 1): a world without it loads undamaged, an older build on the line never opens it, a newer
  record is kept unread, not overwritten. No new compatibility line.
- **Test switch:** `ServerConfig::one_hit_breaks` (the tests' `flat()` turns it on) breaks every block in
  one hit, for tests that break blocks with one click.

## 6. As built (D2, 0.52.28)

- **Sync** (optional, as the creatures'): a server keeping damage sets `inventory_flags::DAMAGE` in
  `Inventory`; a client that sees it sends `DamageHello` and is sent every damaged cell, then each change
  (`Damage`: per cell its fraction 0..255 and marks; and the cells gone). An older client never sees the
  bit and is never sent `Damage` (an unknown reliable message would close it); an older server never gets
  `DamageHello`. Rather than a section in the chunk message, the damaged cells come in `Damage` after the
  hello, as the axe's `Cuts` do (the chunk message has no optional sections).
- **The struck point:** a break request's optional three bytes (1/255ths of the cell, world-oriented),
  sent only to a DAMAGE server (an older one would read trailing bytes as malformed); the client finds it
  where the aim meets the targeted face. The server records it in the hit mark (the face centre without it).
- **The look:** `damage.look` in the block data: `kind` (`shards` | `scoops` | `chips`), `size` (a mark's
  width in cells at full damage), `color` (the fresh face inside), `crack` (the cracks); validated by
  `shared/tools/gen.mjs`. Stone: shards in a pale fresh rock tone; dirt, grass, snowy grass: dark damp
  scoops; sand: a darker sand scoop. The mesher (`mesher/damage_mesh.rs`) draws each mark as decals 4 mm
  off the face in the opaque mesh: a shard's uneven five-sided facet with a darker inner facet, a scoop's
  rim and darker middle, and past 20 % damage three crack wedges that grow with it, kept on the face.
  Face-local: a face-B chunk's cells, faces and points are turned over as its cuts are (`both_faces` test).
  Collision is unchanged.
- **Feedback:** a hit short of breaking plays the material's recorded `Hit` sound at the struck point
  (a one-hit break keeps its break sound).
- **e2e:** `edit.spec.ts`'s `breakBlock` and `menu.spec.ts` hit until the cell breaks; the palette test
  picks the stone pickaxe before breaking a stone piece.

**Built in 0.52.31 (sounds round 2b, AUDIO.md §2e):** hit particles — a block's `damage.chips`
(`count`, `color`, `speed`, `size`; stone grey chips, soil clods, sand grains) fly out of the struck
face at the struck point; other players' hits are heard and seen (the client reads a hit off each
damage update's changed cell, at its newest mark, and skips its own hits' echoes).

**Scoops and shards as dents (scoops-and-herds; the scoop replaced by §6.1's per-pixel relief):** the owner's note that a scoop read "more as a dark
stain than a shallow bowl". A scoop and a shard are now modelled cut into the face, `damage.look.depth`
deep (cells; optional, default 0.1, validated in [0, 0.5]; dirt and grass 0.14, sand 0.12, stone 0.1),
growing with the damage like the mark's width: a scoop is a spherical cap of two rings of quads and a
floor fan (12 segments) in the look colour's hue over the tile, darkest at its damp rim; a shard is five angular facets round an off-centre
apex knocked in (one mark in five a flake left proud). Each piece is still drawn flat 4 mm off the face
(the face's own greedy quad stays whole and collision is unchanged) but lit by its modelled normal
(`ground_mesh::push_polygon_shaded`), so the bowl's far wall catches the light and its near wall falls
into shade. Test: `dwell-mesh` `damage_mesh` `a_scoop_is_a_bowl_recessed_into_the_face` (both faces, the
top and a side: modelled behind the face, deepest at its data depth, walls tilted toward the middle,
drawn just off the face). A geometric recess (cutting the face's quad round the mark) was not done: it
would split the greedy face per damaged cell.

**Not done:** the e2e specs are not run locally; the look's tuning with the owner (the cracks read strong);
in-game screenshots of the new dents (not taken in this change's time box).

### 6.1 Carved scoops: per-pixel relief (WORK_QUEUE item 18)

Owner, 2026-11-11: "the 'chunks' of dirt aren't at all what we discussed. It should be like a normal map
change (do we support normal/bump maps)? It's completely flat should look like mesh deformation (preferably
without actually doing it for performance)"; "That part was about digging". The renderer had no normal or
bump maps; the scoop of rings above was flat facets lit by a modelled bowl and read as chunks. Now a
scoop is a hollow computed per pixel, with no new geometry, pass or texture:

- **One quad per struck face** (`mesher/damage_mesh.rs` `relief_quad`): over the face's marks' reach
  (`relief::EXTENT` radii) inside the cell, 4 mm off the face in the opaque mesh, with the face's own uvs
  (metres; a turned or hanging tile as the face's quad lays it), so the texture meets the face's. Its
  parameters ride in its vertex colour and light, packed as whole numbers under 2^24 (`relief::pack`; up to
  the four marks' centres in 1/63 of the cell, radius, depth, rim, march steps, damp), marked by a negative
  blue (a lava face's green is), read flat by the shader (`TerrainOut.code`). The face's quad and collision
  are unchanged. Shards keep their facets (§6), and get the relief later on the same function.
- **The height field** (`dent_height` in `render/shaders.rs`; its CPU copy `dwell_mesh::relief`, which the
  tests pin): the marks' distances merged by a smooth minimum (two hits run into one hollow), a smooth
  bowl `depth` deep (slope 0 at the centre and lip) and a raised rim `rim` high with value-noise crumbs, faded
  to the face at the cell's edge. Width and depth grow with the damage (55 % → 100 %).
- **Shading** (`dent_shade`): the normal bent by the height's gradient (a normal map computed
  analytically, central differences), the texture's uv marched down the view ray into the hollow (steep
  parallax, `parallax` steps and a linear refine), a soft shadow from the rim toward the sun (or moon),
  occlusion growing with depth and the damp floor darker by `darken`. The texture axes come from the
  screen derivatives of the position and uv, so nothing assumes which way is up; the face's tint is taken
  face-locally (`face_rise`), so face B matches face A.
- **Data:** `damage.look` gains `rim` (m, [0, 0.03], default 0.012), `parallax` (steps 0–8, default 6) and
  `darken` ([0, 1], default 0.35), validated by `shared/tools/gen.mjs` (a fixture's bad values fail by name).
- **Tier:** the march runs on the tiers that afford the scene copy (Medium); Low (phones) and the CPU
  adapter light the dent by its bent normal only (`dwell_look::water_tiers::relief_parallax`, bit 11 of the
  tier bits). Deviation: tied to the water tier, the quality preset's GPU budget, until there is a general
  quality tier.
- **Tests:** `dwell-mesh` `relief` (lowest at the centre, walls tilt toward it, deeper and steeper with
  damage, flat at the quad's and the cell's edge, the rim proud, two hits one hollow, the packing round
  trip), `damage_mesh` `a_scoop_is_one_relief_quad_per_struck_face` (both faces, top and side),
  `dwell-client` `the_relief_shares_its_shape_with_its_cpu_copy` with the naga validation, `dwell-look`
  `only_the_scene_reading_tiers_march_the_relief`; the bench's `dirt-relief` scenario (dirt dug 2 and 4
  times, standing and close, day and low sun).
- **Cracks** are no longer drawn on dug soil (shards and chips keep them).
- **Not done:** the merge is limited to the four marks a cell keeps; the face's baked corner light is not
  carried into the quad (it is lit as an open face); the owner's look review.

### 6.2 Carved marks and real pieces (WORK_QUEUE item 18, "both")

Owner, 2026-11-11: "Remember with the digging, mining, chopping it should be a real 3D chunk, not a 2D
decal"; asked whether the block's mesh should be carved or a real piece knocked off, the owner chose
**both**. Built:

- **(a) The mesh carved** (`mesher/damage_mesh.rs`, `mesher/relief.rs` `Carve`): a damaged opaque cube's
  struck faces (those with a mark, visible, of a block with a `damage.look`) leave the greedy merge in
  `mesh_cells`, keeping the corner light the greedy quad would have had. Each is meshed as a tensor grid:
  the face's edges plus `grid` divisions over the marks' reach (flat strips round it), its vertices pushed
  in along the face's inward normal by the height field:
  - `profile: "bowl"` (soils' default): §6.1's merged bowls (`Dent`, smooth minimum of the marks'
    distances) without the rim (a carve only cuts), smooth normals from the field's gradient, the
    vertices shared;
  - `profile: "facets"` (stone's default): per mark a shard of `facets` uneven corners round an
    off-centre apex `depth` in (`Shard`, linear across each facet), the deepest where they overlap, each
    triangle lit flat (its own vertices).
  Width and depth grow with the damage (55 % → 100 %) as before. The uvs are the face's own (the
  material runs on into the dent); the colour is the face's under the normal's tint, darker by
  `darken` × the depth share; the light is the face's corners' interpolated, less 40 % at the floor.
  The field is faded to 0 over 5 cm at the cell's edge, so **no boundary vertex moves**: the carved
  face's outline is the face's, which the cell's other faces and the neighbours' greedy quads meet
  (watertight; the grid's edge vertices are T-junctions on a straight edge, as greedy meshing makes
  everywhere). No side walls are needed. Collision stays the full block. Face-local: a face-B chunk's
  damaged cells are turned over (`face_local`) and meshed as face A's.
- **Budget** (data): a cell's struck faces share `triangles` (default 256, 128–2048; 128 fits four faces
  at the coarsest grid); `grid` (default 8, 2–16) is the most divisions, coarsened to fit
  (`grid_for`: a face's grid is at most (g + 2)² quads). Validated by `shared/tools/gen.mjs`.
- **Tiers:** the carve is drawn on every tier; the decals (§6) and the per-pixel relief quads (§6.1) are
  no longer emitted. Decision: a carved cell is ~200 triangles in the chunk's own opaque mesh, cheaper on
  Low and the CPU adapter than a per-pixel march, and real from every angle; the far LOD draws no damage.
  The shader's relief code (`dent_*`) is unreached and kept until the owner's look review, then removed.
- **Cracks** are not drawn as decals on carved faces (they were decals over a now curved surface); §6.3's
  crack overlay draws them on every face, carved or not.
- **Chopping:** the axe's notches and kerfs on logs were already real carved geometry (`carve.rs`, ADR
  0067 §11): unchanged. No block uses the `chips` look today; one would carve facets by default.
- **(b) The pieces:** each hit's drops (§5, §7: a stone pickaxe's hits drop one rock shard each, dirt's
  a dirt clump) were already loose items drawn as 3D models from their item look (`look_mesh`), falling,
  spinning and auto-picked up (ITEMS.md). Their shapes now match the chunk taken out: a new look shape
  `shard` (`LookShape::Shard`, an uneven five-sided pyramid with an off-centre tip: five angular facets,
  as the carved shard) for `dwell:rock_shard` (`[0.11, 0.05, 0.08]` m, inside the mark at full damage);
  `dwell:dirt_clump` keeps the rounded `lump` (a clod); wood chunks come from felling and keep theirs.
  Held, a shard is drawn as a lump is.
- **Measured** (native, release, `damage_mesh::tests::fifty_damaged_cells_cost`): a chunk with 50
  damaged stone and dirt cells, one to four struck faces each: 12,328 opaque triangles against 1,674
  undamaged (+213 a cell), meshed in 7.8 ms against 4.3 ms (+3.4 ms, ~70 µs a cell). A hit remeshes one
  chunk.
- **Tests:** `dwell-mesh` `damage_mesh`: `a_carved_face_lies_inside_the_cell_below_the_face_by_its_height_field`
  (both faces, dirt and stone: more than the cube's triangles, every vertex inside the cell, the top's
  vertices below the face by exactly the field, as deep as the data), `a_carved_face_is_watertight_and_keeps_its_outline`
  (both faces, top and sides, one hit, two merged, a mark at the edge: inner edges paired, open edges
  exactly the face's outline on its plane), `a_cells_struck_faces_keep_to_its_triangle_budget`,
  `a_fixture_material_by_data_alone_gets_its_facets` (seven flat-lit facets from a fixture's data);
  `ground_mesh` the shard's faces face outward; `dwell-sim` `damage` a hit's piece is the `shard` look,
  inside its mark; `shared/tools/gen.test.mjs` the new fields by a fixture and their bad values.
- **Not done:** no browser check (cloud session, no GPU): the look of the carve and the shard in game is
  for the owner's review (bench `dirt-relief` shows the same digs); the T-junction seams are not measured
  on screen.

### 6.3 Cracks spreading over the block (WORK_QUEUE item 18)

Owner, 2026-11-12: "Perhaps we also show a 'cracking' texture around the block too as you do more
damage." Built, in addition to the carve (not instead of it):

- **The tiles** (data, PAINTERLY_TEXTURES.md "Crack tiles"): `shared/textures/cracks.json` lists crack
  sets, each a material family's stages, hairline to branching (`stone`, `soil` drier fissures, `wood`
  splits along the grain; four tiles each), painted by the stroke generator from `materials.json` into
  the atlas (53 of its 64 slots): white multiply masks with dark strokes, centred on the tile's middle.
- **Per block** (`damage.look.cracks`, validated by `shared/tools/gen.mjs`): `set` (a set of
  cracks.json), `stages` (the damage fraction each of its tiles starts at, ascending in (0, 1], one a
  tile), `strength` (0–1, how far a crack darkens the face). Stone, red sandstone, basalt, obsidian and
  ice: `stone` at 0.15 / 0.4 / 0.6 / 0.8, strength 0.75; dirt, grass, snowy grass, sand and snow: `soil`
  at 0.2 / 0.45 / 0.65 / 0.85, strength 0.6. The stage is the last whose start the fraction reached
  (`Cracks::stage`; none below the first).
- **The mesh** (`mesher/damage_mesh.rs` `CrackLayer`): once its stage is reached, a damaged opaque cube
  leaves the greedy merge on **every** exposed face, not only the struck ones: struck faces are carved
  (§6.2), the others drawn as one flat quad each (with the corner light the greedy quad had). Each
  vertex carries the crack overlay in the opaque mesh's `cracks` stream, four unorm bytes: the crack
  tile's (s, t), its atlas slot + 1 (0: none) and the strength. The tile is centred on the newest hit
  on that face, else the hit nearest the face's plane, so a crack struck into the top runs over the
  edge and down the sides from where it reached them (consistent across the cell's faces); it spans
  `span` (1.5 m) in world metres on every face (a ½ m side shows the tile's middle strip), a side's t
  running up (wood's grain), mirrored per cell so neighbours differ. On a carved face the (s, t) come
  from the vertex's place on the face before the carve, so the cracks follow the dent. Packed over
  [−½, 1½] of the tile; past the tile (a far corner from the hit) no crack.
- **The shader** (`terrain_fs` `crack_shade`): one atlas lookup on cracked faces only (a branch
  constant over each face), the face's albedo multiplied by `mix(1, tile, strength)`, the tile's mip
  level from its own uv's footprint. The same on every tier (a texture lookup, no pass); the CPU adapter
  too. Chunks without cracks send no stream; the terrain buffer fills the slot with 0 (its vertex
  72 → 76 bytes, every chunk).
- **Measured** (native, the test profile, `damage_mesh::tests::fifty_damaged_cells_cost`, all 50 cells
  past the first stage): 12,296 opaque triangles against 1,674 undamaged (+212 a cell; §6.2's
  measurement was +213), meshed in 6.0 ms against 2.6 ms. The pillars' unstruck faces were one quad each
  already; where a cracked cell sits in a flat wall its faces now cost 2 triangles each instead of a
  share of a merged quad.
- **Tests:** `dwell-mesh` `damage_mesh`: `the_crack_stage_follows_the_damage_fraction` (thresholds
  inclusive, none below the first, every carved look cracks), `every_exposed_face_of_a_cracked_cell_carries_the_overlay`
  (both faces: the five exposed faces of a struck stone cell at the stage's tile and strength, not its
  hidden bottom, none on an undamaged neighbour), `no_cracks_without_damage_or_below_the_first_stage`
  (both faces), `cracks_centre_on_the_hit_and_run_round_the_corner`, `the_crack_tiles_are_in_the_atlas_and_grow`,
  `a_fixture_material_by_data_alone_gets_its_cracks`; `dwell-client` `terrain_vertices_carry_the_crack_bytes`
  and the naga validation of the shader; `shared/textures/gen.test.mjs` (the sets painted, white where
  uncracked, each stage covering more; bad sets fail by name), `shared/tools/gen.test.mjs` (a fixture's
  cracks by data alone; a bad set, stage count, order, range or strength fails by name).
- **Not done:** no browser check (cloud session, no GPU): the look in game is for the owner's review;
  the wasm32 clippy and the Vitest files were not run here (no wasm target or `node_modules` in the
  container). The wood set is drawn round the axe's bites on a cut log (§6.4, the wood's `cut.cracks`).

### 6.4 The axe's bites (WORK_QUEUE item 26)

Owner, 2026-11-12: the axe's slice "cuts a line all the way across which is much bigger than the tool
head. It should align more with the dirt and stone ones"; the proposal approved: head-sized V bites where
each hit lands, joining into the notch, grain cracks round them, felling unchanged. Built:

- **Where it struck** (`dwell_protocol::chopping::Cut::bites`): the server records each landed chop's place
  across the side it hit (`u`, 0..1 along the side's other axis; face-locally up for y, as the bands) and
  the band it cut (after `open_band`). A hit on a spot already bitten in that band comes in beside the
  nearest bite (`step` × the width away, nearest the aim, inside the cell); a side keeps 12 bites, later
  hits deepening them. The cut that fells (notch, kerfs, the fell fraction) is ADR 0067 §2's, unchanged.
- **The carve** (`dwell-mesh` `carve/bites.rs`): each bitten side's bark face is a height field on a
  tensor grid through the V's and masks' breakpoints, two flat-lit triangles a quad (a hewn face): the
  notch's V along the grain under the mask of all the side's bites, each band's kerf V (its depth notch +
  kerf, apex in the band) under that band's bites' mask, the deepest; a bite's mask 1 over the head's
  width, its ends sloping in over `endsM`. One hit is a V the axe head's size where it struck; bites side
  by side merge; at the fell the notch spans the log (round 2, below: across its chamfers too). Every V
  ends inside the cell (watertight; T-junctions on straight edges as §6.2's carve); opposite sides keep
  the 2 cm waist.
- **Cracks**: the wood's `cut.cracks` (the `wood` set of `shared/textures/cracks.json`, splitting along
  the grain) over every face of the cell, §6.3's overlay and shader, its stage by the share of the fell
  (or buck) fraction cut, centred on the deepest side's newest bite.
- **Data** (`shared/tools/tools.json`, the wood's `cut`): `bite` (`widthM` 0.14, `endsM` 0.03, `step` 0.6)
  and `cracks` (`set`, `stages` 0.15 / 0.4 / 0.65 / 0.85, `strength` 0.85, was 0.6); validated by
  `shared/tools/gen.mjs` (ranges, unknown fields, the set and stages), a fixture wood's by data alone.
- **Wire and save, additive**: `Cuts` ends with each cell's bites only to a client that said `BitesHello`
  (a datagram; ARCHITECTURE.md §8.3); the `cuts` record appends them after its cells. A cut without bites
  (an older server's or save's) is drawn as before (the notch across the side, the kerfs as slits).
- **Budget**: one bite about 30 triangles; a cell hacked to 12 bites in both bands on all four sides stays
  under 2,048 (`a_bitten_cells_triangles_keep_to_a_budget`).
- **Tests**: `dwell-mesh` `carve::bites` (one hit no wider than `widthM` centred on it, bites at nearby
  spots merge, 11 one-spot swings fell at the rule with the notch widened across the side, closed on every
  grain and inside the octagon, the budget, an older cut unchanged and mirrored bites following their
  side), `mesher_tests` `a_bitten_log_is_carved_where_it_was_struck_and_cracks` (both faces; red on the
  old carve: fresh wood across the face); `dwell-protocol` the placement, the record and message round
  trips, an older record loading without bites; `dwell-sim` `tests/chopping.rs` the bite recorded where
  struck and the fell still at 11 (both faces), bites only after `BitesHello`, saved.
- **Not done:** no browser check (cloud session, no GPU): the look in game is for the owner's review.

**Round 2 (2026-11-12, the in-game check of 0.54.x).** Found: the first bite head-sized, but more
swings grew it down into a slot the cell's full height, still ~0.14 m wide; no cracks seen round the
bites; a swing after the fall cleared the stump's slanted top. Fixed:

- **Across, not down.** A hit lands beside any bite on its side, in either band (`Cut::bite_in`), so one
  aim's hits alternate left and right of it one `step` apart and 11 of them span the log; kept inside the
  log's own width (`span`, a thinner trunk's octagon: the server's `log_span`). Every V lies in the
  notch's band, the cell's middle half (a band's height, ¼ m): the notch's V opens at most that tall
  (before, `open` × its depth reached the cell's ends by the 2nd–3rd hit), each band's kerf V in its half
  of it (apex ⅛ m off the middle, mouth ⅛ m). Tests: `dwell-protocol`
  `eleven_hits_at_one_spot_spread_across_the_log` (red: 0.56 of the log), `carve::bites`
  `chopping_one_spot_…` (red: the cut faces 0..1 along the grain).
- **Over the chamfers.** A bitten side's front, its flat face and the chamfers it owns (both, unless the
  side round the corner is bitten too and comes first), is one grid across the whole log: each triangle
  is the bites' surface where it lies inside the wood and the octagon's (bark) where it doesn't, split on
  the same line (closed); a cut stops short of the far side's outline there. A thinner trunk, whose
  sides are all off the cell's faces, is carved too (it wasn't). Test: `a_notch_spans_an_octagonal_log_…`
  (a full log and a half-width one; red: only the flat face, 0.29..0.71).
- **Cracks that read.** The wood splits were painted in the bark ramp's shadow, the colour of the bark's
  own streaks along the grain, at strength 0.6: a split the same dark as the grain it runs along. Now in
  `ink`, a little wider, at `strength` 0.85 (`mesher_tests` `wood_cracks_read_darker_than_the_barks_own_streaks`,
  red: the split 64 against the streaks' 70; `a_bitten_logs_cracks_show_on_the_bark_round_the_bites`).
- **The stump keeps its gash** (owner: "I think let's do your recommendation"): a wood's `fellWedge` is
  chopped as that wood (`chopped_wood`), never cleared; the axe's `clears` lost `dwell:log_ramp`, and the
  generator fails a `clears` naming a fell wedge. A hit on the stump bites it and wears the axe (a hit on
  its slope takes the side the swing comes in from); it comes down cut through at the wood's fell
  fraction and drops its `buckDrops`. `dwell-sim` `a_swing_after_the_fall_bites_the_stump_…` (both faces;
  red: the 12th swing's arc cleared it). **Not done:** the stump's bites aren't drawn yet (the mesher
  carves full logs; a ramp is drawn as its shape), only recorded, worn and counted toward its fall.

**Round 3 (2026-11-12, the bite rule; WORK_QUEUE item 18).** Owner: **"Yes the notch should grow to follow
the 50% rule."** Round 2's V within the ¼ m band carved about a fifth of the rule by the fell. Now each bitten
side's notch is **one V along the grain** centred on the row's middle, grown to its share of the rule's volume
(§10; the sides share it as their cuts do): `x` m deep, its mouth `cut.bite.bandM` (0.25) at least and
`x ÷ depthPerMouth` (2) past it, the cell's length at most, under the mask of the side's bites, each widened to
`x × widthPerDepth` (1) across (the head's width at least). `x` is found by bisection on the side's carved
wood summed over the very triangles its grid draws (`for_grid_tris`), so the mesh matches the rule exactly.
The first swing is a V about 0.45 m deep, as wide, its mouth the ¼ m band, centred where it struck; more
swings deepen and widen it, then open its mouth past the band; by the fell it is the gash's V: the mouth
the row's height, the apex at the row's middle near the far side, its lower face rising away from the
notch (facing up and out, the stump ramp's short side at the cut) and its upper face falling to it (the
trunk ramp's). The per-band kerf V's are gone (the grown V holds them). The fell, its swings and the gash
are unchanged. Data: `cut.bite.bandM`, `depthPerMouth`, `widthPerDepth` (validated by `shared/tools/gen.mjs`).
Tests: `carve::bites` `the_notch_carves_half_the_log_over_its_swings_k_bites_after_k` (red on main: 0.0013
of the rule's 0.0455 at the first swing), `one_hit_bites_a_v_centred_where_it_struck_…`, `chopping_one_spot_…`
(the mouth spans the row by the fell); `mesher_tests` `a_bitten_log_is_carved_where_it_was_struck_…` (both
faces: centred on the hit, in the band, the V's faces turned up and down out of the notch); `dwell-protocol`
`a_cuts_progress_is_its_swings_over_the_swings_to_fell_for_any_axe`.

### 6.4 Stone shatters, it doesn't dent (WORK_QUEUE item 18)

Owner, 2026-11-12: **"The stone hits should be elongated low poly shards stone doesn't dent, it
shatters."** The browser pass of §6.2 had found stone's `facets` (an apex pit) reading as near-black
spiky wedges.

- **Spall** (`damage.look.profile: "spall"`, now every stone look's; `facets` stays in the vocabulary):
  each hit chips the face with `flakes` (data, 1–4; stone 2) flat chip-outs (`relief::Flake`): a convex,
  elongated (~2.8:1) polygon turned along a direction of the hit's own, its floor one tilted plane
  (deepest at the strike, `depth` × 0.55–1, feathering to 30 % at the far end, like a conchoidal scar),
  its wall a 4 mm bevel inside its edge (on the mesh grid the wall is the straddling triangles: a
  sharp-edged step). Each next flake is smaller (0.7×), turned a little and deeper: a stepped scar. More
  hits add flakes; where they overlap the deepest wins (merged scars). The field is 0 at the cell's edge
  (§6.2's fade), so the face stays watertight. Lit flat per triangle, and the scar is fresh rock: the
  vertex colour lifted toward the look's `color` (× 1.4 × colour, by depth share), occluded 0.15 by depth
  instead of 0.4, `darken` unread. Stone's data: `flakes` 1, `size` 0.6, `depth` 0.1 (after the browser check), `darken` 0, `grid` 12,
  `triangles` 512 (one struck face 392 triangles; four still fit at the coarser grid `grid_for` picks).
- **Pieces:** the rock shard's `shard` look is now an elongated low-poly shard: a triangular waist and
  two off-axis tips, 6 flat triangles, closed all round (it tumbles in the air as every loose item
  does), size 0.18 × 0.045 × 0.06 m (~3:1). One shard a hit stays the picked-up drop (the drop rules
  are unchanged); the hit's `chips` (data `count`, stone 7) are the extra visual-only fragments, the
  billboard splinters of the chips pass, not 3D.
- **Tests:** `dwell-mesh` `damage_mesh`: `a_fixture_material_by_data_alone_spalls_into_flat_elongated_flakes`
  (convex, ≥ 2.2:1, one plane past the bevel, a few cm deep, stepped, merged, flat-lit, a floor of many
  coplanar triangles, the budget), `stone_spalls_shallow_and_light_not_a_dark_pit` (every stone look
  spalls; the scar lighter than the face, barely occluded; red on the old data), the watertight/outline,
  inside-the-cell and budget tests now run on stone's spall (both faces); `ground_mesh`
  `the_rock_shard_is_an_elongated_low_poly_shard` (6–10 triangles, ≥ 2.5:1, closed; red on the old
  size); `shared/tools/gen.test.mjs` (a fixture's spall and flakes by data alone, a bad count fails).
- **Browser check** (2026-11-12, M1 GPU, the warm bench; scenarios in the PR, a flat world's stone floor
  hit 1/3/4 times by the stone pickaxe, which breaks it on the 5th; before = 0.54.29): first tuned
  in data to `flakes` 1, `size` 0.6, `depth` 0.1 (the two-flake 0.44/0.06 scar was a hand's width and
  invisible from standing height). Verdict, honestly: **it does not yet read as shattered stone.** Close
  up there are a few light flat planes at the strike, but the dark crack overlay (§6.3) dominates the
  cell; from standing height the mark reads as a dark crack starburst, as before; at a grazing angle
  barely anything shows (the carve fades to 0 well inside the cell's edge, so nothing shows in the
  silhouette). The light stone texture leaves the lifted floor little contrast. No flying shard was seen
  in the burst frames (one small 18 cm drop and the billboard chips). Next levers: weaker or no cracks
  on spalled faces, a stronger lift or a distinct fresh tint, flakes that reach the cell's edge (break
  the silhouette; needs the fade relaxed with the neighbour's face kept watertight), and real 3D flying
  fragments.

- **Round 3** (2026-11-12): spalled stone's cracks start at half damage at 0.3 strength (`cracks`
  data: stages 0.5/0.7/0.85/0.95); the scar lifted 1.8× toward the fresh-rock `color`, at full lift
  from an eighth of the depth; each pickaxe hit flings `damage.chips.shards` (new data, 0–8; stone 4)
  real 3D fragments: the rock shard's look at half size (6 triangles) through the pieces' renderer
  (`dwell-client` `hit_shards.rs`, ids `0xA000_0000 | ring`), tumbling about a random axis, falling
  toward the face's ground, removed after 1 s (no fade: pieces have no alpha); none on a CPU adapter,
  half on a phone (the flag exists; no phone signal is wired to it yet). Browser (M1, same scenarios):
  close up the scar now reads as light, flat chipped facets with a darker step wall, not a dark pit
  or starburst; the burst shows the small fragments flying; from standing height the chip shows as a
  light flake with a dark edge, small but readable. Not built: flakes reaching the cell's edge to
  chip the silhouette (lever 3; needs the cell's own side walls meshed where the notch cuts the
  edge), so a struck block's outline is still a perfect cube.

## 7. Pieces (0.52.46)

- **Pieces** (owner, 2026-11-11: "8 Rock shards make a rock slab, 8 dirt clumps make a dirt slab, 8 wood
  chunks to make a log slab"): a ½ m cell breaks into 8 pieces of its material, as its block's `drops`
  (`{ "item": "dwell:rock_shard", "count": 8 }`): stone and its ramp → `dwell:rock_shard`; dirt, grass,
  snowy grass and their ramps → `dwell:dirt_clump`; log → `dwell:wood`, relabelled "Wood chunk" (its
  name kept: saves hold it; the bucked log's `buckDrops` keep it too). BLOCK_DAMAGE's split applies:
  half (4) over the hits before the last, the rest on the destroy (stone pickaxe: 1 a hit, 4 on the
  break). Grass, snowy grass and now dirt keep (dirt: gain) the random rock (chance 0.5, "we still
  want rocks to be found in grass and dirt at random"); leaves keep their sticks; tall grass drops a
  `dwell:grass_fiber` every time (owner: "2 grass fibers would be the final ingredient for stone
  tools"). Felled pieces (bodies), sand, snow, ice and the rest keep their drops.

## 7. Creative breaks without a tool in one hit (0.52.51)

- Owner, 2026-11-11: "I noticed I can't break blocks like stone with my fists in creative, that's not
  right. I should be able to break any block while holding anything or nothing with the exception of
  tools."
- **Data:** a mode ability, `instantBreaks` (`shared/modes/modes.json`; `dwell_protocol::modes::Ability::InstantBreaks`),
  creative's only. In a mode with it, a Break holding anything but a breaking tool (an empty hand, a
  block, any item not in `tools.json`'s breaking tools) takes one hit, whatever the block's `damage`
  says: no damage record, no hit drops. A tool held keeps its hits and its "can't" (a pickaxe on stone 5
  or 3 hits; a shovel on stone can't; the axe's chopping unchanged). Adventure lacks it: bare hands
  can't break stone.
- **One rule for both sides:** `dwell_protocol::damage::hits_to_break_for(block, held, instant_breaks)`,
  asked by the server (`tick_edits`, the world's mode) and by the client's break prediction
  (`Game::set_instant_breaks`, from the world's mode at join and when the host changes it), so a creative
  break without a tool is predicted at once and not refused.
- Tests: `dwell-protocol` `damage` (the rule and the modes' data), `dwell-sim` `damage`
  (`in_creative_anything_but_a_tool_breaks_any_block_in_one_hit`, both faces; adventure bare hands and a
  creative shovel still can't break stone), `dwell-client` `game_local`
  (`in_creative_a_block_in_hand_breaks_stone_at_once_as_predicted`: predicted, no refusal).

## 8. Hits land at the swing's contact (WORK_QUEUE item 25)

Owner, 2026-11-12: "the gashes in blocks from tools show up on click instead of when the animation lines
up with the block. We want that timing." And: "Tools should only consume durability on hit."

- **The cause:** both sides. The server applied a break the tick its request arrived (on a local world the
  same frame as the click), and the client played the hit's sound and chips on the click; the axe's
  `Chop::Sweep` was cut on arrival too (its chips and sound were already at the contact, `SwingRun`).
- **Data:** a breaking tool's `contactS` (`breaking.tools[]`, seconds after the click, optional, 0 at once,
  validated in [0, 2]; the pickaxes and shovels 0.26 s, where the arms' `break` clip strikes), read by
  `dwell_protocol::chopping::break_contact_s`; a hand or a block lands at once. The axe's contact is its
  `swing`'s: `windupS` plus the arc's time to the step it hits (`SwingDef::time_of_step`).
- **Server:** a break with such a tool waits in the session's edit queue until `contactS` after it arrived
  (`contact_ticks`, rounded up: never before the client's contact, since the request left at the click),
  then goes through the usual checks (reach, the target, the hits, drops, wear); if the tool that swung it
  is no longer held it is refused (a cancelled swing lands nothing). An axe swing (`Sweep`, `Swing`) starts
  on arrival (its wait spent then, a swing too soon refused), its contact found by a sweep then, and lands
  at it (`land_swings`): swept again from the eye at that tick, refused if the axe was put away.
- **Wear** happens exactly once per landed hit, at contact: a break hit (as before), a chop into wood, and
  now a bounce off a non-wood solid; never a miss, a swing through only the blocks it clears (leaves are
  cleared, not hit: the client follows through as on a miss) or a cancelled swing. Creative unchanged.
- **Client:** a tool's hit is sent at the click with the arms' swing; its sound and chips (a one-hit
  break's break sound) wait for the contact (`hit_feedback::PendingHits`), dropped if the tool is put away;
  a tool's one-hit break is not predicted (the server's change at the contact shows it); one tool swing at a
  time. The mark (carve, cracks) comes with the server's `Damage` at the contact, so on a local world at the
  contact and on a remote one a round trip after it. The axe's swing stops if the axe is put away before its
  contact.
- **No protocol change:** the wire and its timing fields are unchanged (no start time is sent; the server
  times from arrival).
- **Not done:** no camera kick for pickaxe and shovel hits (they have no `swing.camera` data; the axe's
  kick was already at contact); no browser check (cloud session).
- **Tests:** `dwell-sim` `damage` `a_tool_hit_lands_at_its_swings_contact_not_when_it_arrives` and
  `a_hit_whose_tool_is_put_away_before_contact_lands_nothing`, `chopping`
  `a_chop_lands_at_its_contact_not_on_the_click_and_wears_the_axe_once`,
  `a_bounce_wears_the_axe_once_and_a_miss_or_only_leaves_wear_nothing`,
  `a_swing_whose_axe_is_put_away_before_contact_lands_nothing` (both faces; red before the change);
  `dwell-client` `game_local` `a_pickaxe_hit_marks_the_stone_at_its_contact_not_on_the_click` (red: the hit
  sounded on the click) and `hit_feedback` `PendingHits`; `dwell-protocol`
  `a_tools_contact_time_is_its_data_and_a_hand_lands_at_once` and `shared/tools/gen.test.mjs` (a fixture
  tool's `contactS` by data alone; a bad one fails by name).

## 9. Metal tools are twice as good (WORK_QUEUE "Waiting on the owner" answer 7)

Owner, 2026-11-12: **"Metal tools break double sized bigger chunks off and reach 50% broken faster, which is
when the blocks break. Also have double durability."** What exists: three metal tools, the metal axe,
pickaxe and shovel (`shared/items/items.json`, `shared/inventory/inventory.json`, `breaking.tools`), each with
a stone-tier counterpart of its class (the stone axe, the stone pickaxe, the wooden shovel). No content was
added. Built, as data:

- *Superseded by §10 (the bite rule): the tier `chunk` is removed; a mark's size follows its hits.*
- **A tier is `{ speed, chunk }`** (`shared/tools/tools.json` `breaking.tiers`; a bare number is still a
  speed, chunk 1): `chunk` (×, [0.25, 4]) is how big a piece each hit of the tier takes off. Stone
  `{ 1.5, 1 }`, metal `{ 3, 2 }` (was speed 2.5). Validated by `shared/tools/gen.mjs` (a speed above 0, the
  chunk's range, unknown fields), emitted as `BreakTool::chunk`; `dwell_protocol::damage::hit_chunk(held)`
  is the held tool's chunk in sixteenths (16: ×1; a hand or a block ×1).
- **Half the hits:** hits to break are the class's entry over the tier's speed, rounded up: stone and its
  ramp 5 with the stone pickaxe, **3** with the metal one (7 / 3, rounded up; 2.5 exactly can't be a hit
  count); soils and sand the same with the shovels. So a metal hit takes the block a third of the way (the
  stone's a fifth). Per-hit drops follow: a block's half of its pieces spread over the hits before the
  last is 1 a hit with the stone pickaxe and 2 a hit with the metal one (§5's `hit_share`), the pieces
  themselves the same items.
- **A mark twice the size:** each `HitMark` records its `chunk`; the carve (`damage_mesh::carve_of`)
  scales that mark's width and depth by it (the soils' merged bowl takes its biggest mark's; the depth kept
  within 45 % of the cell along the face's normal; the cell's edge fade unchanged, so the carve stays
  watertight). Saved: the `block_damage` record appends a `0xC4` byte and one chunk a mark only when some
  mark is not ×1 (a world hit only by stone tools and hands writes the old record; an older build on the
  line opening one with metal marks doesn't read the record, keeping it unwritten-over, as §5 says of a
  newer record). Sent: the new capability **`damage.chunks`** (`shared/protocol/capabilities.json`): a
  `Damage` message ends with the marks' chunks only to a client that listed it; an older client gets the
  old message and draws every mark ×1. No new compatibility line.
- **The axe** (ADR 0067): the metal axe's `biteM` was already 0.2 to the stone axe's 0.1 (its bite twice
  the depth); it fells a standing trunk at one aim in 5 swings to the stone axe's 11. The chopping
  prototype's `chop2.bite` is now 0.09, twice the stone axe's 0.045 (was 0.075). The bite's mouth width
  (`cut.bite.widthM`) is the wood's, the head's size, the same for both axes.
- **Double durability:** `durability.items` gives the metal axe, pickaxe and shovel 64 hits each (the stone
  ones 32; before, metal tools never wore).
- **Tests:** `dwell-protocol` `damage` `a_metal_tool_is_twice_its_stone_counterpart` (speed, chunk and
  durability doubled, at most half the hits, rounded up, on every block of its class),
  `a_fixture_tiers_chunk_by_data_alone`, `a_marks_chunk_is_saved_only_when_not_one_and_an_older_record_reads_as_one`,
  `a_damage_message_carries_the_chunks_only_when_asked_and_an_older_one_reads_as_one`; `dwell-mesh`
  `a_metal_tools_mark_is_twice_the_size_of_a_stone_tools` (both faces, dirt's bowl and stone's spall: twice
  as deep, about twice as wide); `dwell-sim` `damage` `a_metal_tool_breaks_in_about_half_the_hits_with_marks_twice_the_size`
  (both faces: the metal pickaxe and shovel 3 hits to the stone ones' 5, their marks ×2, a wear a hit),
  `a_marks_chunk_reaches_only_a_client_that_lists_damage_chunks` (an older client's message as before),
  `chopping` `a_metal_axe_fells_in_about_half_the_stone_axes_swings_and_wears_half_as_fast` (both faces,
  5 against 11); `shared/tools/gen.test.mjs` (the doubled tiers, a fixture tier's chunk, bad chunks fail by
  name, the metal tools' 64).
- **Not done:** no browser check (cloud session, no GPU): the bigger marks in game are for the owner's look.

## 10. The bite rule: one volume per hit (WORK_QUEUE item 18)

Owner, 2026-11-12: **"the chunks that fly off and show chunked off the blocks should be equivalent to 50% of
the block's volume / number of swings required to break it."** One rule in one place, over the data:

- **The rule** (`dwell_protocol::damage::bite_m3`): a hit's bite = `BROKEN_AT` (0.5) × the block's volume ÷
  its hits to break with the tool held (`hits_to_break_in`, from the block's `damage`). The volume is the
  cell's (`CELL_M3`, 1 × ½ × 1 m, pinned to `CELL_H` by a dwell-mesh test) times the block's shape share,
  generated by `shared/tools/gen.mjs` into `BlockDamageDef::volume` (a full block 1, a stored ramp ½;
  another shape fails the generator by name). `bite_m3_of(block, held)`, `carved_m3(block, fraction)`
  (fraction × 0.5 × V: k bites after k hits, half at the break). Chopping: `chopping::swings_to_fell`
  (a standing cell at one aim, as the server lands swings: 11 stone axe, 5 metal) and `chop_bite_m3`.
- **Metal tools** get bigger bites by their fewer hits alone: the tier's `chunk` data (§9) is removed;
  a tier is its speed. The mark's `HitMark::chunk` (wire and save unchanged) now carries the hit's bite
  in sixteenths of `HitMark::UNIT_M3` (a full block's bite at 8 hits): 26 for a 5-hit tool, 43 for a
  3-hit one (`damage::hit_chunk(block, held)`, set by the server). It weighs a cell's marks against each
  other and sizes another player's pieces. Since every mark is now not ×1, a saved `block_damage` record
  carries the chunks (§9's `0xC4` tail), which an older build on the line keeps unread.
- **The carve** (`damage_mesh::carve_for`): each struck face's height field (the look's profile and
  shape) is sized to its share of `carved_m3` (the cell's marks' chunks on that face over all of them):
  deepened until its integrated volume matches (`carve_volume`, a 64² sum over the face), down to
  `DEEPEST` (0.9) of the cell, then widened. The meshed cell's closed volume follows within 8 % (the
  mesh grid against the field; stone's flat flakes lose most).
- **The pieces** (`dwell-client` `hit_shards`): `chips.shards` 3D pieces a hit (or a chop) flings, drawn
  with `chips.piece`'s item look (new data, default `dwell:rock_shard`; stone shards, soils' and sand's
  `dwell:dirt_clump` clods, the log's `dwell:wood` chips), each scaled by ∛(bite ÷ (count × the look's
  mesh volume)), so together they are the bite. Counts: stone 4, soils 3, wood 3. The axe's chop flings
  them too, at its `chop_bite_m3`. The per-hit drops (inventory items) keep their item sizes.
- **Numbers** (top face, before → after, m³): dirt + wooden shovel (5 hits) one hit 0.0014 → 0.050, at the
  break 0.021 → 0.25 (half the 0.5 m³ cell); stone + stone pickaxe (5) 0.0008 → 0.050, 0.014 → 0.25;
  metal pickaxe (3) one hit → 0.083. A stone pickaxe hit's 4 shards are 0.0125 m³ each (about a 23 cm
  chunk; before, half-size rock shards of under a litre together). The carve reaches 0.45 m (dirt's bowl)
  and up to the cell's 0.9 (stone's spall) by the break.
- **The axe's notch** (owner, 2026-11-12: "Yes the notch should grow to follow the 50% rule"; §6.4 round 3):
  a cut log cell's carve after k of its swings to fell has carved k × ½ × the log's volume ÷ those swings.
  The swings are read from the cut alone (`Cut::progress`: its hits, the bites recorded, over the swings at
  one aim at the bite they cut on average, `chopping::swings_to_cut`), so any axe's cut is sized right
  (stone 11, metal 5). Measured on the carved mesh (`carve::bites`
  `the_notch_carves_half_the_log_over_its_swings_k_bites_after_k`): within 0.1 % at every swing on a square
  cell and in the octagon, the square cell 0.47 of its 0.5 at the fell (the waist kept); in the octagon the
  fell's half (0.41 of the cell) is more than a V inside it holds (0.36: the chamfers take the mouth's
  corners), but that carve is never drawn: the cell splits into the gash, half the cell.
- **Not done:** the chips of a chop are `chop_bite_m3`, half the *cell* over the swings, while the octagon's
  carve is half its *wood* (0.83 of the cell): the chips run ~20 % over the carve on an octagonal log. Ramps'
  bite is half a cell's for the pieces; ramps aren't carved (drawn as their shape). No browser check (cloud
  session): the look in game is for the owner's local check.
- **Tests:** `dwell-protocol` `a_bite_is_half_the_blocks_volume_over_its_hits` (dirt, stone wood/metal,
  sand, the ramp's half), `a_tool_taking_half_the_hits_bites_twice_as_much_and_a_fixture_block_by_data_alone`,
  `chopping` `a_chop_bites_half_the_cell_over_the_swings_to_fell`; `dwell-mesh`
  `the_carve_removes_half_the_block_over_its_hits_k_bites_after_k_hits` (both faces, the closed mesh's
  volume, k = 1..n, half at the break, metal 5/3 of stone), `the_bite_rules_cell_is_the_worlds`;
  `dwell-client` `a_hits_pieces_total_its_bite` (stone, dirt, grass, wood chips); `dwell-sim` `damage`
  (marks' chunks 26/43, both faces); `shared/tools/gen.test.mjs` (the ramp's volume, no tier chunk).

## 11. The axe like the other tools (WORK_QUEUE item 51)

Owner, 2026-11-12: "Also the axe doesn't behave like the other tools. I think it should." Answer to how:
**"Like the others, felling on top"**: logs take block damage like stone or dirt, and breaking a trunk's log
fells the tree above as a body, as before.

- **Data:** the log's `damage` (`shared/blocks/dwell.json`): `{ default: 24, axe: 15, chips }`, so the stone
  axe breaks it in **10** hits (15 ÷ 1.5, rounded up: the old fell's 11 swings scaled to the 50 % rule,
  11 × 0.5 ÷ 0.55), the metal axe in **5** (15 ÷ 3), anything else 24 where it may break a log at all
  (adventure: only an axe, `breakWith`; creative: a hand or a block breaks it in one hit, a tool keeps its
  hits). The stump's `dwell:log_ramp` (an older world's) `{ default: 12, axe: 8 }` (6 and 3; half a cell).
  The `axe` class already named both (`breaking.classes`). Its `chips` are the wood's.
- **Server** (`server/chopping.rs` `chop_hit`): an axe swing that lands on a wood with `damage` (aimed or
  swept, at its contact, §8) is a block-damage hit: one wear, a `HitMark` (the edit face struck, the point,
  the bite rule's chunk: `hit_chunk(log, axe)`), the hit's share of the log's drops (`hit_share`: 8 wood,
  4 over the hits before the last); the hit that reaches the count breaks the block (`drop_destroyed`, the
  rest), sends its damage gone and fells what it held: a standing trunk above as **one body**, tipped toward
  the side struck (the uncut side the hinge, as the deepest notch steered before), a lying log's
  neighbours as after any break (bucking a settled trunk is the same hits). No fell wedges: the cell is
  broken whole (the gash of WORK_QUEUE item 24 is no longer left; an older world's stump ramps stay and take
  damage too). A moving body's logs still break in one hit (`BodyEditRequest`, as every block on a body).
  The chopping prototype (`chop2`, off) keeps its cuts.
- **No new cuts:** a new hit records no `Cut` (no notch, kerfs, bands, bind or grain share; a vertical
  swing hits as a level one). **Older saves' cuts** still load (the `cuts` table, `Cuts::from_bytes`
  unchanged), are sent to clients that said `ChopAction::Hello` and drawn as before (notch and kerfs, or
  bites); the next hit on such a cell carries its progress over as hits (`Cut::progress` × the hits to
  break, rounded down), drops the cut (`Cuts` `gone`) and goes on as damage.
- **Client** (`dwell-mesh` `mesh_cells`): a damaged wood cell (any block of `woods`) with no cut is drawn
  with the axe's head-sized bites at its marks (`chopping::cut_of_marks`: each mark's side, band and place
  across), grown by its damage fraction as the progress: the same carve as §6.4's bites, which follows the
  bite rule on the octagon (k of n hits carve k × ½ × the log's volume ÷ n); the wood's cracks (`cut.cracks`,
  `wood` set) by the fraction. The chips of a chop are sized by `bite_m3_of(log, axe)`. Only the newest four
  marks are kept (`MAX_HIT_MARKS`), so a cell's bites are its last four hits, the V deepening with the rest.
- **Compatibility:** additive. A new `damage` entry, a new client drawing an existing message (`Damage`) on
  a new block; the `cuts` record and the `Cuts` wire are unchanged and still read. An older client on the
  line gets `Damage` for logs (it would draw nothing on an octagon; a cube's carve skips octagons) and no
  new cuts; no new compatibility line.
- **Tests:** `dwell-sim` `chopping` `an_axe_breaks_a_log_in_its_damage_hits_stone_ten_metal_five`,
  `each_axe_hit_marks_where_it_struck_with_the_bite_rules_chunk_and_wears_once`,
  `breaking_a_trunk_log_fells_the_tree_above_as_one_body_toward_the_side_struck`,
  `a_lone_log_breaks_in_its_hits_drops_its_wood_and_nothing_falls` (all both faces),
  `creative_breaks_a_log_in_its_hits_and_drops_nothing`,
  `an_older_saves_cut_still_loads_and_draws_and_its_progress_carries_into_damage`; `dwell-mesh`
  `a_damaged_log_is_carved_by_its_marks_k_bites_after_k_hits` (both faces, the closed mesh's volume). The
  old model's tests of notches, kerfs, the gash and bites recorded by the server were removed with it.
- **Not done:** no browser check (cloud session, no GPU): the look in game is for the owner's check.
