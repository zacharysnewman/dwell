# Proposal: systemic terrain — rock, climate and life, with biomes as results

**Status:** recorded 2026-11-10 from the owner's direction; extends ADR 0074 (biomes and Chisel) and
[`TERRAIN_HYDROLOGY_AND_VOLUMES.md`](./TERRAIN_HYDROLOGY_AND_VOLUMES.md). **Adopted by
[ADR 0075](../adr/0075-systemic-terrain.md)** (2026-11-10), Phase 57 T1–T5; **T1, the rain shadow, built
in 0.52.7** (§7).

## 1. The owner's direction (2026-11-10)

> "I think perhaps biomes are more about meta composition then, I like building it systemically. So
> these cliffs are shaped this way because the 'rock type' (not a specific block) is hard and brittle
> so leaves sheer cliffs like in Monument Valley. Stuff like that. Then you can see what it looks like
> at different elevations/climates?"
>
> "I like the rain shadow idea."
>
> "We'll need like tectonic plates too, so we can make volcanos later, (like mount rainier), and
> volcanic islands / island chains."

Earlier the same day: elevation is world-level and separate from biomes; biomes vary by height (a
desert turns to tundra up high); the world must feel **cohesive, unlike Minecraft** (biomes by
geography, wide transitions).

Real-world grounding: biomes follow climate, and elevation matters through temperature (≈ 6.5 °C per
1,000 m); the tree line is where the growing season averages ≈ 6–7 °C (≈ 3,500–4,500 m in the
tropics, ≈ 2,000 m in the Alps, sea level in the Arctic); deserts follow missing rain (subtropical
highs and **rain shadows** behind mountain ranges), not low elevation (Death Valley −86 m, the Atacama
2,000 m+, the Tibetan plateau ≈ 4,500 m).

## 2. The layers (each feeds the next)

| Layer | What it is (data) | What it decides |
|---|---|---|
| 0. Tectonics | plates as data (a few, each with a motion vector) and **hotspots** (fixed points under a moving plate); boundaries classified from the plates' relative motion: convergent (subduction or collision), divergent (rift), transform | where uplift and ranges go (collision → mountain ranges; subduction → a coastal range plus a **volcanic arc** of stratovolcanoes like Mount Rainier); rifts → long valleys and lakes; hotspots under a moving plate → **volcanic island chains** (Hawaii-style, older and more eroded down the chain); the rock provinces (volcanic arcs → andesite/basalt; old cores → granite) |
| 1. World shape **[built]** | `terrain.json`: inland rise, uplift, hills, rivers, lakes, coast — later driven by layer 0's uplift instead of hand-tuned noise | the island's height field and water |
| 2. Geology | rock provinces across the island, each a stack of strata; **rock types** as data: hardness, jointing (vertical → sheer walls; bedding → terraces), solubility, the blocks/colours it shows | landforms by erosion of hard over soft: mesas and buttes (hard caprock), domes (granite), columns (basalt), arches later (sandstone, 3D) |
| 3. Climate | a temperature gradient across the island minus the lapse rate; a **prevailing wind** carrying moisture — the windward side of mountains wet, the lee side dry (**rain shadow**); rivers add wetness | temperature and moisture per column |
| 4. Weathering & erosion style | from rock × climate | wet → rounded river valleys, deep soil; dry → wind-cut cliffs, thin soil, bare rock; cold → U-shaped glacial valleys, scree |
| 5. Soil & life | soil depth/type from rock + weathering; vegetation from climate + soil | surface blocks, plants, trees; the tree line and snow line emerge from temperature |
| 6. "Biome" | a **label** for a recognisable combination ("high desert", "alpine meadow"), and in Chisel a saved preset of rock + climate settings | naming and presets, not the generator's primary input |

## 3. Chisel

You tune causes — rock types, the wind, wetness, temperature — and the preview shows what emerges;
scrub elevation or climate to see the same rock across a rain shadow or up a mountain. Biomes become
saved presets.

