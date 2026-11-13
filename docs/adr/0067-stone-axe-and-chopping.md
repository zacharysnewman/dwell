# 0067 — The stone axe: aimed chopping, notches, felling past half and bucking

**Status:** Accepted (2026-11-08; data, server and client input built in 0.50.34; the owner's
answers, the gouges, chips, arms and the axe on the creative hotbar in 0.50.36, see "Built so far";
the cuts carved into the chunk mesh in 0.50.60). Builds on ADR 0066 (items by
name, drops, the Welcome-flag message pattern) and docs/FELLING.md (how a cut trunk falls).

## Context

The owner (2026-11-08): "a stone axe with tree cutting mechanics. We'll actually show gouging in the
tree with chunks flying off (in the GPU only I think?), but I'd like it to take some aim. Right click
switches angle from vertical to horizontal swing. Vertical swings on a vertical tree are very
ineffective, ideally we'd have it work like real wood cutting where you chop into the tree, then you
chop a little above or below to cut the chunk out. And you keep going until you get just past the
half way point through the tree and it falls over. Then you chop it up on the ground, at some level
of granularity, it drops wood items." And: "Chopping on the ground would then be more effective with
vertical mode."

Answers: the axe is an item (no crafting yet: given by a debug give or found in an adventure world);
about 8–12 good swings fell a 1 m trunk, clumsy straight hacking 3–4× more; no durability yet; on
the ground the log is cut into ~1 m lengths across the grain, each freed length dropping a few wood
items. The general rule: **a swing cuts well when its plane crosses the wood's grain** (a standing
trunk: level swings; a log on the ground: vertical swings across it; leaning: whichever crosses
best), its effectiveness by the angle between the swing plane and the grain, as tool data.

Who adds things: the owner or a mod, by data a tool could edit: a new tool is an item in
`shared/inventory/inventory.json` and an entry in `shared/tools/tools.json`; a new wood is a block
with an `axis` and an entry in `woods`. Fixed in code: the tool kinds (`axe`), the cut model below,
felling (FELLING.md). Scope: tools and woods are per build; cuts are per world, saved with it.

## Decision

1. **Tools are data.** `shared/tools/tools.json`'s `tools`: an item name (an inventory item), a
   `kind` from a fixed vocabulary (`axe`), `swingSeconds` (0.45), `biteM` (0.1 since 2026-11-12, was 0.12: metres a swing
   straight across the grain cuts into open wood), `bindM` (0.05: a kerf this deep past the notch
   halves a swing into it), `across` (1) and `along` (0.15) shares and `sharpness` (2): a swing's
   share is `along + (across − along) |cos θ|^sharpness`, θ the angle between the swing plane's
   normal and the grain. `woods`: a block by name whose `axis` property is its grain,
   `fellFraction` (0.55), `buckFraction` (0.55) and `buckDrops` (`dwell:wood` ×2–3, as a block's
   `drops`). `shared/tools/gen.mjs` validates it (names against the items and blocks, ranges,
   unknown fields; a message naming the entry) and emits `crates/dwell-protocol/src/tools.gen.rs`
   and `client/src/ui/tools.gen.ts`; CI regenerates and fails on a diff; `gen.test.mjs` adds a
   fixture tool by data alone. The stone axe (`stack` 1) and wood are inventory items.
2. **The cut model** (`dwell_protocol::chopping::Cut`, shared by server and client). A wood cell has
   four **sides** (the faces across its grain) and two **bands** per side (its halves along the
   grain: a standing log's ½ m row in two quarter-metres, face-locally lower first; a lying one's
   1 m in two halves). Each side has an open **notch** (metres cut out) and each band a **kerf**
   past it. A swing into a band cuts `bite × share / (1 + kerf / bind)` (at most to the far side):
   a deeper kerf binds. When both bands of a side are cut past the notch, the chunk between comes
   out: the notch deepens by the shallower kerf. So the owner's technique (chop in, then a little
   above or below) cuts 0.12 m per two swings, while hacking one spot slows as its kerf deepens.
   With the data: notching fells in **9** swings, hacking one spot in **28** (3.1×), vertical
   swings at a standing trunk take **61** (level and vertical alternated by band). The share of the
   cross-section cut is the most of any two opposite sides together over the cell's size.
   **Amended 2026-11-12 (BUGS.md #75):** a quarter-metre band is too fine for a player to aim at,
   so in game every swing at one spot hit the same band and no tree fell. A swing aimed at the band
   cut deeper than the other now lands in the other (`Cut::open_band`, the head coming in beside
   its last kerf, in the open wood), so the same aim notches; hacking one kerf no longer happens.
   With the stone axe's `biteM` 0.1 (was 0.12; the owner's "about 12 good chops", WORK_QUEUE item 7)
   a standing trunk falls in **11** swings at one aim, as alternating the bands does.
