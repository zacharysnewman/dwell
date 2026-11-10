# ADR 0073: Creatures — animals that spawn near players and wander

- **Status:** Accepted (2026-11-10; Phase 54 S3, built in 0.51.13). The behaviour beyond wandering
  waits on the owner's answers below.
- **Context:** ADR 0070 imported seven animals (`shared/models/models.json`, idle/walk/run clips)
  and drew other players with the skinned renderer; the animals were data for the Model Viewer only.
  The owner has not specified animal behaviour. This is the smallest extendable step: animals in the
  world that stand and wander, nothing more.

## Decision

1. **Data, one generator, validation.** `shared/creatures/creatures.json`: each creature (a
   namespaced name, its skinned `model` from `models.json`, `walkSpeed`, `wanderM`, `idleSeconds`
   [min, max], how many rows tall it is, the most rows it steps up and down in one step), the spawn
   `rules` (the creature, the full blocks its ground may be, the `chance` a chunk column gets a group,
   the group's size [min, max]) and `limits` (chunks around each player that spawn, the despawn
   distance, the most in the world). `shared/creatures/gen.mjs` checks every block against the
   registry and every model against `models.json` (bad entries fail naming the entry; `gen.test.mjs`)
   and writes `crates/dwell-sim/src/creatures.gen.rs`, and from `shared/creatures/fixtures` the
   fixture table (`test:walker`, a creature added by data alone, used by the Rust tests). CI runs both.
   Biome-free for now: the rules read only the ground block.
2. **Server entities, not bodies** (`dwell_sim::creatures`, like loose items, ADR 0066): a point
   (its feet), a heading, a speed, a target. Every 30 ticks each player's nearby chunk columns not yet
   rolled are rolled, once per column and face, from the world seed: a rule that hits tries up to 12
   cells of the column for its ground (the surface within 48 rows of the player's row) and places the
   group around it. Deterministic: the same seed and column spawn the same group. A column whose
   cells aren't loaded waits.
3. **Wandering**: idle for a random time in `idleSeconds`, then walk at `walkSpeed` toward a random
   point within `wanderM`. Each step finds the ground in the next column within `climbRows` up and
   `dropRows` down, a solid full block with `rows` open, dry cells over it; none (water, a cliff's
   edge, a wall, an unloaded cell) ends the walk and it idles. Face-local through `FaceFrame`: face B
   mirrors face A (`both_faces!` tests).
4. **Despawn**: a creature with no player within `despawnM` is removed; a column whose creatures are
   all gone and that no player is near is forgotten, and rolls again (the same group) when one returns.
   Creatures are not saved with the world.
5. **Wire, optional** (non-breaking on the 0.51 line): the Welcome flags byte is full, so the server
   says it keeps creatures in `Inventory`'s flags (`inventory_flags::CREATURES` = 4; older clients
   ignore the bit). A client that sees it sends `CreaturesHello` (0x5f, control), and only such a
   client gets `Creatures` (0x5e, reliable, world channel: each creature's id, model name, feet, yaw and
   speed; and the ids gone), all of them every 6 ticks (at most `limits.max`, 64, about 2 KB).
6. **Drawing**: the client keeps each creature's latest point and draws it smoothed toward it with
   the skinned renderer, which now holds a mesh per model (the player's first, the others uploaded
   when first drawn) and keys its clip players by model and id. The clip is picked from the speed:
   idle below 0.05 m/s, `sprint` (the run clip) above 3 m/s, else walk. Every model's GLB is built
   into the client (`dwell_models::GLBS`, ~1.6 MB of WASM).

7. **Group mix and what spawns (0.52.1).** A rule's optional `mix` lists other creatures its groups
   may hold: each part is rolled once a group (from the seed) with its `chance` and puts up to `max` of
   that creature in place of the rule's own (`dwell_sim::creatures::group_kinds`). The deer rule
   spawns does (`dwell:doe`, the `doe` model) with a 40 % chance of one stag (`dwell:deer`). Owner,
   2026-11-10: "let's disable all the animals except wolves and deer for now. We'll be working on
   simulation for them soon too." The horse and the chicken stay listed as creatures but have no
   rule; wolves (`dwell:wolf`, the dog recoloured at twice its size) spawn in packs of 2–4 on grass,
   at a third of the herds' chance.

## Open questions for the owner

- What should animals do beyond wandering: flee from players, follow, be fed, be hunted (drops, damage),
  be ridden (the horse), sleep at night?
- Which animals where: biomes (snow for the penguin, forests for the deer), day/night, rarity? Which of
  the seven ship at all (the tiger, the dog and the kitty are not spawned now)?
- Should animals be saved with the world (tamed or penned ones), or always respawn as now?
- Who adds creatures and with what: the JSON by hand as now, or an editor page beside the Model Viewer?
- Should the GLBs be fetched on demand instead of built in (WASM size)?
- Are animals per world (a world setting to turn them off, more or fewer)?

## Consequences

- Adding an animal is data: import its model (ADR 0070), add a creature and a rule, run the generator.
- Creatures do not collide with players, bodies or each other, and do not fall or swim: they only
  walk on ground they can stand on. Physics bodies for them are a later step if behaviour needs it.
- Reversing: drop `step_creatures` from the server's tick (no `CREATURES` flag then) and clients draw
  none.
