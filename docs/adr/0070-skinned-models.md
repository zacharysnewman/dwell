# 0070 — Skinned models from the Asset Store packs replace the rigid-part characters

**Status:** Accepted (2026-11-09; phase 1 built: other players drawn as the Peasant, the animals in
the data and the model viewer; phase 2 (2026-11-10, 0.51.7): held items in the skinned hand, your
shadow as the Peasant). Supersedes part of [ADR 0041](0041-characters.md): characters are no
longer made only of rigid parts from `shared/characters/characters.json`; that system stays for your
first-person arms and your own body seen in first person, and the held items' meshes.

## Context

Phase 40 (ADR 0041) built characters procedurally from chunky rigid parts. The owner has bought
Unity Asset Store packs of skinned, animated low-poly models (ithappy's *Animals FREE*: seven animals
with idle, walk and run; Winged Boots' *Stylized NPC – Peasant Nolant* demo: one humanoid with idle and
walk) and decided (2026-11-09):

- **Skinned models replace the rigid-part characters.**
- **Restyle by remapping** the packs' colours and textures to Dwell's palette first; repainting them
  with the painterly textures is a later consideration.
- **Characters and animals first**, the tools pack after.

The packs ship FBX and textures in Unity package layout, with no licence files (the Asset Store
EULA covers them). The game reads neither FBX nor Unity data, and the web build has no image decoder.

What gets added later, by whom, with what (CLAUDE.md's questions, answered from the brief): more
models of the same kind (more packs, more animals, the tools) by the team, with the import script
and the data file; new roles mapped to clips in the data; new kinds of rule (attachments, IK, morphs)
are code and rare. Per build: the models ship with a release, like the block registry.

## Decision

1. **The GLBs are the source of truth, made once by a checked-in script.**
   `node shared/models/import.mjs --from <unpacked packs>` drives Blender headless
   (`shared/models/blender_import.py`) for each model: FBX in, GLB out (`shared/models/glb/`), the
   meshes joined into one, at most four joints a vertex, the clips named by their last word
   (`Deer_001_idle` → `idle`) and sampled at 15 keys a second. The packs themselves are not in the
   repo; `models.json` records each model's pack and paths inside it, so the import can be rerun.
2. **The palette remap, at import.** Each face takes one colour: the pack's texture sampled at the
   face (the median of five points, so a stray texel such as a watermark doesn't win), snapped to the
   nearest stop of `shared/textures/palette.json` in Lab and replaced by **its ramp's base** (the
   packs' painted shading is the renderer's to do), unless the model's `remap` names the colour.
   Utility and effect ramps are excluded (`palette.exclude`). The texture and UVs are dropped: no
   image decoding at run time, small GLBs (113–325 KB). Small painted detail (the Peasant's eyes
   inside one face) can be lost; repainting is the later step.
3. **Data, one generator, validation.** `shared/models/models.json`: each model's GLB, scale, yaw,
   the clip (and speed) for each role it plays (the roles of `dwell_characters::animator::body_role`),
   a `fallback` role, the remap; and which model is the player. `shared/models/gen.mjs` validates it
   against the GLBs and the palette (missing GLB, a clip the GLB lacks, an unknown role, a fallback
   not among the roles, more than 64 joints, bad remaps, all failing with the entry named) and writes
   `crates/dwell-models/src/models.gen.rs` (and `fixture.gen.rs` from `shared/models/fixtures`, a
   two-joint model added by data alone); CI fails on stale output. Only the player's GLB is built
   into the client (`include_bytes!`); the animals are data until they spawn.
4. **A crate of its own, `dwell-models`**: a small GLB reader (no new dependency: `serde_json`), clip
   sampling (linear and step), blending (nlerp), joint matrices, and a role → clip `Player` that
   cross-fades 0.2 s when the clip changes and keeps the phase when only the speed does.
5. **The renderer** (`render/skinned.rs`, owned by `CharacterRenderer` so the frame's call sites are
   unchanged): GPU skinning in the vertex shader (four joints from a 64-joint uniform block per body,
   a dynamic offset), the rigid characters' ramp shading built from the vertex's palette colour,
   lit by the same flood light, sun, moon, shadow maps and point lights; bodies cast shadows. The
   body is placed by the same frame as the rigid bodies (`place_frame`: face-local, upside down on
   face B by a half turn about its forward axis).
6. **Phase 1's scope**: other players are the Peasant, driven by their existing roles (idle and walk
   clips; sprint, crouch-walk and slide play the walk at another speed; the rest fall back to idle).
   Your first-person arms, your own body and your shadow stay rigid this phase. The animals are
   imported, in the data and in the model viewer (`docs/prototypes/model-viewer.html`), not spawned.
