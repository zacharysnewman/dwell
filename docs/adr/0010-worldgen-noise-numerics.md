# 0010. Worldgen noise numerics: strict IEEE float with integer-hash gradients

- Status: Accepted
- Date: 2026-09-25
- Resolves: ARCHITECTURE.md Open Decisions #8

## Context

Terrain is a pure function of `(worldSeed, generatorVersion, ChunkCoord)` (ARCHITECTURE.md §6.3).
The native server, local mode (WASM) and the client (WASM) must produce bit-identical chunks:
the network then sends only differences from the generated baseline, and client prediction
collides with exactly the terrain the server has. Noise is the numerically sensitive part: the
question was whether to evaluate it in fixed point, which is exact by construction, or in
IEEE float, which is exact only if every operation is.

## Options considered

Both were prototyped with the same algorithm: improved Perlin gradient noise, gradients chosen by
an integer hash of the lattice point, and a quintic fade (`server/tools/noise_numerics.cpp`). The
prototype evaluates 2 million 3D samples spread over the whole ±65 536 m world at terrain
frequencies, natively (x86-64, GCC, `-O2`) and in WASM (Emscripten 6.0.10, `-O3`, Node 22).

| | Strict IEEE float | 16.16 fixed point |
|---|---|---|
| Native vs WASM checksum | identical | identical |
| Time per 3D evaluation, native | ~182 ns | ~177 ns |
| Time per 3D evaluation, WASM | ~142 ns | ~134 ns |
| Difference from the other | — | ≤ 0.0007 (noise range ±1) |

(Times include generating the sample coordinates, which dominates; the two differ by ~5 %.)

- **Fixed point.** Exact everywhere with no compiler caveats. But everything downstream of the
  noise (splines, smoothsteps, biome blends, interpolation, divisions) must be fixed point too,
  or the exactness is lost; the 16.16 range overflows for high-frequency terms at large
  coordinates (`x × frequency` must stay under 32 768 lattice units), so every term needs
  range analysis; and tuning in integer units is slow.
- **Strict IEEE float.** `+ − × /` and comparisons are correctly rounded in IEEE 754, so x86-64
  (SSE, no x87 excess precision) and WebAssembly give the same bits, provided the compiler
  neither fuses operations (FMA) nor reorders them (fast-math), and no libm function is called
  (their results are not specified to the last bit). The project already builds this way:
  `-ffp-contract=off` for Dwell's targets and Jolt's `CROSS_PLATFORM_DETERMINISTIC`
  (PLAYER_CONTROLLER.md §8.3), and the player controller is already checked native vs WASM.

## Decision

Worldgen uses **strict IEEE float** with **integer-hash gradients**:

- Lattice hashes are integer (`Hash2`/`Hash3`: coordinates times odd constants, xor-ed with the
  seed, one murmur3 avalanche). Floats never feed a hash.
- Only `+ − × /`, comparisons, and float↔int conversions (`FloorToInt`) are used. No `sin`,
  `exp`, `pow`, `sqrt` or other library calls in `server/core/src/worldgen`; round shapes (tree
  crowns, boulders, ore blobs) use integer distance tests.
- Built with `-ffp-contract=off`, no `-ffast-math`, for every target.
- Interpolated fields are computed in one fixed order that the chunk path and the point queries
  (`SolidAt`, `GroundY`) share, so a feature placed by a point query agrees with the chunk.
- **Enforcement:** a golden test (`server/tests/worldgen/golden/chunk-hashes.txt`) hashes a set
  of chunks across the pipeline (surface, caves, bedrock, sky, ocean, mountains; two seeds). CI
  runs it natively (`dwell_tests`) and under WASM (`dwell_worldgen_tests.js`); a mismatch fails
  the build.

## Consequences

- Terrain code reads as ordinary float code, and tuning stays quick.
- A contributor adding a library call or a new build flag could break determinism; the golden
  test catches it on the next CI run, and this ADR and the comment in `noise.h` state the rules.
- Any intended change to generator output changes the golden hashes: bump the terrain generator
  version (§6.3) and regenerate them (`DWELL_UPDATE_GOLDEN=1`). Saved worlds record the version.
- Reversal: the fixed-point prototype stays in `server/tools/noise_numerics.cpp`. If a platform
  ever diverges (a new compiler, an ARM server without the same guarantees), the noise functions
  can switch to it behind the same interface, at the cost of converting the downstream terrain
  math and a generator version bump.
