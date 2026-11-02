# 0052. Built bodies: editable pieces, the build platform, saved and floating

- Status: Accepted (the owner's answers, 2026-11-02); built in stages B1–B3
- Date: 2026-11-02
- Design: [`BUILT_BODIES.md`](../BUILT_BODIES.md); builds on ADR 0051 (felling)

## Context

The owner (2026-11-02) wants dynamic voxel structures off the world grid: moving pieces (trees)
edited while they move, and a "build platform" block whose builds are moving objects that never
settle (a boat, a cart). Felling (ADR 0051) already runs pieces as server Rapier bodies of voxel
cells, replicated to clients and ridden by players; they cannot be edited, always settle and are
not saved.

## Options

1. **Edit the falling pieces only**; no built objects. Small, but not what was asked.
2. **Body kinds as data on the existing pieces** (the owner's answers): `natural` pieces (break
   only, settle) and `built` ones (break and place, never settle), splits for both, edits by body
   id and the body's own cell grid, bodies saved with the world, buoyancy per cell.
3. **A separate vehicle system** (bodies of their own, not pieces). Duplicates the piece pipeline
   (meshing, replication, prediction stand-ins, collision).

## Decision

Option 2. Each body has a kind whose rules are data (`shared/physics/bodies.json`: place, break,
split, settles, saved). A build platform block (`"support": "platform"`) starts a built body when
placed, a body from the start (owner); blocks placed against a built body join it. Clients target
bodies by a ray in each body's frame and send `BodyEditRequest`; the server applies it, splits
what a break disconnects, and broadcasts `BodyEdit`. Bodies are saved with the world. Bodies float:
buoyancy per cell by the water it is under and its block's density. Built in three releases,
B1–B3; driving is later.

## Consequences

- New protocol messages (`BodyEditRequest`, `BodyEdit`, the body kind in `EntitySpawn`) and a new
  saved-world record (bodies): a new compatibility line (0.41.0, the owner raises it), opened at B1.
- Built bodies persist and never settle, so a world's body count grows: a limit (`maxBodies`) and,
  later, interest management (bodies sent only to players near them).
- Bodies far from every player are frozen until the terrain round them loads.
- Reversal: set the `built` kind's `place` false and remove the platform item; existing built bodies
  stay in saved worlds (they could be settled into the grid by a migration).
