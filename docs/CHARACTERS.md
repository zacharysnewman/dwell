# Dwell — Characters and Creatures

The design of Dwell's characters: the player's first-person arms and the full body other players
see, and the first two creatures, a wolf and a deer, in the same style. Characters are made
procedurally from data: chunky parts (boxes and rounded shapes) on a small rig, painted with the
shared palette and materials of [`ART_DIRECTION.md`](./ART_DIRECTION.md). Adopted by
[ADR 0041](./adr/0041-characters.md); Phase 40 of [`IMPLEMENTATION_PLAN.md`](./IMPLEMENTATION_PLAN.md).

**Status.** The design and concept (owner, 2026-10-20): this document, the data file
(`shared/characters/characters.json`), its validator and tests, and the
[Character Lab](./prototypes/character-lab.html) prototype page. **In the game since C1
(2026-10-20, owner: *"add the player character into the game"*):** other players are drawn as the
full body, animated from their snapshots, and you see your own first-person arms holding the
hotbar's block or the brush, with the break and place swings; what each player holds and its swings
cross the network (protocol v17). §9 describes what is built and what is still to come (looks chosen
per player, foot IK on steps, the creatures). The characters themselves (their proportions, outfits,
colours and clips) are the first art direction, still prototype in scope (`ARCHITECTURE.md` §6.1):
the system is what this phase fixes, and the content grows by data.

