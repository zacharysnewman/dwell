# The far ecology (deer and wolves)

WORK_QUEUE item 11: the first slice (2026-11-11, the data-level core) and the second (the server's
tick and save, §6; no floors, §2). Architecture: [`ARCHITECTURE.md`](./ARCHITECTURE.md)
§5.4. Code: `crates/dwell-ecology`. Data: `shared/ecology/ecology.json` (`gen.mjs` →
`ecology.gen.rs`). The owner's mockup it grows from: [`proposals/ECOSYSTEM_MOCKUP_NOTES.md`](./proposals/ECOSYSTEM_MOCKUP_NOTES.md).

## 1. The owner's direction

"Simulate at a data level only when you're not nearby, render simply from afar, and exactly when
close by." Answers (2026-11-11):

- **Far = herds and packs**: each one record (position on the island, size, hunger and energy
  averages, state); individuals are made only when a player comes near (a later slice).
- **Deer eat tall grass**, but "it shouldn't affect the world until you are close enough to see":
  far away the truth is a **grazing value per map cell** (its most from the ground, depleted by
  grazing, regrowing over time); the real blocks are not touched (a later slice applies it near
  players).
- **The whole island, coarse**: fast-forwarded in big steps, like the water grid's far pools
  (ADR 0060).
- **Players watch only** (no interaction yet).
- **Tuning goals**: "resistant to complete destruction, quick to replicate (only up to a max per
  herd/pack), slow to starve"; and no runaway: breeding fast only up to each group's cap, the whole
  island's total bounded (by grazing for deer, by prey for wolves).
- **They can die** (owner's correction, 2026-11-11): "I just want them systemically resistant to
  being wiped out, not immune from it." No hard floor: the resistance is the system's, as data —
  predation that eases at low prey density (Holling type III), refuges hunters rarely reach, fast
  breeding at low density up to the cap, hunters dispersing and leaving when prey is scarce, slow
  starvation.

What is extendable here: **more species and more grounds by data** (a species names its diet, what
it flees and what it hunts by name; a test adds a hare and a fox by data alone); the **rules**
(graze, hunt, flee, refuge, disperse, split, merge, die, capacity) are the engine's small fixed
vocabulary. The species' tuning is per build; per world are the ecology's on/off switch and its speed
(saved with it, §6).

## 2. The model

- **The island** (`Island`): an `n` × `n` grid of `island.cellM` (200 m) cells over the face's
  world map. A cell is water (sea or lake: no forage, not walked) or land, whose **most forage** is
  the first `grazing.grounds` entry its height and slope are under (meadow, upland, crag). Forage is
  in deer-days: one unit feeds one head for a day. A ground may be a **refuge** (`refuge`, 0 to 1:
  the share of a herd on it a hunter can't take; the crag's 0.8).
- **Forage** regrows toward the cell's most at `regrowPerDay` of the gap a day (exactly:
  `g = max − (max − g)·e^(−r·dt)`) and is eaten by herds.