## 4. How it fits what is built

- B1–B3 carry over: the data format and generator, the biome map's geography inputs (temperature,
  wetness, coast), Chisel.
- B4 (banks, water hue, archipelago) fits as properties fed by rock and climate.
- Tectonics sits under the world shape: today's uplift and hills become the default when no plates
  are set (golden-equal); plates and hotspots then place ranges, volcanoes and island chains. A
  volcano is a landform (a cone with a crater, lava rock, later lava flows/activity) placed by the
  tectonics, shaped by its rock and eroded by the climate like everything else.
- Re-plan B5–B8 around the layers: (a) the rain shadow in the climate field (wind direction and
  strength as data; moisture carried and dropped over relief) — the owner liked this first; (b) rock
  provinces and rock types; (c) soil and vegetation from climate (tree line, snow line); (d)
  erosion-shaped landforms (mesas/buttes from hard caprock), then 3D features (arches) from
  `TERRAIN_HYDROLOGY_AND_VOLUMES.md`.
- Compatibility: changing the default world's terrain is a new line; a phase can stay golden-equal by
  leaving its new inputs neutral in the default data.

## 5. Questions (to settle with the owner before B5)

1. Order after the rain shadow: rock types first, or soil/vegetation from climate first?
2. How many rock types to start (e.g. granite, sandstone, limestone, basalt, shale), and are they
   prototype content (§6.1) like the blocks?
3. Should the default world change to use these (a new compatibility line), or stay as is until the
   systems are tuned in Chisel?
4. Tectonic scale: the world is one ~50 km island disc — plates as real-scale (one boundary crossing
   the island, hotspots offshore making chains) or stylised (several small plates within the
   island)? And should the world stay one island, or can tectonics make several islands / an
   archipelago world?
5. Volcanoes: dormant landforms first (cone, crater, lava rock), with eruptions/lava as a later
   system — or active from the start?
6. Erosion: a cheap rule-based shaping per rock (terraces, caprock) first, or a simulated erosion pass
   on the world map (slower to generate, more natural)?

## 6. The owner's answers (2026-11-10)

1. **After the rain shadow: rock types next** (rock provinces and types shaping landforms), then soil
   and vegetation.
2. **The default world stays as today until the systems are tuned** in Chisel: each new layer ships
   neutral in the default data (no terrain change, goldens equal), and they are adopted together on
   one new compatibility line.
3. **Erosion: rule-based shaping first** (per-rock rules: caprock terraces, cliff faces, scree), "but
   we will need to compare with erosion later likely" — a simulated erosion pass to compare against
   later.
4. **Tectonics at realistic scale:** one plate boundary crossing the island, hotspots offshore making
   island chains (not several small plates inside the island).

Still open: volcanoes dormant first or active (§5 q5).

## 7. T1, the rain shadow (built, 0.52.7)

The climate layer's wind, as data in `terrain.json` `biomeMap` (`windDirection`, `windReach`,
`baseRain`, `upliftRain`, `leeDry`, `rainShare`; validated by `biomes.mjs`, emitted into `BIOME_MAP`).
`biomemap::rainfall` marches the wind over each face's world map once per biome field: the sea refills
the air, rising ground (along the wind) rains it out, falling ground rains less, and what rained is gone
from the air, so the lee of high ground is dry. A biome's wetness is `(1 − rainShare)` × the river/lake
wetness + `rainShare` × the rainfall; `rainShare` is 0 in the default world (neutral, goldens
byte-identical). Chisel: sliders for the keys (the biome map group), a **rainfall** overlay (index 4),
recomputed with the biome field per edit (~70–90 ms per 256² tile at 16 km). Not done: a wind chosen
from the seed; rain feeding rivers' flow (the hydrology still uses its own runoff).

