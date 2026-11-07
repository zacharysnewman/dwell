# Explosives: dynamite **[built, 0.49.32; cost, 0.49.33]**

Owner (2026-11-06): "dynamite block that explodes after a short time when placed, it causes blocks in a
radius to be affected by physics and appropriate force applied." The first use of the explosion
system Phase 21 plans (ARCHITECTURE.md §7.2), built on felling's bodies ([`FELLING.md`](./FELLING.md)),
not Tier 2 debris. Content is prototype (the look, the numbers): the system takes more explosives by
data.

## 1. The owner's answers (2026-11-06)

| Question | Answer |
|---|---|
| Lighting | Placed **unlit**; lit by a **torch or a use action**; then the fuse runs (its time data), fizzing with a sputtering flame. |
| Blocks | **Every block in the radius becomes a flung physics piece**, nothing destroyed outright; unbreakable (`indestructible`) blocks stay. Force falls off with distance (heavy blocks barely move at the edge). |
| Players and bodies | **Knockback and damage**, scaled by distance (standing on it can kill). |
| Chain reactions | Dynamite caught in a blast **goes off after a short random delay**. |
| Data | An **explosives table**, dynamite its first entry, a fixture entry by data alone in tests. |
| Effects | Flash, smoke and dust, a boom whose volume falls with distance, a small screen shake nearby: data, each a setting, with a CPU-adapter tier. |
| Water | The blast **pushes water** (through the water host's edits). |
| Modes | Allowed in creative (the hotbar's item). |

What gets added later and by whom: more explosives (entries in `shared/explosives/explosives.json`, by
hand or a tool; blocks of their own in `shared/blocks`), new igniters (block names in `ignitedBy`), new
numbers. Fixed in code: the falloff's shape, the lift, the puffs' motion, the shake's curve.

## 2. Data and lighting

`shared/explosives/explosives.json` → `shared/explosives/gen.mjs` (validated against the block registry;
`gen.test.mjs` adds an explosive by data alone; CI regenerates and fails on stale output) →
`crates/dwell-world/src/explosives.gen.rs`, read by the server (`dwell-sim` `blasts.rs`, `server.rs`
`blast`), the clients' prediction (`dwell_world::edit`) and their effects (`dwell-client` `blasts.rs`).

Each explosive: `block` (placed, unlit; `dwell:dynamite`, a hotbar item) and `lit` (what lighting makes:
`dwell:lit_dynamite`, not placeable, `indestructible` so no break or region edit pulls its fuse; its
`light` is `dwell:fuse`, a small sputtering flame, `shared/look/flames.json`, and its sound set's `burn`
the fizz). Both one state (no properties): the generator refuses one with properties. `ignitedBy`,
`fuseSeconds`, `radiusM`, `impulse`, `falloff`, `lift`, `maxSpeedMs`, `reachM`, `knockbackMs`,
`damage`, `waterRadiusM`, `chainDelaySeconds` and `effects` (§4); `limits` (`blastsPerTick`,
`maxPieces`). The `$comment` says what each means.

**Lighting** (the existing input model, no new action): **Place with a torch held, aimed at dynamite,
lights it** instead of placing the torch — `dwell_world::edit::check_block_edit` turns the target cell
into its lit state (`explosives::ignites`), so the server and the clients' prediction agree (the client
shows the lit fuse at once). Any block named in `ignitedBy` lights it; other blocks are placed against
it as usual. The server starts the fuse for every lit explosive an edit writes.

## 3. The blast (server)

- **Fuses** (`dwell-sim` `blasts.rs` `Fuses`): ticks left per lit cell, burnt each tick after the edits;
  at most `blastsPerTick` blow a tick (more wait a tick). A world saved with a fuse burning lights it again
  (in full) when its chunk loads (`StoreChunks` finds lit explosives).
- **What it does** (`Server::blast`): the explosive's cell and every cell whose centre is within `radiusM`
  (nearest first, at most `maxPieces`) are cleared in one `Explosion` modification (its own cell first),
  then each block becomes a one-cell natural piece (felling's bodies, `Felling::spawn_body`, an
  `EntitySpawn` each) flung along the line from the blast lifted toward the face's up by `lift`, at
  `impulse` × (1 − d / radiusM)^`falloff` over its mass (½ m³ × density), at most `maxSpeedMs`. Liquids
  within `waterRadiusM` are blown out (cleared; the water host refills the crater from round it),
  flooded blocks beyond it leave water; `indestructible` blocks stay; an explosive caught in it is lit
  with a fuse drawn from `chainDelaySeconds` (a lit one's cut to it). Bodies within `reachM` are pushed
  by `impulse` × the share over their mass. What it leaves unsupported falls (felling's rule).
- **Players** within `reachM` (to the nearest point of their capsule): a speed change of `knockbackMs`
  and `damage` points (cause `Explosion`), both × (1 − d / reachM)^`falloff`: standing on it kills,
  5 m off hurts a little.
- **Cost** (native release, `tests/explosives.rs`, a 4 m ball buried in the flat world): 512 pieces, the
  blast's tick 9.5 ms (physics 4.9), the worst tick after it 8.4 ms, every piece blocks again in ~18 s,
  70,076 body-ticks. The terrain collision under the pieces is felling's, at most `terrainChunksPerTick` a
  tick. At seed 0's dam site (`tests/collapse.rs` `measure_dynamite_at_the_dam`, the player watching
  from 10 m): 306 pieces, most landing in the river; the blast's tick ~10 ms (players' terrain collision
  5.8, physics 4.0), the first second 4.2 ms a tick, 50–70 terrain-collision builds a second. Floating
  pieces read the waterfalls' columns once per pass (`falls::FallMemo`, 0.49.33): each wet piece had
  scanned 49 columns up and down their water, the pieces phase 100 → 37 ms of that second. In Chrome
  (0.49.33, `__dwell.serverStats().phasesMs`) the local server runs the blast's first seconds at 16–45
  ticks a second, 20–60 ms a tick, nearly all physics and players (terrain collision); it catches up.

Wire: nothing new. `VoxelModificationReason::Explosion` and `DamageCause::Explosion` existed; the
blocks are synced by name (ADR 0038), so adding them is not a compatibility break.

## 4. Effects (clients)

A client sees a blast where an `Explosion` modification turns a lit explosive's cell to air
(`Game::blasts_from`), and draws its effects (`dwell-client` `blasts.rs`; Settings → Explosions):

- **Flash** (Explosion flash): a point light (`flash`: colour, strength, `lightRadiusM`, `seconds`) first
  in the frame's dynamic lights, and a fireball billboard (`sizeM`), self-lit, added.
- **Smoke and dust** (Smoke and dust): `smoke` puffs spreading out and rising (`spreadM`, `riseMs`),
  `dust` thrown out and falling (`speedMs`), each with its colour, sizes, lives and opacity; billboards
  through the flames' pass (`render/blasts.rs`, kinds 8–10), lit. A CPU adapter draws `cpuCount`.
- **Boom**: the sound event `explode` of the block's sound set (`dynamite_boom`), at the listener, its
  level full within `boom.fullM` and gone at `boom.hearM` (`boom_level`), so a far blast is heard past
  the panner's reach.
- **Screen shake** (Screen shake; off by default with reduced motion): the drawn view turned by up to
  `shake.degrees`, strongest at the blast and gone at `shake.radiusM`, dying over `shake.seconds`. The
  aim never moves.

Dev hook: `__dwell.blasts()` (each blast's place and age), telemetry `blasts.puffs`.

## 5. Tests

- Data: `shared/explosives/gen.test.mjs` (the checked-in output, an explosive by data alone, bad entries
  named). `dwell-world` `explosives.rs` (the table's blocks exist, a torch lights and nothing else, a
  fixture entry's igniter, the falloff and mass, the radius's cells, the lift on both faces, chain delays,
  the boom's level); `tests/placement.rs` (both faces: a torch against dynamite lights it in its cell,
  stone goes against it, lit it can't be broken).
- Server (`dwell-sim` `tests/explosives.rs`, both faces on an empty world's stone ground): lighting starts
  a 3 s fuse, nothing before it, then every breakable cell in the radius is a piece flung outward, the
  near ones faster, bedrock stays; a player beside it dies and one 5 m off is hurt less and pushed out
  and up; a second dynamite 3 m off is lit with a chain delay and blows after it; water within 3 m is
  blown out and farther water stays; a fixture explosive by data alone (2 m, 0.5 s); the cost bound
  (above); a world saved with a fuse burning lights it again (red: no fuse after reloading, then the
  blast found its chunk evicted far from every player).
- Client: `blasts.rs` (smoke rises and dust falls on both faces, each setting, the CPU's fewer puffs, the
  flash dying, the shake by distance); `render/blasts.rs` (quads farthest first); `pieces.rs` (a one-cell
  piece meshes one block: red in Chrome, the blast's ~500 pieces meshed eight chunk-sized blocks each,
  one 3.8 s frame, 0.63 s after; `a_blasts_512_one_cell_pieces_arrive_within_a_frame`, 0.49.33: each
  piece still meshed a chunk-sized block, 512 blocks and 273 ms natively, red; now each state's cell is
  meshed once and kept, 3 blocks, 3 ms; `a_one_cell_piece_meshes_as_its_cell_alone_wherever_its_offset`
  on both faces); Vitest (the settings record).
- Falls: `falls/tests.rs` `many_bodies_pushed_through_one_memo_are_pushed_as_each_alone` (both faces:
  480 bodies through one `FallMemo` pushed exactly as each alone, the columns read once a height).
- Browser (seed 0's dam site, `?local=1&seed=0&fresh=1&at=2364.5,142,7443.5`): placed, lit (the fuse's
  flame), the blast (smoke and flying pieces; the player standing on it died), the crater after with the
  pieces settled and the river refilling it.

## 6. Not yet

A use key (the torch is the igniter); fire spreading; blast occlusion (a wall doesn't shield); Tier 2
debris (Phase 21). **The browser's cost of a big blast** (0.49.33, seed 0's dam site): the one-cell
meshes are gone from the frame (its 0.63 s frame), but frames still reach 60–220 ms for a moment (p95
33 ms over the blast's 8 s; the prediction's ticks with ~300 stand-ins, 20–25 ms a frame, and catching
up after a slow frame), and the local server drops to 16–45 ticks a second for 3–5 s (physics 300–700
ms and terrain collision 150–600 ms of each second). Natively the same blast is 4 ms a tick, under
WASI ~2× that: the browser's worker runs it several times slower again. What would cut it: fewer
bodies (a blast's far cells as one body, or Tier 2 debris), terrain collision not rebuilt for water-only
changes, the stand-ins' prediction only near the player.