3. **Aim.** The client casts its view ray into the targeted cell's box: the face it enters and the
   point, sent with the swing. The server checks the point is on that cell (5 cm slack) and within
   the edit reach (+0.75 m), that the cell is a wood and the face a side (end grain: nothing), and
   takes the band from the point and the swing plane from the eye-to-point direction, all
   face-locally through `FaceFrame` (+y up on both faces): a level swing's normal is up, a vertical
   swing's is level and square to the look's heading.
4. **The swing direction** is the client's: **Use (right click) with the axe held toggles** level
   and vertical, sent with each swing; the HUD shows it ("Axe: level swing (right click:
   vertical)") in the hotbar's status line. Break (left click) swings, at most every
   `swingSeconds` (the server holds the same wait per player).
5. **Felling past half.** A standing wood (grain along the face's up) cut past `fellFraction`: its
   cell goes (the wood is chips; since 0.51.0 a wood with a `fellWedge` splits instead, 5a) and
   FELLING's integrity takes what it held as a piece, **tipped toward the notch** (its deepest side), as real felling falls (owner, 2026-11-08, Question 1): the
   uncut side is the hinge, the notch side opens and the trunk tips into it, toward the chopper who
   stands at the notch. The stump below stays.
5a. **Felling as wedges** (0.51.0, Phase 55 T2; owner, 2026-11-09: the cut cell "should not
   destroy the block, it should turn into a wood ramp and a wood ramp upside down on the piece
   above (both pieces are involved). And then it should fall straight down, which will tilt
   automatically toward the cut just because of the shape of the cut"). A wood with a
   **`fellWedge`** (shared/tools: a stored-ramp block, a carrier; the log's is `dwell:log_ramp`, the
   log's `family: ramp` entry in shared/blocks, never placed and not on the hotbar) is felled
   differently from item 5: the cut cell becomes the stump's ramp, facing the notch (rising away
   from it), the log above it the trunk's upside-down ramp (`half: top`), and the trunk is released
   with **no push** (`detach`'s `away_from` none: no topple spin). How the trunk's ramp lies is the
   wood's **`fellWedgeTrunk`**: `open` (the default) reaches down at the hinge, so the two ramps meet
   on the hinge's edge in a V open toward the notch and the trunk, resting on that edge with its
   weight over the notch, pivots into it; `matching` lies parallel to the stump's (the trunk drops
   ½ m onto the slope). The cut cell's notch-side half is chips. `dwell_sim::felling::fell_wedges`
   picks the states face-locally (the notch's direction from the deepest side; a ramp faces the way
   it falls) and the server turns them over on face B (`FaceFrame::state`). The wedges collide: the
   stump in the terrain mesh as the log's octagon cut by its slant (Phase 55 T4, ADR 0072: the wood
   ramp's `crossSection: "of"`; drawn so too), the trunk's in its body as its square shape's convex
   hull (`PieceCell::hull`, never merged into a box; drawn octagonal; the octagon's hinge let the
   trunk slide off the stump, ADR 0072 T4), the logs above it as boxes (their octagon prisms are
   built but not switched on, ADR 0072 T4). The axe's swing clears a wood ramp (the axe's
   `clears`), and hands can't break one where blocks are not unlimited (`breakWith: axe`, as the
   log); broken, it drops a log. A wood without `fellWedge` falls as item 5. Measured (2026-11-09,
   `tests/chopping.rs`): `matching` stood on its 26.6° slope (friction 0.8); a 45° two-cell split
   slid toward the notch and toppled back over the hinge on face A; `open` is down past 60° toward
   the notch in 0.6 s on both faces. **Revised the same day** (owner: "You can make the tree tip if
   needed, like we used to, maybe 3/4 of the force"): `fellWedgeTrunk` defaults to `matching`
   (`open` stays a value), and the wood's **`fellTip`** (shared/tools, in [0, 2], 0: none) releases
   the trunk tipping toward the notch with that share of item 5's topple spin (away from a point
   past the hinge side). At 0.75 and 0.85 the 3.5 m test trunk leant ~5° and rocked back upright on
   the matching slope; at 1 it is down past 60° toward the notch in ~1.25 s on both faces, so the
   log's `fellTip` is 1. A breaking change (new block states, a changed fell): it ships
   in 0.51.0, the line's first release. **Revised 2026-11-12** (WORK_QUEUE item 24; owner: "The
   tree chop currently makes 2 half slopes both at the same angle, which doesn't line up with the
   chop you make in the side of the tree … supposed to have the big gash … 2 half blocks one upside
   down one right side up, both with the 'short side' facing the cut"; confirmed "Yes that was the
   shape I meant"): the log's `fellWedgeTrunk` is `open` (the default stays `matching`), so the
   stump's ramp is low at the notch rising away and the trunk's thin at the notch, a V open toward
   the side chopped; the notch's direction is the deepest side's (`side_normal`), so any of the
   four sides. `fellTip` stays 1: the trunk falls toward the notch from each side on both faces
   (`tests/chopping.rs` `felled_from_each_side_the_cut_opens_a_gash_toward_the_notch`). Not
   breaking: the states exist, a fell's outcome only.
