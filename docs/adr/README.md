# Architecture Decision Records

One file per decision: `NNNN-short-title.md`, numbered in order. ADRs are immutable once
accepted; to change a decision, add a new ADR that supersedes the old one and update the old
one's status line. Resolving an item in `ARCHITECTURE.md` Open Decisions requires an ADR.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-webtransport-server-library.md) | WebTransport server library: Rust `wtransport` behind a C ABI | Accepted (in the Rust workspace, no C ABI: 0028) |
| [0002](0002-client-renderer.md) | Client renderer: Three.js on WebGL2, behind a thin render interface | Superseded by 0028 |
| [0003](0003-multiplayer-hosting-model.md) | Multiplayer hosting model: player-hosted servers, friend worlds, and a master server | Accepted |
| [0004](0004-player-identity.md) | Player identity: device keys now, optional accounts later | Accepted |
| [0005](0005-domain-and-origins.md) | Domain and web origin: the default Pages path `dropkickarcade.com/dwell/` | Accepted (older builds: 0014) |
| [0006](0006-world-persistence-sqlite.md) | World persistence: one SQLite database per world, holding all data | Accepted |
| [0007](0007-threading-model.md) | Threading model: single-threaded web sim core with worker pools; threads natively | Accepted (Rust and Rapier: 0028) |
| [0008](0008-dedicated-server-transports.md) | Dedicated-server transports: WebTransport primary, WebRTC fallback, no WebSocket | Accepted |
| [0009](0009-friend-world-lifetime.md) | Friend worlds end with their host; migration and cloud worlds deferred | Accepted |
| [0010](0010-worldgen-noise-numerics.md) | Worldgen noise numerics: strict IEEE float with integer-hash gradients | Accepted (coordinates at planet scale: 0011; in Rust: 0028) |
| [0011](0011-planet-scale-world.md) | Planet-scale world: an 8,192 km disc, 8,192 m tall, with double-precision physics | Accepted (scale: 0025; Rapier f64: 0028) |
| [0012](0012-lod-octree.md) | Whole-world view: a 3D level-of-detail octree, generated on the client | Accepted |
| [0013](0013-master-server-on-cloudflare.md) | Master server on Cloudflare Workers + Durable Objects; Cloudflare TURN | Accepted |
| [0014](0014-versioned-releases.md) | Versioned releases: builds as tagged GitHub Releases behind a launcher, worlds locked to their compatibility line | Accepted (version embedded by Cargo: 0028) |
| [0015](0015-block-registry.md) | Block registry: namespaced block states, generated registries, string palettes on disk | Accepted (Rust tables: 0028) |
| [0016](0016-slope-blocks.md) | Slope blocks: shaped block families, one baked shape table, shaped terrain | Accepted (families and slope stage removed: 0024, 0025; Rust tables: 0028) |
| [0017](0017-continents-from-voronoi-plates.md) | Continents from Voronoi plates: a plate layout, a separation clamp, a macro lattice, and `sqrt` | Superseded by 0025, 0026 |
| [0018](0018-drainage-consistent-terrain.md) | Drainage-consistent terrain: rivers as noise contours, terraced static water above sea level | Superseded by 0025 |
| [0019](0019-continental-scale-climate.md) | Climate at continental scale: biomes in regions, snow by temperature and height | Superseded by 0025 |
| [0020](0020-lod-rivers-at-their-width.md) | Level-of-detail rivers at their own width; water drawn at its level | Superseded by 0025 |
| [0021](0021-mountain-detail-cascade.md) | Mountain detail from a derivative-damped ridged cascade | Superseded by 0025 |
| [0022](0022-climate-biome-table-vegetation.md) | Climate with continent biases and rain shadows, the biome table, wetland ponds and biome-tinted vegetation | Superseded by 0025 |
| [0023](0023-bifacial-world.md) | The bifacial world: two faces, gravity toward the midplane | Accepted (amended by 0025) |
| [0024](0024-adaptive-natural-blocks.md) | Adaptive natural blocks: shapes derived from neighbours by five rules | Accepted (amended 2026-10-12; Rust readers: 0028) |
| [0025](0025-baseline-world.md) | The baseline world: a 50 km bifacial disc, a 10-layer core, a spherical build volume, six blocks | Accepted (Rapier f64: 0028) |
| [0026](0026-terrain-definitions.md) | Terrain definitions: terrain as data, evaluated by a fixed vocabulary of node kinds | Accepted (Rust tables: 0028) |
| [0027](0027-webgpu-renderer.md) | WebGPU renderer: Three's `WebGPURenderer` with TSL materials, a WebGL2 fallback, and a look registry | Superseded by 0028 |
| [0028](0028-rust-port.md) | One Rust codebase: server, sim core and client, with Rapier physics and a wgpu renderer (WebGPU only) | Accepted (renderer amended by 0029, which 0044 supersedes) |
| [0029](0029-ray-marched-renderer.md) | A ray-marched terrain renderer, built complete beside the mesher and compared side by side | Superseded by 0044 |
| [0030](0030-fluid-looks-and-water-rendering.md) | Fluid looks as registry data, and a water shader with refraction, underwater and reflections in quality tiers | Accepted |
| [0031](0031-data-driven-structures.md) | Data-driven structures, built in an in-game sandbox world | Accepted |
| [0032](0032-audio.md) | Procedural, data-driven sound effects in plain Web Audio, with events from the Rust game | Accepted |
| [0033](0033-tree-growth.md) | Tree growth: hand-built trees revealed along their skeleton, as a layer composed at load | Accepted |
| [0034](0034-sky-clouds.md) | Sky clouds: a 2D cloud deck drawn in the sky, its tunables as data | Accepted |
| [0035](0035-day-night-cycle.md) | The day/night cycle: world time drives a keyframed sky, light and fog, as data | Accepted |
| [0036](0036-render-modes-and-cross-geometry.md) | Render modes as block data (opaque, cutout, translucent), a cross geometry, and placement rules as data; plants in world generation (partly revisits B10) | Accepted |
| [0037](0037-brushes.md) | Brushes: VoxelSniper-style painting and sculpting over region-edit pastes with `KEEP` cells | Accepted |
| [0038](0038-block-registry-sync.md) | Block registry sync: the server sends its registry at join, the client maps ids at the boundary | Proposed |
| [0039](0039-spyglass.md) | The spyglass: a data-driven zoom whose detail follows the narrow view | Accepted |
| [0040](0040-painterly-textures.md) | Painterly textures: one data-driven stroke generator, baked at build time, behind a switch | Accepted |
| [0041](0041-characters.md) | Characters and creatures: rigid chunky parts on a data rig, a separate first-person arms view | Accepted |
| [0043](0043-structure-placements.md) | Structure placements in world generation, as data: the owner's pines in groves (generator version 17) | Accepted |
| [0044](0044-mesher-over-ray-marching.md) | The mesher is the terrain renderer; the ray marcher is removed (Phase 25's A7; code in `archive/ray-marcher`) | Accepted |

Phase numbers in ADRs follow the implementation plan as it was then. In ADRs 0001–0010, Phases
4–7 became 5–8 when the world-LOD phase (Phase 4) was added on 2026-09-29, and then 6–9 when the
multiplayer-ready phase (Phase 5) was added on 2026-09-30. In ADRs 0001–0013, Phases 6–9 became
14–17 on 2026-10-05, when eight phases were placed before them (6 versioned releases, 7 colour,
8 block registry, 9 slope blocks, 10–12 continents, natural terrain, sky islands, 13 the bifacial
world). In ADRs 0001–0024, Phases 14–17 became 20–23 on 2026-10-12, when the baseline rework
(Phases 14–19) was placed before them.

## Template

```markdown
# NNNN. Title

- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD
- Resolves: ARCHITECTURE.md Open Decisions #N (if any)

## Context
What forces are at play and what problem needs deciding.

## Options considered
Each option with its pros and cons.

## Decision
What we chose, stated plainly.

## Consequences
What becomes easier or harder; follow-up work; how we would reverse it.
```