**The owner's first review (2026-10-20)** asked for three fixes, now in: hair and hats compose (a
part can hang from another, so a hat that hides the top hair takes the fringe with it, and the
wide-brimmed hat tucks its own fringe under the brim, §4.1); the wolf's and the deer's tails are
**chains** whose segments join end to end and bend smoothly in every clip (§4.2); and the player has
a clip for every controller state, with first-person versions of the sprint, slide and jump and a
transitions preview in the Lab (§6).

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
| Hanging parts | A part's `with` names a part it hangs from; hiding that one hides it too (§4.1). |
| Chain check | A segment runs along its part's longest side, between the centres of its end caps; each meets the next within 3 cm (`CHAIN_TOLERANCE`) and turns from it by at most 60° (`CHAIN_MAX_BEND`); the first starts at its bone's pivot and on the part it grows from (§4.2). |
| Clip flags | `grounded` (a base clip stands on the ground; `false` in the air, in water, on a ladder) and `blend` (the cross-fade into it, seconds; 0.2 by default). Switching base clips cross-fades every bone either clip keys (`crossfade`). |

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
    view?, with? }`. `with` names an earlier part this one hangs from (the fringe from the top hair).
  - `chains`: named chains of parts, `{ from, parts }`: the part the chain grows from and its
    segments, root first (the wolf's tail: `haunch` → `tail_base`, `tail_brush`, `tail_tip`).
  - `accessories`: the default look's accessory names; `variants`: named looks `{ label, accessories,
    slots }` (the player's *wanderer*, *ranger*, *tinker*; the deer's *stag* and *doe*).
  - `clips`: roles → clip names (`idle`, `walk`, `hold`, `break`, `place`; the wolf's `trot`, the
    deer's `graze`).
  - `firstPerson`: the arm chain (`["arm_r", "forearm_r", "hand_r"]`), its `anchor` in view space
    (metres from the eye: `[-0.47, -0.4, 0.17]`, right, down and ahead), the arms view's `fov`, and
    its clips (§5).
- **`accessories`**: `{ label, place, fits (rigs), slots (its own colour slots and defaults), hides
  (body part ids it covers), parts }`. A hat hides the hair's top; gloves hide the hands. An
  accessory's parts may paint with the wearer's slots (`hair`, `skin`), so it can bring a variant of
  what it hides (§4.1).
- **`items`**: things held in hand: `{ label, on (a socket), light?, parts }`. `block` (a 0.2 m cube of
  `$block`), `brush` (a wooden handle, a brass ferrule, a tuft in the selected block's colour) and
  `torch` (a stick, a wrap and a glowing flame; the hand shows it for a block whose registry entry says
  `"held": "torch"`, [`LIGHTS.md`](./LIGHTS.md)). `light` is the point on the socket where the
  light a held block gives off sits (the flame), resolved by the generator to a bone and an offset
  (`Item::light`); a material's `emissive` (shared/textures) makes it self-lit, its colour whatever the light (`Material::emissive`; the torch's flame, a yellow bulb under an orange cone).
- **`clips`**: `{ rig, space?, layer, duration, loop, ease, cycle?, grounded?, blend?, keys }`. A key is `{ t, bones:
  { bone: [x, y, z] degrees }, root?: [x, y, z] metres }`; each bone interpolates between the keys
  that name it, so a key lists only what moves at that moment. `cycle` is the distance travelled per
  loop: a gait plays at `speed / cycle` loops a second, so feet don't slide. The arms' gaits have no
  feet, so their `cycle` sets the hand bob's rate (§5).

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
first-person chain connected and its clips keying only it, a `with` naming an earlier part of the
character (not an accessory's), chains naming parts, `grounded` only on a base clip, `blend` 0–2 s —
and **the rest pose**: feet on the ground, the body (without accessories) as tall as `height` within
3 cm, and every chain joined and smooth.

### 4.1 Hair under hats

The hair is three parts: `hair_top`, `hair_back` and `hair_fringe`, the fringe hanging from the top
(`"with": "hair_top"`). A hat says what it covers (`"hides": ["hair_top"]`), and anything hanging
from a hidden part is hidden with it, so the fringe never floats on the forehead without the hair it
grows from (the owner's review: *"a lone brown bar floats across the forehead under the brim"*). A
hat that wants hair showing brings its own: the wide-brimmed hat adds `hat_fringe` and two
`hat_tuft` side locks tucked under the brim, painted with the wearer's `hair` slot so they take any
look's hair colour; the hood hides the fringe and shows none. A new hat chooses by data: hide the top
(the fringe goes too), bring a tucked variant, or both. Tests: in every look of every character no
drawn part hangs from a hidden one, every hat that hides the top hair hides the fringe, and the wide
hat's own hair sits under its brim.

### 4.2 Chains: tails

A chain is a list of parts joined end to end, each on its own bone or riding the last one's. A part's
segment runs along its longest side between the centres of its end caps (half its length less its
radius from the middle), so neighbouring segments overlap into a smooth tube. The chain is joined
when each segment starts within 3 cm of where the last ends, the first starts at its bone's pivot
and on the part it grows from, and no segment turns more than 60° from the last. The validator
checks the rest pose; a test samples every clip of the character (25 times each, every look) and
checks the same, so a clip that kinks or tears a tail fails.

The rule that keeps it joined in motion: **each tail bone's pivot is the end of the segment before**,
so turning a bone swings the rest of the tail about the joint instead of opening a gap. The wolf's
tail grows from the top of the haunch, 40° below the horizontal, then 60° and 70° for the brush and
its dark tip (each part's `rot` lays its long side along the chain); the deer's short tail hangs from
the rump with its white flash continuing it, and its flicks lift it back (+x), not forward into the
rump. Before the fix the wolf's base segment ran across the chain (0.16 m from its pivot) and the
deer's tail hung 7 cm behind the rump.

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
  body would be. The hand and its held block sit in the lower right: at 16:9 and rest, their box
  is ~0.55 of the screen's height, its centre at (0.52, −0.90) in NDC, its top below the crosshair
  (owner's playtest, 2026-10-20: was 0.65 high and up to the crosshair, then 0.43 read too small; set by moving the anchor
  out and ahead, the FOV unchanged; `crates/dwell-characters/tests/first_person.rs` pins it).
- **Its own clips.** `fp_idle` (the arm held into view, breathing), `fp_walk` (a bob on the anchor,
  two dips a loop at 4 m a loop, not the legs' 1.6 m: ~2.5 Hz at the 5 m/s walk; `fp_sprint` shares
  the 4 m, so its bob is faster in proportion to the speed, ~4 Hz at the 8 m/s run), `fp_break` (a wind-up and a chop down-forward, 0.5 s), `fp_place` (a push
  forward, 0.36 s). They key the same bones as the body's `hold`, `break` and `place`, so a swing
  seen by others is the same gesture.
- **Held items.** The hotbar's block (its own painted tile on every face) or the brush tool, on the
  `grip_r` socket. Tools come later with an item system: an item is data (`items`), so a pickaxe is
  an entry, and its swing a clip.

### 5.1 Your own body in first person **[built, C1b]**

The owner (2026-10-21): "I want to see legs in first person, particularly when sliding." The
answers (question tool):

- **Full body, no head.** In first person you see your own body from the neck down, playing the
  same clips other players see. The first-person arms view stays as it is, drawn on top; the full
  body's arms are hidden in first person so there are never two pairs.
- **Always, when in view.** Look down while walking and you see your feet; slide and the legs come
  into view at the bottom of the screen.
- **The constraint** (verbatim): "The main thing to watch out for with full body draw is not
  clipping the camera into the player model." No body geometry may come inside the camera's near
  plane or fill the view, in any clip or pose, looking fully down or up, on both faces.

**The design** (data: `firstPerson.body` in `characters.json`; code: `dwell-characters` `own.rs`,
`render/characters.rs` `own_frame`):

- **What it hides.** `hideBones` (the head, so its hair, face and hat; both arm chains, with gloves
  and cuffs, since the arms view draws your arms) and `hideParts` (the neck, and what is worn round
  it: the hood's mantle, the scarf's wrap, whose broad faces came within 13 cm of the eye in the
  sweep). `"view": "arms"` parts are never in it. A part added on a hidden bone is hidden by data
  alone. The arms view stays on top as it was, holding the item; the body holds nothing.
- **Where it stands.** On your feet, turned to your look's yaw, and moved back along its own −z:
  `back` (0.1 m) always, `lookDown` (0.3 m) more looking straight down (by the sine of the downward
  pitch), and then as much more as keeps every vertex `clear` (0.22 m) of the eye
  (`own::shift`, per frame from the posed vertices: the crouch, whose clip leans the chest forward to
  the 0.8 m crouched eye, the slide, a swim stroke, the eye lagging a step low while the step view
  smooths it). The feet stay on the ground; only the horizontal place moves. It is computed in the
  body's own frame (+y up, the feet at the origin), so face B is the same.
- **Safety net.** Fragments within the camera's near plane (5 cm) plus `near` (0.08 m) of the eye
  are discarded in the shader (`Instance::cfg.x`), for any pose the sweep has not seen.
- **Shadow.** The whole body, head and all, is still cast into the shadow maps as before
  (`cast_only`); the drawn one casts nothing.
- **The test** (`render::characters`'s `your_body_never_comes_near_the_camera_or_fills_the_view`,
  `both_faces!`): every body clip at six phases, the swings (hold, break, place) on the idle and the
  walk, every look, the pitch from −90° to 90° in 15° steps, the eye at its height, a step low
  (`max_step_lift`) on the ground and, crouched, still at the standing eye: no triangle (not just
  vertex) within 0.13 m of the eye (the nearest 0.146 m), the body over at most `fill` (0.5) of a
  16:9 screen at the base FOV and 1.25× it (the most 48.5%, swimming looking straight down;
  standing straight down 24%), the feet in view looking down (−75° and below) and the legs while
  sliding (−15° and below). With the offsets at 0 it fails: a triangle 4.3 cm from the eye, inside
  the near plane.
- **What it does not do.** At a level look while sliding the legs are just below the frame: the
  crouched eye is 0.8 m up and the feet on the ground ahead, 60° below the horizon, past the 37.5°
  half FOV; they come in from about 15° down. Showing them level would need a slide camera (lower,
  or pitched) or a slide clip with the lead leg raised, a question for the owner (§10).


## 6. Poses and animation

Clips are keyed rotations on the rig (and a root offset for bob), sampled and layered:

| Character | Base | Hold | Action |
| --- | --- | --- | --- |
| Player (body) | `biped_idle` (breathing, 3 s), `biped_walk` (1 s, 1.6 m a loop: legs, counter-swinging arms, hips and chest twist, bob), `biped_sprint` (0.66 s, 3 m: chest forward, arms pumping, high knees), `biped_crouch` (a deep squat, chest forward), `biped_crouch_walk` (1 s, 0.9 m), `biped_slide` (low and leaning back, the lead leg out, the other folded under, the left arm out for balance), `biped_jump` (0.42 s, one-shot: a dip, the push onto the toes with the arms swinging up, then knees tucked), `biped_fall` (legs loose, arms up and out), `biped_land` (0.4 s, one-shot: the squash on touching down, to standing), `biped_climb` (hand over hand, 1.2 m a loop), `biped_swim` (a breaststroke and a frog kick, leaning into it), `biped_fly` (a hover, legs trailing) | `biped_hold` (the right forearm forward) | `biped_break`, `biped_place` |
| Player (arms) | `fp_idle`, `fp_walk`, `fp_sprint` (a bigger, faster bob and the arm pumping), `fp_slide` (the arm out to the right and lowered), `fp_jump` (a dip on the take-off, the arm lifting with the push), `fp_fall` (raised a little, fluttering), `fp_land` (a dip, back to the hold) | (in the base) | `fp_break`, `fp_place` |
| Wolf | `wolf_idle` (tail sway, head turns, an ear flick), `wolf_trot` (0.56 s, 1.4 m: diagonal pairs, a suspended bob, the tail up and swaying) | | |
| Deer | `deer_idle` (head turns, ear and tail flicks), `deer_walk` (1.2 s, 1.1 m: four beats, left hind, left fore, right hind, right fore), `deer_graze` (6 s: head down to the grass, nibbling, a look up with ears forward, down again) | | |

Layering is `base`, then `hold` (when an item is held), then `action` (a swing), each replacing the
bones it keys: the player walks with the block held in front and swings without stopping. A test
samples every grounded base clip and checks the lowest point stays within 6 cm of the ground (no foot
through the floor, no floating); the crouch, crouch-walk and slide stay low (under 1.3 m, the slide
1.4 m) and the slide's hand reaches out past 0.45 m. The new clips' legs were solved with two-bone
IK onto planted feet, then rounded to whole degrees in the data.

**The controller's states → roles** (`PLAYER_CONTROLLER.md` §3; the state comes from what snapshots
already carry, §9.3):

| Controller | Body role | Arms role |
| --- | --- | --- |
| `Idle` | `idle` | `idle` |
| `Walking` (plays at `speed / cycle`) | `walk` | `walk` |
| `Running` | `sprint` | `sprint` |
| `Crouching`, still / moving | `crouch` / `crouch_walk` | `idle` / `walk` |
| `Sliding` (the steep slide and the sprint slide) | `slide` | `slide` |
| `JUMPED` event, then `Jumping` | `jump` (one-shot) | `jump` |
| `Falling` (and `Jumping` after the jump clip ends) | `fall` | `fall` |
| `LANDED` event | `land` (one-shot, then the ground state) | `land` |
| `Climbing` | `climb` (at `climb speed / cycle`) | `idle` |
| `Swimming` | `swim` | `idle` |
| `Flying` | `fly` | `idle` |
| hotbar item / an edit | `hold` / `break`, `place` | `break`, `place` |

**Transitions.** Switching base clips cross-fades from the old pose to the new over the new clip's
`blend` (0.25 s into the sprint, 0.12 s into the slide, 0.06 s into the jump, 0.05 s into the land;
0.2 s when it doesn't say), every bone either clip keys taking part (a bone one clip doesn't key is at
rest in it). The Lab's **Transitions** player previews it: presets (walk → sprint → slide → walk;
idle → jump → fall → land → idle; walk → crouch; climb, swim, fly; the wolf's trot, the deer's graze) or a sequence built clip by
clip, the jump's rise drawn as the controller's 1.25 m arc. The death pose is still to come.

## 7. Tools: generator, tests, the Character Lab

- **`shared/characters/gen.mjs`** validates the data against the shared style and writes the data,
  the style and the two modules (`rig.mjs`, and `shared/textures/paint.mjs`, the painter) into the
  Lab page between its generated markers. CI regenerates the page and fails on any diff, so editing
  `characters.json`, the palette or the materials means running `node shared/characters/gen.mjs`.
  It also writes the game's Rust tables (§9.1): `crates/dwell-characters/src/characters.gen.rs` and
  `characters.tiles`, and the test fixture's (`fixture.gen.rs`, from the data plus
  `shared/characters/fixtures/*.json`); CI fails on stale output.
- **`shared/characters/gen.test.mjs`** (`node --test`): the data valid and the page fresh; 36 bad
  entries each failing with its message; a part, an accessory and a variant **added by data alone**
  composed and worn (a mirrored pauldron reaches past the shoulder; a top hat hides the hair and
  stands on the head); the held item on its socket and the arms view sharing the body's parts;
  sampling, looping and layering; feet on the ground in every grounded clip; hair never left floating
  under a hat (§4.1); every quadruped's tail a chain, joined and smooth at rest and through every clip
  in every look, and a turned or detached segment caught (§4.2); a body clip for every controller
  state and arms clips for the ones that show (§6); cross-fades over every keyed bone.
- **The [Character Lab](./prototypes/character-lab.html)** (three.js, standalone, publishable):
  the player full body, the first-person view (a little world, the hotbar, the arms in their own
  pass), the wolf, the deer and a group to scale with 1 m blocks; orbit, front/back/side/¾ views and
  a turntable; clip playback with a scrubber and speed, break and place swings, and the transitions
  player (§6); looks (variants),
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
shared/textures/palette.json, materials.json ─┐                               ┌► docs/prototypes/character-lab.html
shared/characters/characters.json ────────────┼─ gen.mjs (rig.mjs validate, ─┼► crates/dwell-characters/src/characters.gen.rs, .tiles
shared/characters/rig.mjs, textures/paint.mjs ┘   compose every look)        └► fixture.gen.rs, .tiles (+ fixtures/*.json; tests)
dwell-characters (meshes, clips, animator) ─► dwell-client bodies.rs (animators) ─► render/characters.rs (bodies, arms pass)
```

## 9. In the game

**[built, C1]** unless marked otherwise. ARCHITECTURE.md §5.3 has the architecture; this section the
choices.

### 9.1 Meshes and rendering

- **Built as designed below**, with these specifics: the Rust module is a new crate,
  `dwell-characters`; `gen.mjs` composes every look in JS (rig.mjs `compose`, the one composer), so
  the Rust side reads looks as flat parts on bones and only meshes, samples and selects; the brush
  tiles are baked as luminance over the tile's mean (the Lab's `lum(tile) / uTileLum`); the shader
  scales the ramp by the frame's light on the fragment's face (`CHARACTER_EXPOSURE` 0.5, tuned by eye)
  so night and face B's lights reach characters; the outline (~6 mm, dropped with the small parts past
  64 m) is **off for now** (owner, 2026-10-21: the hull showed through cutout held items such as grass
  as dark faces; `render/characters.rs` `OUTLINES`); the shader always draws the brush tiles;
  the held block uses its atlas tiles.
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
  back-facing draw.
- **The arms pass.** After the world and the water, before the HUD: clear depth, draw the arm chain
  with its own projection (`fov`, a 2 cm near plane), the sun turned into view space. Not drawn while
  dead (the body is drawn instead, as today).
- **Distance.** Past ~64 m a character drops its outline and small parts (eyes, buckles: a size
  threshold, data-free); past the far view it isn't drawn.

### 9.2 Remote players

The capsule is gone: `PlayerView` carries the feet, face, yaw, death, the look's name, the pose the
game animated (`bodies.rs`, one `Animator` per remote player from its interpolated snapshot) and
what the hand holds. A frame samples one moment of the snapshots, so a jump also starts when its
first Jumping snapshot arrived since the last frame (`RemoteView::jumped`, `MoveInput::jumped`): at a
few frames a second the frame can fall past the 0.33 s rise, or past the whole jump, and the body
would otherwise go straight to the fall or the landing, or miss the jump. The jump is noted when its
snapshot arrives, not by its time: a slow page reads its snapshots late, in bursts, often after the
interpolation time has passed the time they were sent, and a long frame reads more of them than the
buffer keeps (`remotes.rs` `a_jump_is_reported_however_late_its_snapshots_are_read`). The local player is drawn as the full body only when dead (lying on its back,
seen from the death camera) or in a future third-person camera. **Face B**: a body there is turned a
half turn about its forward axis (a proper rotation: the winding and its handedness hold).
**Steps** **[built, except the step pose]**: a remote body's root is drawn at its feet plus the
step view's offset (`StepSmoother::follow`: its interpolated step-up followed at no more than a
45° ramp at its speed, PLAYER_CONTROLLER.md §9), and `MoveInput::foot_ground` (the render-side
ground under each foot from the last pose's foot positions, within `max_step_lift()`, face-local)
feeds the animator's `plant_feet`: the body drops by the lower foot's ground when it is below
the feet, and each leg bends by two-bone IK (thigh forward, shin back, foot level) to lift its
foot by the rest. `Bodies::step_started` reports a step's start, the hook for a step pose
[planned].

### 9.3 Netcode: what poses need

Poses are **not sent** **[built]**. Each client animates others from what snapshots already carry
(§8.3): position (velocity for the gait rate), `yaw` and `pitch` (the body turns with yaw; the head
60 %, the chest 15 % and the held arm 50 % follow pitch, added on the clips), `state` and `flags`
(grounded, crouched, climbing, swimming, flying, face B, dead). The clip for a state is §6's table
(`dwell-characters` `animator::body_role`): fly, swim and climb from their flags, jump while
airborne and rising faster than 0.5 m/s along the face's up, else fall; the landing one-shot when
touching down slower than 1.5 m/s (faster goes straight into the gait, so the feet don't slide);
slide (the steep slide and the sprint slide are both `Sliding`), sprint, walk, crouch and
crouch-walk by speed (0.4 m/s). The rest, as built:

- **The look** **[planned, C2]**: which character, variant, accessories and slot overrides a player
  wears, by name. Everyone wears the default look (`wanderer`) for now: sending it is a message, a
  settings record, a picker and server validation, more than C1 needed. To come: sent once on join and
  on change, reliably, validated by the server against its character data (unknown names fall back to
  the default look, as unknown blocks do in registry sync, ADR 0038). The renderer already draws any
  look by name (`PlayerView::look`).
- **What the hand shows** **[built, 0.39.3]**: a block's registry `held` names its item (`torch`;
  `card`, a flat card of its tile, for the ladder and tall grass); any other block is the `block`
  item, a ½ m piece on its edge whose sides show the tile's top half (part `sideV`, for a `$block`
  box). Owner, 2026-10-28: the held blocks didn't look like the hotbar's ½ m pieces, and a ladder or
  grass held as a textured block looked bad.
- **The held item** **[built, protocol v17]**: a `u16` per remote player in every snapshot (a block
  state, the server's id, in the low 15 bits; `0x8000` the brush tool painting with it; 0 an empty
  hand), from a reliable `HeldItem` the client sends when its hand changes (the hotbar's block, the
  brush tool on or off), mapped between the client's and the server's ids (registry sync). **Chosen
  over an event on change** (§10 Q9): snapshots are the one per-player stream, so a late joiner sees
  it at once and a lost datagram costs nothing, for 2 bytes a remote player (32 players × 20 Hz:
  1.3 KB/s); an event would need its own state sync for joiners.
- **Swings** **[built, v17]**: `VoxelModification` doesn't name its editor, so a swing rides in the
  snapshot too: a `u8` per remote player, a count (6 bits) the server steps on every
  `BlockEditRequest` and `RegionEdit` it receives from that player, refused or not, and whether it
  broke or placed (`swing_flags`); a client starts the swing when it sees the count change (not on
  the first value it sees). Brush strokes and region tools swing as a place.

The held item and swings changed the protocol (v17), so they ship on **a new compatibility line**:
the owner raises `package.json` to the next minor.

### 9.4 Choosing a look **[planned, C2]**

Players choose from the data's variants and slot colours (not free authoring). The choice is per
player, kept in the browser's settings store (append-only, CLAUDE.md) and sent at join. A server's
character data could add looks the way servers register blocks (ADR 0038); that is a later choice.

### 9.5 Creatures **[planned, C3]**

The wolf and the deer are art only here. Spawning, AI (wander, graze, flee, hunt), their physics
bodies and netcode (Tier 1 entities, §7.2) are a later phase; the clips are shaped for it (gaits with
a `cycle` for speed-matched playback, `graze` as a loop an AI can enter and leave at the head-up
key).

### 9.6 Cost **[measured, C1]**

Chrome on an Apple GPU, 1280 × 720, unthrottled, the machine busy with other agents' builds (so read
the differences, not the absolutes), `__dwell.puppets` standing in for remote players in view:

| Bodies | `gpu.frame` p50 | `gpu.near` p50 | Draw calls | Triangles |
| --- | --- | --- | --- | --- |
| 0 (arms only) | 12.98 ms | 6.15 ms | 1,246 | 1.24 M |
| 1 | 13.01 ms | 6.56 ms | 1,250 | 1.26 M |
| 8 | 12.38 ms | 6.03 ms | 1,278 | 1.42 M |
| 32 | 13.11 ms | 6.65 ms | 1,374 | 1.97 M |

A body is 4 draws (body and item, each with its outline) and ~23 k triangles; 32 are within the
noise. The CPU's frame stays ~1.2–1.4 ms (`frame.cpu`). Your arms' pass (`gpu.arms`) overlaps the
others on a tile-based GPU; with it on and off `gpu.frame` moved less than the run-to-run noise.
GPU memory: one mesh per look (wanderer 385 KB, ranger 420 KB, tinker 428 KB), its far version, the
arms 55 KB, the block 1 KB and the brush 21 KB: 0.49 MB with everything the run drew. Uploads: 1,280
bytes of uniforms a body a frame (41 KB at 32 bodies with the arms).

## 10. Open questions for the owner

**Assumed for C1 until answered** (the Lab's defaults): the proportions as they are (Q1); everyone
wears the wanderer (Q2, Q8); dot-eye faces (Q3); the ink outline on, as an inverted hull (Q5); the
arm low on the right with only the right hand (Q6, the data's anchor and its 62° FOV); a held item
in the right hand only; the death pose is the whole rest pose lying on its back (Q4); the crouched
body is the crouch clip as drawn, poking above a 1-block crawlspace (Q4); creature scale untouched
(Q7, no creatures drawn).


1. **Proportions.** The player is chunky with a big head (a fifth of 1.8 m) and big hands and boots.
   Keep, or go less stylised (smaller head, longer limbs) or more (a quarter)?
2. **The three looks.** Wanderer (wide hat, backpack, bedroll, gloves), ranger (hood, cape, bracers,
   satchel), tinker (braids, scarf, bright cuffs, satchel). The right archetypes? How many at
   first, and should players mix accessories freely or pick whole outfits?
3. **Faces.** Dot eyes, brows and a nose, no mouth. Add a mouth line, or faces as decals later?
4. **More clips.** *Answered (2026-10-20):* sprint, slide, jump (take-off, air, landing), crouch and
   crouch-walk, fall, climb, swim and fly, with first-person sprint, slide and jump. Still open: the
   death pose, and whether the crouched body should fit the 0.9 m crouch capsule (with the big head
   its top is ~1.2 m, so it pokes ~0.2 m above a 1-block crawlspace's ceiling when seen from outside).
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
10. **Legs in a level slide** (§5.1). Your legs show sliding from about 15° down; looking level they
    are just below the frame. Keep, or add a slide camera (lower or pitched a little down) or raise
    the slide clip's lead leg so they show looking level?
