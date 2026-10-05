# Floating Island Terrain Generation: An Engine-Agnostic Spec

Oct 5, 2026 · @Zachary Newman

## Overview

The islands come from one idea: sample a 3D noise field, fade it toward "empty" near the top and bottom of a fixed height band, and call every point above zero solid. There is no heightmap and no per-island shape logic. Everything after that step is layering and decoration.

This spec is derived from the Aether mod, version 1.5.11 for Minecraft 1.21.1. The numbers are the mod's own, read from its world generation data and decompiled placement code. Names of blocks are replaced with generic roles.

Conventions used throughout:

- One unit is one voxel edge. Treat it as 1 metre if your engine is not voxel-based.
- `y` is up. The world band runs from `y = 0` to `y = 128`, with nothing but open sky below and above.
- "Region" means a 16 x 16 unit column of the world, the grain at which decoration is scheduled.
- Solid means density greater than 0. A mesh-based engine can run marching cubes on the same field.

Out of scope: anything about items or gameplay. This covers terrain only. We are not adding new blocks for this at this time, so every material role named here maps to a block that already exists.

## Pipeline at a glance

Generation runs as eleven ordered passes, and the order matters because later passes read what earlier ones wrote.

1. **Seed.** Derive an independent sub-seed for every noise field and for every decoration pass in every region, from the world seed plus region coordinates plus a per-pass salt.
2. **Climate.** Sample two 2D noise fields and look up a region type.
3. **Density.** Evaluate the shaped 3D density on a coarse lattice, interpolate, and fill every positive cell with base stone.
4. **Surface.** Convert the top of every exposed floor to grass-topped soil and the cells under it to soil.
5. **Edge shelves.** Hang flat disks of a special sand under thin island rims.
6. **Lakes.** Carve sealed basins into island tops and half-fill them with water.
7. **Cloud banks.** Lay long cloud banks in the empty zone under the islands.
8. **Ores.** Replace pockets of base stone with ore, ice-stone and soil.
9. **Springs.** Open water sources in cliff faces so they fall off the island.
10. **Vegetation.** Scatter trees, grass, flowers and bushes on every walkable layer.
11. **Sky dressing.** Add small clouds and tiny one-tree islets.

Passes 5 to 11 run per region and may write up to one region beyond their own. Generate neighbours' terrain (passes 1 to 4) before decorating a region.

## Density field

The raw shape is a blend of two independent fractal noise fields, switched by a third. Call the result `N(x, y, z)`. It is centred on 0 and mostly falls between -0.5 and +0.5.

**Two shape fields, A and B.** Each is 16 octaves of gradient (Perlin) noise with its own seed. The longest wavelength is 191.5 units in all three axes. Each further octave halves both wavelength and amplitude.

```latex
A(p) = \sum_{k=0}^{15} 2^{-(k+1)} \, P_k\!\left(\frac{2^k \, p}{191.5}\right), \qquad P_k \in [-1, 1]
```

**One selector field, S.** Eight octaves built the same way, but stretched: longest wavelength 59.85 units horizontally and 119.7 units vertically. It is amplified hard and clamped, so it behaves like a switch.

```latex
q = \mathrm{clamp}\big(0.5 + 12.8 \, S(p),\; 0,\; 1\big), \qquad N = A + (B - A)\, q
```

In a re-implementation, `q` sat at exactly 0 or 1 for about 85% of samples. Most of the world is therefore pure A or pure B, with narrow seams between them. Those seams are what give islands abrupt cliffs and ragged outlines instead of smooth blobs.

**Coarse lattice.** Evaluate density only at lattice points spaced 8 units apart horizontally and 4 units vertically, then interpolate trilinearly. Octaves with wavelengths under about 12 units alias into faint jitter at this spacing. Four or five octaves per field reproduce the look; the rest is inherited from the host engine.

**Height gain.** The host engine's legacy noise has a quirk that makes amplitude grow with altitude. Measured standard deviation of `N` was 0.15 from y = 0 to 32, rising to 0.29 at y = 128. With clean Perlin noise, multiply `N` by a gain of 1.0 up to y = 32, rising linearly to 1.9 at y = 128. In a four-seed test this substitute matched the original's solid share at every height to within 1 percentage point. Skip the gain and high terrain mostly disappears.

