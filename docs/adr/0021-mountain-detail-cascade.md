# 0021. Mountain detail from a derivative-damped ridged cascade

- Status: Accepted
- Date: 2026-10-07
- Builds on: [0018](0018-drainage-consistent-terrain.md) (the height model `V + D × relief`),
  [0010](0010-worldgen-noise-numerics.md) (strict-float determinism)

## Context

Phase 11a left the mountains' sharp detail to the old ridged field (`Ridged2`, 360 m, five octaves),
scaled by the uplift. It has no notion of slope: fine octaves pile onto the steepest faces as much
as onto the gentle ones, so mountains were uniformly noisy — no smooth valleys and talus slopes
between sharp crests — and its finest octave was a 22 m lattice. `WORLD_GENERATION.md` §3.4 specifies
the Epic Terrain mod's technique instead (technique 7): a ridged cascade whose octaves are damped by
the accumulated slope and multiplied by the coarser octaves' result.

## Decision

1. **`DampedRidges2` (noise.h)**: nine octaves from a 4,096 m wavelength down to a 16 m lattice.
   Octave `i` adds `a_i × ridge_i × damp_i × prev`, with `ridge_i = 1 − 1.5 |n_i|` (clamped at 0),
   `damp_i = 1 / (1 + k |G|²)` where `G` is the slope (m per m, with the cascade scaled to 600 m)
   accumulated over the octaves so far, `k = 0.6`, and `prev` the coarser octaves' result normalised
   to [0, 1] (1 for the first octave). Amplitudes halve and the sum is normalised by the total, so
   the result is in [0, 1].
2. **Analytic derivatives.** The slope comes from `Perlin2d`, which returns the value and the
   partial derivatives of the gradient noise in closed form (the quintic fade's derivative is
   `30 t² (t − 1)²`): only `+ − ×` and one division per octave, so the cascade is as deterministic
   as the rest of the generator (no finite differences, no library calls).
3. **Where it applies.** It replaces `c.ridges` in the uplift belts' relief (900 m) and in the upland
   term (18 m + 150 m), and adds up to 300 m on the planet-scale ranges. Everything stays scaled by
   the distance factor `D` of the rivers, so mountains rise away from rivers and a channel keeps its
   valley floor. The ridged field remains for the mountains' weight and the biome rule.
4. **Evaluated sparingly.** At the 4 m lattice corners only on land where an uplift weight (the
   upland weight, the belts, the ranges) is not zero; elsewhere it reads 0. The weights are smooth,
   so the columns around a skipped corner differ by well under a metre.
5. **Level of detail.** A cell keeps the octaves whose lattice spacing it resolves, normalised by
   their amplitude; a cell that resolves fewer than two (4 km and up: one octave is a ridged field
   without the damping or the products, biased high) reads the mean (0.55, measured over the world
   by a test, as `kLodRidges` is).
6. **A hashed lattice offset** (1–4,000 m, per world), as the river tiers have: Perlin noise is zero
   at its lattice points, and the coarse levels' cell centres (multiples of 4,096 m) would all lie on
   the cascade's, reading its maximum (ridge 1) — a bias of tens of metres against the level of
   detail's mean that the column-surface test caught.
7. **Generator version 9.** Goldens regenerated.

## Consequences

- **A breaking change to the terrain a seed generates**, so a new compatibility line (before 1.0
  the next MINOR; the owner raises `package.json`, `CLAUDE.md`).
- Chunk generation near the origin is unchanged (the cascade is skipped there); in mountains it
  costs about a quarter more per chunk; a LOD section costs 15–25 % more at levels 1–10.
- Tests pin the analytic gradient against finite differences, the cascade's range, the damping, the
  octave dropping, that mountains are far rougher than lowland, and that channels stay at the
  valley floor.
- Reversal: restore `c.ridges` in the three terms of `Finish` and bump the generator version.
