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
