# Explosives: dynamite **[built, 0.49.32; cost, 0.49.33–0.49.34; 5 m with a destroyed core, 0.49.38; 4 m ×1.5, loosen and no-contact options, 0.49.41; out through the open side, 0.49.45; 5 m, debris thrown clear of a flush charge, 0.49.46]**

Owner (2026-11-06): "dynamite block that explodes after a short time when placed, it causes blocks in a
radius to be affected by physics and appropriate force applied." The first use of the explosion
system Phase 21 plans (ARCHITECTURE.md §7.2), built on felling's bodies ([`FELLING.md`](./FELLING.md)),
not Tier 2 debris. Content is prototype (the look, the numbers): the system takes more explosives by
data.

## 1. The owner's answers (2026-11-06)

| Question | Answer |
|---|---|
| Lighting | Placed **unlit**; lit by a **torch or a use action**; then the fuse runs (its time data), fizzing with a sputtering flame. |
| Retune (later the same day) | "maybe keep it at 4m but increase the force?": **radius 4 m**, the 2 m core destroyed, damage only within 2 m, the impulse, its speed cap and the knockback **×1.5** (13,500 N·s, 33 m/s, 33 m/s). Two more ideas as data, measured side by side (§3.1): "dislodge (make physical) blocks in a 5m radius, but only exert force on 4m" (`loosenRadiusM`) and "what if all the physics blocks couldn't collide with each other for the first 1-2s? … keep it down to 1s" (`noPieceContactS`). |
| Blocks | **Radius 5 m** (owner, later the same day; was 4; back to 4 m in 0.49.41, above). Blocks **within 2 m are destroyed outright**; from 2 to 5 m **every block becomes a flung, real physics piece** (no visual-only debris); unbreakable (`indestructible`) blocks stay. Force falls off with distance (heavy blocks barely move at the edge). Was: every block in the radius a piece, nothing destroyed. |
| Players and bodies | **Knockback** scaled by distance out to its reach; **damage only within the destroy radius** (2 m; standing on it kills): a player 3–5 m off is pushed but unhurt. Walls don't shield the blast. |
| Saved fuses | A world saved with a fuse burning restarts it in full when it loads (the simplest; kept). |
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
`fuseSeconds`, `radiusM`, `destroyRadiusM` (0 to `radiusM`, the generator refuses more), `loosenRadiusM`
(at least `radiusM`, the generator refuses less), `impulse`, `falloff`, `flingFalloff`, `lift`, `maxSpeedMs`, `reachM`, `knockbackMs`,
`damage`, `waterRadiusM`, `noPieceContactS` (0–10 s), `openSenseM`, `openBlend`, `openPathM`, `openSpreadMin` (0–1), `shell` (optional: `fling`, the default, or `crack`) and `crackOverCleared` (optional, only with `crack`; §3.6), `chainDelaySeconds` and `effects` (§4); `limits` (`blastsPerTick`,
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
  (nearest first, at most `maxPieces` pieces) are cleared in one `Explosion` modification (its own cell
  first). A block within `destroyRadiusM` (`explosives::destroys`) is gone, no piece; each one beyond
  becomes a one-cell natural piece (felling's bodies, `Felling::spawn_body`, an
  `EntitySpawn` each) flung along the line from the blast lifted toward the face's up by `lift`, at
  `impulse` × (1 − d / radiusM)^`flingFalloff` over its mass (½ m³ × density), at most `maxSpeedMs`
  (0.49.46: `flingFalloff` 0, every piece alike; players and bodies keep `falloff`). Liquids
  within `waterRadiusM` are blown out (cleared; the water host refills the crater from round it),
  flooded blocks beyond it leave water; `indestructible` blocks stay; an explosive caught in it is lit
  with a fuse drawn from `chainDelaySeconds` (a lit one's cut to it). Bodies within `reachM` are pushed
  by `impulse` × the share over their mass. What it leaves unsupported falls (felling's rule).
- **Loosened, not flung** (`loosenRadiusM`, 0.49.41): the cells from `radiusM` out to it become pieces too,
  with no impulse (the falloff's share is nothing past `radiusM`): they slump and fall. Equal to
  `radiusM` (the shipped dynamite), none.
- **Pieces kept apart** (`noPieceContactS`, 0.49.41): for that long a blast's pieces touch only the
  terrain, players and other bodies, not one another (`dwell_physics::Physics::set_apart`: their
  colliders move to the `APART` collision group, which every layer's filter takes but its own). Past it
  each touches the others again only once it overlaps none (`overlaps_pieces`, `Server::rejoin_apart`
  each tick after the step), so pieces that landed in one another aren't pushed apart. 0 (the shipped
  dynamite): always touching. Pieces of two blasts in their windows don't touch either.
- **Players** within `reachM` (to the nearest point of their capsule): a speed change of `knockbackMs`
  × (1 − d / reachM)^`falloff`; `damage` points (cause `Explosion`) × (1 − d / destroyRadiusM)^`falloff`
  only within `destroyRadiusM` (`explosives::blast_damage`): standing on it kills, 1.2 m off hurts
  (~40), 2 m and beyond pushed but unhurt. Walls don't shield (no occlusion, §6).
- **Cost** (native release, `tests/explosives.rs`, the 5 m ball buried in the flat world, 0.49.38): 1,013
  cells, 61 in the 2 m core destroyed and **952 pieces** from the shell (the 4 m blast's: 512), the
  blast's tick 12.2 ms (physics 7.6), the worst tick after it 11.3 ms, every piece blocks again in ~12 s,
  172,078 body-ticks (4 m: 9.5 ms, 8.4 ms, 70,076). Half-height cells make the shell ~950 cells, not
  the ~490 m³ of its volume. At seed 0's dam site 571 pieces (4 m: 306), the blast's tick 11.7 ms
  (physics 6.1), then ~6 ms a tick for the first second, 107,735 body-ticks. The terrain collision under the pieces is felling's, at most `terrainChunksPerTick` a
  tick. At seed 0's dam site (`tests/collapse.rs` `measure_dynamite_at_the_dam`, the player watching
  from 10 m): 306 pieces, most landing in the river; the blast's tick ~10 ms (players' terrain collision
  5.8, physics 4.0), the first second 4.2 ms a tick, 50–70 terrain-collision builds a second. Floating
  pieces read the waterfalls' columns once per pass (`falls::FallMemo`, 0.49.33): each wet piece had
  scanned 49 columns up and down their water, the pieces phase 100 → 37 ms of that second.
- **Out through the open side** (0.49.45; owner, 2026-11-06: pushed radially, "at least half the shell is pushed
  into the ground, where it jams and takes 10–30 s to settle"; "putting dynamite on the side of a cliff or the
  underside of a cave roof would blast out or down"). Before the blast clears anything, the cells within
  `openSenseM` (3 m) give the way out (`explosives::open_dir`): the mean of the unit directions to the open
  cells (air, liquid) less those to the solid ones, doubled and at most 1 long: flat ground up, a cliff face out,
  a cave roof down (each length 1), buried about 0. Computed from world cells, so face B's mirrored ground gives
  the mirrored way out with no frame code. Each flung piece goes along its line from the blast (no `lift`: that
  is the players' and bodies' push now) turned toward the way out by `openBlend` (0.6) × the openness
  (`piece_dir`); buried, radial. A push going out keeps at least `openSpreadMin` (0.7, 0.49.46) of it
  sideways, along its line's own sideways part, so a flush charge's debris flies out in a cone rather than
  straight up and back into the crater (a piece straight over the charge keeps its push). **What can't move stays** (`launching`): a shell cell flies only if its push,
  out to `openPathM` (1 m) or until it reaches open space (air, liquid, the cleared core), meets no rock and
  every shell cell it passes flies too; decided as a spread from the opening until nothing changes (owner's
  correction: the rock in the way "would need to exclude the pieces that are about to be thrown"), so the order
  does not matter. The rest stay as terrain. Loosened cells (past `radiusM`) fly whatever. `openPathM` 0: every
  cell flies, as before.

### 3.2 The open side, measured (0.49.45)

Native release (`tests/explosives.rs`, the scene's stone, both faces): pieces' mean velocity, face-local, against
the open side's normal (at 4 m): flat ground 112 pieces, cos 1.00 (0.62 m/s mean, up); a cliff face 34, cos 0.94–0.95
(out); a cave roof 112, cos 1.00 (down); buried, none (a cavity). On the flat world, a charge on the ground:

| | Pieces | Body-ticks | Settled | Worst tick |
|---|---|---|---|---|
| Before (radial, every shell cell; `openPathM` 0, `openBlend` 0) | 208 | 13,420 | 1.4 s | 3.9 ms |
| Now (open side, 3 m sense, 0.6 blend, 1 m path) | 112 | 14,086 | 5.7 s | 5.1 ms |

About half the pieces, the same body-ticks: natively the pieces once driven into the flat ground came to rest
at once, where now they fly up out of the crater and land. The jam the owner saw (10–30 s) was in Chrome at
seed 0's dam site; the Chrome measurement there and the 5 m radius under the new rules are §3.3 (0.49.46). The surface test pins fewer than 3/5 of the old pieces and at most 20,000 body-ticks.

### 3.3 The 5 m radius and the flush charge (0.49.46)

**Chrome at the dam** (§3.1's scene, method and view; `?local=1&seed=0&fresh=1&at=2364.5,142,7443.5&time=0.45`,
settled 20 s; ticks from `__dwell.serverStats()` each second, frames over the 15 s from the blast, settled = the
worker's `physics` back near idle). Native: `dwell-sim` `tests/collapse.rs` `measure_dynamite_at_the_dam`.

| | Config | Pieces (native, dam) | Slowest second | Seconds below 55 | Physics (Chrome, 23 s) | Worst frame / p95 | Settled (Chrome / native) |
|---|---|---|---|---|---|---|---|
| A (§3.1) | 4 m, ×1.5, radial | 452 (flat) | 48 | 1 | ~1.6 s | 50 / 18 ms | ~9 s / 12 s |
| 0.49.45 | 4 m, open side | 116 | 60 | 0 | 0.43 s | 33 / 19 ms | ~7 s / ~8 s |
| 0.49.45 at 5 m | 5 m, open side | 261 | 56 | 0 | 1.1 s | 34 / 18 ms | ~9 s / ~11 s |
| **0.49.46** | 5 m, impulse ×2, `flingFalloff` 0, `openSpreadMin` 0.7 | 214 (flush, flat) | 52 | 1 | 2.0 s | 19 / 19 ms | ~13 s / ~10 s |

At 4 m the open-side blast barely showed at the dam (the shell's push meets the bank's rock and stays);
5 m under the new rules cost about what 4 m did, so the radius went back to 5 m. 0.49.46's dam blast throws
visibly more (a 1–2 s dip to 52 ticks, `advance` up to ~200 ms in the worst second while the terrain collision
rebuilds under the pieces, ~13 s to settle against A's ~9).

**The flush charge** (owner, 2026-11-06: "blowing it up level with the surface of a flat area, just cracks the
surface with little to no changes"; "most of the debris does fall right back in"). Natively (`tests/explosives.rs`
`flush_blast`, the charge's top level with the flat world's ground; clear = pieces that came to rest more than
`radiusM` across the ground from it; crater = air cells under the surface within `radiusM`):

| Config (5 m) | Pieces | Clear of the rim | Crater (cells) | Settled | Body-ticks |
|---|---|---|---|---|---|
| 0.49.45 (13,500 N·s, falloff 1.5, no floor) | 250 | 10 | 43 | 10.5 s | 44,408 |
| ×2, falloff 1, floor 0.7 | 214 | 12 | 69 | 20 s+ | 58,776 |
| ×4, falloff 1, floor 0.7 | 214 | 62 | 110 | 9.6 s | 54,002 |
| ×2, falloff 1, floor 0.7, no contact 0.5 s | 214 | 63 | 115 | 20 s+ | 53,568 |
| ×1.5, fling falloff 0, floor 0.7 | 214 | 86 | 121 | 9.2 s | 48,944 |
| **×2, fling falloff 0, floor 0.7 (shipped)** | 214 | **101** | **136** | 9.8 s | 54,350 |
| ×2, fling falloff 0, no floor | 250 | 107 | 139 | 10.7 s | 75,740 |
| ×2, fling falloff 0, blend 0.35, floor 0.7 | 72 | 52 | 84 | 8.8 s | 19,944 |
| ×3, fling falloff 0, floor 0.7 | 214 | 113 | 145 | 14.7 s | 76,754 |

What held the debris in was not the force alone: in the first ticks the packed pieces ram one another (an inner
piece, faster under the falloff, into the slower one outside it) and the stationary rim, and lose most of their
speed (0.49.45's mean 1.5 m/s two ticks after the blast; ×4 at falloff 1 only 6 m/s). Flinging every piece alike
(`flingFalloff` 0) lets neighbours fly together; the sideways floor turns those near the axis outward but
matters less than the falloff (the packed start re-mixes directions). In Chrome on the flat world
(`?world=flat`, a block broken and the dynamite placed in its hole): an obvious crater with debris strewn round it,
60 ticks every second, the worst frame 19 ms, settled ~10 s. Still not "most" clear (47%): the pieces from deep
in the crater's walls hit the rock beside them. The test `a_flush_charge_throws_its_debris_clear_and_leaves_a_crater`
pins at least 35% clear, 4× 0.49.45's, a crater 2.5× as large, settled within 15 s. Screenshots (scratch, not
kept): the dam mid-flight and after, the flush crater before, mid and after.

### 3.1 The owner's four, compared (0.49.41)

Chrome on the GPU (`?local=1&seed=0&fresh=1&at=2364.5,142,7443.5&time=0.45`, the LOD settled 20 s; the
player flying, dynamite placed and lit by `__dwell.select`/`edit('place')` from (2364.5, 141, 7441.5)
looking down 60°, its cell (2364, 281, 7442) on the bank; then the view from (2364.5, 144, 7432.5), yaw
0, pitch −15). Ticks a second from `__dwell.serverStats()` (it reports every 1–2 s), `physics` the
worker's step summed over the 23 s after the blast, frames from `requestAnimationFrame` over the 15 s
from the blast. Native (`tests/explosives.rs` `measure_dynamite_configs`, buried in the flat world's
stone, release): pieces, physics summed, time until every piece is a block again.

| | Config | Pieces (native) | Slowest second | Seconds below 55 | Physics (Chrome) | Worst frame / p95 | Settled (Chrome / native) |
|---|---|---|---|---|---|---|---|
| A | 4 m, ×1.5 (**shipped**) | 452 | 48 | 1 | ~1.6 s | 50 / 18 ms | ~9 s / 12 s |
| B | A + no contact 1 s | 452 | 16 | ≥ 20 (still 16–24 at 23 s) | ~36 s | 34 / 18 ms | not in 23 s / 27 s |
| C | loosen 5 m, force 4 m, ×1.5 | 952 | 16 | ~10 | ~8.6 s | 51 / 33 ms | ~15 s / 30 s |
| D | C + no contact 1 s | 952 | 16 | ≥ 20 (16 at 23 s) | ~44 s | 67 / 33 ms | not in 23 s / 30 s+ |

(The 5 m blast of 0.49.38 for comparison: 32–48 ticks a second for 4–7 s.) Natively: A 397 ms of
physics, B 4,516, C 1,939, D 15,882; the first second's ticks 172, 196, 451, 320 ms.

**The no-contact window costs more than it saves**, so dynamite ships as **A** (`noPieceContactS` 0,
`loosenRadiusM` = `radiusM`), B–D a data change away. In the window the step is no cheaper (natively
3.2 ms a tick against A's 1.9: packed pieces that touch are held by their neighbours and stop, while
pieces that don't all fly at full speed, spread wider, and build more terrain collision); after it the
pieces lie in heaps inside one another, those that overlap stay apart (293 of 452 at 3 s), and the heaps
of interpenetrating bodies resting on the ground and on rejoined pieces keep the solver busy for 10–20 s
(natively 6–11 ms ticks at 3–9 s). No popping was seen (the re-enable waits until a piece overlaps
none), but the cost is the reason. Making it pay would need the apart pieces to settle while still
overlapping (or to rejoin at once with capped depenetration) — not tried. Screenshots
(`docs/explosives/`): `A-mid.jpeg`, `A-after.jpeg`, `B-mid.jpeg`, `B-after.jpeg`, `C-mid.jpeg`,
`C-after.jpeg`, `D-mid.jpeg`, `D-after.jpeg` (mid: ~0.1–0.5 s after the blast; after: 23 s on).

- **Terrain collision** (0.49.34) is rebuilt only for writes that change what a cell is to collision
  (`Chunk::collision_revision`, bumped when `dwell_blocks::collision_class` — nothing, or the shape —
  changes): the water the host lets into the crater had rebuilt the chunks round it, 54 of the first
  second's builds; now 0 (terrain and players 10 → 2 ms of that second natively). Builds stay coalesced
  per chunk per tick and capped at `terrainChunksPerTick` new chunks for the pieces' boxes.
- **The prediction's stand-ins** (0.49.34): a client stands in only for the pieces that could touch its
  player soon (`dwell_physics::predictor::stands_in`: within the piece's cells' reach plus `STAND_IN_M`
  4 m plus its closing speed on the player over `STAND_IN_LOOKAHEAD_S` 0.5 s; kept until `STAND_IN_KEEP_M`
  2 m past that; none while the player is dead), re-judged at each snapshot from each piece's latest
  pose; the rest are drawn from the snapshots, interpolated, and never stepped. Each had been posed,
  stepped and replayed in every predicted tick: ~300 cost 20–25 ms a frame.
- **In Chrome, 5 m** (0.49.38, with SIMD, same scene and method as below): physics 6,048 and 6,207 ms
  of the worker's 12 s from lighting (the 4 m blast's with SIMD: 2,785–4,379); 32–48 ticks a second for
  4–7 s before it catches up (4 m: 45–62); the worst frame 50 ms, p95 19 ms (4 m: 33–35, 18).
- **In Chrome** (0.49.34, `__dwell.serverStats()`; the player 10 m off and 3 m up, flying, the world's
  LOD settled first): 58, 40, 48, 52, 57, 58, 59 ticks a second from the blast on (0.49.33: 16–45 for
  3–5 s), `terrain` 14–52 ms a second (0.49.33's `players`, which held it: 150–600), `physics` 560–900,
  `pieces` 80–150; no frame over 33 ms, p95 19 ms (0.49.33: 60–220 ms worst). Measured before the LOD
  settles, the page's mesh and LOD workers hold most of the machine's cores and the server's worker runs
  its physics at half speed: ~24 ticks a second for 2 s.

Wire: nothing new. `VoxelModificationReason::Explosion` and `DamageCause::Explosion` existed; the
blocks are synced by name (ADR 0038), so adding them is not a compatibility break.

### 3.5 Waves (0.49.48)

The owner (2026-11-06): "Those waves were super awesome from the queued up waves reflecting, can we
make dynamite trigger waves?" A blast in or beside water gives it an outward kick, and the water
grid's own law does the rest (it carries the wave, reflects it off banks and walls, and damps it):

- **Data, per explosive:** `waveReachM` (the kick's reach, m), `waveImpulse` (the outward speed given
  to the water at the blast, m/s; 0: no wave), `waveFreeM` (held water within it is freed, so the
  wave runs to the banks and back instead of being taken in by held water, which stays at its
  level). Red, blue and green: 6 m, 3 m/s, 24 m. Validated by `gen.mjs` (`gen.test.mjs` adds a
  fixture's by data alone).
- **The kick** (`Water::wave`, docs/WATER_SURFACE_GRID.md §2f): every face between two water columns
  within `waveReachM` of the blast (its distance from the column's water span) gets discharge away
  from the blast: `waveImpulse` × the face's wet height × (1 − d / reach) × the face's share of the
  outward direction. Only discharge is added, so the water's volume is conserved; the columns are
  freed and their chunks woken. The blast sends it after the cells it cleared, in the same batch
  (`In::Wave`), so it works with the water inline, on its own thread or in the browser's worker.
- **What rides it:** floating pieces and swimmers take the grid's currents (§2c of the water doc), so
  they ride the wave with nothing added.
- **Measured** (`water::tests`, a 24 × 6 m pond 1.5 m deep, kicked at its west end at 3 m/s): the
  crest reaches the far bank (22 m) in 5.2 water s at +0.32 m, comes back to it at 16.8 s, sloshes
  on as a seiche losing about a tenth of its height a crossing, and the pond sleeps again at about
  2,100 water s (about 30 real s at the 72× day).

### 3.6 Variants (0.49.47, yellow 0.49.50)

Four explosives, all by data (`shared/explosives/explosives.json`; blocks, items and painterly tints
like red's, `shared/textures/materials.json`):

| Explosive | Destroys | Shell (to `radiusM`) | Owner |
|---|---|---|---|
| `dwell:dynamite` (red) | 2 m | 2–5 m flung (`shell` `fling`), along the open side | 2026-11-06 |
| `dwell:dynamite_blue` | 4 m | none (`radiusM` = `destroyRadiusM` = 4) | 2026-11-06: "just blows up 4m" |
| `dwell:dynamite_green` | 4 m | 4–5 m flung as red's | 2026-11-06: "destroys 4m and adds force like red on 4-5m" |
| `dwell:dynamite_yellow` | 4 m | 4–7 m cracked (`shell` `crack`, `crackOverCleared`; 4–5 m until 0.50.8) | 2026-11-07: "destroys 4m radius, and cracks anything touching air"; "anything touching air in 5m not queued for destruction"; "increase the radius … by 2 meters, for cracking range only" (flush on flat ground: 280 destroyed, pieces 114 → 366, surface cells 32 → 100); "to make cave collapses cooler where the ceiling drops" |

**Crack** (`ShellRule::Crack`, `Server::blast`, `dwell_world::explosives::cracking`): the open (air,
liquid) cells round the charge are read before any cell clears; a shell cell cracks if one of its six
neighbours was open then, or, with `crackOverCleared`, if its cell below (the face's down, through
`FaceFrame`, so face B's roof is mirrored) is one the core destroys — a cave roof over the blast drops.
A cracked (exposed) cell breaks loose and takes the blast's push whatever lies in its way, even when
pushed into rock and moving little; every other shell cell is judged by the open-side path test as a
fling shell's is — it flies with the push if it has a way out, else stays as terrain (0.50.1; owner,
2026-11-07: "I want the wall check for everything not exposed to air"; before, cracked pieces got no
push and every other shell cell stayed, so on flat ground yellow looked like blue: supported cracked
pieces settled back where they were). Test: `tests/explosives.rs`
`yellow_flings_every_exposed_cell_and_wall_checks_the_rest` (both faces: a charge in rock with a cliff
open from 5 m; every exposed cell cracks loose, the roof cell 4.5 m over the core among them, the one
facing the open cliff is flung > 5 m/s (red on 0.50.0: 0.33 m/s, no push), cells pushed into rock with
no way out stay).

### 3.7 Touching water (0.50.3)

Owner, 2026-11-07: "Make dynamite not destroy or dislodge (physics) blocks when it's touching water."
`waterSparesBlocks` (data, on every explosive): a charge whose own cell or any of its six neighbours is
water (or a flooded block, such as dynamite placed into water) destroys and dislodges no blocks — no core
cleared, no shell pieces. Its wave (§3.5), the players' and bodies' knockback and damage, and chain
reactions still happen. Test: `tests/explosives.rs` `a_charge_touching_water_spares_the_blocks` (both
faces, every explosive; red on 0.50.2: red dynamite flung pieces beside water).

### 3.8 Water shields what is behind it (0.50.4)

Owner, 2026-11-07: "it shouldn't destroy blocks or move them through water, so on the other side of water
blocks". `waterShields` (data, on every explosive): a cell whose straight line from the charge passes
through water (or a flooded block) in any cell between them, as the world stood before the blast, is
neither destroyed nor dislodged (`dwell_world::explosives::behind_water`: cell centres in metres, rows
½ m, sampled every 12.5 cm). Tests: the unit test `a_cell_behind_water_is_shielded_and_one_beside_it_is_not`
and `tests/explosives.rs` `water_shields_the_rock_behind_it` (both faces: blue in rock with a water wall
2 m out; red on 0.50.3: the rock 3 m out behind the water was destroyed).

## 4. Effects (clients)

A client sees a blast where an `Explosion` modification turns a lit explosive's cell to air
(`Game::blasts_from`), and draws its effects (`dwell-client` `blasts.rs`; Settings → Explosions):

- **Flash** (Explosion flash): a point light (`flash`: colour, strength, `lightRadiusM`, `seconds`) first
  in the frame's dynamic lights, and a fireball billboard (`sizeM`), self-lit, added.
- **Smoke and dust** (Smoke and dust): `smoke` puffs spreading out and rising (`spreadM`, `riseMs`),
  `dust` thrown out and falling (`speedMs`), each with its colour, sizes, lives and opacity; billboards
  through the flames' pass (`render/blasts.rs`, kinds 8–10), lit. A CPU adapter draws `cpuCount`.
  **They follow the blast's direction** (0.49.51; owner, 2026-11-07: "Make the smoke particles follow
  explosion direction"): the client reads the blast's way out from its own cells round the charge,
  before the modification clears them (`blasts::open_way`, the server's `explosives::open_dir`; no
  protocol change), and each puff's way out is turned toward it by `openBias` × the openness
  (`blasts::toward_open`; smoke 0.7, dust 0.6): a cliff charge's smoke billows out of the face, a cave
  roof's down, flat ground's up as before; a buried blast (openness about 0) keeps the even spread.
  Test: `blasts.rs` `the_smoke_follows_the_blasts_open_side_and_a_buried_one_spreads_evenly` (both
  faces: the smoke's mean offset along the cliff's and the roof's open side, cos > 0.8, red with
  `openBias` 0 at cos −0.31; the CPU's count unchanged; buried as before).
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
  fixture entry's igniter, the falloff and mass, the radius's cells (5 m: 1,013; the 2 m core: 61; 4 m: 513), damage
only within the destroy radius and knockback past it, the lift on both faces, chain delays,
  the boom's level); `tests/placement.rs` (both faces: a torch against dynamite lights it in its cell,
  stone goes against it, lit it can't be broken).
- Server (`dwell-sim` `tests/explosives.rs`, both faces on an empty world's stone ground): lighting starts
  a 3 s fuse, nothing before it, then every breakable cell within 2 m is gone with no piece, and every
  one from 2 to 4 m is a piece flung outward (each starting past 2 m), the near ones faster, bedrock
  stays; a player 0.5 m off dies, one 1.2 m off is hurt, one 4 m off is pushed out and up but unhurt; a second dynamite 3 m off is lit with a chain delay and blows after it; water within 3 m is
  blown out and farther water stays; a loosen shell by data (force to 2 m, loosened to 3 m: every cell
  1–3 m a piece, those past 2 m given no impulse, the near ones flung); a blast's pieces with a 1 s
  window make no piece-piece contact for its 58 ticks (without it, over 50 pairs the tick after), touch
  again after it and mostly rejoin; a fixture explosive by data alone (2 m, its 1 m core destroyed, 0.5 s); the cost bound
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
- Cost (0.49.34): `dwell-physics` `terrain.rs` `water_coming_and_going_rebuilds_no_collision` (water in a
  dug cell, on the ground and at a chunk corner, a ramp flooding and draining: no build; a block placed:
  one; red: 5 builds on the chunk's revision); `tests/predictor.rs`
  `only_the_pieces_that_could_touch_the_player_soon_are_stood_in_for` (300 still pieces 8–38 m off: none;
  one closing at 20 m/s from 14 m and one beside the player: both; the margin and re-entry; none while
  unplaced; red: all stood in for); `dwell-sim` `tests/netcode.rs` sets every body's latest pose each
  snapshot, as `game.rs` does (a log at rest is stood in for once the player nears it).

## 6. Not yet

A use key (the torch is the igniter); fire spreading; blast occlusion (a wall doesn't shield); Tier 2
debris (Phase 21). **The browser's server at a big blast** (0.49.34, seed 0's dam site): 40–60 ticks a
second for 2–3 s, nearly all the physics step (560–900 ms of a second in the worker; natively ~2 ms a
tick). Natively the step is half narrow phase and half solver (rapier's counters: ~3,100 contact pairs,
1,350 constraints and 4,500 contacts a tick in the first second, ~10 pairs a piece: they fly out
packed), CCD and islands nothing: CCD off for the pieces measured the same (kept on). What would cut it
changes how the debris behaves, so it is the owner's: fewer bodies (a blast's far cells as one body,
or Tier 2 debris); fewer solver iterations (rapier's 4; the players' and the prediction's too); no
contacts between pieces. Not gameplay but the build: the wasm module is built with WebAssembly SIMD
(`+simd128`, 0.49.37, ARCHITECTURE.md §2.1). Same scene, the LOD settled, 12 s from lighting: the
worker's `physics` 4,721 and 5,436 ms (main) → 2,785, 4,193 and 4,379 ms (SIMD), about a quarter less;
the slowest second 40 and 32 ticks (main) → 57, 45 and 46; at rest 8.5–11.4 → 6.3–9.1 ms a second. The
page's frames were the same (worst 33–35 ms, p95 18 ms, vsync-bound) and the controller's cost did not
move (`sixty_four`, wasip1 release: 0.80 → 0.79 ms a tick): the gain is the solver's, over many bodies. Separately, the browser's
`stream+lod+save` phase took 100–860 ms of a tick at times with no blast at all. Split (0.49.35), the
time was the chunk streaming's (`stream`, 50–640 ms a second at a join and after the player moves,
calls up to 400 ms), not the autosave's: the save was 3–25 ms of one tick every 5 s, and is now
spread over ticks at ≤ 2 ms of encoding a tick plus a commit tick of its own (ARCHITECTURE.md §6.4).
The streaming's cost (0.49.36) was the air test's: a chunk column's sky floor (its column heights,
~1.2 ms natively) computed on the tick for each new column the view, the prefetch and the LOD's chunk
requests reached. It is now computed in the server-jobs worker (a thread natively, or within 1 ms a
tick without either), the chunks waiting for it (ARCHITECTURE.md §6.3, *Sky floors off the tick*):
in the browser `stream` went from 25–367 ms a second at a join to at most 1.5 ms.
