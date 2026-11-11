# Proposal: active volcanoes, lava and eruptions

**Status:** proposal (owner's direction, 2026-11-11). Not architecture until an ADR adopts it. Builds on
the tectonics layer (ADR 0075 T5: dormant stratovolcanoes on a volcanic arc, hotspot islands, the
Rainier shaping, `coneHardness`), the water surface grid (ADR 0060) and explosives (docs/EXPLOSIVES.md).

## 1. The owner's direction

- "We'll add lava for active volcanoes as well. Some volcanoes are 'always on active' spewing lava (like
  in Hawaii) and some are like Mount St. Helens, active but they're just waiting to go off. Once we model
  eruptions, we could have volcanoes that did go off in the past, and are partially destroyed."
- Earlier: dormant landforms first, eruptions later (SYSTEMIC_TERRAIN §6).

## 2. Volcano states, as data per cone

| State | Real example | In game |
|---|---|---|
| Dormant | Rainier today | The cone as built (0.52.25–0.52.45): no lava. |
| Effusive, always on | Kīlauea, Hawaii | A vent (summit crater or a flank rift) feeding, as data per vent, a crater lava lake with occasional flows, or lava rivers that reach the sea and build new land (both kinds, owner Q2); flows crust over and cool to new rock. Steady, local danger. |
| Explosive, waiting | Mount St. Helens before 1980 | Quiet on the surface; signs as it builds (steam vents, tremors, a bulging flank); an eruption when a trigger fires: time-based (a timer, or random within a window, as data) or caused by players (dynamite in the vent), both (owner Q3). |
| Blown | Mount St. Helens after 1980 | Generated already erupted: a summit gone, a horseshoe crater opening to one side (a lateral blast and flank collapse), a debris field and ash-grey slopes, a lava dome growing in the crater. |

Hotspot islands map to effusive; arc cones to dormant, waiting or blown.

## 3. Pieces it needs

- **Lava as a fluid:** on the water surface grid as a second fluid (thick: slow, high viscosity), or its
  own simple flow model, simulated like water: near players, fast-forwarded elsewhere (owner Q5); hot (light, damage, sets fire to wood), cools from its edges into rock
  (basalt, obsidian at the quench edge with water), steam where it meets water.
- **Vents and sources:** a volcano's vent cells as world data, fed at a rate (effusive) or released at
  once (explosive).
- **Eruptions:** an explosive eruption as an event: ejected rock (projectiles landing as blocks), an ash
  column and ash clouds (particles, sky darkening) with ash fall settling on the ground, lahars (a new
  fluid, below) down the valleys, and the cone reshaped (the blown state's shape applied live). Builds
  near the cone are mostly flattened (cleared or replaced) or flooded, not broken into physics bodies
  (owner Q4).
- **Lahar as a fluid:** a mud flow, fast and heavy, running down the valleys from the cone and
  depositing mud and ash where it stops (a second new fluid beside lava, owner Q4).
- **Generated history:** the blown state as worldgen data on the cone (crater bearing, collapse depth,
  debris spread, ash cover), so a world can start with a volcano that went off long ago.

## 4. Phases (≤ 45 min each, sketch)

- V1: lava as a fluid on a test patch (flow, cooling to rock, light, damage).
- V2: effusive vents on hotspot cones (data per vent: a crater lake with occasional flows, or rivers to
  the sea building land), in the starter world's islands.
- V3: the blown state at worldgen (crater, collapse, debris, ash) as cone data.
- V4: explosive-waiting signs (steam vents, tremor) and an eruption event, triggered by time (a timer or
  random within a window, data) or by players (dynamite in the vent).
- V5: the eruption reshaping the cone live; ejected rock; ash clouds and ash fall; lahar as a fluid;
  builds near the cone flattened or flooded.

## 5. Questions for the owner

1. **Lava's danger:** instant death on contact, heavy damage over time, or damage you can survive briefly?
2. **Effusive scale:** a lava lake in the crater with occasional flows, or continuous rivers of lava
   reaching the sea (building new land over time)?
3. **Explosive trigger:** on a timer (days), random within a window, or caused by players (dynamite,
   digging into the vent)?
4. **Eruption reach:** how destructive to players' builds near the cone (a blast that flattens
   everything within a radius, or mostly cosmetic ash and a crater change)?
5. **Server cost:** lava simulated everywhere a volcano is active, or only near players (like water
   fast-forwarded elsewhere)?

### Answers (owner, 2026-11-11)

1. **Lava's danger:** open. Default until answered: heavy damage while in it, plus a moment of burning
   after leaving it.