## Vertical shaping

Two height ramps pull the noise toward negative values at the edges of the band, which is what turns endless 3D noise into a layer of separate islands with tapered undersides.

```
bottom(y) = clamp((y - 8) / 32, 0, 1)      // 0 at y <= 8, 1 at y >= 40
top(y)    = clamp((128 - y) / 72, 0, 1)    // 1 at y <= 56, 0 at y >= 128

a = N * gain(y) - 0.13
b = -0.2 + top(y)    * (a + 0.2)           // fade toward -0.2 going up
c = -0.1 + bottom(y) * (b + 0.1)           // fade toward -0.1 going down
density = c - 0.05
solid   = density > 0
```

The same rule read as a single cutoff: a point is solid when its noise value beats a threshold `T(y)` that depends only on height.

```latex
T(y) = \begin{cases} \dfrac{0.15}{\mathrm{bottom}(y)} + 0.03 & y < 40 \\[2ex] \dfrac{0.25}{\mathrm{top}(y)} - 0.07 & y \ge 40 \end{cases}
```

&#91;embedded content: Threshold computed from the mod's density settings; noise ceiling measured in a re-implementation over four seeds, 1024 x 1024 units each\]

The bottom ramp is short and steep, so undersides pinch off quickly into hanging points. The top ramp is long and shallow, so most tops sit just above the core band and a few rare peaks and high islets reach far above it.

What this produced in the re-implementation, as rough targets for tuning:

- About 6% of the whole band is solid, peaking near 17% at y = 56.
- About one third of the map has land somewhere above it.
- Islands are typically 15 to 20 units thick, with a long tail to about 50.
- Roughly one land column in five has a second walkable layer under an overhang.

The mod then passes density through a squashing curve, `d/2 - d^3/24` after clamping to -1..1. It never changes the sign, so it matters only if you mesh the field and want bounded values.

## Surface layering

After the density pass every solid cell is base stone. Two rules, applied per column from the top down, add the soil skin.

1. **Top cell.** A solid cell with empty space directly above it is a floor. It becomes grass-topped soil, or plain soil if the cell above holds water.
2. **Subsoil.** Solid cells within the soil depth below any floor become soil. Everything deeper stays base stone.

Soil depth comes from the host engine: about 3 cells, varied by a slow 2D noise so it ranges from roughly 1 to 5. Treat that as approximate and tune it to taste.

Three consequences worth copying:

- The rules fire on every floor in a column, not just the highest. Ledges under overhangs get grass too.
- There is no ceiling rule. Undersides are bare base stone, which reads well from below.
- There is no sea level and no groundwater. Water exists only where the lake and spring passes put it.

The tiny-islet pass re-runs the top-cell rule on the cells it places, so islet tops match the terrain around them.

## Regions and climate

Region type changes tree density and colour only. Terrain shape, ores and clouds are identical everywhere, so this whole section is optional.

Two independent 2D noise fields drive it, each normalised to about -1..1 and sampled once per 4 x 4 units:

- **Temperature**: two octaves with wavelengths of about 1024 and 256 units, weighted 1.5 to 1.
- **Humidity**: two equal octaves with wavelengths of about 512 and 256 units.

Both are lightly domain-warped by a few units so borders are not perfectly smooth. The pair is looked up in this table, and the type sets tree placements per 16 x 16 region.

| Temperature | Humidity | Region type | Tree attempts per region |
| --- | --- | --- | --- |
| below -0.8 | any | Meadow | 1 |
| -0.8 to 0 | below 0 | Meadow | 1 |
| -0.8 to 0 | 0 and above | Forest | 6, sometimes 7 |
| 0 to 0.4 | below 0, or above 0.8 | Grove | 2, sometimes 3 |
| 0 to 0.4 | 0 to 0.8 | Forest | 6, sometimes 7 |
| 0.4 to 0.93 | below -0.1 | Grove | 2, sometimes 3 |
| 0.4 to 0.93 | -0.1 and above | Forest | 6, sometimes 7 |
| above 0.94 | below -0.1 | Meadow | 1 |
| above 0.94 | -0.1 to 0.8 | Woodland | 5, sometimes 6 |

"Sometimes" is a 1 in 10 chance. Each attempt succeeds about half the time, as the vegetation rules below explain. The mod also has a thin transition strip between temperatures 0.93 and 0.94 that is not worth reproducing.