7. **Phase 2: held items and your shadow** (2026-11-10). A model's `hand` in `models.json` names the
   skin joint a held item follows (the Peasant's `hand.R`) and the turn and offset from that joint to
   the rigid rig's hand bone (`hand_r`), the frame every held item in `characters.json` is authored
   in; so the held items (blocks, the axe, the torch, held looks) stay one set of data, drawn by the
   rigid renderer on the skinned hand, with the item's own turns of the hand (the axe's tilt) on top,
   and the held light from the same frame. The turn was measured once from the Peasant's idle hand
   against the rigid rest hand (−2°, 1°, −175°: the joint's +y runs to the fingers, the rig's
   down). Your shadow (the `cast_only` body) is the Peasant, into the nearest cascade only as before.
   Your first-person arms and your own body seen from the eye stay rigid: the Peasant's arms are
   the whole-body mesh (no separate arms mesh, the sleeves and hands skinned to the shoulders), and
   drawn at the eye they would need their own view and clips the pack doesn't have. There is no
   third-person view in the game yet; a bench `camera` view shows only your shadow.
8. **Props: static models** (2026-11-10, 0.51.24; the owner: "Let's do tools", and "All the models are
   good for commercial use"). The NexusArcadeStudios pack *3D Low Poly Tools Weapons Containers*
   (53 FBX, one gradient texture) is imported by the same pipeline as static models:
   `node shared/models/import.mjs --props --from <unpacked packs>` runs one Blender process
   (`shared/models/blender_props.py`) over every prop: meshes joined, transforms applied, scaled so
   the longest side is the prop's `length` (else its category's), origin at the bounds' centre, the
   same per-face palette remap, and the pack's own colour layers removed before ours is added (the
   chicken bug of 0.51.22: a pack's white `COLOR_0` pushed ours to `COLOR_1`). No skin, clips, UVs
   or materials; 7.9 MB for all 53. The data is `shared/models/props.json`: `categories` (tools,
   weapons, instruments, containers, tableware, lamps: a label, a default `length` and the `min`/`max`
   a prop of it may be) and `props` (category, GLB, optional `length` and `grip`, metres from the
   origin where a hand holds it). `gen.mjs` validates it (missing GLB, more than one mesh, a skin or
   clips, no `COLOR_0`, an unknown category, a length outside its category's or not the GLB's) and
   writes `PROPS` into `models.gen.rs` (`fixture.gen.rs` from `fixtures/props.json`, a plate added by
   data alone). Only the props an item's look names (`shared/inventory` `look.model`) are built into
   the client (`PROP_GLBS`); the rest are data for the Model Viewer's Props view until the owner makes
   them items. `dwell_models::glb::parse_static` reads one (every vertex on joint 0 at weight 1).
   Multi-object files (`axes`, `katanas_swords`, `golf_bats`, …) are one prop each, their variants
   side by side; splitting them is a later data step (an `object` filter in the import).
