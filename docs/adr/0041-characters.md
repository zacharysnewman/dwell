# 0041. Characters and creatures: rigid chunky parts on a data rig, a separate first-person arms view

- Status: Accepted (2026-10-20: the owner's brief and answers, `CHARACTERS.md` §1)
- Date: 2026-10-20
- Builds on: [0040](0040-painterly-textures.md) (the shared palette, materials and painter)
- Design: [`CHARACTERS.md`](../CHARACTERS.md)

## Context

The owner asked (2026-10-20) for the character design concept, the player's first-person arms and
hands and the full body other players see, plus a wolf and a deer in the same style, "so we can get
a cohesive style together". First person only; a design and a prototype, not built into the game
yet. Characters are to be made procedurally from data (chunky boxes and rounded shapes, proportions,
colours, accessories, simple poses) with no external modelling tool, in the bright painterly
storybook style of the textures (ADR 0040). Remote players are drawn today as a coloured capsule.
CLAUDE.md requires an extendable system: data as the source of truth, one generator, references by
name, validation and tests, and a tool that can add to it.

## Options considered

- **How a body is built.** (a) Meshes from a modelling tool (glTF), skinned; (b) a voxel model per
  character; (c) primitives (boxes, rounded boxes, ellipsoids, cylinders, capsules, cones) placed on
  a bone tree, as data. (a) is ruled out by the brief and can't be edited by a tool we write; (b)
  reads as blocky, not chunky-rounded, and animates poorly; (c) is editable field by field, small,
  and fits the style.
- **Deformation.** Skinning (vertex weights) or rigid parts. Weights are hard to author by data and
  blur the chunky joints; rigid parts are one matrix each and draw as one instanced batch.
- **Proportions and creatures.** One rig per character, or rigs as shared body plans with each
  character's proportions as rest offsets. Shared rigs let the wolf and the deer (and later beasts)
  share clips' structure, tests and tooling; a new body plan is still only data.
- **Accessories.** Hard-wired outfits, or accessories attached to named sockets with a place each
  (one per place), colour slots and parts they hide. Sockets make an accessory fit any character
  that has them, and places make conflicts a validation error.
- **First person.** Draw the full body from the eye (it clips the camera and fills the view), a
  separate hand-built arms model (it drifts from the body), or the body's own arm chain in a
  separate view space and pass. The third keeps one source of geometry and materials.
- **Animation.** Procedural only (sines), keyframed clips only, or keyframed clips with a fixed set
  of easings and layers. Keyframes are data a tool can edit; layers let a hold and a swing play over
  a walk.
- **Colour.** Hex colours in the character data, or names from the shared style. Names keep one
  palette for the world and the characters.

## Decision

- Characters, accessories, held items, rigs and clips live in `shared/characters/characters.json`.
  The fixed vocabulary in code (`rig.mjs`): six part kinds, three joint kinds, four easings, three
  layers (`base`, `hold`, `action`), two spaces (`body`, `arms`), part views (`both`, `body`,
  `arms`), accessory places, and the run-time material `$block`.
- Rigid parts on a bone tree; rigs are shared body plans (`biped`, `quadruped`); a character gives
  rest offsets (proportions), sockets, colour slots, parts, an accessory look and variants, and clips
  by role. Materials are names from the shared style, never hexes.
- The first-person arms are the body's own arm chain (`firstPerson.bones`), anchored in view space,
  drawn in their own pass with their own FOV after the world over a cleared depth buffer, with their
  own clips on the same bones; parts may add arms-only detail.
- `shared/characters/gen.mjs` validates the data (a bad entry fails with its path; the rest pose
  must stand on the ground at its declared height) and writes it, the style and the modules into the
  Character Lab page (`docs/prototypes/character-lab.html`); CI fails on a stale page. Tests add a
  part, an accessory and a variant by data alone.
- Poses are not sent over the network: clients animate others from snapshot state; the look, the
  held item and swings without an edit are the later protocol additions (`CHARACTERS.md` §9.3).
- This phase is the design, the data and the prototype only: no game code, no protocol, save or
  terrain change, so a patch release.

## Consequences

- New parts, accessories, outfits, items, clips and creatures are data edits, made in the Lab and
  copied back; a new shape, joint or animation rule is a code change to a small vocabulary.
- Editing the shared palette or materials now also regenerates the Character Lab page (CI's stale
  check names the command).
- Building it into the game needs a Rust loader and mesh builder from the same data, a character
  shader and an arms pass in the renderer, and the netcode of §9.3 (a new compatibility line).
- Rigid parts can't bend smoothly (a cape, a tail); a two-bone tail and a hinged cape are the way,
  and a soft-body or spring layer would be a later, separate decision.