2. **Active scale:** "Both?" — crater lava lakes with occasional flows **and** lava rivers reaching the
   sea, building new land, chosen as data per vent.
3. **Eruption trigger:** "both?" — time-based (a timer, or random within a window, as data) **and**
   player-caused (e.g. dynamite in the vent).
4. **Damage:** "a sudden eruption will spew rock, clouds of ash, cause lahar flows (Lahar would be a new
   liquid), I think we might mostly flatten/flood vs physics destruction": ejected rock (projectiles and
   blocks), ash clouds and ash fall, and lahar as a new fluid (a mud flow: fast, heavy, depositing mud and
   ash when it stops); builds mostly flattened (cleared or replaced) or flooded, not broken into physics
   bodies.
5. **Simulated:** "same as water": near players, fast-forwarded elsewhere.

## 6. As built: V1, the simulation core (2026-11-11)

Lava as a fluid on a test patch, simulation and data only (no shaders, no drawing, no browser; the
look and the in-game check come next, locally). ARCHITECTURE.md §6.8 has the model.

- **Its own surface grid, not a second liquid on the water grid.** The water grid
  (`dwell-sim` `water.rs`, ADR 0060) carries one depth and one law per column; giving it a fluid kind
  per column would mean a heat per column, a second law and its sleeping, held-sea and thread paths
  learning both. Lava has its own small grid (`crates/dwell-lava`, `Field`) with the same near/far
  scheduling instead. Lava and water meet through the field's water flag (the water grid's columns,
  once wired) and the steam events.
- **Flow:** the viscous gravity-current law with a yield depth (a Bingham plug), far slower than the
  water's for the same slope (tested against the water's own Manning speed).
