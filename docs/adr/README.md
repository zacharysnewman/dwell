# Architecture Decision Records

One file per decision: `NNNN-short-title.md`, numbered in order. ADRs are immutable once
accepted; to change a decision, add a new ADR that supersedes the old one and update the old
one's status line. Resolving an item in `ARCHITECTURE.md` Open Decisions requires an ADR.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-webtransport-server-library.md) | WebTransport server library: Rust `wtransport` behind a C ABI | Accepted |
| [0002](0002-client-renderer.md) | Client renderer: Three.js on WebGL2, behind a thin render interface | Accepted |
| [0003](0003-multiplayer-hosting-model.md) | Multiplayer hosting model: player-hosted servers, friend worlds, and a master server | Accepted |
| [0004](0004-player-identity.md) | Player identity: device keys now, optional accounts later | Accepted |
| [0005](0005-domain-and-origins.md) | Domain and web origin: the default Pages path `dropkickarcade.com/dwell/` | Accepted (older builds: 0014) |
| [0006](0006-world-persistence-sqlite.md) | World persistence: one SQLite database per world, holding all data | Accepted |
| [0007](0007-threading-model.md) | Threading model: single-threaded web sim core with worker pools; threads natively | Accepted |
| [0008](0008-dedicated-server-transports.md) | Dedicated-server transports: WebTransport primary, WebRTC fallback, no WebSocket | Accepted |
| [0009](0009-friend-world-lifetime.md) | Friend worlds end with their host; migration and cloud worlds deferred | Accepted |
| [0010](0010-worldgen-noise-numerics.md) | Worldgen noise numerics: strict IEEE float with integer-hash gradients | Accepted (coordinates at planet scale: 0011) |
| [0011](0011-planet-scale-world.md) | Planet-scale world: an 8,192 km disc, 8,192 m tall, with double-precision physics | Accepted |
| [0012](0012-lod-octree.md) | Whole-world view: a 3D level-of-detail octree, generated on the client | Accepted |
| [0013](0013-master-server-on-cloudflare.md) | Master server on Cloudflare Workers + Durable Objects; Cloudflare TURN | Accepted |
| [0014](0014-versioned-releases.md) | Versioned releases: builds as tagged GitHub Releases behind a launcher, worlds locked to their compatibility line | Accepted |
| [0015](0015-block-registry.md) | Block registry: namespaced block states, generated registries, string palettes on disk | Accepted |
| [0016](0016-slope-blocks.md) | Slope blocks: shaped block families, one baked shape table, shaped terrain | Accepted |
| [0017](0017-continents-from-voronoi-plates.md) | Continents from Voronoi plates: a plate layout, a separation clamp, a macro lattice, and `sqrt` | Accepted |
| [0018](0018-drainage-consistent-terrain.md) | Drainage-consistent terrain: rivers as noise contours, terraced static water above sea level | Accepted |
| [0019](0019-continental-scale-climate.md) | Climate at continental scale: biomes in regions, snow by temperature and height | Accepted |
| [0020](0020-lod-rivers-at-their-width.md) | Level-of-detail rivers at their own width; water drawn at its level | Accepted |
| [0021](0021-mountain-detail-cascade.md) | Mountain detail from a derivative-damped ridged cascade | Accepted |
| [0022](0022-climate-biome-table-vegetation.md) | Climate with continent biases and rain shadows, the biome table, wetland ponds and colourful vegetation | Accepted |

Phase numbers in ADRs follow the implementation plan as it was then. In ADRs 0001–0010, Phases
4–7 became 5–8 when the world-LOD phase (Phase 4) was added on 2026-09-29, and then 6–9 when the
multiplayer-ready phase (Phase 5) was added on 2026-09-30. In ADRs 0001–0013, Phases 6–9 became
14–17 on 2026-10-05, when eight phases were placed before them (6 versioned releases, 7 colour,
8 block registry, 9 slope blocks, 10–12 continents, natural terrain, sky islands, 13 the bifacial
world).

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