## Decoration

Every decoration pass is scheduled per 16 x 16 region with its own seeded random stream. The table is in generation order.

| Feature | Frequency per region | Height range (y) | Core rule |
| --- | --- | --- | --- |
| Edge shelf | 1 in 5 regions | 0 to 48 | Flat disk under one-cell-thick grass rims |
| Lake | 1 in 15 regions | Island surface | Sealed basin, lower half water |
| Soil pockets | 20 attempts, veins up to 33 cells | 0 to 128 | Replaces base stone only |
| Ice-stone pockets | 10 attempts, up to 32 cells | 0 to 128 | Replaces base stone only |
| Common ore | 20 attempts, up to 16 cells | 0 to 128 | Replaces base stone only |
| Mid-tier ore | 14 attempts, up to 5 cells | 0 to 75 | Half of air-exposed cells discarded |
| Rare ore, buried | 5 attempts, up to 3 cells | 0 to 74 | Half of air-exposed cells discarded |
| Rare ore, low | 7 attempts, up to 4 cells | Weighted to the lowest terrain | Triangular height distribution peaking at y = 8 |
| Spring | 30 attempts | 8 to 128 | Water source in a cliff face |
| Trees | By region type | Every walkable layer | Layered ground finder |
| Ground cover | About 10 grass patches; flowers and bushes 1 in 8 to 1 in 16 regions | Every walkable layer | Layered ground finder, then scatter |
| Small cloud, common | 1 in 7 regions | 32 to 96 | 16-step blob walk |
| Small cloud, special | 1 in 24 regions | 32 to 96 | 8-step blob walk |
| Small cloud, rare | 1 in 75 regions | 96 to 128 | 4-step blob walk |
| Tiny islet | 1 in 50 regions | 32 to 96 | 13-cell pad with one tree |

Ore attempts pick a uniformly random point in the height range, and most of those points are open sky. Ore density per unit of rock therefore scales with how much rock there is, with no extra logic.

**Edge shelf.** For each column, scan upward from y = 0 to 48. At the first empty cell that has a grass cell directly above it and an empty cell above that, place a flat disk of shelf material with radius 3.46. Fill empty cells only, then move to the next column.

**Lake.** Start 4 units below the surface in a 16 x 8 x 16 box. Mark the union of 4 to 7 random ellipsoids, each 3 to 9 units wide and 2 to 6 tall. Abort if any cell bordering the marked shape in the lower four layers is not solid, since the lake would drain into the sky. Otherwise fill the lower four layers with water, clear the upper four, and re-grass exposed soil around the rim.

**Spring.** At a random point, require solid stone or soil above and below, exactly three solid horizontal neighbours, and exactly one empty one. Place a flowing water source. This yields waterfalls that pour out of cliffs and off the island edge.

**Layered ground finder.** For layer 0, 1, 2 and so on: with a 1 in 2 chance, pick a random column in the region and find its nth floor from the top that has at least 4 empty cells above it. If one is found, emit it and try the next layer down; otherwise stop. Lower layers get progressively sparser growth.

**Trees.** 99 in 100 are small: trunk 4 to 6 units, round canopy of radius 2. The rest are a large variant with a 10-unit trunk and radius-3 canopy. A tree needs soil under it and no water.

**Blob walk for clouds.** Start at a random offset in the region and pick one diagonal heading. Each step, move 0 or 1 units along each horizontal axis, and half the time move up or down by one. Then stamp a box 3 to 4 wide and 2 tall, keeping only empty cells within a Manhattan distance of 4 to 5 from the box corner. The result is a ragged diagonal streak.

**Tiny islet.** Only if its tree can be placed. Top two layers are a diamond of radius 2 (13 cells), grass over stone; the third layer is a plus of 5 stone cells.

## Cloud banks

Long cloud banks are the one feature sited on a world-scale grid instead of per region. Sites are chosen first, before any terrain is decorated.

**Jittered grid.** Divide the map into square cells. In each cell pick one site at a random offset from the cell corner, limited so that sites in neighbouring cells stay at least a minimum gap apart. Seed the choice from the cell coordinates plus a fixed salt.