- **Cooling:** from the edges and top into `dwell:basalt` (new land); the quench edge with water is
  `dwell:obsidian` plus a steam event. Both blocks are added (break, drop and sound like stone;
  placeholder looks: stone's tile under a dark tint).
- **Vents as data:** lake (held at its level: the crater fills to its rim and stays molten under its
  crust) or river (steady: it runs downhill and crusts behind its front).
- **Danger (Q1's default):** `damagePerSecond` while in it, `burnDamagePerSecond` for `burnSeconds`
  after, as data.
- **Simulated like water (Q5):** every tick near a player, fast-forwarded elsewhere; a far run matches
  a near one within a few percent (tested).
- **Saves:** a versioned record per field, for an optional new table (additive; older builds ignore it).
- **Data:** `shared/fluids/lava.json` (speed, viscosity, yield, cooling, rocks, light, damage, burn),
  `shared/fluids/lava.mjs`, a fixture with a fluid and vents by data alone.

### 6.1 As built: V1 slice 2, the server (2026-11-11)

- **Fields in the server, per face** (`dwell-sim` `lava.rs`): placed round a vent with their beds and
  water read from the world, ticked every `tickEvery` ticks like the water (near a player every lava
  tick, fast-forwarded elsewhere).
- **In the chunks:** molten rows are `dwell:lava` cells (a new block: no collision, light 13, placeholder
  look), cooled rows `dwell:basalt`, the quench edge `dwell:obsidian`, all as one modification a lava
  tick through the edit sync, so clients get them with no protocol change. Steam takes the water's top
  cells and is queued for the clients (not sent yet).
- **Danger:** a player in lava takes `damagePerSecond` and burns `burnSeconds` after through the
  server's damage (none in creative), cause `Burn` from slice 3 (§6.2).
- **Saves:** an optional `lava` table, written only by a world with lava (older builds ignore it).
- **Vents:** a dev way to place one (`Server::place_lava_vent`, a `VentSpot`); a world's own from slice 3
  (§6.2); the default world and every seed's terrain are unchanged.
- **Fixed on the way:** a `Rock` event's rows were metres, not ½ m cell rows (test red → green).

### 6.2 As built: V1 slice 3, the logic leftovers (2026-11-11)

- **A world's vents as worldgen data** (`terrain.json` `tectonics.lavaVents`, opt-in: no default list,
  so the default world and every seed's terrain are unchanged): each vent names its cone (`cone` "arc"
  or "hotspot", `index` youngest first), its `kind` (lake or river), its hot fluid (by name, one of
  `shared/fluids/lava.json`'s), `rateM3s`, a lake's `depthM`, its field's `radius` (columns) and an
  offset from the summit (`distance` m toward `angle`°). Checked by `shared/worldgen/biomes.mjs`
  `checkLavaVents` (and the fluid by `gen.mjs`); read by `dwell_worldgen::config::lava_vents_from_json`
  into the edit (`Edit::lava_vents`), its sites by `config::lava_vent_sites` (`Tectonics::vent_site`:
  an arc cone's offset summit, a hotspot island's centre).
- **Placed by the server** (`Server::place_world_vents`) when its world is created or first loaded
  with no lava, on each face, at the cone's summit as the terrain generates it (`surface_y_at`); a
  saved world keeps its fields (and a newer build's record is left alone).
- **The starter world** (`shared/worldgen/worlds/starter.json`) lists one: a lake vent on its youngest
  hotspot island's summit (2 m³/s, 1.5 m deep, a 25-column field). A hotspot shield has no crater, so
  the lake spreads over the summit and crusts at its edges. The river vent to the sea is left for V2
  (a field of 25 columns can't reach a coast 2.6 km off).
- **`Burn` on the wire** (`damageCauses` 5): a client says which causes it knows with `CausesHello(n)`
  (`DamageCause::MAX` as built), sent only to a server whose `Inventory` has `inventory_flags::CAUSES`
  (an older server would refuse the message). A cause past a client's `n` (or `Drown` without
  `VitalsHello`) goes to it as `Fall`, so an older client never sees `Burn`; later causes need no new
  hello. Lava's damage is `Burn`.
- **Edits refresh a field:** every change through the edit sync but the lava's own (players' edits,
  blasts, the water's writes) marks its column in any field over it (`Lava::touch`); at the next lava
  tick the field reads that column's bed again (up through blocks set on it, down through cells dug out)
  and its water (any liquid over the bed but the lava). A raised bed pushes the lava above it; a lowered
  one keeps its depth, so the surface drops and the lava flows in (a dug channel takes it).

### 6.3 As built: V1 slice 4, the look (2026-11-11, part)

- **Lava's surface:** the painterly `lava` tile (`materials.json` `lava_crust`: dark crust plates over a
  bright molten ground and glowing cracks), self-lit (`emissive`) so it reads at night, its `emits` 13
  lighting the summit through the flood light. `molten: true` (material data) makes the terrain shader
  drift the crust slowly and breathe the cracks' heat; the CPU tier draws it flat and still
  (WATER_RENDERING.md §17). Opaque, as asked (no transparency).
- **Basalt and obsidian** have their own painterly tiles (ropey pahoehoe folds on dark rock; near-black
  glass with a violet sheen), replacing the tinted stone placeholder. Their `color` (the LOD's) is the
  tiles' base.
- **Spots:** `starter:lava-lake` and `starter:lava-lake-night` (`client/bench/spots.json`) aim at the
  vent site (30000, −8000) from over the summit (its ground ~232 m at seed 0, 189 m at the native
  test's seed). The first look found no lava only because the shots were badly placed (the eye under
  the summit's ground, or far over it): `__dwell.lava()` in the page lists both faces' fields round
  the vent with molten columns, as the native test does (`tests/lava_world.rs`, which prints the site).
  Not a placement bug.
- **Dev tools:** `__dwell.lava(op, args)` (`report`, `rate` [k], `vent` [x, y, z, rate, depth],
  `ventNear` [dx, dz, rate, depth]), the bench's `lava` step and a scenario's `query`; `npm run bench --
  --warm lava-lake` fills the summit lake ×30, shoots it by day and night, then a vent by the sea.
- **Steam to the clients:** `SteamHello` / `Steam` (ARCHITECTURE.md, the wire), gated by
  `inventory_flags::STEAM`, so an older client never gets it (tested both faces); the client draws a
  rising puff over each cell (`steam.rs`, a third on a CPU adapter). Puffs use the explosions' smoke
  colour for now.
- **Round 3:** world-space crust plates with glowing seams (WATER_RENDERING.md §17.1); steam's own look
  as data, a hiss, the burning screen edge and a sizzle (§17.2).
- **Outstanding:** heat per cell (brighter where hotter/thicker: the field's heat isn't in the cells);
  §6.5 has its data and plan.

### 6.5 Lava glow, part 1: heat stops (2026-11-11)

Owner, 2026-11-11: "Lava should have bloom glow." / "Lava looks dark red instead of bright red/orange" /
"It should darken as it solidifies though."

- **Built:** the look by heat as data (`lava.json` `heat` stops: colour, emissive, crust cover, hottest
  first; `heatHysteresis`), validated by `lava.mjs` (descending `above`, the coolest at 0, names
  lower_snake_case, ranges) and generated into `dwell_lava::DATA.heat`; a fixture's fifth stop by data
  alone; `LavaData::heat_stop(heat, current)` picks a column's stop with hysteresis, so a cooling column
  changes stop once per stop and heat wavering at a threshold changes nothing (tested). The terrain shader
  draws every molten cell in the **hottest** stop (bright orange seams at emissive 2.4 over a 30 % crust),
  so the lake and the rivers read bright red-orange, not dark red.
- **Chosen for heat in the cells (not built yet): a `heat` property on `dwell:lava`** whose values are the
  stops' names (the first, `hot`, the default: "as before", ADR 0063, so additive), written by the server's
  lava tick only when `heat_stop` changes a column's stop. Cheaper than a per-chunk side channel: the cells
  already go through the edit sync with no protocol change, a cooling column is rewritten once per stop
  (4 writes over its life, on top of its molten and rock writes), and the mesher already keys looks by
  state. Left: the property in `shared/blocks/dwell.json` (a `gen.mjs` check that its values equal the stop
  names), the server's write in `dwell-sim` `lava.rs` (both faces), and the mesher passing the state's stop to
  the shader (the molten branch's constants become a small stop table indexed per vertex).
- **Not built: the bloom pass** (bright pass on the HDR scene, a few half-res blur levels, added before tone
  mapping in the composite, data in `shared/look`, per tier: off on CPU and low).

### 6.4 As built: V2, rivers to the sea (2026-11-11)

Logic and tests only (no shaders): the owner's "rivers of lava reaching the sea and building new land",
beside V1's lakes. §6.2's note stood: a 25-column field can't reach a coast 2.6 km off.

- **Fields that grow with their flow.** A field is a set of tiles of 8 × 8 columns keyed by tile
  (`dwell_lava::field::TILE`), not a fixed rectangle. A growing field (`Field::growing`; every field the
  server places) asks, as a run goes, for the tile past an edge its lava reaches, read from the world
  by the run's `Ground` (bed and water per column; `None` a wall), up to `maxTiles` (`lava.json`, the cap
  on its area: 2048 tiles, 0.13 km²; a refused tile is a wall), and drops the tiles that are all rock at
  the end of a run (no lava in or beside them, no vent, nothing written as lava left in the world). A
  re-added tile reads its bed again, so in the server it gets its rock back from the world's cells. V1's
  rectangle (`Field::new`) is kept as a fixed field (tests; a V1 record).
- **Cost by molten area.** A step visits the molten tiles' columns and the columns their flow reaches out
  of them, nothing else (`Field::work` counts them); links across tile edges are cached per column.
  A long river's tick near a player is tested under a budget (columns a substep ≤ the molten tiles' and
  their edges'; under 20 ms a tick).
- **Tubes (`tubeSpeedMs`, per fluid).** Moving lava keeps its heat under its crust: a column's cooling
  is divided by `1 + speed / tubeSpeedMs` (its outflow over its depth). Without it a river crusted in
  ~6 m; with the build's 0.002 m/s it runs a kilometre molten and its front, once stopped, cools at the
  full rate (the V1 river test runs 480 s now, not 240, for its front to crust).
- **Ponds (`stepDepthM`).** A substep is sized for lava up to 2 m deep; each face's flow is limited to
  the explicit law's stable share of the two surfaces' difference, so a deeper pond levels instead of
  shrinking every substep (it had cut the step to 2 ms behind a crusted dam).
- **The sea.** Lava now quenches *in* a water column, not beside one: it runs into the sea, turns to
  obsidian on the sea floor (a steam event per quenched row), and once the rock stands above the water's
  surface (`Field::water_top`, read with the water) the column is dry land. Before, lava beside water
  quenched on the shore and never entered it, so no coast could grow.
- **Saves:** record version 2 (tiles, the water's surface per column, whether it grows); version 1
  still loads as a fixed field. An older build keeps a version 2 record unread (V1 slice 2's rule), so
  the change is additive.
- **The server:** fields are placed with the vent at their column (0, 0), covering `radius` to start
  (`lavaVents` `radius` is now the starting reach); a tick reads new tiles from the world round the bed
  beside them (`dwell-sim` `lava.rs` `ground_at`) and writes only the tiles its steps visited.
- **The starter world's river** (`tectonics.lavaVents`, with a new optional `faces` key): on the
  youngest hotspot island's flank on face B, 400 m out at 75° (158 m up), 2 m³/s of `dwell:lava`. Each
  face generates its own island: face A's drains its flanks into an enclosed basin above the sea at
  almost every bearing (a pour walk, filling each hollow until it spills, never reaches the sea), so the
  river is face B's alone (`faces: ["b"]`; the lake stays on both). Its way down (the same pour) meets the
  sea 1.3 km off with no hollow deeper than 2 m (tested).
- **Breakouts (`breakoutDepthM`, per fluid; `dwell:lava` 3 m).** Each column remembers the rock its own
  lava laid (`Field::crust`, saved); molten lava at least that deep beside a dry column of that crust
  standing over its bed melts it down to the bed again, at the pond's heat (volume kept, a
  `LavaEvent::Melt` the server clears to air before writing the lava), so a river dammed by its own
  frozen overflow runs on through the gap. At 1 m it melted the river's own levees (its channel runs
  ~2 m deep) and the flow spread into a sheet; 3 m keeps the levees and breaks the dams.
- **The starter river did not reach the sea (V2).** On the fixture flank (a smooth slope) the
  build's lava runs 1.1 km to the sea in ~3 lava hours (14 world days) and builds land. On the generated
  terrain, without breakouts it reached ~480 m in 6 world days and then dammed itself, ponding 16 m deep
  behind walls its overflow froze into. With breakouts (3 m) it no longer dams (the deepest pond stays
  ~3 m) and reaches ~630 m in 18 world days (~740 m at 4 m³/s), but it slows as it spreads sideways into
  a wide sheet (~300 tiles at 2 m³/s, ~540 at 4) instead of keeping one channel; at 1 m breakouts it
  stalled at ~300 m as a sheet. What would close it: channelling (levees from the edges' cooling that a
  breakout at the front doesn't melt, or a channel's banks held as data), or a runnier `dwell:lava`.
  The ignored test `the_starter_river_runs_its_course` (`dwell-sim` `tests/lava_world.rs`,
  ~3 min, `DWELL_LAVA_TRACE=1` for progress) is the check; too long for CI either way.
- **Outcomes, not a target (owner, 2026-11-11):** "Lava may not reach the sea since it solidifies over
  time as it cools… Sometimes it may, other times it may not reach the sea." Whether a river gets there
  is the physics' (the vent's rate, the slope, the cooling), not a goal tuned for. What changed:
  - **Levees hold (channelling).** A breakout melts only a dam on the pond's way down: the ground under
    the dam's crust lower than under the pond's. A levee beside the channel (overflow frozen on ground
    as high as the channel's) is not melted, so a channel keeps its banks; before, a pond 3 m deep broke
    out sideways as readily as forward. Tested: `a_levee_beside_the_channel_holds` (red on the old rule),
    and the dam test now stands its dam on lower ground.
  - **The outcomes, tested on the fixture flank** (`rivers.rs`, the fixture's `test:runny`): the strong
    vent (1 m³/s) on the 0.3 flank reaches the sea within two lava hours and builds land (as before); a
    weak one (0.2 m³/s) on a 0.03 slope runs ~150 m in 45 lava minutes, crusts and stands still, no land
    (`a_weak_vent_on_a_shallow_slope_crusts_and_stops_short`; at 0.1 it piles at its vent, at 0.3 it
    still runs at the hour).
  - **The starter river's outcome:** with the build's lava it reaches ~600 m of its 1.3 km by 18 world
    days and crusts there as a wide flow (~260 tiles), no land in the sea. The ignored long test reports
    this and checks only that the river ran well down the flank; `DWELL_LAVA_TUNE=rate=4,breakout=3,…`
    runs it with other data (viscosity, yield, cooling, edge, thin, tube, breakout, rate, step, far).
  - **Tried and dropped:** lava stiffening as it cools (viscosity × 30 or × 300 at heat 0, geometric in
    heat) only slowed the whole flow (~420 / ~370 m by day 6 against ~500) at the same width per metre of
    reach; it did not channel, so it was not kept.
- **Tests** (`crates/dwell-lava/tests/rivers.rs`, both faces): on a fixture flank (a 0.3 slope 1.1 km to
  a shore, a fixture runny lava `test:runny` and vent `test:flank_river` by data alone) a river runs past
  1 km and reaches the sea within two lava hours, builds dry land past the shore out of the sea, its
  tiles follow it there; a stopped river cools and every tile drops; the cap holds; a save mid-flow runs
  on identically; a V1 record loads. In the server (`dwell-sim` `tests/lava.rs`): a river's field grows
  down a stone ramp and its lava is in the chunks past where it started.
- **Fixed on the way:** none shipped (a stale lake index after a tile's removal was caught by the
  server tests before commit).

**Next:** the rest of the look (heat per cell); the water grid's columns as the fields' water (and the sea's level taken back where
lava builds land); eruptions (V3–V5).
