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
| [0005](0005-domain-and-origins.md) | Domain and web origin: the default Pages path `dropkickarcade.com/dwell/` | Accepted |
| [0006](0006-world-persistence-sqlite.md) | World persistence: one SQLite database per world, holding all data | Accepted |
| [0007](0007-threading-model.md) | Threading model: single-threaded web sim core with worker pools; threads natively | Accepted |
| [0008](0008-dedicated-server-transports.md) | Dedicated-server transports: WebTransport primary, WebRTC fallback, no WebSocket | Accepted |
| [0009](0009-friend-world-lifetime.md) | Friend worlds end with their host; migration and cloud worlds deferred | Accepted |
| [0010](0010-worldgen-noise-numerics.md) | Worldgen noise numerics: strict IEEE float with integer-hash gradients | Accepted (coordinates at planet scale: 0011) |
| [0011](0011-planet-scale-world.md) | Planet-scale world: an 8,192 km disc, 8,192 m tall, with double-precision physics | Accepted |
| [0012](0012-lod-octree.md) | Whole-world view: a 3D level-of-detail octree, generated on the client | Accepted |
| [0013](0013-master-server-on-cloudflare.md) | Master server on Cloudflare Workers + Durable Objects; Cloudflare TURN | Accepted |

Phase numbers in ADRs follow the implementation plan as it was then. In ADRs 0001–0010, Phases
4–7 became 5–8 when the world-LOD phase (Phase 4) was added on 2026-09-29, and then 6–9 when the
multiplayer-ready phase (Phase 5) was added on 2026-09-30.

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
