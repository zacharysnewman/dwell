# 0072 — Octagonal logs: a cross-section shape kind, squared where logs join

**Status:** Accepted (2026-11-09; T1 built in 0.51.0, a new compatibility line; T2's felling as
wedges built in 0.51.0; T4, the wood ramps as octagons and felled bodies drawn so, built in 0.51.0;
outstanding: the bodies' octagon colliders switched on, the carved notch on the octagon). Builds on ADR 0042 (½ m cells, stored
shapes), ADR 0067 (chopping) and docs/SLOPE_BLOCKS.md (the shape mechanism).

## Context

Owner (2026-11-09): "trees should be octagons around. But smoothly connect when placed next to
each other, full width sides where they meet." A real shape, with collision (owner's choice), not a
look only. Logs are the only blocks today; the mechanism must take more (a post, a beam, a mod's
wood) by data.

What gets added later: more blocks with a cross-section (data: a block's `crossSection`), other
chamfers, other join groups (two woods that join each other). Fixed in code: the kind (`octagon`),
the join rule (same group, same axis), the geometry.

## Options considered

1. **Stored per state** (16 join combinations × 3 axes as states, or a `joined` property). Every
   placement and every neighbour's edit would rewrite states; more states, the shapes table grows,
   and changing the rule later is a state change (breaking). Rejected.
2. **A look only** (draw the octagon, collide as the cube). Cheap, but the owner chose a real shape.
3. **Derived from the neighbours where the geometry is built** (chosen): the block's data names the
   cross-section; the mesher and the terrain collision read the four neighbours across the axis and
   square the sides that join. No new states.

## Decision

1. **Data.** A block may have `crossSection: { kind: "octagon", chamfer, joins }`
   (`shared/blocks/*.json`): `chamfer` the corners' cut as a fraction of the cross-section's
   shorter side (in metres, so the cut stays 45° on ½ m cells; `dwell:log` uses 1/(2+√2) ≈ 0.293,
   a regular octagon on a 1 m section), `joins` a join-group name (`dwell:wood`). `gen.mjs` validates it
   (a full, cube-look block with an `axisProperty`; chamfer in (0, ½); unknown fields fail) and emits
   `StateInfo::cross_section`; the fixture's `test:post` (chamfer 0.15, the logs' group) shows a new
   entry working by data alone. The registry sync carries it as a rule field (tag 13).
2. **Join rule.** Two cells join when both have a cross-section of the same `joins` group along
   the same axis (`dwell_blocks::octagon::joins`). For each side across the axis whose neighbour
   joins, the side is full width: both its corners square off to the cell's edge. A corner is cut
   only when neither of its sides joins.
   **Lying logs' halves** (0.52.2, BUGS.md #63): a 1 m log lying along x or z is two ½ m cells,
   its lower and upper half, each knowing which by the log cells it joins above and below (none is
   stored). Across a level side a cell joins only a cell of the same half (both lower, both upper,
   both lone or both in the middle of a stack), so a log lying a row higher or lower beside it stays
   its own octagon, not its upper half squared against the other's lower one. The rule reads one
   cell up and down beside the cell too (offsets on up to three axes, inside the mesher's apron).
3. **One geometry** (`dwell_blocks::octagon`): `outline` (the cross-section), `faces` (the prism:
   two caps, the sides, the chamfers) and `open_faces` (culled: a joined side is shared and drawn by
   neither cell; a side or cap against a plain opaque cube is hidden; a cap against a joined log
   along the axis is hidden when that log's squared corners include this one's). Mesher and
   collision both call it, so what is drawn is what is collided with. The shape is symmetric in y,
   so face B (meshed and collided mirrored) needs no case of its own.
4. **Mesher.** An octagonal cell is drawn whole (`octagon_voxel`), not in the greedy slices: bark
   (the side tile) on the sides and chamfers, end grain on the caps, lit by the sky its sides see.
   It covers no neighbour's face (its corners are open).
4a. **The caps' bark ring** (Phase 55 T3; owner, 2026-11-09: "the outside bark ring should follow
   the octagon, it doesn't, it gets cut off on the corners"). The square end-grain tile's painted
   edge was clipped at the chamfers. Now the ring is geometry: `crossSection.ring` (its width, a
   fraction of the shorter side, in metres so it is even on ½ m cells; validated in (0, ¼)) and
   `crossSection.heart` (the tile inside it, a ringless end grain: `logHeart`; the cap's own tile
   when absent). `octagon::cap_pieces` moves each edge of the outline in by the ring's width, the
   heart being the moved outline, and lays a bark band (the side tile) along each edge that is not
   joined, chamfers included; along a joined side there is none and the heart reaches the cell's
   edge, so a joined wall's top reads as continuous end grain, not a ring per cell. Collision keeps
   the whole cap. Sync: tag 13 carries the ring and the heart after the chamfer and the group.
5. **Collision.** Octagonal cells go into the chunk's triangle mesh with the shaped cells (not the
   voxel collider), so a player standing on a lone log's cut corner slides off, and joined logs are
   flat across. A chunk with logs is rebuilt when a chunk round it changes, as shaped ones are.
6. **Compatibility.** Logs collide differently, so 0.51.0 starts a new compatibility line (owner,
   2026-11-09); worlds from 0.50 open after the version warning (ADR 0061).

## Consequences

- No new states; the rule and the chamfer are data. Targeting, the selection box and the LOD still
  treat a log as its cube (the LOD at distance cannot tell).
- **T2:** felling as wedges is built (0.51.0; ADR 0067 §5a): the cut cell becomes a wood ramp
  stump and the log above an upside-down wood ramp under the trunk, released with no push, tipping
  toward the notch.
- **T4** (0.51.0; owner, 2026-11-09: "everything log/tree should be octagonal"): pieces are meshed
  by the terrain mesher, so felled logs draw the same octagons and joins. A piece cell whose hull is
  a log is its octagon prism (`merged_boxes` joins a cell's sides to the same axis's logs beside it
  in the piece, as the terrain's, and merges a run of one log along its axis into one prism,
  `PieceCell::joined`, `octagon_hull_points`), built and tested, but `piece_hull` does not give a
  log that hull yet: with it, a player's push no longer slid a log on land (the netcode push test:
  0 m square on, from 1.5–4 m as a box), cause not found. A shaped state may carry a cross-section (the wood ramp's `crossSection: "of"`
  takes the log's): `octagon::state_faces` clips the prism to the state's convex shape (the slant a
  `Part::Cut`, drawn as end grain), so a wood ramp is the octagon cut by its slant in the mesh and
  the terrain's collision; a non-convex shape stays square (`section_of`). A cap is hidden only
  against a neighbour's *whole* cap (`cap_whole`), and sides join only cells of one shape. Deviation:
  the trunk's riding wedge, a piece cell, keeps its square ramp's hull (with the octagon's narrower
  hinge the felling test's trunk slid off the stump instead of tipping into the notch); it is drawn
  octagonal. Outstanding: the carved notch on an octagon (`crates/dwell-mesh/src/carve.rs` still
  carves a cut log as a cube).