6. **Bucking.** A lying wood (grain level) is cut the same way, best by vertical swings across it,
   and cut through at `buckFraction`: its cell (a 1 m length) goes and, in a mode that uses items,
   drops `buckDrops` (rolled with the server's entropy) as loose items (ADR 0066). In creative
   nothing drops, as for breaks.
7. **Mode gating.** Chopping works in every mode that allows edits. In a mode that uses items the
   held inventory slot must hold the tool; in creative the swing is trusted, and the axe is a slot
   of the creative hotbar (Question 3, below).
7a. **Blocks that need a tool** (owner, 2026-11-08, Question 5: "in adventure, logs need an axe").
   A block's **`breakWith`** (block data, `shared/blocks/*.json`; a tool kind of the fixed
   vocabulary, `axe`) says hands can't break it **where blocks are not unlimited**: the rule
   applies exactly when the mode lacks `unlimitedBlocks` (adventure), the same switch that makes
   breaks drop items, so no new mode ability; creative breaks anything. `shared/tools/gen.mjs`
   validates it (an unknown kind fails naming the block) and emits `BREAK_WITH`
   (`dwell_protocol::chopping::break_with`, `may_break_holding`); the server refuses such a block
   edit (and a region clearing one) unless the held slot holds a tool of that kind. The log has
   `breakWith: "axe"`; leaves and plants have none and break by hand. With the axe held the client
   swings (`Chop`) rather than breaks, so a log comes down only by chopping.
8. **Cuts go with their wood.** Any change to a cut cell (broken, cut through, collapsed into a
   piece, replaced) drops its cut (`Server::cuts_dropped`, in `broadcast_changes`). A felled
   trunk's other cuts are not carried by the piece (Question 4).
9. **The wire: new, optional messages (no new compatibility line)**, as ADR 0066:
   - Welcome gains flag **`CHOPPING` (32)**. Older clients ignore it; and **`CHOP_SWEEP` (64)**
     (0.50.39): the swing's path and held tools (`Sweep`, `Held`, `HeldTools`).
   - C→S **`Chop`** (88), only to a server with `CHOPPING`: `Hello`, or `Swing { tool, cell, face,
     point, vertical }`.
   - S→C **`Cuts`** (89), world channel, only to a client that said `Hello`: changed cells' cuts
     and gone cells; after Hello all of them (at least one message, empty when none: the client's
     sign that the server chops).
   An older client on a newer server never sends `Chop`: it breaks logs by hand as before. A newer
   client on an older server sees no flag: the axe does nothing (its swing is not sent).
