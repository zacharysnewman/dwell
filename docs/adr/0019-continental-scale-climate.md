# 0019. Climate at continental scale: biomes in regions, snow by temperature and height

- Status: Accepted
- Date: 2026-10-06
- Resolves: a Phase 11a playtest finding (snow scattered everywhere); the first part of Phase 11c's
  climate
- Amended by: [0022](0022-climate-biome-table-vegetation.md) (decision 3: the mountains biome is replaced by altitude bands; the snow line's numbers)
- Builds on: [0018](0018-drainage-consistent-terrain.md), [0017](0017-continents-from-voronoi-plates.md)

## Context

Temperature and humidity were fractal noise of 0.7–1.1 km wavelength, and the mountains biome came
from a 0.7 km erosion noise. Biomes therefore alternated every few hundred metres: measured over
60 km around the origin, snowy ground was 9 % of the land in runs 216 m long, mountains 39 %. Snow
"scattered everywhere" was the reported symptom. Phase 11c replaces the climate properly (per-continent
bias, coast distance, rain shadow, a biome table); this change fixes the scale and the snow line now,
within the existing biome set.

## Decision

1. **Temperature** is 95 % a 1,200 km noise (two octaves) and 5 % a 1.1 km pair, so climate zones are
   hundreds of kilometres across and their borders only slightly ragged; **humidity** likewise, at
   600 km (three octaves) and 0.9 km.
2. **A lapse rate.** The temperature field is in units of about 20 °C; the ground's temperature is the
   sea-level one minus 0.000325 per metre of height (6.5 °C per km). Snow lies where the ground's
   temperature is low: on high ground in temperate regions, and everywhere in cold ones.
3. **The mountains biome is relief:** the ground stands more than 200 m above its valley floor
   (`Column::valley`, ADR 0018), instead of a noise threshold. The terrain's own mountain weight
   (relief, spawn avoidance, overhangs) is unchanged.
4. **Generator version 8.** Goldens regenerated; the level of detail evaluates the same fields (the
   continental octaves always resolved; the local ones dropped by cell size).

## Consequences

- **A breaking change to the terrain a seed generates**, so a new compatibility line: the owner raised
  the app version to 0.6.0 (the generator moved from version 7 to 8 after 0.5.0 was cut).
- Whole-world shares over 8 seeds: plains ~28 %, forest ~28 %, desert ~11 %, snowy ~12–17 %,
  mountains ~19 % of the land; near the origin the climate is one region (seed 0: temperate), so a
  new world's spawn area is uniform for tens of kilometres. 11c's per-continent biases and biome
  table will vary it.
- A test pins the scale (neighbouring samples 2 km apart differ in climate biome in under 8 % of
  pairs; before, over 60 %), that snow lies only on cold ground, the lapse rate, and the biome shares.
