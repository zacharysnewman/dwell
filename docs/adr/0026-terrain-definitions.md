# 0026. Terrain definitions: terrain as data, evaluated by a fixed vocabulary of node kinds

- Status: Accepted
- Date: 2026-10-12
- Resolves: Phase 14 of the implementation plan (the baseline's decision D10,
  [`BASELINE_EVALUATION.md`](../BASELINE_EVALUATION.md) §7)
- Replaces: the hardcoded generator pipeline (`terrain.cpp`) and the generator switch
  (`GeneratorFor`); keeps [0017](0017-continents-from-voronoi-plates.md)'s Voronoi coast distance as
  a node kind
- Builds on: [0010](0010-worldgen-noise-numerics.md) (strict IEEE numerics),
  [0011](0011-planet-scale-world.md) (the integer/remainder noise lattice),
  [0015](0015-block-registry.md) (block names and the registry hash), [0025](0025-baseline-world.md)
  (the baseline world)

## Context

Generator version 11 is a C++ pipeline of ~2,000 lines with 72 `constexpr` tunables and ~70
references to blocks by C++ constant (`Materials::kSand`); generators are chosen by a hardcoded
switch on the version. Changing the terrain means editing C++ and bumping the version by hand, and
no tool can add to it. The baseline ([0025](0025-baseline-world.md)) asks for simple noise terrain
with one Voronoi island, a 10-layer core and a soil skin — and for it to be an **extendable
system**: a data source of truth, one generator that turns it into what the server, WASM core and
client use, references by name, and validation over the data (`CLAUDE.md`).

## Options considered

1. **Keep the C++ pipeline, cut down.** The least work, but it fails the system test: every change
   is a code change. Kept only as Phase 15's interim step.
2. **A general scripting language** (expressions or a small language in data). Anything can be
   said, but determinism across native, WASM and the client module (ADR 0010) must then hold for
   arbitrary programs, validation can say little, and a tool has nothing structured to edit.
3. **A fixed vocabulary of node kinds in code; parameters and composition in data** (chosen, D10).
   The engine is small and testable once; a definition is a structured file a tool can edit and the
   generator can validate.

## Decision

1. **A terrain definition** is a JSON file in `shared/worldgen/` (beside `shared/blocks/`), named
   like a block (`dwell:baseline`). It holds:
   - `layout` — where land is: an extensible `kind`; the baseline's is `voronoiIsland` (site `cell`,
     `warp`, `land.radius` and `radiusNoise`, `rimClearance`, `coastDetail`), from which the coast
     distance and named masks (`inland`, `coast`, …) are derived;
   - `height` — land and sea lists of nodes, summed: `fbm` (wavelength, octaves, gain, amplitude),
     the coast-distance profile, constants; each scaled by a mask by name; sum, scale and clamp;
   - `core`, `surface`, `fill`, `seaLevel`, `liquid` — the column, all blocks by name: the core from
     the midplane up (never carved), conditioned surface layer lists from the surface down (the
     conditions a fixed set: `underwater`, coast distance, height above sea level, slope), the fill
     between them;
   - `spawn` — a rule (e.g. nearest dry, level land to the origin).
2. **The engine is code and stays small.** Node kinds, layout kinds and surface conditions are a
   closed vocabulary in `server/core/worldgen`, evaluated under ADR 0010's rules (integer-hashed
   gradient noise, integer wavelengths, `+ − × /`, correctly rounded `sqrt`, strict IEEE) on the
   4-column lattice. The pipeline is **layout → height field → fill by layers**: a height field, no
   3D density, so no overhangs, floating pieces or stability pass. A new kind is a code change with
   its own tests; a new use of existing kinds is data.
3. **One generator.** `shared/worldgen/gen.mjs` validates each definition — block names against the
   registry, node and layout kinds, conditions, parameter ranges, integer wavelengths, a core of at
   least one layer — and fails with a message naming the entry. It emits C++ tables for the server
   and WASM core and TypeScript for tools and the client's worldgen module; CI fails on stale
   output, as for the block registry and the protocol.
4. **A definition hash identifies the generator.** An FNV-1a 64 hash over the definition's
   canonical form (like the registry hash) is recorded by each world beside the generator version
   and carried in `Welcome`; a changed definition is a changed generator without a hand-bumped
   number. The generator version still marks changes to the engine.
5. **Generators are definitions.** `GeneratorFor`'s switch becomes a lookup by name: the flat test
   world is a definition with no noise; the movement playground is a structure file
   ([0025](0025-baseline-world.md) decision 7), not a generator.
6. **Per face.** A world names a definition and a seed stream for each face; by default both faces
   use `dwell:baseline`, face B with `FaceSeed(seed, B)` ([0025](0025-baseline-world.md) decision 4).
7. **Everything reads the same fields**: chunk generation, `GenerateLod`, point queries (`ColumnAt`,
   `GroundY`) and spawn, so near and far terrain and the server's checks agree.
8. **Tools.** `dwell_worldgen_inspect` takes a definition file (`--def path`) for height maps,
   whole-disc images, first-person views and `bench`; an editor page with a live preview in
   `docs/prototypes/` is the natural next tool.

## Consequences

- Adding a noise layer, a surface layer or a second liquid is an entry in a file; a test loads a
  modified definition and sees the terrain (and the hash) change. A wrong entry fails the generator
  with a message, not with a broken world.
- `terrain.cpp`'s pipeline and constants are deleted; worldgen tests become tests over definitions,
  with goldens for `dwell:baseline` natively, in WASM and in the client module.
- The terrain a seed generates changes: part of the baseline's new compatibility line
  ([0025](0025-baseline-world.md)).
- Changing a definition changes the ungenerated terrain of every world that uses it. A world keeps
  the hash it was created with; a build whose definition hashes differently refuses to extend it,
  and a client checks the hash in `Welcome` as it checks the registry hash. Phase 17 decides the
  exact handling of an old world opened by a build with a changed definition.
- What a height field can't express (caves, overhangs, structures) needs a new kind or a later
  system, by its own decision.
- Reversal: the evaluator could be replaced by code again without changing saved worlds (they store
  voxels and edits, not the definition), at the cost of the tools.
