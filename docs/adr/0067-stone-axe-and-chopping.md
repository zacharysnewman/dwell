# 0067 — The stone axe: aimed chopping, notches, felling past half and bucking

**Status:** Accepted (2026-11-08; data, server and client input built in 0.50.34; the owner's
answers, the gouges, chips, arms and the axe on the creative hotbar in 0.50.36, see "Built so far"). Builds on ADR 0066 (items by
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
   `kind` from a fixed vocabulary (`axe`), `swingSeconds` (0.45), `biteM` (0.12: metres a swing
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
   cell goes (the wood is chips) and FELLING's integrity takes what it held as a piece, **tipped
   toward the notch** (its deepest side), as real felling falls (owner, 2026-11-08, Question 1): the
   uncut side is the hinge, the notch side opens and the trunk tips into it, toward the chopper who
   stands at the notch. The stump below stays.
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
     (0.50.38): the swing's path and held tools (`Sweep`, `Held`, `HeldTools`).
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
11. **The client draws** (0.50.36, `dwell-client` `chips.rs`, draw-only), all through the
    billboard pass the blasts' puffs use (render/blasts.rs, `PuffKind::Chip` and `Gouge`):
    - **Gouges** (chosen as the cheapest that reads: a decal, not a remesh): per cut cell and side,
      the notch as a quad lying on that side's face (lifted 4 mm), centred along the grain and
      opened 2.5× its depth, shaded as a wedge (its upper cut face in shadow, its lower lit, darker
      along the deep middle and the deeper it is), and each band's kerf past the notch as a dark
      slit at the band's middle; in the wood's `chips.color`. Face-local through `FaceFrame`.
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
6. **Numbers.** 9 notching swings, 28 hacking (3.1×; the owner asked 3–4×: a smaller `bindM` raises it), 0.45 s a swing; 0.55 of the cross-section to fell
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
- **Visuals:** `chips.rs` (gouges for a known cut, both faces; chips per swing: count, out of the
  face, along the swing, falling, a CPU third), `PuffKind::Chip`/`Gouge` through the billboard
  pass and its shader (`chip_colour`, validated with naga in both tiers), the chop clips and the
  held axe (`shared/characters`), the hotbar picture.

## Built in 0.50.38: the swing as weighty melee

The owner (2026-11-08): "This axe is a lot more like Dying Light melee mechanics than Minecraft":
weighty first-person physical swings (the mechanics only). Built:

- **The swing as data** (`tools.json` `swing`, validated, a fixture tool by data alone):
  `windupS` 0.12, `activeS` 0.14 (the arc), `hitStopS` 0.08, `recoverHitS` 0.22, `recoverMissS`
  0.36, `reachM` 2.2, `arcDeg` 110, `samples` 12, `kickDeg` 1.6, `staminaCost` 8 (unused until
  stamina exists: swinging, swimming and sprinting will draw on it), `swish` and `bounce` sound
  events. A miss must last at least `swingSeconds` (the server's wait).
- **The path is what hits; free swings.** A swing goes any time, at nothing too. `dwell_world::swing`
  sweeps the head's arc face-locally (level: the screen's right to its left; vertical: top to
  bottom); the first solid cell is hit. The client sends `Chop::Sweep` (look, plane) to a server
  with `CHOP_SWEEP` (64), which sweeps the same arc from its own eye and cuts a wood it meets, or
  spends the swing (a bounce off stone, a miss). An older server gets the aimed `Swing` when the
  path meets a wood. Tests: an arc passing a trunk cuts it though the crosshair misses; a nearer
  stone post on the path takes the hit; free swings and stone do nothing and spend the swing (both
  faces). A sweep hit's face is converted to the chopping wire's `axis × 2 + positive` (`SwingHit::axis_face`; red → green in the browser and `tests/chopping.rs`: the notch went on the far side).
- **The feel** (`chop.rs` `SwingRun`, tested): the arms' clip (timed to a miss) is held at the
  impact for the hit-stop and played back to the rest after a hit, runs through its follow-through
  on a miss; the camera winds back, sweeps, jolts against the swing at a hit (harder off stone);
  the impact shows chips, the hit's sound by the cut (`sounds`: bark thud, wood chunk, creaks from
  40%), shakes leaves from the tree's leaf blocks (`leaves`), or clanks off stone (no effect).
- **The axe the right way round, two holds, a flint head**: `stone_axe` and `stone_axe_vertical`
  (`held`, `heldVertical`) with an `axe_blade` edge; red → green: `dwell-characters`
  `tests/axe.rs` failed with the blade at (−0.24, −0.30, −0.92) in view space (toward the eye);
  now it faces away in both holds, the holds differ and the edge leads the strike. The head is the
  `flint` material (a warm grey-brown ramp), not the ground stone's violet shadow.
- **Cuts that read as cuts**: the notch is a V, each face lit by its own normal (`Gouge::cut_normals`,
  packed into the billboard's info bits; both faces tested), fresh tan wood (`#d9b37a`) keeping its
  hue in shade, end-grain rings, a crease at the apex darkening with depth, a bark lip; it opens
  wider and deeper as the notch deepens. Chosen over a remesh as the cheapest that reads as depth.
- **Progress you can feel**: the tree leans toward its notch from 40% of the fell fraction, more
  toward the fall (`sway`, `TreeLean`, a shiver after each hit; the terrain's vertex shaders), the
  hit sounds deepen, leaves fall each hit.
- **The reticle** draws the swing's line (level or vertical) through the crosshair.
- **Others see the held axe**: `Chop::Held` (tool by name, the hold) and S→C `HeldTools` (90),
  only to clients that sent a `Held`; tested on the server and the client's drawing.

**Next:** cuts on bodies (chopping a felled piece before it settles), a give command, stamina; the
falling trunk's fling onto its chopper fixed at its root (BUGS.md #52: 0.50.37's `max_body_kick`
cap was removed in 0.50.38 on the owner's word, "we shouldn't be capping player kick like that").

## Consequences

- A new tool is an item and a `tools` entry; a new wood a block with an `axis` and a `woods` entry;
  a new tool kind (a pick, a saw) is code with its data.
- Reversing it: drop the flag; the `cuts` table stays, unread.
