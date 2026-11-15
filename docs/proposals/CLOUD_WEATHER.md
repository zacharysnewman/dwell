# Clouds from a weather model (proposal, first slice built)

WORK_QUEUE item 13. The owner, 2026-11-14: "Queue up for after all this, the clouds look too uniform and
patterned at a macro level so some research and optimize this to be like real clouds, non uniform height, shape,
etc based on actual changing weather models."

What this covers: why the sky read as one weather everywhere, how real cloud fields vary at 10–200 km, how games
and papers model that cheaply, and what was built (the weather field, [`CLOUDS.md`](../CLOUDS.md) §11). The open
questions for the owner are at the end. Research was written from the papers and talks themselves; the session had
no web access, so citations are to the talks, not to pages.

## 1. Why the sky looked uniform

Before this change the volume's weather map (CLOUDS.md §9.2) was two one-octave value-noise fields on the deck's
lattice: a cover field with 8 km cells moving the coverage by at most ±0.17, and a type field with 16 km cells. Every
part of the sky had nearly the same cover (0.25–0.6 at the default coverage 0.42), the same base (the layer's floor,
1350 m, everywhere) and the same top for its type, and the fields repeated exactly every 128 km (the deck's wrap).
From the ground the 2 km base-noise tile and the near-constant cover showed as an even scatter of similar cumulus to
the horizon; from altitude as a carpet. Real skies are never like that over 50 km.

## 2. How real cloud fields vary (10–200 km)

- **Synoptic systems (500–2000 km, hours to days).** Lows and highs. A mid-latitude low carries a **warm front**
  (a broad shield of stratiform cloud — cirrus, then altostratus, then nimbostratus, lowering over 300–1000 km
  ahead of the surface front) and a **cold front** (a narrow band, 20–100 km, of deep convection and
  cumulonimbus), with a **warm sector** between (stratocumulus, low stratus) and **post-frontal** cold air behind
  (open-cell cumulus over sea, scattered cumulus over land). Highs: subsidence, clear or a thin capped deck. So at
  the scale a player sees from altitude (50–200 km), the sky should have *edges*: a band of cloud, a clear region, a
  deck ending.
- **Mesoscale organisation (10–100 km).** Shallow convection organises: **cloud streets** (rolls aligned with
  the wind, 2–10 km apart) when the wind is strong and the boundary layer heated; **open and closed cells**
  (Bénard-like hexagons, 10–50 km) over cool seas; **convective clusters** where cumulus merge into towers. Gaps
  (clear slots) are as characteristic as the clouds.
- **Cloud base varies with humidity.** The base is the lifting condensation level, ~125 m per °C of dew-point
  depression: moist air (sea, after rain) gives bases of 300–800 m, dry continental air 1500–3000 m. A field of
  cumulus has nearly *flat, equal* bases locally (that is why bases look flat) but different bases from region to
  region.
- **Cloud top varies with stability.** Shallow fair-weather cumulus stop at an inversion (1–2 km thick); with a
  steep lapse rate and moisture they grow to congestus (3–6 km) and cumulonimbus (to the tropopause, 10–15 km,
  with an anvil spreading downwind). Stratiform decks are thin (a few hundred metres) under an inversion.
- **Land and sea.** Over the sea the boundary layer is moister and less heated: lower bases, more stratocumulus and
  stratus decks, fewer towers by day. Over land by day, convection; the coast often shows a clear line where the
  sea breeze sinks.
- **Mountains.** Air forced up a **windward** slope cools and condenses: orographic cloud and rain on the windward
  side and over summits (cap clouds, banner clouds), towers where the lift triggers convection. The air coming
  down the **lee** warms and dries: the **rain shadow**, often clear for tens of kilometres (the föhn gap), with
  stationary **lenticular** clouds in the waves above.
- **Time.** Systems move with the steering flow (10–20 m/s at mid-levels) and develop over hours; single cumulus
  live 10–30 minutes; cells and streets over an hour or two. A sky should change noticeably over a play session.

## 3. How games and papers model it

- **Schneider & Vos, "The Real-time Volumetric Cloudscapes of Horizon Zero Dawn" (SIGGRAPH 2015 Advances; GPU Pro
  7).** A 2D **weather map** (coverage, precipitation, cloud type) over the play area, painted and procedurally
  animated, drives the 3D noise; the map is what a weather system writes. Coverage and type per region, type giving
  each column a height gradient. The lesson: the macro variety lives in the 2D map, not in the 3D noise.
- **Schneider, "Nubis: Authoring Real-Time Volumetric Cloudscapes with the Decima Engine" (SIGGRAPH 2017).** The
  weather map at a few metres to a few hundred metres a texel over the world, scrolled and blended between
  authored weather states; per-type height profiles mixed by the map; the anvil bias for cumulonimbus. "Nubis³"
  (SIGGRAPH 2022): authored voxel clouds for the near ones — not needed here.
- **Hillaire, "Physically Based Sky, Atmosphere and Cloud Rendering in Frostbite" (SIGGRAPH 2016 PBS course).** The
  same weather-texture model; the cloud layer's base and top as parameters, the weather texture tiled over the
  world.
- **Bauer, "Creating the Atmospheric World of Red Dead Redemption 2" (SIGGRAPH 2019 Advances).** The weather
  system's state (types, coverage, wind) drives the clouds, fog and their shadows; transitions between weather
  types over time; the clouds advected by the wind.
- **Microsoft Flight Simulator (2020, Asobo).** Real weather data (Meteoblue's model output) on a coarse grid
  — humidity, temperature by pressure level — turned into cloud density, base and top per column: the closest to
  "an actual changing weather model", and the proof that a coarse grid of a few kilometres a cell is enough.
- **Cheap physics in other work.** Orographic precipitation models (Smith & Barstad 2004, the linear upslope
  model: condensation ∝ wind · ∇terrain, decaying downstream) give a one-line rule for windward cloud and the lee
  shadow; the lifting condensation level (Espy's 125 m/°C) for base heights; a convective index (CAPE-like) for
  how tall clouds grow.

The common shape: **a coarse 2D field per region (cover, type, base, top), slowly evolving, advected by the
wind, driving fixed 3D noise**. The 3D noise makes single clouds; the field makes the weather.

## 4. What was built (CLOUDS.md §11)

A **weather field** per world and face, all data (`clouds.json` `weatherField`), computed on the CPU in the job
workers and sampled by the march as a 64 × 64 texture (1 km a texel, a 65 km window round the eye):

- **Humidity** from three octaves of evolving 3D value noise (x, z, time), **not periodic** (f64 world metres): a
  synoptic one 160 km across domain-warped by 45 km (bands and fronts) changing over 3 h, mesoscale cells 42 km
  (45 min), local cells 11 km (15 min); all moving with the wind at 2.5 × its speed (the steering flow, 15 m/s);
  sharpened about the mean (`contrast`), so ~28 % of the world is clear, ~12 % overcast, the rest between, with
  fronts between them. The cover follows it: `coverVariation` 0.55 lets a dry region clear entirely and a moist one
  close over.
- **Terrain** from the world's map (`TerrainGenerator::map_height`, the eroded height, cheap): the upslope rule
  (`upslopeGain` × the ground's rise over 4 km along the wind, plus `elevationGain` per km) raises humidity, type
  and depth over windward slopes and summits; the highest ground within 16 km upwind above the column takes humidity
  away (`shadowGain` per km: the rain shadow's clear lee); the sea is a little moister with flatter, lower decks.
- **Type** from an instability field (60 km, 90 min; ±0.3 about the mean type), + the orographic lift (towers over
  windward mountains), − over the sea.
- **Base**: dry air lifts the clouds' floor by up to 25 % of the layer (790 m), less over the sea — regional bases
  from ~1.4 to ~2.1 km.
- **Depth**: each type's height profile scaled 0.6–1.25 by the instability and the humidity (+ lift), so a cumulus
  region's tops range over ~1 km.

The march reads one texel per sample in place of the two value-noise regional fields it computed before (cheaper per
step); the light samples reuse the column's. The map is rebaked every 2 s of the clouds' clock (~1.6 ms natively in
a worker) and when the eye moves 4 km from its centre; the window snaps to whole texels, so a move samples the same
field. Before the first bake the map is the mean weather (the old look, minus the regional fields).

## 5. Not done (next rounds)

- **Cloud streets and cells**: anisotropic noise along the wind for streets at high wind speed and cellular noise
  over the sea (the mesoscale octave is isotropic today).
- **A real front's structure**: a warm front's lowering shield and a cold front's narrow band of towers, as a moving
  feature rather than noise. Needs cirrus/altostratus layers above the volume (one layer now, 1.35–4.5 km).
- **Rain** from the field (precipitation is still one number), the clouds' shadows on the ground, and the 2D deck
  (Low, phones) and reflections taking the same field (they keep the old uniform deck).
- **Server-authoritative weather**: today each client computes the field from the seed and the clouds' clock, so
  clients agree only as far as their clocks do (the world clock: close). A weather system on the server would send
  its state.

## 6. Questions for the owner

1. Should weather become gameplay (rain, storms, wind affecting players), and so server-side, or stay a look?
2. Should a world have a climate setting (dry, temperate, stormy) — per world, saved with it — or one climate for
   every world?
3. How fast should weather change: a front passing every ~hour of play (now), or slower?
4. Are higher cloud layers (cirrus, altostratus decks) wanted, at a cost on the M1 Air?
