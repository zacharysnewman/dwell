# The far ecology (deer and wolves)

WORK_QUEUE item 11: the first slice (2026-11-11, the data-level core), the second (the server's
tick and save, §6; no floors, §2) and the third (individuals near players, §8). Architecture: [`ARCHITECTURE.md`](./ARCHITECTURE.md)
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
  the share of a herd on it a hunter can't take; the crag's 0.8). Only the land is stored: its cells
  are numbered row-major (`0..Island::cells()`), each row's land kept as runs (`Island::index`,
  `cell_at` search the row), with each land cell's most forage (f32) and ground class (a byte, its
  refuge the class's); water is not stored, so off the land is not walked and holds nothing.
- **Forage** regrows toward the cell's most at `regrowPerDay` of the gap a day (exactly:
  `g = max − (max − g)·e^(−r·dt)`, in f64, stored as f32 rounded up, so a regrown cell reaches its
  most exactly) and is eaten by herds.
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
  switch and `speed`, then each face's `Ecosystem::to_bytes` (version 2: seed, day, the land cells'
  forage as runs — a count of cells at their most, not written, then a count of cells that aren't
  and their f32 values — species by name, the groups; `dwell_ecology::codec`). A version 1 record
  (the whole grid's forage as f64, before the land-only island) still loads: its land cells' values
  are kept. A build from before reads a version 2 record as unreadable, so it keeps it as it is and
  doesn't step it. Builds older still ignore the table; a world without it (an older save) starts a
  fresh ecology from its seed and catches up its days; an unreadable record (a newer build's) is
  kept, not overwritten, and not stepped. Saved species are mapped by name (one the build doesn't
  know is dropped); a forage grid of another size (the data's `cellM` or the land changed) starts full.
- **Opting out**: `ServerConfig::ecology` (on by default) is a new world's switch; the world keeps
  its own after (an opted-out world opened by a build whose default is on stays off). **Default on**
  for new worlds and for older ones opened by this build, because it is additive: nothing in the
  terrain, the blocks or the other saved tables changes, the table is optional, and the ecology does
  not touch the world yet (no new compatibility line).
- **The dev read-out**: `Server::ecology_snapshot()` (enabled, running, the ecology's and the
  clock's day, and every herd and pack: face, id, species name, face-local x/z, size, hunger,
  energy, activity name), with `ecosystem(face)` and `ecology_stats()` (the last tick's steps and
  ms). The clients' read-out of the far groups is §9 (`Ecology`, `__dwell.ecology()`).
- **Local worlds** (the browser's single-player and friend-hosted worlds) run it too, from the same
  code: the local worker installs both faces' maps in its own WASM instance before the world starts
  (`installWorldMap`, the bytes the page built or kept, ADR 0049), so the islands are made from the
  installed maps (no second copy of a map; a test world's installed map is the one used). It steps
  and saves as on a native server (the world file's `ecology` table). Only when the page could not
  give the maps (the worker's fallback: each worker computes its own) does the local server's
  instance have none, and then its ecology waits.
- **Cost** (measured, seed 1): each face's grid is the whole map's span at 200 m (251 001 cells a
  face), of which ~50 000 are land; only those are stored (9 bytes a cell: forage and most forage
  as f32, the ground class), so the islands and ecosystems hold **0.93 MB** for both faces (12.5 MB
  before, when every cell held f64s; `Server::ecology_heap_bytes`, the tests bound it at 3 MB:
  `game_local` and `dwell-ecology`'s `a_seed_1_worlds_ecology_is_small_in_memory_and_saved`), made
  once on the first tick both maps are ready (~30 ms natively, a one-off). The saved record is ~0.5 KB
  a face fresh or regrown and ~5 KB after a month of grazing (~3 MB before; the tests bound it at
  200 KB), and **a save still writes it only when it changed** since the store last held it
  (`Ecology::record_to_save`, a hash of the record): a local world saves every 5 s, the ecology steps
  once a world day. A failed save writes it next time.

Tests (`cargo test -p dwell-sim --test ecology`): the schedule; each face's ecology steps by the
world's days (online a day at a time, idle waiting then catching up), its own seed, and the
read-out lists its herds and packs (both faces); a save → load → continue matches an uninterrupted
run on both faces, and loads exactly as saved; an older world (meta 1000 days on, no table) starts
fresh and catches up within the per-tick step bound and time, never over a capacity; an opted-out
world has none and keeps its choice; the flat world has none to run; an unreadable record is kept.

## 7. What's next

1. ~~**Individuals near players**~~: built (§8).
2. **Simple far rendering**: a herd drawn as a few low-detail figures or a marker in the LOD, from
   the client's read-out (§9, built).
3. ~~**Local worlds**~~: built (§6). Still to do: native ↔ WASM determinism of the step (it uses
   `exp`, `atan`, `hypot`). ~~A smaller island~~: built (land cells only, f32 forage, a run-length
   record: 0.93 MB and a few KB, §6).
4. **Grazing applied to the real tall grass** as chunks load: a cell's forage share decides how much
   of its tall grass stands (eaten grass as a composed layer, like trees' tombstones). Slice 3 eats
   single blocks by the deer near players only (§8).
5. **Forage and refuges from the climate**: the vegetation's envelopes (`vegetation.json`'s
   temperature, moisture and soil), woods as refuges, and the biome map in place of the height and
   slope bands.
6. Per-world species tuning (a world's own copy of the data), a setting in the world's menu for the
   switch, and prey switching for hunters with several prey species.

## 8. Individuals near players (slice 3)

`dwell-sim` `herds.rs` (pure: a `World` view of the islands, the ecosystems, the players and the
cells; unit-tested on a stone floor), driven by `Server::step_herds` each tick before the creatures
move. Data: `shared/ecology/ecology.json`'s `near` (where) and each species' `near` (how; a species
without one stays a record — the fixture's hare and fox).

- **Coming near**: a record within `near.spawnM` (64 m, face-local horizontal) of a player on its
  face becomes `round(size)` individuals (at least 1) of its `near.creature` — ADR 0073's creatures
  (`dwell:doe`, `dwell:wolf`), `driven`: they walk only to targets the herd logic gives (at its `pace`,
  a sprint or a flight) and the creature rules never despawn them — placed 1–4 m around the record on
  ground found from the player's row, with the record's hunger and energy. The record leaves its
  face's `Ecosystem` (the far step no longer steps it, but still sees it: below). The whole group
  spawns or none: it waits while its cells aren't loaded or the world is at its most creatures (64).
- **The herd's mix** (slice 3b): the individuals are the **creature rule's mix** for the species'
  creature (shared/creatures/creatures.json: the doe rule's `mix`, a stag in 40% of herds, at most
  one), rolled once from (world seed, record id), so a herd is does and sometimes a stag; fawns, or
  any other member, come by adding a mix part there (no code). A species whose creature has no rule
  comes as that creature only. Young born near players are the species' own creature.
- **The ecology's grounds** (slice 3b): an individual is placed only where the far model allows — its
  island cell walkable (land, not sea or lake), a dry full block under open cells, and a rise to each
  next column no steeper than `near.maxSlopeDeg` (40°). A member none of whose 8 tries is such ground
  takes the nearest such column within `near.searchM` (24 m) of its record (one each, nearest
  first); with none there the group waits as a record.
- **Folding back**: once no player on its face is within `near.foldM` (96 m: the hysteresis) of any
  member, the creatures go and the record returns (by id order) with size = the survivors, hunger and
  energy their averages, place their centroid, resting. A group with none left (all killed) is gone.
  A save while near writes each near group as it would fold back now.
- **The rules** (every `near.thinkS`, 0.5 s; the mockup's priority order, its numbers as data):
  hunger rises `hungerPerS`, energy falls `energyPerS` awake. (1) **Fleeing**: a species with
  `near.flee` runs `radiusM` away from the nearest of its far `flee.from` within `radiusM`, at
  `pace` × its walk (a deer 14 m from a wolf, 2×). (2) **Sleeping**: `gainPerS` energy, at least
  `minS`, waking once over `wakeAboveEnergy` or hungrier than `wakeAboveHunger`. (3) Falling asleep:
  under `belowEnergy`, or at night (the face's own: `night_on`, face B's day is face A's night) with
  `nightChancePerS` while under `nightBelowHunger`. (4) **Food**: a grazer over `graze.aboveHunger`
  is **Eating** while its island cell has forage — `eatPerS` hunger and `foragePerS` of the cell's
  forage a second (the far grazing value drops), until under `fullBelow` — else **Looking for
  Food** (wandering); while eating, `blockChancePerS` a second it eats the real `graze.block`
  (`dwell:tall_grass`) by its feet: the only world change, near players only, none when the data
  names no block. A hunter over `hunt.aboveHunger` **Chases** the nearest prey individual within
  `senseM` at `sprint` × its walk (2.2×: a wolf 3.5 m/s, a fleeing deer 2.6) and kills it within
  `reachM`: the prey is gone (its group's `deaths`), the killer fed and asleep, packmates within
  `feedsPackM` lose `feedsPack` of their hunger. (5) **Mating**: under `mate.belowHunger` and over
  `aboveEnergy` with a groupmate like it within `radiusM` (walking to it), within `reachM` a young
  with `chance` a try (the deer 0.5, the wolf 0.02, the mockup's), each parent's hunger + `cost`, a
  failed try `tired` energy; never past the group's `cap`. (6) **Looking for Company**: back toward
  the nearest groupmate farther than `companyM`, else **Wandering** (up to `wanderM`).
- **Death near players** is by kills only: hunger stops at 1 and never kills here (starving is the
  far step's, over days; owner: "slow to starve").
- **Deterministic**: each member's own hash chain from (world seed, record id, member); the near
  state's digest (`Near::digest`) repeats for a seed.
- **The creature rules**: while the ecology runs, the creature rules whose creature is a species'
  `near.creature` are left out (`Creatures::reserved`: the doe rule with its stag, the wolf rule), so
  the deer and wolves near players are the ecology's only. A world without an ecology (opted out, no
  map) keeps the rule-spawned ones.
- **The far step sees near groups** (slice 3b): after each decision the near side writes each near
  group's **summary** (its record as it would fold back, the activity most members share) into its
  face's `Ecosystem::near`; the far step treats those as present but never steps them: far hunters
  target and close on them, far grazers flee near packs, and their head counts in the species'
  totals (the grazing bound, a hunter's prey per head). A far kill lowers the summary's size; the
  near side reads the loss back before its next decision, a death (the last member, its creature
  removed) for each whole head, keeping the fraction. Not saved (a save writes near groups as they
  would fold back).
- **Players** are noticed (deer flee, wolves keep away; §8.2); far rendering is §11.

Tests: `cargo test -p dwell-sim --lib herds` (both faces): a herd near a player becomes its deer
(on the floor, the record out of the far step, its hunger) and folds back keeping its head (+ births
− deaths) at their centroid, staying near inside the hysteresis; a hungry wolf chases at a sprint
and kills a deer, the kill feeding the pack; a deer flees a wolf within its radius; a fed pair has
young up to the cap and never past it; deer graze the cell's forage and sometimes eat the real tall
grass by their feet, and never with no block in the data; at night fed animals fall asleep; the
same seed gives the same run and another seed another; every species names a creature and a block
the build has. `cargo test -p dwell-sim --test ecology` (both faces): through the server, a herd
near a joined player becomes driven deer on the floor with no rule-spawned animals beside them and
folds back when the player leaves; a save while near keeps the herd; an opted-out world makes no
individuals; night is each face's own. `node --test shared/ecology/gen.test.mjs`: the `near` data
validates, emits its creature and block, a species without it stays a record, bad entries fail.

Slice 3b tests (`cargo test -p dwell-sim --lib herds`, both faces): a herd comes near as the rule's
mix (some herds a stag, never two; a fixture mix of two stags by data alone); individuals spawn only
on the ecology's ground — a herd on stairs steeper than the data's stands on the nearest flat
columns, one on the island's sea on its land, and with none within `searchM` it waits; the summary
is in its face's ecosystem while near, a far kill on it becomes deaths (fractions adding up), a far
pack closes on it, and it goes when the group folds back. `cargo test -p dwell-ecology`: the far step
closes on, kills from and flees near summaries, and their head bounds breeding.

### 8.1 Hurt and killed (WORK_QUEUE item 35)

A weapon's hit (ARCHITECTURE.md "Weapons") takes health from a creature (its `health`, `hitRadiusM` and
`drops` are its creature's, shared/creatures: the doe 30, the stag 40, the wolf 36; raw meat), slides
it back (`knockbackM`) and tells its mind where the hit came from (`Creature::hurt_from`). Each
species' `near.hurt` says how one reacts, before anything else, for `seconds` after the hit at `pace`
× its walk: the deer `flee` (runs `runM` from the striker each decision), the wolves `fight` (turn on
the striker and close on it; they can't bite players yet) unless under `fleeBelowHealth` (0.35) of
their health, when they flee. A creature killed is removed and drops its `drops`; its herd counts it a
death at its next decision (`Near::refresh`: a member whose creature is gone), and folds back without
it. Tests: `herds.rs` `a_hurt_deer_flees_and_a_hurt_wolf_fights_unless_badly_hurt`,
`a_member_killed_leaves_its_group_as_a_death` (both faces).

NPCs (the villager) are struck by the same rule but react by their `npc.combat` and `npc.fight`
(CHARACTERS.md §11.11): a `fightBack` NPC holding a sword blocks its foe's wind-up, strikes back in its
recovery and gives up `giveUpS` after the last hit; its blows never hurt players (owner's open question).

### 8.2 Players, roaming packs and idles (WORK_QUEUE item 39)

Owner, 2026-11-12, after the showcase video had no chase or flight: "deer flee from players", "packs
roam toward herds", "a grazing pose and more idle behaviour". All of it is data in each species'
`near` (and a hunter's `roam`), on a small fixed vocabulary:

- **Noticing players** (`near.players`): a player is noticed within `noticeM` walking, `crouchM`
  crouching (sneaking: the smaller), `stillM` standing still (horizontal speed under
  `stillBelowMps`), `sprintM` running (over `sprintAboveMps`); hungrier than `boldAboveHunger`, only
  within `boldM`. The server passes each living player's stance (`herds::Stance`: the controller's
  `crouching`, its horizontal speed) beside its feet. `react` is one of two: **`flee`** — any member
  noticing a player alarms its whole group (a pass before the decisions), which runs `runM` from that
  player each decision at `pace` × its walk, calming `calmS` after the last notice (the deer: 22 m
  walking, 8 m crouching, 12 m still, 34 m running; 4.5× their walk, 5.9 m/s, over a walking
  player's 5); **`avoid`** — it walks off at `pace` to keep the notice distance (the wolves: 24 m,
  14 crouching, 18 still; bold over hunger 0.6, within 8 m only; creatures can't hurt players, so a
  bold pack simply stops keeping away). Priority: after a hurt reaction and fleeing predators, before
  sleep, food and the rest; an alarm wakes a sleeper. No line of sight yet.
- **Packs to herds, far** (a hunter's `roam`): above `towardPreyAboveHunger` (wolves 0.3) a group
  that senses no prey heads for the island's nearest prey group at any distance (records and the
  near summaries alike), `Roaming`, instead of a random heading; starving past `disperse.aboveHunger`
  it still disperses to the biggest. **Near** (`hunt.trackM`, past `senseM`): a hungry hunter with no
  prey in sense walks toward the nearest prey within `trackM` (160 m), a near individual or a herd's
  far record, so a pack near players closes on a herd and the existing chase (`senseM` 28 m) starts.
- **Idles** (`near.idles`): when a decision has nothing to do (wandering), it walks at `walkChance`,
  else picks an idle by `weight` and holds its pose for `seconds` [min, max]. The poses are a fixed
  vocabulary (`dwell_ecology::IdlePose`): `stand`, `graze`, `lookUp`, `earFlick`, `sit`, `lie`;
  eating takes `graze.pose` (default `graze`), sleeping `sleep.pose` (default `lie`), moving `stand`.
  The deer graze, look up, flick an ear or stand; the wolves sit, lie, stand or flick an ear. The pose
  is each `Member::pose` on the server. **Not yet drawn**: the deer, doe and wolf GLBs have only
  `idle`, `walk` and `run` clips (no grazing, sit or lie), and the pose isn't on the wire; the next
  step (local, with a GPU) is the pose as a capability-gated part of `Creatures` (like the NPCs'
  `NpcNet::pose`) and the poses built from the rig as data in `shared/models/models.json` (the neck
  and `scull` down for graze, the hind legs folded for sit, all four for lie).

Tests (both faces unless noted): `herds.rs` `a_herd_flees_a_player_within_its_notice_distance_and_calms`,
`a_crouching_player_gets_closer_before_the_deer_notice`, `wolves_keep_their_distance_from_players_unless_hungry`,
`a_hungry_pack_tracks_a_herd_out_of_sense_and_the_hunt_starts` (red with `trackM` 0),
`idle_picks_follow_the_data_weights` (one face: a pure pick), `deer_graze_head_down_and_wolves_sit_and_lie_by_their_idles`,
`a_fixture_species_flees_players_and_idles_by_data_alone` (the fixture's fox); `dwell-ecology` tests
`a_hungry_pack_roams_toward_the_nearest_herd_and_the_hunt_starts` (red without `roam`) and
`a_fed_pack_and_one_without_roam_dont_head_for_the_herd` (the fixture's fox roams by data alone);
`gen.test.mjs` the new fields validate, emit and fail with a message. The older near scenes run with
players unnoticed (`unnoticing`): they are about the animals among themselves.

## 9. The clients' read-out (slice 3b)

For the far rendering to come (§7), the server sends the far groups to the clients that ask:

- **Gated** like block damage (0.52.28): the server's `Inventory` carries `inventory_flags::ECOLOGY`
  (64); a client that sees it sends `EcologyHello` (C→S, control channel) once; only those clients
  get `Ecology` (S→C, reliable, world channel). An older client never sees the flag's meaning, never
  asks, and is never sent one; an older server never sets the flag. Additive: no new line.
- **`Ecology`**: the ecology's day (f32) and up to `MAX_ECOLOGY_GROUPS` (1024) far groups, each its
  record id (u32), face (u8: 0 A, 1 B), species by name (≤ 64 bytes), face-local x and z and head
  (f32). The groups near players aren't in it (they are creatures). Sent at the hello, then every
  `ECOLOGY_SEND_EVERY` (2 s) while the ecology runs; none from a world without one. ~2 KB for the
  shipped data's ~100 groups.
- **`__dwell.ecology()`**: the last one, `{ day, groups: [{ id, face: "a" | "b", species, x, z,
  size }] }`, or null before the first (docs/DEV_TOOLS.md), in local worlds (the host's own client and
  friends joining) as on native servers. Until this change the client's session dropped `Ecology` (it
  passes only listed message kinds to the game), so it was null on every server and no far herd was
  drawn (§11).

Tests: `cargo test -p dwell-protocol ecology` (round trip, truncation, a bad face or a NaN malformed);
`cargo test -p dwell-sim --test ecology` (both faces): the Inventory's flag, a client that asked gets
the far groups at the hello and every 2 s as the server holds them, face-local on their faces, and a
client that didn't ask never does; a world without an ecology sends none. `cargo test -p dwell-client --lib net::session`
(`hello_goes_only_to_a_server_that_keeps_inventories`): `Ecology` reaches the game. `cargo test -p
dwell-client --test game_local` (the local world's `LocalCore`, the code the WASM worker runs, with the
maps installed from bytes as the worker does): deer and wolves on both faces, stepping by world days,
the measured memory bound, an unchanged record not written again, a reload exactly as saved
(`a_local_world_runs_the_far_ecology_on_both_faces_saves_and_reloads_it`); the host's client and a
guest get the far groups (`a_local_worlds_far_herds_reach_its_own_client_and_a_guest`); a
test-generator world has none (`a_test_generator_local_world_has_no_ecology`).

## 11. Far herds drawn simply (scoops-and-herds)

- **Data:** a species' `far` (shared/ecology/ecology.json): `color` (#rrggbb), `sizeM` (a head's height),
  `fadeStartM`/`fadeEndM` (whole until the start, gone at the end). Deer: tan, 1.3 m, 450–700 m; wolves:
  grey, 0.9 m, 350–550 m. `shared/ecology/gen.mjs` validates it (an unknown field, a bad colour, an end
  not past its start fail naming the field) and emits `FAR_LOOKS` into `dwell-client` `far_herds.gen.rs`
  (checked stale like the others). A species without `far` is not drawn far.
- **Lay-out** (`dwell_client::far_herds::lay_out`, on each new `Ecology`, §9): each group of the eye's
  face within its fade's end draws round(size) heads (1..24), scattered in a disc of 2 + 1.6·√n m round
  its place, hashed by the group id and head (stable between snapshots), each stood on the loaded
  terrain's ground (`bodies::ground_at`, 48 m round the eye's height; a head past the loaded chunks waits
  for the next snapshot).
- **Drawn** (`far_herds::puffs`, each frame): one lit billboard per head through the blasts' billboard
  pass (kind 12, `PuffKind::Herd`): a body's oval over two leg strokes in the species' colour (sRGB in
  the seed), darker underneath, lit facing up as the chips are; faded by distance; **none on a CPU
  adapter**. Groups near players are not in the snapshot (they are creatures), so the two never draw at
  once; the hand-over is the group leaving the snapshot as its creatures spawn (`near.spawnM` 64 m,
  where a head is a few pixels, so the swap is small).
- **Not yet:** heads past the loaded chunks (the LOD's heights are not on the main thread); a fade-in at
  the near hand-over; in-game screenshots and the frame cost from the bench (not measured in this
  change's time box; at ≤ 24 heads × ~100 groups it is at most a few thousand billboard vertices).

Tests: `cargo test -p dwell-client --lib far_herds` (both faces): round(size) heads capped, on the
ground round the group's place, stable, a species without a look none, faded past its end and partly
between, none from the other face and none on the CPU tier; the shipped species have looks. `node --test
shared/ecology/gen.test.mjs`: the look reaches the client table, a fixture species' by data alone, bad
entries fail.

## 10. Tall grass growing and spreading (ADR 0075 T3c)

Owner (2026-11-11): "I want tall grass to steadily grow and spread. It should probably have a max
depending on climate/biome." Data: `shared/worldgen/grass.json` (`gen.mjs` `validateGrass` →
`dwell-worldgen` `grass.gen.rs`). Code: `dwell_worldgen::grass` (the face-local step, pure),
`dwell-sim` `server/grass.rs` (the server's step and save).

- **The cap is the climate's.** A column is **fertile** when the tall grass decoration's own per-cell
  roll (the one that decides generated tall grass, `Placer::roll`) is under `maxDensity` × the
  climate's **share** there: the T3b undergrowth factor of the vegetation kind governing the decoration
  (`dwell:meadow_grass`: thins with the moisture, fades below 1 °C), 0 in a biome that doesn't list the
  decoration, 1 without a climate (`TerrainGenerator::grass_envelope`). Generated tufts roll under
  `density` × patch × share ≤ `maxDensity` × share (`maxDensity` ≥ the decoration's density is
  validated), so every generated tuft is fertile, and ground never holds more tall grass than
  `maxDensity` × share of its open grass cells. Lush meadows fill toward 0.8; dry steppe stays sparse.
- **Near players**: every 5 s of the tick the server's grass step takes up to 64 loaded chunks not yet
  grown to today's whole world day (the world's days × `growth_speed`, as trees) and grows the days
  each missed, at most `catchUpDays` (20) at once: a chunk first loaded on day 1000 grows days
  980–999, not a thousand. Each day an open cell (air, with `openAbove` cells of air over it) over an
  `on` block (`dwell:grass`; with `onRamps`, also over a ½ m ramp of it, as worldgen's decorations put
  tufts, never over a 1 m ramp's pieces) of a fertile column takes `block` with `spreadPerDay` (0.15) when tall grass
  stands within `reach` columns (and a metre up or down), or `regrowPerDay` (0.25) when it held
  generated tall grass that is gone (**grazed** by deer, §8, or cut). Rolls hash (seed, day, face-local
  cell): face B grows face A's mirror.
- **A player's block wins**: a cell grows only where it is air now and was air or tall grass as
  generated, over ground that is its generated block. A placed block, a placed grass block, a roofed
  cell or a dug cell never takes it.
- **Far away**: the ecology's island cells (§2) hold at most their ground's forage × the same share at
  the cell's centre (`Island::scale_grazing`, `grass::far_share`), and regrow toward it. **The mapping**:
  near, a stretch's tall grass at saturation is `maxDensity` × share of its open grass cells; far, a
  cell's most forage is its ground's × share; so a cell's forage over its ground's most equals its tall
  grass over `maxDensity`. Applied to the islands made from world maps while the world grows grass (a
  world without grass growth keeps its forage as before; the default world's single biome has share 1
  everywhere, so its forage is unchanged).
- **The world's switch**: saved in the optional `grass_growth` table (a version byte, the switch, each
  grown chunk's last day; another version's is kept unread and the growth stays off). `ServerConfig::grass`
  (on) is a **new** world's; a world saved before this (meta, no table) has none, so existing worlds'
  meadows don't change under their players; a world keeps its own after. Not a compatibility change:
  worldgen, the goldens and the protocol are unchanged (growth writes ordinary block edits).
- **Not yet**: a setting in the world's menu; grass on a chunk's edge cells seeded only from loaded
  neighbours.

Tests: `cargo test -p dwell-worldgen --test grass` (both faces): a meadow spreads over days toward
`maxDensity` and never onto an infertile column; a dry stretch (share 0.1) stays sparse and share 0
grows nothing; a placed stone, a placed grass block and a roofed cell are never grown over; eaten
tufts regrow; a load catches up at most `catchUpDays`, the same as growing those days one by one; a
fixture rule by data alone (on dirt, reach 2) grows its square and not on grass. `cargo test -p
dwell-ecology` `the_far_forage_regrows_toward_the_same_climate_cap_as_the_near_grass`: the far most
over the ground's is the near cap over `maxDensity`, and a dry cell's forage stays sparse. `cargo
test -p dwell-sim --test grass` (both faces): the server grows a loaded chunk's missed days and a
player's block stands; the switch and the days survive a save; an opted-out world keeps its choice;
the saved form round-trips and another version is unread. `node --test shared/worldgen/gen.test.mjs`:
grass.json validates, bad entries fail naming the field.