9. **An item's look may be a prop** (`look: { "model": "stone_pickaxes" }`, `LookShape::Model`, ADR
   0066): the loose item and the hotbar and backpack picture are the prop's triangles in their palette
   colours (`dwell_mesh::model_faces`, lying along +x on y = 0; `look_mesh`, `look_swatch`). The first
   is `dwell:stone_pickaxe`, which proves the path: no one is given it and it does nothing.
   **Not built yet:** the prop in the hand (the skinned hand and the first-person arms); the hand
   shows empty for a model look. The held items are `characters.json` parts drawn by the rigid
   renderer, whose vertices carry a material index, not a colour; drawing a prop there needs a
   `model` part kind with per-vertex colours (or the skinned renderer drawing the prop on the
   `hand_frame`, its joint 0 the grip).
10. **Model detail (0.52.1).** Owner, 2026-11-10: "The animal models are too smooth I think. Can we use
   some tools to make them lower poly style? Can you make this a slider in the graphics and then later
   we'll set an opinionated value." The levels are data: `models.json` `detail` (`levels`: an id, a
   label and a `ratio` of the faces kept, the first 1; `default`, `full` until the owner picks). The
   import (`blender_import.py`) makes one mesh per level from the joined mesh with Blender's Decimate
   (collapse, triangulated; the vertex groups, so the skin weights, carried), paints each level's faces
   from the texture as before (the pack's own colour layers removed first, so the palette colours are
   `COLOR_0`) and shades every level **flat** (one normal a face, custom normals cleared: most of the
   "too smooth" look was the smooth normals). The GLB holds the levels as meshes `detail_0`,
   `detail_1`, … on the one skin, sharing the clips. `gen.mjs` validates the levels (ids, the first
   ratio 1, falling ratios, the default one of them) and that each model has a mesh per level, and
   writes `DETAIL_LEVELS`/`DETAIL_DEFAULT`; the fixture adds a level of its own by data.
   `dwell_models::glb::Model::level(k)` reads a level (past the last: the coarsest). Settings →
   Graphics → Look's **Model detail** slider runs through the levels (stored as the level's id,
   `dwell.modelDetail`, a default/pin setting like the others; not in the graphics presets: a look, not
   a cost they trade); `setModelDetail` re-uploads every skinned mesh at the level, live, the clip
   players untouched. **Since 0.52.5 the slider is gone** (owner, 2026-11-10: "Keep the logic for
   changing the detail. But we can remove the slider and make 60% the default"): `detail.default` is
   `high` (60 %); the setting, `setModelDetail`, `?modeldetail=` and the Model Viewer's buttons stay,
   and a pin stored before still applies (it has no control to change it now; clearing site data or
   Settings → Reset drops it). Flat shading is on at every level, not a separate toggle: it is what makes a
   level read as low-poly. Cost: the flat corners split the vertices, so the ten GLBs are ~5.6 MB
   (were ~1.8 MB), all built into the WASM. Seen in the Model Viewer: 60 % and 35 % read as clean
   low-poly; 20 % breaks the small models (the wolf's and the dog's heads collapse, the stag loses
   antler tines), so the owner's pick is likely 35 % or 60 %.
11. **Variants by import (0.52.1).** A model's `source.remove` (`{ joint, above }`) deletes the loose
   parts weighted wholly to that joint that rise above that height: the **doe** is the deer's FBX
   without its antlers (the two antler parts on `scull`, above 1.7 model units; the ears end at 1.61),
   the same skeleton and clips. The **wolf** is the dog's FBX with a `remap` of the pack's colours to
   the palette's wolf ramps (owner: "use the dog recolored. Make it twice as big for default too"),
   `scale` 2, then 1.5 (0.52.5, owner: "The wolf should be shrunk about 75% of its size").
   `import.mjs --colours` prints each source colour seen and what it became, for writing a remap.
12. **Coats (0.52.5).** The per-face colours read as "very harsh color changes" on the wolf ("those
   wolves look like Balto"; owner, 2026-11-10, with a grey wolf's photo: "this is more the blend").
   A model's optional `coat` in `models.json` paints it by body region instead: a `base` colour and
   `patches` laid over it in order, each a box in the body's normalised coordinates (`along` 0 the
   nose → 1 the tail's tip, `height` 0 the feet → 1 the top, `side` 0 the centre line → 1 the widest;
   `head` names the GLB axis the head points along) with soft edges (`blend`, a smoothstep centred on
   each edge) and a cover (`amount`). `keep` names ramps whose imported colours stay (the wolf's black
   eyes and nose); everything else, the dog's blue collar and red tongue included, takes the coat.
   `shared/models/coat.mjs` runs after Blender (`import.mjs`, or `import.mjs --coat` over the
   checked-in GLBs, no packs needed): every vertex of every detail level takes the coat's colour at
   its bind-pose position (bounds over all levels together), so faces stay flat-shaded (their own
   normals, the vertices still split per face) while the colour runs smoothly across the body, the
   renderer interpolating it inside each face. Colours are palette references, mixed in sRGB: a
   vertex may hold a colour **between** palette stops (a deliberate departure from §11's snapped
   per-face colours, for coats only). `gen.mjs` validates a coat (unknown keys, ranges, blends,
   colours, a patch painting a kept ramp) and fails when a coated model's GLB is not its coat's
   output; tests pin the wolf's smoothness (neighbouring faces step by under 0.2 a channel; 0.62
   before), no red or blue, a back darker than its belly, and a fixture model coated by data alone.
   The wolf's coat: fawn flanks (`sand.shadow`), a dark saddle (`flint.shadow`) and grey shoulders
   and head (`wolf`), a cream belly, legs and throat and a pale muzzle (`wolf_pale`), a grey tail with
   a dark tip (`bedrock.shadow`). The collar is part of the dog's one mesh (not a loose part a
   `remove` could drop), so it is fur-coloured but its raised ring still shows at full detail.

