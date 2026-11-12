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
  (shovel 5, metal 3, by hand or another tool 11). Logs keep the axe's chopping (no `damage`: felling
  and its notches unchanged).
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
  container). The wood set waits for a wood block with a `damage.look`.

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
