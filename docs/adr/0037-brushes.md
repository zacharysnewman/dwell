# 0037. Brushes: VoxelSniper-style painting and sculpting over region-edit pastes

- Status: Accepted
- Date: 2026-10-15
- Builds on: [0031](0031-data-driven-structures.md) (the sandbox world type, the region edits of S3),
  [0014](0014-versioned-releases.md) (compatibility lines; additions within a line by Welcome flags)
- Design: [`BRUSHES.md`](../BRUSHES.md)

## Context

The owner (2026-10-15) asked for painting tools in the sandbox like VoxelSniper's. VoxelSniper is a
long-range brush editor: a brush (a shape) and a performer (what it does) applied at the block
the player looks at, from far away. Dwell already had region edits, `RegionEdit` with fill, hollow,
replace and paste up to 32³. They are server-validated and rate-limited, and negotiated by the
`regionEdits` Welcome flag. Its region tools were box-based, with a 64 m reach.

## Options considered

- **The engine**:
  - hardcoded tools (rejected: CLAUDE.md requires an extendable system);
  - a fixed vocabulary of brushes and performers, with named presets as data (chosen);
  - fully scriptable brushes (rejected: no one asked, and validation would be much harder).
- **The wire**:
  - **A new brush message** (rejected): the server would have to run the brush and re-validate a
    shape, which means more protocol, and so a new compatibility line, or another flag and a server
    engine.
  - **Plain region pastes of the client's current states** (rejected): a paste over the sandbox's
    unbreakable floor is rejected. The pastes would also overwrite other players' concurrent
    changes, and the server would rewrite cells the brush never touched.
  - **Pastes with a `KEEP` cell value** (chosen): the client computes the stroke, and the server
    validates each paste as it validates any region edit. It is additive, gated by a `brushes`
    Welcome flag. Older servers reject the unknown state, so a stroke sent by mistake fails safe.
- **The reach**:
  - a global longer reach (rejected: survival worlds would gain 128 m edits);
  - per world type, as data (chosen): only types whose policy enables brushes, today `sandbox`.

## Decision

- **The engine.** `dwell-world` `brush.rs` holds the vocabulary:
  - brushes: ball, disc, cylinder, cube, ellipsoid, splatter, line;
  - performers: set, replace, overlay, erase, smooth, erode, flatten, raise, lower;
  - masks: none, air, solid, target.

  A stroke is a pure function of the brush, the aim and the world grid. Splatter is seeded by the
  preset and the centre.
- **The data.** `shared/brushes/brushes.json` holds the presets (name, label, brush, performer,
  size, an optional free key, parameters) and the per-world-type policy (`brushes`, `reach`).
  `gen.mjs` validates it and writes Rust and TypeScript tables, and CI fails on stale output.
- **The wire.** A stroke is sent as `RegionEdit` pastes of at most 32³ cells, each trimmed to its
  changed cells, with the other cells `region_edit::KEEP` (0xFFFF). The server skips `KEEP` cells.
  It sets `welcome_flags::BRUSHES` (4) in world types whose policy allows brushes. There, its
  region-edit reach is the policy's reach plus the slack. Clients send `KEEP` only with that flag.
  There is no `protocolVersion` bump and no new message.
- **Client behaviour.** The client mirrors the server's cell bucket and paces a stroke's edits.
  A stroke is one undo step, however many edits it took. With `KEEP` available, undo keeps
  unbreakable cells as `KEEP`.

## Consequences

- **More content is data.** A new preset is a data entry. A new brush or performer is code plus one
  entry in `gen.mjs`'s vocabulary.
- **The server stays authoritative.** Every cell a stroke changes passes the region-edit checks:
  unbreakable cells, players, bounds, permission, reach and rate. The server never runs a brush.
- **Large strokes arrive late.** A size-16 stroke is about 36,000 cells, half a second of the rate,
  and appears about a round trip later, since nothing is predicted. Prediction could be added
  client-side later without protocol changes.
- **Open questions.** Brushes in other world types would need a permission decision (open
  questions, `BRUSHES.md` §5).
- **Reversal.** To remove brushes, drop the flag and the policy. Servers that keep accepting
  `KEEP` cells harm no one.