## Amendment (2026-11-10): the Peasant driven by the rig's clips

The owner: "My shadow isn't lining up with my first person feet and hands" (the shadow was the
Peasant playing its own idle and walk; the first-person body and arms the rigid rig's clips), and
"We should use the existing or slightly modified animations we already setup for our character."
Decision: a skinned model may carry a **retarget map** (`models.json` `retarget`: rig bone → skin
joint with a rest-pose correction, validated by `gen.mjs`), and a body drawn as such a model is
posed each frame from the characters rig's animator pose (`dwell_models::retarget`, CHARACTERS.md
§11.9): the same clips, times and blends the rigid body plays, the model's own bone lengths kept.
The model's own clips stay as the fallback where no rig pose is given (and for the animals). Tests
put the alignment as numbers (each hand and foot within 9.2 % of its limb's length; the skinned hand
within 0.15 m and 5.2° of the rigid hand bone on both faces). Held items stay on the hand joint
(§11.5). Rejected: authoring new Peasant clips per role (two clips in the pack, ~15 roles to cover,
and they would still drift from the first-person body's).

## Consequences

- Characters now come from bought art; adding one is: unpack, add an entry, run the import and the
  generator. The palette keeps them in Dwell's colours.
- The rigid-part system (ADR 0041) is still used by the arms and your own body; the two systems
  coexist until those move over (Phase 54's next steps), then `dwell-characters`' meshes go.
- **Licence**: the packs are under the Unity Asset Store EULA (Standard); `shared/licenses` lists
  them. The owner should confirm that redistribution inside a public web build is allowed for these
  packs (the GLBs are fetchable from the site).
- Held items aren't attached to the skinned hand yet; the Peasant has no jump, fall, swim, climb or
  fly clips (those roles play idle); its eyes are lost to the per-face colours.
- Reversing: drop `skinned` from `CharacterRenderer::prepare`'s remote-body branch and the rigid
  bodies draw again.