First look (seed 7, wind from +x, `windReach` 8 km, `rainShare` 1): every hill shows a green windward
face and a tan lee, and a broad wet region where the wind meets the high ground first; with a short
reach (1.5 km) the interior is uniformly dry and only the windward coast is wet. The pattern is
dominated by per-slope aspect at the map's 100 m cells (the intensity saturates near a 7 % grade with
`upliftRain` 1.5); a regional shadow behind a range is weaker than the slope striping. Tuning in Chisel
(lower `upliftRain`, longer reach), or smoothing the ground the wind sees, is the next step.

Follow-up (0.52.8): the wind sees the ground smoothed over `windSmoothM` (3 km, a tent low-pass of the
map's heights), so the rain follows ranges: the windward flank of a range is wet and its lee dry over
kilometres, hillsides included (`tests/rain_shadow.rs` `a_whole_range_shadows_a_wide_band_of_its_lee`).

## 7b. T2, rock types and shaping (built, 0.52.9)

Rock types, stacks, provinces and a rule-based shaping by rock, as data (`shared/worldgen/rocks.json`;
ADR 0075 T2): hard-over-soft beds step into caprock terraces with walls and talus, vertical jointing into
sheer walls, massive rock unstepped; `strength` 0 by default. The rule-based shaping the owner asked for
first (§6 q3) is T2's; T4 keeps the isolated buttes and the simulated comparison. First look: stepped
hills following the contours rather than isolated mesas.

## 7d. T4, rule-based erosion (built, 0.52.17)

Isolated mesas, buttes and spires on a flat floor, from the world map's plan view (ADR 0075 T4): scarps
retreat from the drainage lines (flow accumulation over a threshold) by the cap rock's softness, the
valley's size and an outline noise, less for taller hills; nearer than the retreat the land is the floor,
farther it stands, with a sheer wall and a talus skirt at the edge, and the badlands' rills. A biome opts
in (`erosion`, `gullies`; 0 by default). First look: reads as Monument Valley from above. The simulated
erosion pass (§6 q3's comparison) is still to do.

## 7c. T3, soil and vegetation from climate (built, 0.52.14)

Soil depth and type per rock, thinning with slope and deepening on wet valley floors, and vegetation
kinds as climate envelopes on placements (`shared/worldgen/vegetation.json`; ADR 0075 T3): the tree line
emerges from temperature (°C with the ≈ 6.5 °C/km lapse), a dry lee grows no trees; a biome opts in by
its `vegetation` share (0 in the default biome: goldens byte-identical; the boreal, rainforest, savanna,
desert and alpine presets at 1). The snow line from temperature, the soil's blocks and the grass and
ground items as kinds follow (T3b).

## 8. Presets (built, 0.52.8)

Owner (2026-11-10): "We need a lot of prefabbed biomes as options to use as starting points." Fourteen
preset biomes in `shared/worldgen/biomes/presets/` (meadow, mixed forest, boreal, cold rainforest,
alpine tundra, snowfield, steppe, savanna, sand desert, red-rock mesa, badlands, tropical beach,
wetland, coastal cliffs), each with a `label` and `description`, validated like any biome, listed by
Chisel's "Add from preset", never in the default world. They use today's blocks; T2's rock types and
T3's soil will give the mesa, badlands and tundra their own materials.

First look (Chisel, seed 7 and 0, 16 km, the default, desert, tundra, forest, steppe, beach, wetland and
boreal): the desert takes the warm coast's lowlands, the tundra the high cold ground, the beach a thin
coastal strip, as their `where` says. With `rainShare` 0 the wetness is the rivers' alone (the land's
mean 0.1), so the wet presets (forest, rainforest, wetland) only follow river corridors and the dry ones
cover most land; raising `rainShare` lets the rain shadow spread them. In the blocks view the grass
presets (meadow, steppe, forest) look alike: their differences are relief, banks and rules, not blocks.
The smoothed rainfall overlay reads as broad wet and dry regions, no per-slope striping.

