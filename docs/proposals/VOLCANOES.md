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
- **Spots:** `starter:volcano` and `starter:volcano-night` (`client/bench/spots.json`) look down on the
  summit lake.
- **Outstanding:** heat per cell (brighter where hotter/thicker: the field's heat isn't in the cells);
  steam to the clients (a `Steam` message gated by a hello like `CausesHello`) with puffs and a hiss; the
  burning screen edge and sizzle.

**Next:** the rest of the look (steam, burning feedback, heat per cell); the water grid's columns as the
fields' water; then V2's vents on the hotspot cones (rivers to the sea, eruptions).
