# Proposal: systemic terrain — rock, climate and life, with biomes as results

**Status:** recorded 2026-11-10 from the owner's direction; extends ADR 0074 (biomes and Chisel) and
[`TERRAIN_HYDROLOGY_AND_VOLUMES.md`](./TERRAIN_HYDROLOGY_AND_VOLUMES.md). Not architecture until an ADR
adopts it.

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