- **A group** (`Group`): species, face-local x/z, size (fractional head, an average), hunger (0 fed,
  1 none), energy (1 rested), activity (resting, grazing, moving, fleeing, hunting, roaming,
  dispersing). Each step, in id order:
  - **A grazer** scores every walkable cell in its sense: feed (up to ten days' worth), danger
    (closeness of anything it `flee`s within `radiusM`, doubled), distance, a little stickiness to
    its own cell (the danger scaled down by the cell's refuge, so a threatened herd makes for the
    crags); walks toward the best (water stops it) and eats where it ends, then, over a long
    step, from the other cells it could reach, best first.
  - **A hunter** goes for the nearest prey group in its sense and kills what it needs within
    `reachM`: at most `killsPerHeadPerDay` a head times the **Holling type III** share
    `N² / (N² + h²)` of the prey head `N` it can find in its sense outside refuges (`h` the data's
    `halfSaturationHead`, 15 for the wolf): with few deer left the last are hard to find, so each
    deer's risk falls as they get scarce; never more than the target's head outside its refuge;
    senses none: rests if fed, else roams on a heading hashed from (seed, id, 10-day bucket). Above
    `disperse.aboveHunger` it **disperses**: heads for the island's biggest prey group at
    `speedFactor`, members leaving at `leavePerDay`, instead of starving.
  - **Hunger** relaxes toward `1 − fed share` with time constant `hungerDays`; **energy** toward its
    activity's (rest 1, travel 0.6, flight 0.3) over 2 days, and scales its speed (½ to 1×).
  - **Size** multiplies by `exp(rate·dt)`: breeding `breedPerDay` × how well fed (hunger under
    `breedBelowHunger`, falling to 0 at it) × room on the island; minus starving (`starvePerDay`,
    scaled by hunger above `starveAboveHunger`); minus leaving while dispersing. **No floor**: a
    group under its `dieBelow` (½ head) is gone, and a species with none left has died out.
  - **Room on the island** (the owner's no-runaway goal): `1 − total / capacity`, the capacity
    `Island::carrying_capacity`: a grazer's is the forage regrown a day were it grazed bare over its
    feed a head; a hunter's the smaller of its prey's capacity and its prey's head now, over
    `preyPerHead`. The growth is a Ricker step (`N·e^(b·dt·(1 − N/K))`, `b·dt` ≤ 1), which never
    overshoots `K`: a species' total stays under its capacity, and grows fastest when it is small.
  - After all groups: **merge** (a group under `mergeBelow` joins one of its kind within
    `mergeRadiusM` if together they fit the cap), then **split** (above `cap` a group splits in two
    halves, each alive as `cap ≥ 2 × dieBelow`; at `maxGroups` it stops at the cap instead).
- **Steps**: `step(island, data, eco, days)` cuts `days` into steps of at most `island.stepDays`
  (5), every rate in closed form over its step. 100 days at once land within ~6 % of 100 single
  days on the test island (the test allows 15 %).
- **Determinism**: the only randomness is a hash of (seed, group id, bucket); `Ecosystem::digest`
  hashes the whole state. Not yet checked native ↔ WASM (it uses `exp`, `atan`, `hypot`).
- **Faces**: face-local; each face has its own `Island` (from its own map) and `Ecosystem`; nothing
  depends on which way is up. The world-map test runs on both faces' maps.

## 3. The tuning (prototype, ARCHITECTURE.md §6.1)

| | deer | wolf |
|---|---|---|
| dies below / cap / max groups | ½ / 12 / 40 | ½ / 7 / 8 |
| start | 8 herds of 6 | 2 packs of 4 |
| breeding (well fed) | 3 % a day (doubling ~23 days) | 0.8 % a day |
| starving | at most 1 % a day (half a herd lost in ~2 months unfed; gone in under 3 years) | at most 0.5 % a day |
| speed, sense | 3 km a day, 800 m | 8 km a day, 3 km |
| diet | 1 forage a head a day | kills ≤ 0.12 × N²/(N²+15²), needs 0.08 deer a head a day; 20 deer a wolf |
| flee / disperse | wolves within 500 m | above hunger 0.6: 2× speed, 1 %/day leave |

Forage: meadow (≤ 120 m, ≤ 18°) 40, upland (≤ 400 m, ≤ 30°) 16, crag 2 deer-days a cell, regrowing
5 % of the gap a day; the crag is a refuge (0.8 of a herd there safe from packs). On the test island (3.6 km across, ~250 land cells) the deer's capacity is
~406 and the wolves' ~20; with the floors gone and predation easing at low density, 1000-day runs
of four seeds all keep both species, settling near 340 deer and 17 wolves, never over capacity.

## 4. Tests

`cargo test -p dwell-ecology` (`tests/ecology.rs`): seeded 1000-day runs keep both species in at
least 3 of 4 seeds and never pass a capacity, and grow well past the start; a herd over its cap
splits, and at the island's most groups stops at the cap; small herds merge; a herd with no forage
loses nothing in a day, under 1 % in a week, keeps over 70 % after a month, is alive at 100 days and
**dies out** in the end; an island whose grazing collapses loses its deer, then its wolves; wolves
with no deer die out; the **Holling type III** curve (0 at none, ½ at `h`, → 1, quadrupling as the
prey doubles at low density, each head's risk falling as prey gets scarce) and, in the step, a pack
takes a smaller share of a scarce herd than of a big one; a herd on a refuge loses under half what
it does in the open; forage regrows exactly and is depleted where a herd eats; 100 days at once
match 100 single days within 15 %; the same seed gives the same digest and another another; deer
flee a pack, a hungry pack closes on a herd, a starving pack disperses; a hare and a fox from the
fixture (by data alone) live and breed under their capacity; both faces' world maps of a seed run
1000 days with every group on land; an ecosystem round-trips through its bytes, and saved species
map by name (unknown ones dropped). `cargo test -p dwell-sim --test ecology`: §6's. `node --test
shared/ecology/gen.test.mjs`: the data validates, the output is current, names become indices, a
fixture species by data alone, and bad entries fail with messages naming them.

## 5. What's next

See §7 (the server's tick and save, the first item here, is built: §6).

## 6. On the server (slice 2)

`dwell-sim` `server/ecology.rs`. **One `Ecosystem` per face** on that face's island
(`Island::from_world_map` of the face's world map), each seeded with its face's seed
(`bifacial::face_seed`). The islands are made once both faces' maps are in the process (the terrain
computes them; never on the tick); a world without a map (the flat world, test generators) has no
ecology, and tests hand the server islands (`ServerConfig::ecology_islands`).

- **The clock**: the world's days (ticks over its clock's day length, DAY_NIGHT.md) × the world's
  ecology `speed` (1; a per-world tuning hook, saved with it; tests and dev raise it).
- **The schedule** (`next_step`), a pure function of how far behind the clock the ecology is and
  whether anyone is online, so a save → load → continue runs the very steps an uninterrupted run
  does: online and under 10 days behind, one day at a time; nobody online, nothing until 10 days
  behind, then big steps (`stepDays`, 5); 10 or more behind (a load of an older world, an idle
  stretch), big steps. Whole days only, at most **4 steps a tick** (`MAX_STEPS_PER_TICK`): a
  1000-day catch-up takes ~50 ticks, each well under a millisecond's share of the 16 ms tick in a
  release build (the test bounds it at 8 ms in a debug one).
- **Saved** in the world's optional `ecology` table (ARCHITECTURE.md §6.4): the world's `enabled`
  switch and `speed`, then each face's `Ecosystem::to_bytes` (seed, day, the forage grid, species by
  name, the groups). Older builds ignore the table; a world without it (an older save) starts a
  fresh ecology from its seed and catches up its days; an unreadable record (a newer build's) is
  kept, not overwritten, and not stepped. Saved species are mapped by name (one the build doesn't
  know is dropped); a forage grid of another size (the data's `cellM` changed) starts full.
- **Opting out**: `ServerConfig::ecology` (on by default) is a new world's switch; the world keeps
  its own after (an opted-out world opened by a build whose default is on stays off). **Default on**
  for new worlds and for older ones opened by this build, because it is additive: nothing in the
  terrain, the blocks or the other saved tables changes, the table is optional, and the ecology does
  not touch the world yet (no new compatibility line).
- **The dev read-out**: `Server::ecology_snapshot()` (enabled, running, the ecology's and the
  clock's day, and every herd and pack: face, id, species name, face-local x/z, size, hunger,
  energy, activity name), with `ecosystem(face)` and `ecology_stats()` (the last tick's steps and
  ms). Server-side only for now (tests): the client's existing dev channel, `__dwell.serverStats()`,
  is the local worker's once-a-second figures, and the local world's ecology doesn't run yet (below),
  so wiring `__dwell.ecology()` waits for that.
- **Not yet**: the browser's local server never has the world maps in its own instance (the
  server-jobs worker computes them), so a local world's ecology waits forever; a dedicated server's
  runs. Worlds whose installed worldgen config differs from the default use the default's map.

Tests (`cargo test -p dwell-sim --test ecology`): the schedule; each face's ecology steps by the
world's days (online a day at a time, idle waiting then catching up), its own seed, and the
read-out lists its herds and packs (both faces); a save → load → continue matches an uninterrupted
run on both faces, and loads exactly as saved; an older world (meta 1000 days on, no table) starts
fresh and catches up within the per-tick step bound and time, never over a capacity; an opted-out
world has none and keeps its choice; the flat world has none to run; an unreadable record is kept.

## 7. What's next

1. **Individuals near players**: a herd within a player's range becomes its deer (ADR 0073's
   creatures, its size and averages handed to them), and the deer near no player fold back into a
   herd record; the near simulation follows the mockup's rules (ECOSYSTEM_MOCKUP_NOTES.md).
2. **Simple far rendering**: a herd drawn as a few low-detail figures or a marker in the LOD; with it
   a snapshot message or `__dwell.ecology()` for the client (the read-out above).
3. **Local worlds**: the maps (or the islands, a few KB) handed to the local server, so its ecology
   runs; then native ↔ WASM determinism of the step (it uses `exp`, `atan`, `hypot`).
4. **Grazing applied to the real tall grass** near players: a cell's forage share decides how much
   of its tall grass stands as chunks load (eaten grass as a composed layer, like trees' tombstones).
5. **Forage and refuges from the climate**: the vegetation's envelopes (`vegetation.json`'s
   temperature, moisture and soil), woods as refuges, and the biome map in place of the height and
   slope bands.
6. Per-world species tuning (a world's own copy of the data), a setting in the world's menu for the
   switch, and prey switching for hunters with several prey species.
