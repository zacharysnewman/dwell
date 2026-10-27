# 0051. Felling: support as block data, pieces as server bodies

- Status: Accepted (the owner's answers, 2026-10-27)
- Date: 2026-10-27
- Design: [`FELLING.md`](../FELLING.md); builds on ARCHITECTURE.md §7.1–7.3 (Phases 20, 22)

## Context

The owner (2026-10-27): trees should be cut down, "any piece should become a separate entity when
disconnected from their root, fall, tipping over, and then settling back to a static block". The
collapse pipeline (integrity, clustering, awakening, re-bake) is designed in §7 but unbuilt; the
physics player controller already rides moving bodies.

## Options

1. **Trees only, hardcoded** (logs and leaves by name). Quick; the next collapse feature starts over.
2. **A general support rule as block data, trees opted in** (the owner's choice). The same integrity
   and pieces serve terrain and builds later by data alone.
3. **Everything at once** (full Phase 20): overhangs and builds fall too.

For the simulation: each client locally (cheap, divergent) or server-run (§7's plan, the owner's
choice).

## Decision

Option 2, server-run. Each block has a `support` role (`ground` default, `carrier`, `rider`) and a
`passable` flag; logs carry, leaves ride and are passable to players (the owner). A removal runs a
budgeted integrity pass over carriers and riders; each unanchored component becomes one Rapier body
(colliders for carriers only) replicated to clients, which interpolate it and collide with it
kinematically; at rest it snaps to the grid and becomes blocks again, riders in taken cells
destroyed (the owner). Built in three releases, F1–F3.

## Consequences

- New protocol messages (entity spawn, despawn, poses): a new protocol version and compatibility line
  at F2.
- Leaves stop being solid to players at F1 (the owner).
- Terrain and builds can opt in later by data; large structures, crush and explosions stay Phases
  20–21's.
- Reversal: set every block's `support` to `ground`; nothing falls.
