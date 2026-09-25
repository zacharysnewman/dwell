# Architecture Decision Records

One file per decision: `NNNN-short-title.md`, numbered in order. ADRs are immutable once
accepted; to change a decision, add a new ADR that supersedes the old one and update the old
one's status line. Resolving an item in `ARCHITECTURE.md` §11 (Open Decisions) requires an ADR.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-webtransport-server-library.md) | WebTransport server library: Rust `wtransport` behind a C ABI | Accepted |
| [0002](0002-client-renderer.md) | Client renderer: Three.js on WebGL2, behind a thin render interface | Accepted |

## Template

```markdown
# NNNN. Title

- Status: Proposed | Accepted | Superseded by NNNN
- Date: YYYY-MM-DD
- Resolves: ARCHITECTURE.md §11 open decision #N (if any)

## Context
What forces are at play and what problem needs deciding.

## Options considered
Each option with its pros and cons.

## Decision
What we chose, stated plainly.

## Consequences
What becomes easier or harder; follow-up work; how we would reverse it.
```