10. **Save: a new table, append-only.** `cuts`: one record (key 0, version 1, each cell and its
    sides' notch and kerfs as f32), the whole set each save. Older builds leave it; a record this
    build can't read is kept, not overwritten.
11. **The client draws** (0.50.36; the cuts carved into the mesh since 0.50.60):
    - **Cuts are real geometry** (0.50.60, replacing 0.50.36–0.50.59's decal gouges; the owner,
      2026-11-09: "our chopping display on logs doesn't look anything like a 3D chop, it just looks
      like a backless texture?"; a normal map would give no real depth either). A cut wood cell is
      meshed in the chunk mesh path as the block with its cut carved in (`dwell-mesh` `carve.rs`,
      `mesh_chunk_cut`), and its cube's faces are not drawn. What is left at a point along the grain
      is the cell's cross-section trimmed by each side's depth there, so the cell is a stack of
      spans over which each side is one planar face: each side's **notch** a V across the whole side
      (its apex its depth at the cell's middle, opened along the grain `open` (2.5) × its depth,
      at most the cell's length), each band's **kerf** past it a slit `kerfWidthM` (0.035 m) wide
      and as deep as the notch and the kerf, half way out along the V's arm in its band, its walls
      square to the grain; opposite sides never meet (a 2 cm waist is left). The faces cut in are
      fresh wood: the wood's `cut.tile` (`logCut`, painted as `fresh_wood` from the heartwood ramp,
      shared/textures/materials.json) times `cut.tint` (`#ffe2b8`, warm so it stays tan in shade),
      lit by the sky the cut side sees and darker the deeper they lie; the bark left on each side and
      the end grain are the cube's faces, trimmed, and hidden where a neighbour covers them. As
      chunk geometry it takes the terrain's lighting, shadows and the CPU adapter's tier for free.
      The client's `ChunkStreamer::set_cut` meshes the cell's chunk again whenever a cut changes
      (cuts are few). Face-local: a face-B chunk is meshed mirrored, its cuts' sides along y swapped
      (`carve::mirror_cut`). `cut` is per wood in `shared/tools/tools.json` (validated: the tile
      against the atlas, the tint, the ranges).
    - **Chips**: each swing the client sends knocks out the wood's `chips.count` (a CPU adapter a
      third) flat splinters out of the hit face, carried along the swing (a level swing sweeps from
      the look's right to its left, a vertical one down), falling toward the face's ground for
      0.9 s; `chips` (count, colour, speed, size) is per wood in `shared/tools/tools.json`.
    - **The arms**: `fp_chop_level` (wound out right, swept across) and `fp_chop_vertical` (raised,
      brought down), the first person's `chop_level` and `chop_vertical` roles
      (`shared/characters/characters.json`; `Swing::ChopLevel`/`ChopVertical`, a character without
      them plays its `break`); the held axe is the `stone_axe` item there (a stick, a knapped stone
      head, a leather lashing), named by the tool's **`held`** in `tools.json` (validated against
      the characters' items). Remote players see an empty hand: the held-item wire (v17) carries
      blocks only.
12. **Tools on the creative hotbar** (owner, 2026-11-08, Question 3). `shared/items/items.json`'s
    items take **`tool`** (a tool of `tools.json` by item name) instead of `block`: such an entry
    places nothing (no `rows`, `drag`, `state`, `facing`, `upper`; each fails naming the entry).
    `shared/items/gen.mjs` validates it against the tools and emits `ItemDef.tool`; the palette
    slot is a tool (`PaletteSlot.tool`, material 0) whose picture is drawn as a handle and head.
    Holding it in creative selects the chop input exactly as the inventory's axe does in adventure
    (left click swings, right click toggles level and vertical). The stone axe sits after the log.

## Questions

Answered by the owner (2026-11-08):

1. **Which way it falls: toward the notch**, like real felling (not away, as 0.50.34 built): the
   uncut side is the hinge; the notch side opens and the trunk tips toward it (§5; built 0.50.36,
   `tests/chopping.rs`, both faces).
2. **Wood items or logs: wood items**, as built (2–3 `dwell:wood` per bucked length).
3. **The axe in creative: on the creative hotbar**, by tool entries in `items.json` (§12; 0.50.36).
5. **Hands on logs: in adventure, logs need an axe**; leaves and plants stay breakable by hand;
   creative breaks anything (§7a, the block's `breakWith`; 0.50.36).

Still parked (defaults chosen):

4. **Cuts on a felled trunk.** Cuts on cells that fall with a piece are dropped; carry them into the
   piece and back when it settles?
6. **Numbers.** 11 swings at one aim with `biteM` 0.1 (2026-11-12, BUGS.md #75: a swing at the deeper band lands in the open one, so hacking one kerf, once 28 swings at 3.1× the notching the owner asked, no longer happens), 0.45 s a swing; 0.55 of the cross-section to fell
   or buck. All data.
7. **Leaning trunks** (a log at an angle): logs are only ever along x, y or z, so a lean is not
   stored; the share rule handles any angle once a body's cells can be chopped (bodies: next).

## Built so far (0.50.34)

- **Data:** `shared/tools/tools.json`, `gen.mjs` and its test (the fixture tool by data alone,
  every validation message), CI regenerating it; `dwell:stone_axe` and `dwell:wood` items.
- **Model and wire:** `dwell_protocol::chopping` (tools, woods, `Cut`, `Cuts`, the share and swing
  plane), `Chop` and `Cuts` and the Welcome flag; round trips; the scripted chopper (9 vs 28).
- **Server:** `dwell-sim` `server/chopping.rs` (aim checks, the wait, notching, felling away from
  the notch, bucking with drops, cuts dropped with their wood, the `cuts` table, Hello gating);
  `tests/chopping.rs`, both faces: notching fells in 8–12 and leans away from the notch, hacking
  3–4×, vertical swings at a standing trunk barely cut, vertical swings buck a lying log and it
  drops wood while level ones barely cut it, creative drops nothing, no axe in hand does nothing,
  saved and sent after Hello, nothing without Hello.
- **Client:** `dwell-client` `chop.rs` (the ray into the cell, the wait, the toggle), Hello on the
  flag, cuts kept, Use toggling and Break swinging with the axe held, the swing hint in the HUD.

## Built in 0.50.36

- **The owner's answers:** felling toward the notch (server and both-face test); `breakWith` block
  data, the log's `axe`, enforced in adventure (red → green: the hand break of a log is refused
  in adventure, leaves still break, creative breaks it; both faces); tool entries in
  `items.json` with the stone axe on the creative hotbar (generator validation, a fixture tool
  entry by data alone).
- **Visuals:** `chips.rs` (gouges for a known cut, both faces, replaced by the carved mesh in
  0.50.60; chips per swing: count, out of the
  face, along the swing, falling, a CPU third), `PuffKind::Chip`/`Gouge` through the billboard
  pass and its shader (`chip_colour`, validated with naga in both tiers), the chop clips and the
  held axe (`shared/characters`), the hotbar picture.

## Built in 0.50.39: the swing as weighty melee

The owner (2026-11-08): "This axe is a lot more like Dying Light melee mechanics than Minecraft":
weighty first-person physical swings (the mechanics only). Built:

- **The swing as data** (`tools.json` `swing`, validated, a fixture tool by data alone):
  `windupS` 0.12, `activeS` 0.14 (the arc), `hitStopS` 0.08, `recoverHitS` 0.22, `recoverMissS`
  0.36, `reachM` 2.2, `arcDeg` 110, `samples` 12, `kickDeg` 1.6, `staminaCost` 8 (unused until
  stamina exists: swinging, swimming and sprinting will draw on it), `swish` and `bounce` sound
  events. A miss must last at least `swingSeconds` (the server's wait). (2026-11-12: `kickDeg` and the
  hardcoded camera curve were replaced by `swing.camera`, keyed view sway per swing kind with a kick at
  contact; docs/CHARACTERS.md §5.2.)
- **The path is what hits; free swings.** A swing goes any time, at nothing too. `dwell_world::swing`
  sweeps the head's arc face-locally (level: the screen's right to its left; vertical: top to
  bottom); the first solid cell is hit. The client sends `Chop::Sweep` (look, plane) to a server
  with `CHOP_SWEEP` (64), which sweeps the same arc from its own eye and cuts a wood it meets, or
  spends the swing (a bounce off stone, a miss). An older server gets the aimed `Swing` when the
  path meets a wood. Tests: an arc passing a trunk cuts it though the crosshair misses; a nearer
  stone post on the path takes the hit; free swings and stone do nothing and spend the swing (both
  faces). A sweep hit's face is converted to the chopping wire's `axis × 2 + positive` (`SwingHit::axis_face`; red → green in the browser and `tests/chopping.rs`: the notch went on the far side).
- **The feel** (`chop.rs` `SwingRun`, tested): the arms' clip (timed to a miss) is held at the
  impact for the hit-stop and faded from there to the rest after a hit (BUGS.md #93: no longer played
  back through the wind-up), runs through its follow-through
  on a miss; the camera winds back, sweeps, jolts against the swing at a hit (harder off stone);
  the impact shows chips, the hit's sound by the cut (`sounds`: bark thud, wood chunk, creaks from
  40%), shakes leaves from the tree's leaf blocks (`leaves`), or clanks off stone (no effect).
- **The axe the right way round, two holds, a flint head**: `stone_axe` and `stone_axe_vertical`
  (`held`, `heldVertical`) with an `axe_blade` edge; red → green: `dwell-characters`
  `tests/axe.rs` failed with the blade at (−0.24, −0.30, −0.92) in view space (toward the eye);
  now it faces away in both holds, the holds differ and the edge leads the strike. The head is the
  `flint` material (a warm grey-brown ramp), not the ground stone's violet shadow.
- **Cuts that read as cuts** (decals; replaced by real geometry in 0.50.60, §11): the notch is a V, each face lit by its own normal (`Gouge::cut_normals`,
  packed into the billboard's info bits; both faces tested), fresh tan wood (`#d9b37a`) keeping its
  hue in shade, end-grain rings, a crease at the apex darkening with depth, a bark lip; it opens
  wider and deeper as the notch deepens. Chosen over a remesh as the cheapest that reads as depth.
- **Progress you can feel**: the tree leans toward its notch from 40% of the fell fraction, more
  toward the fall (`sway`, `TreeLean`, a shiver after each hit; the terrain's vertex shaders), the
  hit sounds deepen, leaves fall each hit.
- **The reticle** draws the swing's line (level or vertical) through the crosshair.
- **Others see the held axe**: `Chop::Held` (tool by name, the hold) and S→C `HeldTools` (90),
  only to clients that sent a `Held`; tested on the server and the client's drawing.

## Built in 0.50.60: cuts carved into the mesh

- **Data:** a wood's `cut` (`tile`, `tint`, `open`, `kerfWidthM`; `CutLook`), validated by
  `shared/tools/gen.mjs` (the tile against `shared/textures/materials.json`'s tiles; a test adds
  and breaks it by data alone); the `logCut` atlas tile (`fresh_wood`), which the texture generator
  paints because a wood names it (`toolTiles`).
- **Mesher:** `dwell-mesh` `carve.rs` and `mesh_chunk_cut`: tests pin a notch's apex and mouth from
  its depth and `open`, a deep notch opened over the row, kerfs as slits with walls, a lying log's
  cut along x and z, two sides cut together, the surface closed (its area vectors sum to zero and
  its volume is the cube's less the wedge), and, both faces, a standing trunk's notch and kerf and a
  lying log's cut from the face's up through the chunk mesher with no bark left in the notch.
- **Client:** `MeshJob::cuts`/`Job::MeshChunk` (coded for the workers, round-trip tested),
  `ChunkStreamer::set_cut` (a cut change meshes only its chunk, a job in flight dropped; tested),
  `Cuts` messages fed to it; the decal gouges (`PuffKind::Gouge`, its shader branch) removed, the
  chips kept.
- **Seen in the browser** (bench, a trunk notched by 7 swings): the V reads as a real notch from
  the front, in profile and at an angle, its lower face lit tan, its upper face in shadow (it faces
  down: the ground's green bounce still tints it olive-brown; a terrain-lighting matter, not the
  carve).

## Built in 0.50.62: the axe clears leaves

Owner, 2026-11-09: "the axe should be able to clear leaves too."

- **Data:** a tool's `clears` (`shared/tools/tools.json`): block names its swing goes through and
  breaks; the stone axe's are `dwell:leaves` and `dwell:snowy_leaves`. Validated by
  `shared/tools/gen.mjs` (each a block of the registry, listed once, never a wood), emitted as
  `ToolDef::clears` (`clears_block`). The swing's `rustle` (sound event `axe_rustle`,
  `shared/sounds/dwell.json`) and `rustleLeaves` (leaves each cleared cell scatters, 5). A test adds a
  fixture tool clearing `dwell:tall_grass` by data alone; bad entries fail naming them.
- **The sweep** (`dwell_world::swing::sweep`, now returning `Sweep { cleared, hit }`): the head
  passes through a cell of the arc's `clears` as through a plant, recording each once with the
  step it was first met at, until it reaches another solid (`hit`: a wood, chopped as before, or
  anything else, a bounce) or the arc ends. Tests (both faces): through a leaf wall to the trunk
  behind; only leaves on the path, no hit; a fixture arc clearing stone by data alone.
- **The server** (`chopping.rs` `clear_cells`): the cleared cells are broken in one edit
  (`edit_writes_all`, `VoxelModificationReason::Edit`; flooded leaves leave their water), refused as
  a swing is (a player who can't edit; in a mode that uses items, not holding the tool, or a block
  whose `breakWith` the tool's kind doesn't meet), what they leave unsupported falls (`fell`), and
  where blocks are not unlimited each drops its block's `drops` (`drop_block`: sticks half the
  time); then the wood behind is chopped. Tests (both faces, `tests/chopping.rs`): a swing through
  leaves clears them and still cuts the trunk (red before: the leaves took the hit); through only
  leaves clears them; stone still stops the head with the leaves behind it untouched; cleared leaves
  drop sticks in adventure, nothing in creative.
- **The client** predicts the same sweep, so the hit-stop is on the wood, not the leaves; a swing
  that only clears is a miss (its follow-through, no clank). When the head reaches the first
  cleared cell (`SwingRun::take_clear`, tested) it plays the rustle once and drops `rustleLeaves`
  falling leaves from each cleared cell (`Chips::fall_leaves`, in the leaf colour of the wood whose
  `leaves.block` it is, else the first wood's). The cells themselves go when the server's edit
  arrives.

**Next:** cuts on bodies (chopping a felled piece before it settles), a give command, stamina; the
falling trunk's fling onto its chopper fixed at its root (BUGS.md #52: 0.50.37's `max_body_kick`
cap was removed in 0.50.39 on the owner's word, "we shouldn't be capping player kick like that").

## Amendment 0.52.61: the chopping prototype, behind a switch

The owner's decided rules (2026-11-11, docs/WORK_QUEUE.md item 7) run beside this ADR's chopping
while the owner tries them, off by default (`chop2` in `shared/tools/tools.json`; dev `?chop=v2`):
the chop lands at the crosshair (its row the stump's height); a chop removes a share of the section
(`tools[].chop2.bite`, along the grain `woods[].chop2.along` of it); the first side cut is the notch
and a cut on another side a flat back cut, in the same `Cut` record and wire; a standing wood falls
once more than `woods[].chop2.fellFraction` (0.5) is cut, toward the notch, else the side cut; the
split keeps the notch (`fellWedgeTrunk: open`). With the switch off nothing here changes (a test
pins it). Adopting it would replace §§2–5's swing share, bands and kerfs; see
docs/proposals/CHOPPING_REDESIGN.md §7.

## Note 2026-11-12: the swing lands at its contact (WORK_QUEUE item 25)

The server cut a swing on its arrival, so the notch appeared on the click. Now a `Sweep` (or `Swing`)
starts on arrival (the swing's wait spent; one too soon refused) and lands at its contact: `windupS` plus
the arc's time to the step it hits (or to its first cleared cell), from a sweep at arrival; then it is
swept again from the player's eye and applied as before, or dropped if the axe is no longer in hand
(`land_swings`). A bounce off a non-wood solid now wears the axe once (owner, 2026-11-12: "Tools should
only consume durability on hit"); a miss or a swing through only leaves does not. The client's swing stops
if the axe is put away before its contact. docs/BLOCK_DAMAGE.md §8.

## Note 2026-11-12: the axe's bites (WORK_QUEUE item 26)

Owner, 2026-11-12: "The tree chopping slice doesn't match the other tools style. It cuts a line all the
way across which is much bigger than the tool head. It should align more with the dirt and stone ones."
Then: "Yes axe bites proposal looks good." The cut model (§2: bands, notch, kerfs, `open_band`, the fell
and buck fractions) is unchanged, so a standing trunk still falls in 11 swings at one aim and leaves item
24's gash. What changes is the look: each landed hit also records where it struck the side
(`Cut::bites`, per side at most 12: `u` across the side, face-locally up for y, and the band cut), and
§11's carve draws a head-sized V there instead of the notch and kerf across the whole side
(`dwell-mesh` `carve/bites.rs`). A hit on a spot already bitten in its band lands beside it
(`Cut::bite`: `step` × the width from it, nearest the aim), so hacking one spot widens the bites across
the side and they join into the notch, whose cross-section is their union. Data: the wood's
`cut.bite` (`widthM` 0.14, `endsM` 0.03, `step` 0.6) and `cut.cracks` (the `wood` crack set; stages by the
share of the fell fraction cut), validated by `shared/tools/gen.mjs`. Wire and save, additive: the bites
ride after a `Cuts` message's cells only to a client that said `BitesHello` (a datagram, as
`SwingHello`), and after the `cuts` record's cells (an older record loads with none). A cut without
bites is drawn as before. ARCHITECTURE.md §8.3, BLOCK_DAMAGE.md §6.4.

## Note 2026-11-12: bites round 2, and the stump keeps its gash (WORK_QUEUE item 26)

The in-game check found the bites growing down the cell into a slot, no cracks showing, and a swing
after the fall clearing the stump's ramp (the axe's `clears` held `dwell:log_ramp`). Owner, on the
stump: "I think let's do your recommendation." So: a hit lands beside any bite on its side, both bands,
within the log's width (`Cut::bite_in`), every V in the notch's band (the cell's middle half), the grid
carved across the octagon's chamfers; the wood's cracks painted in ink at 0.85. A wood's `fellWedge` is
chopped as that wood (`chopped_wood`) and never cleared: a bite and a wear a hit, down at the fell
fraction with the wood's `buckDrops`; no tool's `clears` may name one (the generator fails it). The
cut model (§2) and felling at 11 swings are unchanged. BLOCK_DAMAGE.md §6.4 "Round 2".

## Note 2026-11-12: superseded for new hits: the axe like the other tools (WORK_QUEUE item 51)

Owner: "Also the axe doesn't behave like the other tools. I think it should." and, asked how, **"Like the
others, felling on top"**. A log now takes block damage (ADR 0078, docs/BLOCK_DAMAGE.md §12): its `damage`
data gives the axe 10 hits (stone) or 5 (metal), each hit a mark, a wear and its share of the drops, the
last breaking the block. Breaking a standing log fells the trunk above as one body toward the side struck,
as this ADR's felling did. **Superseded:** the cut model of §2 for new hits (bite, bind, kerfs, bands, the
grain share, fell and buck fractions, the fell wedges and the gash); the swing, its arc, contact, clears,
leaves shaken, sounds, chips, durability and felling into a body are kept. **Kept for old data:** the
`cuts` save table and the `Cuts` wire still load, send and draw; a cell's cut gives way to damage at its
next hit, its progress carried over. The chopping prototype (`chop2`, behind its switch) is unchanged.

## Consequences

- A new tool is an item and a `tools` entry; a new wood a block with an `axis` and a `woods` entry;
  a new tool kind (a pick, a saw) is code with its data.
- Reversing it: drop the flag; the `cuts` table stays, unread.

**Note (2026-11-12, metal tools twice as good; BLOCK_DAMAGE.md §9).** Owner: "Metal tools break double
sized bigger chunks off and reach 50% broken faster, which is when the blocks break. Also have double
durability." The metal axe already bit twice as deep as the stone axe (`biteM` 0.2 to 0.1) and fells a
standing trunk at one aim in 5 swings to 11 (`tests/chopping.rs`
`a_metal_axe_fells_in_about_half_the_stone_axes_swings_and_wears_half_as_fast`, both faces); the fell rule
(`fellFraction`) is unchanged. The chopping prototype's metal `chop2.bite` is now twice the stone axe's
(0.09, was 0.075). The metal axe now wears: 64 hits, twice the stone axe's 32 (before: never). The breaking
tiers (`breaking.tiers`) gain a `chunk`, metal's 2, which sizes the pickaxe's and shovel's marks.
