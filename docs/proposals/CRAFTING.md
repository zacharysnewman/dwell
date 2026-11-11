# Proposal: crafting (woodworking, log cabins, hauling)

> **Shelved (owner, 2026-11-11):** "That's a lot of assumptions I don't think I want to do any of those
> crafting question things for now." Nothing below is built; the small combining recipes (8 pieces to a
> slab, the stone tools) were built on their own (0.52.46, ADR 0066's amendment).

**Status:** recorded 2026-11-10 from the owner's direction; not adopted. Builds on the inventory and
drops (ADR 0066), the axe and felling (ADR 0067, `docs/FELLING.md`, `CHOPPING_REDESIGN.md`), ground
items (ADR 0071), block damage (ADR 0078, `docs/BLOCK_DAMAGE.md`), the six tools (0.52.21) and built
bodies (ADR 0052, `docs/BUILT_BODIES.md`). Content named here (boards, beams, shakes) is prototype
(`docs/ARCHITECTURE.md` §6.1); the point is the system.

## 1. The owner's direction (November 2026)

> "I want to start thinking of a crafting system… How do you make boards?… medieval fantasy"
>
> "How did people make log cabins?"
>
> "How did they transport the logs, and raise them up?"

Also from the owner this month: **tools damage blocks and drop resources per hit** (ADR 0078:
"drop some of the resource on [hit], on destroy drop the other half"); **stone vs metal tools**
(the six tools, tiers as data: stone 1.5×, metal 2.5×); **adventure vs creative** (ADR 0065;
damage is the same in both, creative has unlimited blocks and no drops).

## 2. Research: how it was done by hand

### 2.1 From tree to timber

- **Felling and bucking.** An axe notch on the fall side, a back cut leaving a hinge
  (`CHOPPING_REDESIGN.md` §3); the trunk is then **bucked** (cut across) into lengths on the ground,
  as Dwell's axe already does (ADR 0067).
- **Hewing (logs → beams).** A chalk line is snapped along the log; **scoring** cuts (overlapping,
  ~30–40° axe cuts down to the line) are made along it, the wood between is knocked off
  (**juggling**), and a **broad axe** pares the face flat to the line; repeat for each face. Used for
  square timbers in log building, timber framing, ships and railway ties.
  ([Wikipedia: Hewing](https://en.wikipedia.org/wiki/Hewing),
  [Fine Woodworking: hew a beam in 10 steps](https://www.finewoodworking.com/2024/01/25/throwback-how-to-hew-a-beam-in-10-steps),
  [awesomeaxes: hew a log](https://www.awesomeaxes.com/how-to-hew-a-log-from-trees-to-lumber/))
- **Adze.** A blade across the handle, swung toward the feet: flattens and trues hewn faces, floors
  and notches (standard joinery tool; see the Hewing article above).
- **Riving (splitting along the grain).** Wedges and a maul split a log in halves and quarters;
  a **froe** (a blade with a handle at right angles), driven by a wooden club and levered, rives a
  short bolt into thin **shakes**, clapboards and pickets. Riven pieces follow the grain end to end,
  so they are strong and shed water; straight, knot-free green wood (cedar, oak) rives best; a
  **brake** holds the bolt.
  ([Fine Woodworking: tools and techniques to rive wood](https://www.finewoodworking.com/2026/08/13/from-the-archives-the-tools-and-techniques-to-rive-wood),
  [Wikipedia: Wood splitting](https://en.wikipedia.org/wiki/Riving),
  [Mother Earth News: shingle a house](https://www.motherearthnews.com/diy/shingle-a-house-zmaz87mjzgoe/))
- **Pit-sawing (logs → boards).** The log is rolled over a pit (or onto trestles); a **top sawyer**
  on the log steers a long two-man saw along a chalk line, a **bottom sawyer** in the pit pulls the
  cut down. Two men made about a dozen boards a day; a water-powered mill could make ~200, which is
  what replaced it. ([Wikipedia: Saw pit](https://en.wikipedia.org/wiki/Saw_pit),
  [Core77: saw pits](https://core77.com/posts/53118/A-Brief-History-of-Wood-Splitting-Technology-Part-2-Saw-Pits),
  [Building Conservation: conversion of structural timbers](https://buildingconservation.com/articles/structural-timber-conversion/structural-timber-conversion.htm))

So boards come three ways, by cost: **riven** (cheap, short, rough), **hewn** (a beam, one per log),
**sawn** (many boards per log, a station and two people or a mill).

### 2.2 Log cabins

- **Logs** round or hewn on two faces; a **foundation** of stone piers or a stone sill under the
  corners keeps the bottom logs off wet ground.
- **Corner notches:** the **saddle notch** (a rounded cradle over the log below; oldest and easiest,
  round logs, ends stick out), the **V/steeple notch**, the **half dovetail** (most common in
  Appalachia; stable and easier than a full one) and the **full dovetail** (tightest, most work;
  hewn logs, flush corners). Notches lock the walls without nails and shed water outward.
  ([Tennessee Encyclopedia: Log Construction](https://tennesseeencyclopedia.net/?p=1149),
  [NPS: Restoration Guide for Historic Log Buildings](https://www.nps.gov/trte/learn/historyculture/upload/Restoration-Guide-for-Historic-Log-Buildings-508.pdf),
  [The Henry Ford: historic log buildings conservation](https://www.thehenryford.org/docs/default-source/caring-for-your-artifacts/the-henry-ford-historic-log-buildings-conservation.pdf))
- **Chinking and daubing:** the gaps between logs are packed with wood or stone chinks and daubed
  with mud or clay mixed with lime and hair. (same sources;
  [designingbuildings: Log construction](https://www.designingbuildings.co.uk/wiki/Log_construction))
- **Roof:** gable logs shortened up each course, **purlins** (logs) running between the gables, and
  riven **shakes** (clapboards) laid across them, held by weight poles before nails were common.
  ([Gutenberg: Building with Logs (USDA Forest Service, 1920s)](https://gutenberg.org/cache/epub/59380/pg59380-images.html),
  [Fine Homebuilding: raising a hand-hewn log cabin](https://finehomebuilding.com/2005/09/01/raising-a-hand-hewn-log-cabin))

### 2.3 Moving and raising logs

- **Skidding:** oxen or horses dragged chained logs along skid roads (trails of greased cross
  poles), often in winter over snow and ice, on **sledges**; by hand, on **rollers** and with
  levers and hand spikes.
- **River driving:** logs were piled on banks (rollways), rolled in at the spring flood, driven
  downstream with pike poles, bound into **rafts** on slow stretches, and caught at the mill by a
  **boom**. ([Wikipedia: Log driving](https://en.wikipedia.org/wiki/Log_driving),
  [MNopedia: Commercial Logging](https://www.mnhs.org/mnopedia/search/index/thing/commercial-logging-1839-1949))
- **Raising the walls:** two poles leaned against the wall as **skids** (a ramp); ropes fastened to
  the top of the wall, passed **under and around the log** and pulled from above roll it up the skids
  (a **parbuckle**: the log is its own pulley, halving the pull), helped by hand spikes and forked
  sticks; a whole neighbourhood came to a **raising**.
  ([Building with Logs](https://gutenberg.org/cache/epub/59380/pg59380-images.html),
  [Electric Scotland: the log cabin, ch. 4](https://electricscotland.com/hiStory/logcabin/chapter4.htm),
  [Farm Collector: house raising](https://www.farmcollector.com/farm-life/house-raising-zmcz20julzbut/))
- **Lifting gear:** a **gin pole** (one guyed mast with a pulley) or **shear legs** (two poles lashed
  in an A) with a **block and tackle** lifts straight up; big works used **treadwheel cranes**, which
  lifted vertically and had no brakes. ([Wikipedia: Treadwheel crane](https://en.wikipedia.org/wiki/Treadwheel_crane),
  [Low-tech Magazine: human-powered cranes](https://www.lowtechmagazine.com/2010/03/history-of-human-powered-cranes.html);
  gin poles and shear legs are standard rigging, no single source checked)

## 3. How it could map to Dwell

### 3.1 Two ways to craft, and the recommendation

| | **In-world work** (the process is the craft) | **Recipe menu** (inputs → output in a list) |
|---|---|---|
| Feel | the research above, in the hands: hew, rive, saw, haul, raise | quick, familiar, abstract |
| Fits Dwell | uses what is built: tools damage cells, logs are bodies, felling, built bodies, physics | a UI over the inventory; ignores bodies |
| Cost | each process is a small mechanic (a phase each) | one system for everything |
| Risk | many verbs to learn; must stay readable | content from other games creeps in |

**Recommendation: both, one data system.** Big wood (logs, beams, boards, joints, hauling) is
**in-world work** on bodies and blocks with tools; small things (a mallet, a froe, a wedge, rope,
a sledge) are **recipes at a station or in the hand**, so the player isn't sawing every peg. Both are
entries of one **processes** table (§3.3); a recipe is just a process whose action is "use the
station".

### 3.2 The processes, mapped

1. **Bucking** (built, ADR 0067): a felled log body cut across into lengths. Today a freed length
   drops `dwell:wood`; with crafting it can stay a **log body of a known length** (1–4 m) instead.
2. **Hewing → a beam:** aim at a lying log body with an axe, **score** (strikes across, as the
   chopping notch), then hew along it with a **broad axe** (or adze): each face goes flat; four faces
   make a `dwell:beam` body (square section). Each hit's chips drop per hit (ADR 0078's hit share:
   hewing wastes wood as chips/kindling). The look is the octagonal log clipped flat per face
   (ADR 0072's carve), so the work shows.
3. **Riving → shakes and boards:** a short bolt (a ½–1 m log body or block) set upright; a **froe and
   mallet** (or wedges and a maul for halves/quarters) splits it along the grain. Each split is a hit
   that turns one cell-sized piece into N items (`dwell:shake` ×4) or two half-logs. Grain from the
   log's `axis` property (ADR 0067), so only along-grain splits work.
4. **Sawpit → boards:** a **station** (a built structure: a pit or trestles, recognised by a small
   pattern rule, or a placed `dwell:sawpit` block pair), a log body rolled onto it, and a two-hand
   saw: hold to saw; each stroke advances the kerf; a cut frees a board. **Two players saw faster**
   (top and bottom sawyer), one alone slowly: a multiplayer moment from history.
5. **Notched joints → walls:** a log or beam body placed with a **notch** (saddle, half dovetail) as
   its placement shape. Placing a notched log over another at a corner **snaps** to the corner (a
   placement rule like ITEMS.md's `attach`) and joins the cabin as a structure; an unnotched log just
   lies there (a body that rolls). Proposal: walls are **grid blocks** once placed (cheap, settled,
   collision exact) with a log-wall look per course, not live bodies.
6. **Chinking:** a clay/moss item applied to the gap between courses: the wall reads sealed (look,
   and later: draughts, warmth).
7. **Roof:** purlins as beams between gables (placed like logs), shakes placed as a roof ramp
   (the stored ramps, `HALF_HEIGHT_GRID.md`) in a shake look.
8. **Hauling:** logs are bodies (Rapier, mass from density, BUILT_BODIES §5–6): **push and roll**
   them (exists), drag them with a **rope** (a joint between the player, or a draft animal later,
   and the body), on a **hand sledge** or **rollers** (built bodies with low friction / round cells),
   **float them down a river** (bodies already float; the water grid's current carries them, Phase
   53). A **raft** is logs lashed into a built body.
9. **Raising:** **skids** (two beams leaned against the wall: a ramp of bodies or blocks) and a
   **parbuckle rope** (a rope from the wall top, under the log, back to the player: the pull is
   halved and applied as a force on the log body's both ends); later shear legs and a block and
   tackle (a station that multiplies the pull, `mechanicalAdvantage` as data).

### 3.3 Data vs code (the extendable system)

**Data** (a new `shared/crafting/crafting.json`, its `gen.mjs` emitting Rust for `dwell-protocol` and
TS for the shell, validated, CI stale check — the pattern of `shared/tools/gen.mjs`):

- **materials and pieces:** `dwell:log` (lengths), `dwell:beam`, `dwell:board`, `dwell:shake`,
  `dwell:bolt` — items (`shared/inventory`) or body shapes, by name.
- **processes:** `{ "name": "dwell:hew", "on": "body|block|item", "input": "dwell:log",
  "action": "hew|rive|saw|split|recipe", "tool": "broad_axe" (class) or an item,
  "station": null | "dwell:sawpit", "hits": { "stone": 12, "metal": 6 }, "output":
  [{ "item": "dwell:beam", "count": 1 }], "waste": [{ "item": "dwell:chips", "count": [2, 4] }],
  "grain": "along|across|any", "people": { "max": 2, "speedup": 1.8 }, "modes": ["adventure"] }`.
- **tiers:** a process names its tool by class; the class's tier speed (`breaking.tiers`, already
  data) scales hits; a process may require a minimum tier (`"minTier": "metal"` for the sawpit's
  saw, say).
- **joints:** notch kinds as data (`saddle`, `half_dovetail`): which piece, which corner shape it
  snaps to, the wall block it becomes.
- **stations:** a block (or pattern of blocks) by name and which processes it hosts.
- **hauling gear:** rope length and strength, `mechanicalAdvantage` of a parbuckle (2) and a tackle
  (2–4), sledge friction.

**Code** (a small fixed vocabulary): the action kinds (`hew`, `rive`, `saw`, `split`, `recipe`), how
each reads aim, grain and progress (built on ADR 0078's per-cell damage and ADR 0067's cut model), the
body operations (turn a log body into a beam body, split it), the joint-snap rule, the rope joint
and its pull force. A new process of an existing kind is data; a new kind is code, rare.

**Who adds, with what:** the owner or a mod, by editing `crafting.json` (validated by `gen.mjs`, a
`gen.test.mjs` fixture process added by data alone). A **Crafting Lab** page in `docs/prototypes/`
(a table of processes, inputs/outputs as a graph so dead ends and loops show, a preview of each piece)
is the editing tool, exporting the JSON — like the Biome Sandbox proposal. Per build: the table.
Per world: the bodies, stations and walls built. Per player: nothing new (inventory exists).

### 3.4 Adventure vs creative

- **Creative:** every piece (beam, board, shake, notched log, wall) is on the palette, placed
  directly; processes still work (to try them) but cost nothing and drop nothing.
- **Adventure:** pieces come only from processes; the tools themselves come from recipes
  (a stone axe from a rock + stick: the first recipe, using today's ground items); metal tools need a
  later metal chain (ore, smelting: not in this proposal).

### 3.5 Both faces

Everything is face-local: aim, grain (the `axis` property), the rope's pull and skids' slope are
in the body's face frame through `FaceFrame`; parity tests with `both_faces!` per process.

### 3.6 Compatibility

New items and blocks are not breaking (ADR 0038). Log bodies of a named length and beam bodies add a
body shape to the saved world: an optional record if older builds can ignore it, else the next
minor (to decide in the ADR).

## 4. Phases (≤ 45 min each)

- **K1: data.** `shared/crafting/crafting.json` + `gen.mjs` (validation, emitted tables, a fixture
  process by data alone); prototype items `dwell:board`, `dwell:beam`, `dwell:shake`, `dwell:chips`.
- **K2: hand recipes.** The `recipe` action: a small craft panel in the backpack (adventure) for
  processes with no station; the first: rock + stick → stone axe. Server checks and applies.
- **K3: riving.** Froe + mallet (tool data) on an upright bolt block: hits split it along the grain
  into shakes or halves; per-hit drops through ADR 0078.
- **K4: bucking to lengths.** A bucked length stays a log body (length named), not `dwell:wood`;
  pick up a short one as an item.
- **K5: hewing.** Score and hew a lying log body face by face; the beam body; the flat-face carve on
  the octagon.
- **K6: sawpit.** The station block, a log body on it, hold-to-saw, boards out; a second player
  speeds it.
- **K7: notched logs.** Saddle notch as a placement shape; corner snap; a log-wall block per course.
- **K8: chinking and roof.** Chinking item on wall gaps; purlins; shakes as roof ramps.
- **K9: rope.** A rope item: a joint from the player to a body; drag a log; a hand sledge as a
  built body.
- **K10: raising.** Skids (lean a beam: a ramp), the parbuckle (halved pull up the skids), data.
- **K11: river and rafts.** Log bodies carried by the water grid's current; lashing logs into a raft.

Each phase is its own patch release; K4–K5 may need the new compatibility line (§3.6).

## 5. Questions for the owner

1. **In-world work or a menu?** (a) everything in the world, (b) a recipe menu only, (c) big wood in
   the world, small items by recipe. **Recommend (c)**: the hauling and building is the fun the
   research points to; nobody wants to saw pegs.
2. **Where do walls end up?** (a) placed logs become **grid blocks** (log-wall look, cheap, exact),
   (b) they stay **bodies** joined into a built body (could fall down, costly). **Recommend (a)**,
   with (b) only for things meant to move (rafts, sledges).
3. **How much do notches matter?** (a) one generic notch that snaps at corners, (b) saddle and half
   dovetail as two tiers (round vs hewn logs, look and strength), (c) cosmetic only. **Recommend (b)**,
   saddle first.
4. **Two-person work:** should some processes (the pit saw, raising heavy logs) be **faster with two
   players** (a) or need two (b), or be solo (c)? **Recommend (a)**: solo always possible, company
   faster — a host-a-friend game.
5. **Stone vs metal:** which processes need metal? (a) none, metal only faster, (b) saws and froes
   need metal, stone axes can hew roughly, (c) a stone age with no boards. **Recommend (b)**: boards
   and shakes are the reward for reaching metal; riving with wedges (wood/stone) stays stone-tier
   for halves.
6. **Draft animals:** hauling by (a) players only (ropes, rollers, sledges, rivers), (b) animals too,
   once creatures (ADR 0073) can be led. **Recommend (a) now**, animals as a later phase.
7. **How physical is raising?** (a) the parbuckle and skids as real forces on bodies, (b) an assisted
   "raise" action once skids are in place, (c) place logs directly from inventory at any height.
   **Recommend (a) for adventure, (c) for creative.**
8. **Is the first slice right?** K1–K3 (data, hand recipes, riving shakes), then K4–K7 to the first
   log cabin wall, then hauling. Or hauling first?
