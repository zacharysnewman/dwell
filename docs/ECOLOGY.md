# The far ecology (deer and wolves)

WORK_QUEUE item 11, first slice (2026-11-11). Architecture: [`ARCHITECTURE.md`](./ARCHITECTURE.md)
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

What is extendable here: **more species and more grounds by data** (a species names its diet, what
it flees and what it hunts by name; a test adds a hare and a fox by data alone); the **rules**
(graze, hunt, flee, disperse, split, merge, floor, capacity) are the engine's small fixed vocabulary.
Per build for now; per world (a world's own tuning, saved with it) when the ecology is saved.

## 2. The model

- **The island** (`Island`): an `n` × `n` grid of `island.cellM` (200 m) cells over the face's
  world map. A cell is water (sea or lake: no forage, not walked) or land, whose **most forage** is
  the first `grazing.grounds` entry its height and slope are under (meadow, upland, crag). Forage is
  in deer-days: one unit feeds one head for a day.
- **Forage** regrows toward the cell's most at `regrowPerDay` of the gap a day (exactly:
  `g = max − (max − g)·e^(−r·dt)`) and is eaten by herds.
- **A group** (`Group`): species, face-local x/z, size (fractional head, an average), hunger (0 fed,
  1 none), energy (1 rested), activity (resting, grazing, moving, fleeing, hunting, roaming,
  dispersing). Each step, in id order:
  - **A grazer** scores every walkable cell in its sense: feed (up to ten days' worth), danger
    (closeness of anything it `flee`s within `radiusM`, doubled), distance, a little stickiness to
    its own cell; walks toward the best (water stops it) and eats where it ends, then, over a long
    step, from the other cells it could reach, best first.
  - **A hunter** goes for the nearest prey group above its floor in its sense, kills what it needs
    within `reachM` (at most `killsPerHeadPerDay` a head, never taking a herd below its floor);
    senses none: rests if fed, else roams on a heading hashed from (seed, id, 10-day bucket). Above
    `disperse.aboveHunger` it **disperses**: heads for the island's biggest prey group at
    `speedFactor`, members leaving at `leavePerDay`, instead of starving.
  - **Hunger** relaxes toward `1 − fed share` with time constant `hungerDays`; **energy** toward its
    activity's (rest 1, travel 0.6, flight 0.3) over 2 days, and scales its speed (½ to 1×).
  - **Size** multiplies by `exp(rate·dt)`: breeding `breedPerDay` × how well fed (hunger under
    `breedBelowHunger`, falling to 0 at it) × room on the island; minus starving (`starvePerDay`,
    scaled by hunger above `starveAboveHunger`); minus leaving while dispersing; never below the
    `floor`.
  - **Room on the island** (the owner's no-runaway goal): `1 − total / capacity`, the capacity
    `Island::carrying_capacity`: a grazer's is the forage regrown a day were it grazed bare over its
    feed a head; a hunter's the smaller of its prey's capacity and its prey's head now, over
    `preyPerHead`. The growth is a Ricker step (`N·e^(b·dt·(1 − N/K))`, `b·dt` ≤ 1), which never
    overshoots `K`: a species' total stays under its capacity (floors aside).
  - After all groups: **merge** (a group under `mergeBelow` joins one of its kind within
    `mergeRadiusM` if together they fit the cap), then **split** (above `cap` a group splits in two
    halves, each ≥ the floor as `cap ≥ 2 × floor`; at `maxGroups` it stops at the cap instead).
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
| floor / cap / max groups | 2 / 12 / 40 | 2 / 7 / 8 |
| start | 8 herds of 6 | 2 packs of 4 |
| breeding (well fed) | 3 % a day (doubling ~23 days) | 0.8 % a day |
| starving | at most 1 % a day (half a herd lost in ~2 months unfed; the floor kept) | at most 0.5 % a day |
| speed, sense | 3 km a day, 800 m | 8 km a day, 3 km |
| diet | 1 forage a head a day | kills ≤ 0.12, needs 0.08 deer a head a day; 20 deer a wolf |
| flee / disperse | wolves within 500 m | above hunger 0.6: 2× speed, 1 %/day leave |

Forage: meadow (≤ 120 m, ≤ 18°) 40, upland (≤ 400 m, ≤ 30°) 16, crag 2 deer-days a cell, regrowing
5 % of the gap a day. On the test island (3.6 km across, ~250 land cells) the deer's capacity is
~406 and the wolves' ~20; a 1000-day run settles near 200 deer in ~20 herds and ~10 wolves in 2
packs, with neither ever at zero nor over its capacity.

## 4. Tests

`cargo test -p dwell-ecology` (`tests/ecology.rs`): four seeded 1000-day runs never lose a species
nor pass its capacity and grow well past the start; a herd over its cap splits, and at the island's
most groups stops at the cap; small herds merge; a herd with no forage loses nothing in a day, under
1 % in a week, keeps over 70 % after a month and ends at its floor, never below; forage regrows
exactly and is depleted where a herd eats; 100 days at once match 100 single days within 15 %;
the same seed gives the same digest and another another; deer flee a pack, a hungry pack closes on
a herd, predation never takes a herd below its floor, a starving pack disperses; a hare and a fox
from the fixture (by data alone) live and breed under their capacity; both faces' world maps of a
seed run 1000 days with every group on land. `node --test shared/ecology/gen.test.mjs`: the data
validates, the output is current, names become indices, a fixture species by data alone, and bad
entries fail with messages naming them.

## 5. What's next

1. **The server's tick**: an `Ecosystem` per face in `dwell-sim`, stepped by the world clock (a
   day's worth at a time, far cheaper than a tick), saved with the world (an optional world table,
   so no new compatibility line) and its tuning per world.
2. **Individuals near players**: a herd within a player's range becomes its deer (ADR 0073's
   creatures, its size and averages handed to them), and the deer near no player fold back into a
   herd record; the near simulation follows the mockup's rules (ECOSYSTEM_MOCKUP_NOTES.md).
3. **Simple far rendering**: a herd drawn as a few low-detail figures or a marker in the LOD.
4. **Grazing applied to the real tall grass** near players: a cell's forage share decides how much
   of its tall grass stands as chunks load (eaten grass as a composed layer, like trees' tombstones).
5. **Forage from the climate**: the vegetation's envelopes (`vegetation.json`'s temperature,
   moisture and soil) and the biome map in place of the height and slope bands.
6. Native ↔ WASM determinism of the step, if the browser runs it (local worlds).