**Cloud bank.** Sites use a grid cell of 96 units with a minimum gap of 48. Each bank is a 64-step version of the small-cloud blob walk, with a fixed drift of -1, 0 or +1 per axis chosen once. Each step stamps a blob 9 to 12 units wide and 2 tall, and height changes by one on 1 step in 10. Banks start between y = 0 and 32, in the empty zone under the island band, so they read as a cloud floor.

## Parameter reference

These twelve numbers control the terrain shape; everything else in the spec is dressing.

| Parameter | Default | Effect of changing it |
| --- | --- | --- |
| World band | y = 0 to 128 | Rescale both ramps with it |
| Lattice spacing | 8 horizontal, 4 vertical | Coarser is smoother and cheaper; finer shows more small-octave detail |
| Shape noise wavelength | 191.5 units | Sets island size and gap size together |
| Shape noise octaves | 16, halving each time | 4 or 5 are enough at the default lattice |
| Selector wavelength | 59.85 horizontal, 119.7 vertical | Size of the patches where field A or B wins |
| Selector sharpness | 12.8 | Lower gives softer blends and rounder islands |
| Height gain | 1.0 at y <= 32, rising to 1.9 at y = 128 | Lower removes high peaks and islets |
| Core threshold | 0.18 (offsets 0.13 + 0.05) | Higher means less land; the main coverage dial |
| Bottom ramp | y = 8 to 40, toward -0.1 | Shorter gives blunter undersides; longer gives deeper spikes |
| Top ramp | y = 56 to 128, toward -0.2 | Starting it higher thickens islands and raises tops |
| Soil depth | About 3 cells | Thickness of the soil skin |
| Region size | 16 x 16 units | Grain of all decoration scheduling |

To hit a target coverage with your own noise, measure the distribution of your `N` and set the core threshold at the matching percentile. The mod's 0.18 sits near the 85th percentile of its noise in the core band.

## Implementation checklist and pseudocode

Build in this order and check each step visually before moving on.

- [ ] Fractal noise fields A, B and S with independent seeds
- [ ] Blended noise `N` with the hard selector; confirm it is centred on 0 with a standard deviation near 0.15
- [ ] Shaped density on the 8 x 4 x 8 lattice with trilinear interpolation; confirm about one third of columns contain land
- [ ] Surface layering on every floor
- [ ] Seeded per-region decoration scheduler that can read neighbouring regions
- [ ] Ores, then lakes with the leak check, then springs
- [ ] Layered ground finder, trees and ground cover
- [ ] Blob-walk clouds: small ones in the band, banks beneath it
- [ ] Jittered-grid sites for the cloud banks

```
function shapeNoise(seed, p):                # p in world units
    sum = 0
    for k in 0..15:
        sum += perlin(seed + k, p * 2^k / 191.5) / 2^(k + 1)
    return sum

function selector(seed, p):
    q = (p.x / 59.85, p.y / 119.7, p.z / 59.85)
    sum = 0
    for k in 0..7:
        sum += perlin(seed + k, q * 2^k) / 2^(k + 1)
    return clamp(0.5 + 12.8 * sum, 0, 1)

function density(p):
    a = shapeNoise(seedA, p)
    b = shapeNoise(seedB, p)
    n = lerp(a, b, selector(seedS, p))
    n = n * (1 + 0.9 * clamp((p.y - 32) / 96, 0, 1))      # height gain

    bottom = clamp((p.y - 8) / 32, 0, 1)
    top    = clamp((128 - p.y) / 72, 0, 1)
    d = n - 0.13
    d = -0.2 + top    * (d + 0.2)
    d = -0.1 + bottom * (d + 0.1)
    return d - 0.05

function generateRegion(rx, rz):
    lattice = sample density at every (8, 4, 8) lattice point covering the region
    for each cell in region:
        solid[cell] = trilinear(lattice, cell) > 0
    for each column, scanning from the top down:
        for each solid cell with empty space directly above:
            mark it grass-topped soil
            mark the next soilDepth solid cells below it as soil
        all other solid cells are base stone

function decorateRegion(rx, rz):             # after neighbours have terrain
    for pass in [shelf, lake, cloudBanks, ores, springs, vegetation, skyDressing]:
        rng = seeded(worldSeed, rx, rz, pass.salt)
        pass.run(rng, rx, rz)
```
