# Archived approaches

Work that isn't on `main` but is kept in history as `archive/*` tags (branch cleanup, 2026-10-21).
Restore any of them with `git checkout -b <name> archive/<tag>`. The bigger ones are written up
below; the rest are single experiments listed for completeness.

## The ray-marched renderer — `archive/ray-marcher` (tag and branch)

Phase 25 (ADR 0029): a GPU ray marcher over voxel bricks and LOD levels, built to compare with the
mesher/raster renderer. It covered a beam prepass, a temporal start (still, then also moving),
kept unchanged frames, a column-search slope rule, half-scale rendering with FSR 1, and compare mode.
Dropped at A7 by the owner (2026-10-20): it was much slower than raster, especially for distant and
high views (about 18 ms against 6 ms flying high), and three more far-view speed-ups gave nothing.
ADR 0044 records the decision; `RAY_MARCHED_RENDERER.md` keeps a short note; the code is at
`archive/ray-marcher` (main 0.20.3, 8ed481f).

## Half-height slope variants — `archive/hh-variant-v1…v4`

The 2×2 the owner compared before adopting half-height cells (2026-10-20): the ramp rule (old
side/back test or the 3×3 Sobel facing) against the 1 m step snap (on or off). V2, the old rule
with the snap off, was chosen and is what `main` generates (HALF_HEIGHT_GRID.md §4, §8).

| Tag | Rule | 1 m snap | Cliffs / 100 columns | Slideable ground |
|---|---|---|---|---|
| `archive/hh-variant-v1-old-snap` | old | on | 34.0 | 12.9 % |
| `archive/hh-variant-v2-old-nosnap` (adopted) | old | off | 24.5 | 3.2 % |
| `archive/hh-variant-v3-sobel-snap` | Sobel | on | 17.6 | 21.1 % |
| `archive/hh-variant-v4-sobel-nosnap` ("option A") | Sobel | off | 1.3 | 10.6 % |

The Sobel rule picks a column's facing from the 3×3 gradient and needs only the neighbour toward
the facing lower and the opposite one not, which removes the terraces left at every sideways
contour shift but leaves ½ m wedges on diagonal hillsides (a herringbone look).

## The cel texture style — `archive/cel-textures`

A second texture style for the pixel-art preview (0.24.2): the painterly materials repainted from
`shared/textures/cel.json` by `cel.mjs` in at most four flat tones a tile on the `dwell` pixel-art
palette, at 16 texels a tile, baked into `dwell-mesh` `cel.rgba`, with mips by majority tone; the
presets `270-cel` and `360-cel` swapped the atlas to it. It cut the pixels that change frame to frame
by 10–32 % against painterly in the same look but not the sparkle, and the 1 m repeat showed. Removed
by the owner on 2026-10-21: "I think let's throw out the cel shaded ones they're pretty bad."
PIXEL_ART_LOOK.md §8 keeps the measurements; the code is at `archive/cel-textures` (main 0.25.0,
1b5eaf95).

## Smaller experiments

| Tag | Date | What it tried |
|---|---|---|
| `archive/claude-ci-build-once` | 2026-10-13 | CI building once for all e2e shards, cached compiles, parallel server suites |
| `archive/claude-debug-capture` | 2026-10-13 | Debug tooling: setup, teleport, a settled signal, debug views, one-command shots; copying the frame and game state as JSON |
| `archive/claude-fix-lod-flat-skirts` | 2026-10-13 | LOD skirts on section sides only (top/bottom skirts had laid sheets over the sea) |
| `archive/claude-height-fog-menu` | 2026-09-30 | Fog defaults from playtesting (4 km, 50 %, 1.5 km) |
| `archive/claude-pr-previews` | 2026-10-07 | Preview builds of pull requests, linked on the PR |
| `archive/claude-slopes-0.3.1` | 2026-10-06 | A 0.3.x backport line: targetable ladders, slope peak/pit cubes, release tooling for older lines |
| `archive/claude-voxel-multiplayer-physics-99qf51` | 2026-09-25 | Stopping CI's server smoke run instead of waiting |
| `archive/local-exp-ci-*` (3) | 2026-10-15 | CI experiments: the e2e suite without clouds on the software adapter |
| `archive/local-hh-step-up` | 2026-10-20 | A step-up cherry-pick for the half-height branch, superseded when main merged in |
