# Dwell — Characters and Creatures

The design of Dwell's characters: the player's first-person arms and the full body other players
see, and the first two creatures, a wolf and a deer, in the same style. Characters are made
procedurally from data: chunky parts (boxes and rounded shapes) on a small rig, painted with the
shared palette and materials of [`ART_DIRECTION.md`](./ART_DIRECTION.md). Adopted by
[ADR 0041](./adr/0041-characters.md); Phase 40 of [`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md).

**Status.** A design and a concept only (owner, 2026-10-20): this document, the data file
(`shared/characters/characters.json`), its validator and tests, and the
[Character Lab](./prototypes/character-lab.html) prototype page. Nothing is drawn in the game yet;
§9 is what building it in will take. The characters themselves (their proportions, outfits,
colours and clips) are the first art direction, still prototype in scope (`ARCHITECTURE.md` §6.1):
the system is what this phase fixes, and the content grows by data.

## 1. The owner's answers (2026-10-20)

| Question | Answer |
| --- | --- |
| Camera | First person only. A third-person camera is out of scope for now. |
| What to make | A design doc plus a concept (art direction, palette, silhouettes, a prototype page). Building it into the game comes later. |
| How characters are made | Procedurally from data: simple chunky parts (boxes and rounded shapes) in a data file, with proportions, parts, colours and accessories such as hats, capes and backpacks. An editor or prototype page can tweak them. No external modelling tool. |
| Held in first person | The hands, holding the hotbar's block or the brush tool, with simple swings for break and place. Tools such as pickaxes come later with an item system. |
| Who | The player's first-person arms and hands; the full body other players see; a wolf and a deer ("so we can get a cohesive style together"). |
| Style | Bright, painterly storybook; readable silhouettes; chunky stylised gear; clean shaders without visual noise. The wolf lean, with pointed ears and a bushy tail, a readable predator; the deer with slender legs, antlers as a part variant and a soft palette. |
| Textures | One procedural texture generator for everything, characters included, shared with the world (ADR 0040). |

**What gets added later, by whom and with what** (CLAUDE.md's questions, answered from the brief and
recorded here for the owner to correct in §10):

- *More of the same kind*: parts, accessories, outfits (variants), held items, clips, creatures. By
  the team, in the data file, with the Character Lab (it edits parts, colours and accessories live
  and copies the JSON back). Players pick from them later (§9.4), they don't author them.
- *New kinds*: a new body plan is a new rig in data (bones and joints); a new part shape, joint
  kind or animation rule is code (§3), deliberately rare.
- *Fixed in code*: the part kinds, joint kinds, easing, layers, places and the run-time materials
  (§3). *Data*: everything else.
- *Scope*: per build (the data ships with a release, like the block registry). A player's chosen
  look is per player (§9.4).

## 2. Art direction for characters

The world's style ([`ART_DIRECTION.md`](./ART_DIRECTION.md)) applies unchanged: the same palette
ramps (cool shadows, warm lights), the same brush materials, the same lighting assumptions (Lambert
sun and hemisphere, ACES, fog). What is particular to characters:

- **Chunky and readable.** Big heads (a fifth of the height: the player's head is 0.34 m of 1.8 m),
  big hands and boots, thick limbs, no thin detail. Every part is a box, a rounded box, an ellipsoid,
  a cylinder, a capsule or a cone. Read the shape first: the Lab's *Silhouette check* draws
  everything in flat ink.
- **Silhouette breakers.** The torso is broken up by gear: a wide-brimmed hat, a pointed hood, a
  cape, a backpack with a bedroll, braids, a scarf, a satchel strap. Each look (variant) has one
  distinct head shape and one distinct back shape, so players tell each other apart at a distance.
- **Colour against the ground.** Clothes use the wool reds and blues, brass, linen, canvas and
  leather; never large areas of meadow or leaf green, which vanish against grass. Skin and hair
  tones are their own ramps (four each).
- **Clean shading.** A part is shaded by its material's ramp (shadow → base → light by the light's
  angle, a half-Lambert wrap), with the material's brush tile blended in at its `overlay` (skin 0.1,
  cloth 0.35–0.5, fur 0.6, metal 0.7), and a faint warm rim. No noise, no specular, no normal maps. A
  thin `ink_soft` outline (an inverted hull) keeps the silhouette crisp; it is a look option, not
  data.
- **Creatures share the proportions language.** The same chunky parts, the same big readable heads
  and paws, the same materials (`fur_wolf`, `fur_deer`, `antler`). The **wolf** is lean with a deep
  chest, a ruff and a pale bib, a long muzzle held level with its back, tall pointed ears, angled eyes
  and a bushy tail with a dark tip: a predator's silhouette (head low, forward). The **deer** stands
  tall on slender legs (lower legs 5 cm thick), with a long neck, a cream belly, throat and rump, a
  white tail flash (`deer_cream.light`), big soft ears and eyes, and antlers as an accessory (the stag
  wears them; the doe doesn't).

## 3. The vocabulary: fixed in code

The engine behind the data (CLAUDE.md, "extendable system") is small and fixed, in
`shared/characters/rig.mjs` (and later its Rust twin, §9.1):

| Kind | Values |
| --- | --- |
| Part kinds | `box`, `round_box` (`round`: 0–0.5 of the smallest side), `ellipsoid`, `cylinder`, `capsule`, `cone` (`tip`: the top radius, 0–1, 0 a point). Every kind fills its `size` box, its axis along y. |
| Joints | `ball` (x, y, z), `hinge` (one `axis`), `fixed`; optional limits in degrees per axis. |
| Easing | `linear`, `smooth` (smoothstep per segment), `spline` (Catmull-Rom through the keys; loops wrap), `step`. |
| Layers | `base` (idle, walk, trot, graze), `hold` (an arm holding an item), `action` (a swing). A higher layer replaces the bones it keys, at its weight. |
| Spaces | `body` (a clip on the full body) and `arms` (on the first-person arm chain, in view space). |
| Views | A part is drawn in `both` views, the `body` only, or the first-person `arms` only (extra detail). |
| Places | Where an accessory sits: `head`, `hair`, `face`, `neck`, `shoulders`, `back`, `back_top`, `waist`, `hands`, `wrists`, `feet`, `antlers`. One accessory per place. |
| Run-time materials | `$block`: the held block's own painted tile (the brush's tuft takes it too). |

Conventions: metres; +y up, +z the way the character faces, +x its left; rotations are Euler XYZ in
degrees (three.js's default order), so +x pitches a bone's top toward +z and a hanging limb swings
back. Left-side names end in `_l`; a part with `"mirror": true` gets a right-side copy (`_r`) with x
negated, on the mirrored bone or socket.

## 4. The data format

One file, `shared/characters/characters.json` (`format: 1`). Materials are **names** from the shared
style (`shared/textures/materials.json` and `palette.json`): a material (`"leather"`), a ramp or
one stop of it (`"brass"`, `"leather.shadow"`), a swatch (`"ink"`), a colour slot (below) or
`$block`. Never a hex colour.

- **`rigs`**: body plans. A rig is a list of bones, root first, each `{ name, parent, joint, axis?,
  limits? }`. `biped` (16 bones: hips, chest, head, arm–forearm–hand and thigh–shin–foot per side)
  and `quadruped` (21: body, chest, neck, head, ears, a two-bone tail, three bones per leg).
- **`characters`**: each `{ name ("dwell:player"), rig, height, bones, sockets, slots, parts,
  accessories, variants, clips, firstPerson? }`.
  - `bones`: every rig bone's rest offset from its parent's pivot: the proportions. The wolf and the
    deer share a rig and differ only here and in their parts.
  - `sockets`: named attachment points `{ bone, at }` (`head_top`, `back`, `grip_r`, `antler_l`).
    Accessories and items attach to sockets, so they fit any character with those sockets.
  - `slots`: colour slots → materials (`"tunic": "cloth_wool_blue"`). Parts name a slot; a variant
    or the Lab recolours by changing the slot.
  - `parts`: `{ id, on (bone or socket), kind, size, at?, rot?, material, round?, tip?, mirror?,
    view? }`.
  - `accessories`: the default look's accessory names; `variants`: named looks `{ label, accessories,
    slots }` (the player's *wanderer*, *ranger*, *tinker*; the deer's *stag* and *doe*).
  - `clips`: roles → clip names (`idle`, `walk`, `hold`, `break`, `place`; the wolf's `trot`, the
    deer's `graze`).
  - `firstPerson`: the arm chain (`["arm_r", "forearm_r", "hand_r"]`), its `anchor` in view space
    (metres from the eye), the arms view's `fov`, and its clips (§5).
- **`accessories`**: `{ label, place, fits (rigs), slots (its own colour slots and defaults), hides
  (body part ids it covers), parts }`. A hat hides the hair's top; gloves hide the hands.
- **`items`**: things held in hand: `{ label, on (a socket), parts }`. `block` (a 0.2 m cube of
  `$block`) and `brush` (a wooden handle, a brass ferrule, a tuft in the selected block's colour).
- **`clips`**: `{ rig, space?, layer, duration, loop, ease, cycle?, keys }`. A key is `{ t, bones:
  { bone: [x, y, z] degrees }, root?: [x, y, z] metres }`; each bone interpolates between the keys
  that name it, so a key lists only what moves at that moment. `cycle` is the distance travelled per
  loop: a gait plays at `speed / cycle` loops a second, so feet don't slide.

A part, from the file:

```json
{ "id": "boot_l", "on": "shin_l", "kind": "round_box", "size": [0.17, 0.22, 0.19],
  "at": [0, -0.3, 0], "round": 0.3, "material": "boots", "mirror": true }
```

**Validation** (`rig.mjs` `validate`, run by `gen.mjs` and the tests). A bad entry fails with its
path, e.g. `characters.player.parts[0] (head).kind: "torus" is not one of box, round_box, …`.
Checked: unknown keys, names, shapes and their parameters, bones and sockets that exist (mirrors
included), materials that resolve, repeated ids, every rig bone given a rest offset, accessories
that fit the rig and don't share a place, slots that exist, hidden parts that exist, clips on the
right rig and space, keys in time order, hinges turned about their axis only, joint limits, the
first-person chain connected and its clips keying only it — and **the rest pose**: feet on the
ground and the body (without accessories) as tall as `height`, within 3 cm.

## 5. The first-person arms

The arms are a **separate view** of the same character, as first-person games do it:

- **The same parts.** The arms view draws the parts on the `firstPerson.bones` chain (and the
  accessories and held item on them) from the same composed model as the full body, with the same
  materials and shading. Nothing is duplicated, so the arms match the body exactly; parts marked
  `"view": "arms"` add close-up detail (knuckles) the body doesn't need, and `"view": "body"` parts
  are left out. A test pins that every body part on the chain is in the arms view.
- **Its own camera and pass.** The chain's first bone is placed at `anchor` in view space, not on
  the chest; there is no head or torso in the view at all, so nothing clips the camera. The arms are
  drawn after the world over a cleared depth buffer, with their own FOV (`fov`, 62°) and near plane
  (2 cm), so they never clip into walls and keep their size when the world's FOV changes (the speed
  FOV, the spyglass). The sun is the world's, turned into view space, so the arms are lit like the
  body would be.
- **Its own clips.** `fp_idle` (the arm held into view, breathing), `fp_walk` (a bob on the anchor
  at the walk's cycle), `fp_break` (a wind-up and a chop down-forward, 0.5 s), `fp_place` (a push
  forward, 0.36 s). They key the same bones as the body's `hold`, `break` and `place`, so a swing
  seen by others is the same gesture.
- **Held items.** The hotbar's block (its own painted tile on every face) or the brush tool, on the
  `grip_r` socket. Tools come later with an item system: an item is data (`items`), so a pickaxe is
  an entry, and its swing a clip.

## 6. Poses and animation

Clips are keyed rotations on the rig (and a root offset for bob), sampled and layered:

| Character | Base | Hold | Action |
| --- | --- | --- | --- |
| Player (body) | `biped_idle` (breathing, 3 s), `biped_walk` (1 s, 1.6 m a loop: legs, counter-swinging arms, hips and chest twist, bob) | `biped_hold` (the right forearm forward) | `biped_break`, `biped_place` |
| Player (arms) | `fp_idle`, `fp_walk` | (in the base) | `fp_break`, `fp_place` |
| Wolf | `wolf_idle` (tail sway, head turns, an ear flick), `wolf_trot` (0.56 s, 1.4 m: diagonal pairs, a suspended bob, the tail up and swaying) | | |
| Deer | `deer_idle` (head turns, ear and tail flicks), `deer_walk` (1.2 s, 1.1 m: four beats, left hind, left fore, right hind, right fore), `deer_graze` (6 s: head down to the grass, nibbling, a look up with ears forward, down again) | | |

Layering is `base`, then `hold` (when an item is held), then `action` (a swing), each replacing the
bones it keys: the player walks with the block held in front and swings without stopping. A test
samples every base clip and checks the lowest point stays within 6 cm of the ground (no foot through
the floor, no floating).

In the game the state comes from what the controller already has (§9.3): `idle` or `walk` from the
player's state and speed (walk plays at `speed / cycle`), `hold` from the hotbar, `break` or `place`
from an edit. Crouch, swim, climb, fall, slide and the death pose are later clips (§10, question 4).

## 7. Tools: generator, tests, the Character Lab

- **`shared/characters/gen.mjs`** validates the data against the shared style and writes the data,
  the style and the two modules (`rig.mjs`, and `shared/textures/paint.mjs`, the painter) into the
  Lab page between its generated markers. CI regenerates the page and fails on any diff, so editing
  `characters.json`, the palette or the materials means running `node shared/characters/gen.mjs`.
  When the game draws characters it also writes the Rust tables (§9.1).
- **`shared/characters/gen.test.mjs`** (`node --test`): the data valid and the page fresh; 26 bad
  entries each failing with its message; a part, an accessory and a variant **added by data alone**
  composed and worn (a mirrored pauldron reaches past the shoulder; a top hat hides the hair and
  stands on the head); the held item on its socket and the arms view sharing the body's parts;
  sampling, looping and layering; feet on the ground in every base clip.
- **The [Character Lab](./prototypes/character-lab.html)** (three.js, standalone, publishable):
  the player full body, the first-person view (a little world, the hotbar, the arms in their own
  pass), the wolf, the deer and a group to scale with 1 m blocks; orbit, front/back/side/¾ views and
  a turntable; clip playback with a scrubber and speed, and break and place swings; looks (variants),
  accessories by place, colour slots, the held item and block; a parts list (click a part in the view
  to select it) with live editing of every field, duplicate, delete and add; the silhouette check,
  brush strokes, outline, bones and sockets, and the game's tone mapping; validation by the same
  `rig.mjs`; copy the whole file or one character back as JSON.

To add, by data: a **part** (an entry in a character's `parts`), an **accessory** (an entry in
`accessories` with its place and sockets; add it to a variant), an **outfit** (a variant: accessories
and slot colours), an **item** (an entry in `items` on a socket), a **clip** (an entry in `clips`,
named by a role), a **creature** (a character on an existing rig with its own proportions and
parts; or a new rig first).

## 8. Data flow

```
shared/textures/palette.json, materials.json ─┐
shared/characters/characters.json ────────────┼─ gen.mjs (rig.mjs validate) ─► docs/prototypes/character-lab.html
shared/characters/rig.mjs, textures/paint.mjs ┘                              (later: dwell-look characters.gen.rs)
```

## 9. Building it into the game (later)

None of this is built; it is what the next phases need, recorded so the data is shaped for it.

### 9.1 Meshes and rendering

- **Rigid parts, not skinning.** Each part is a rigid mesh on its bone: no weights, no vertex
  skinning, no bind pose. Chunky parts read better rigid (the storybook look has visible joints),
  the data stays a list of primitives a tool can edit, and a character draws as one instanced batch
  (one instance per part: a bone matrix times the part's matrix). Skinning would need weights a tool
  can't reasonably author, against the extendable-system rule.
- **Generated meshes.** A Rust module (`dwell-look` or a new `dwell-characters`) builds each kind's
  unit mesh once (rounded boxes per `round` bucket), and `gen.mjs` writes `characters.gen.rs` (rigs,
  characters, accessories, items, clips) the way `shared/look/gen.mjs` writes its tables; the game
  resolves material names against the painterly set (ADR 0040).
- **Shading.** A character shader in the raster renderer: the ramp shading of §2 with the material's
  tile at its overlay, then the frame's tone mapping and fog like terrain; the outline as a second,
  back-facing draw. The ray marcher (ADR 0029) draws characters with the raster pass over its depth
  (as it does other overlays, A3c).
- **The arms pass.** After the world and the water, before the HUD: clear depth, draw the arm chain
  with its own projection (`fov`, a 2 cm near plane), the sun turned into view space. Not drawn while
  dead (the body is drawn instead, as today).
- **Distance.** Past ~64 m a character drops its outline and small parts (eyes, buckles: a size
  threshold, data-free); past the far view it isn't drawn.

### 9.2 Today's remote players

Remote players are drawn now as a capsule with a visor (`render/geometry.rs` `player_mesh`, from
`PlayerView`: feet, face, yaw, crouched, dead, colour). The full body replaces it: the same
`PlayerView` plus the look and the pose inputs below. The local player is drawn as the full body only
when dead (as today) or in a future third-person camera.

### 9.3 Netcode: what poses need

Poses are **not sent**. Each client animates others from what snapshots already carry (§8.3):
position (velocity for the gait rate), `yaw` and `pitch` (the body turns with yaw; the head and the
held arm follow pitch, a small additive layer), `state` and `flags` (grounded, crouched, climbing,
swimming, dead). What is missing:

- **The look**: which character, variant, accessories and slot overrides a player wears, by name.
  Sent once on join and on change, reliably (a new `PlayerAppearance` message or a field of the
  player-joined event), validated by the server against its character data (unknown names fall back
  to the default look, as unknown blocks do in registry sync, ADR 0038).
- **The held item**: the hotbar's selection (item and block), sent with input when it changes and
  carried in snapshots (a byte or two per remote player).
- **Swings**: a break or place shows on others when the edit's `VoxelModification` arrives (the
  editor's id is on it); a swing that changes nothing (a refused edit, a brush stroke) needs a small
  `PlayerEvent` kind.

These are protocol changes, so they ship on a new compatibility line (the owner raises
`package.json`).

### 9.4 Choosing a look

Players choose from the data's variants and slot colours (not free authoring). The choice is per
player, kept in the browser's settings store (append-only, CLAUDE.md) and sent at join. A server's
character data could add looks the way servers register blocks (ADR 0038); that is a later choice.

### 9.5 Creatures

The wolf and the deer are art only here. Spawning, AI (wander, graze, flee, hunt), their physics
bodies and netcode (Tier 1 entities, §7.2) are a later phase; the clips are shaped for it (gaits with
a `cycle` for speed-matched playback, `graze` as a loop an AI can enter and leave at the head-up
key).

## 10. Open questions for the owner

1. **Proportions.** The player is chunky with a big head (a fifth of 1.8 m) and big hands and boots.
   Keep, or go less stylised (smaller head, longer limbs) or more (a quarter)?
2. **The three looks.** Wanderer (wide hat, backpack, bedroll, gloves), ranger (hood, cape, bracers,
   satchel), tinker (braids, scarf, bright cuffs, satchel). The right archetypes? How many at
   first, and should players mix accessories freely or pick whole outfits?
3. **Faces.** Dot eyes, brows and a nose, no mouth. Add a mouth line, or faces as decals later?
4. **More clips.** Which next: crouch, swim, climb, fall, slide, the death pose, a jump?
5. **The outline.** Keep the thin ink outline in the game (one extra draw per part), or rely on the
   rim and the palette?
6. **First-person framing.** The arm sits low on the right with the block at about a third of the
   screen's height. Bigger or smaller, and should the left hand show too (both hands, or only for
   some items)?
7. **Creature scale.** The wolf stands 1.08 m at the ears (a big, storybook wolf); the deer 1.72 m
   (2.1 m with antlers). Right against a 1.8 m player and 1 m blocks?
8. **Character identity.** Player looks per player (browser) and sent at join, as §9.4 proposes? And
   may servers add their own looks?
9. **Netcode cost.** Is a byte or two per remote player in snapshots (held item) acceptable, or send
   it only on change as an event?
