# 0068 — The minimap and the explored map, explored cells kept in the browser

**Status:** Accepted (2026-11-08; built in 0.50.37).

## Context

The owner (2026-11-08): a **round minimap** in the top right, in creative and adventure (a mode
ability so a future mode can turn it off), hideable from Settings → On screen; it always turns so
the player's facing is up, the player a red dot in the middle; terrain colours from above and other
players as dots of another colour; height shading as a **toggle** to compare; 64 m radius to start.
Whatever has appeared on it is **explored** — per player, per world, saved (a coarse grid, e.g. 8 m
cells) — and **M** or a tap on the minimap opens a big **zoomable, pannable map** of what the player
has explored (the rest hidden), with their place and facing. No north marker yet (a later compass).
Cheap: drawn from data the client already has, into a small image, updated only on a move or a turn.

Answers to the design questions (owner's brief): what is added later is looks and sizes (data), a
compass, perhaps more map layers; tuned by editing a data file; the explored record is per player per
world; the minimap is per build, its looks the same for every world.

## Decision

1. **Data.** `shared/ui/map.json`: the minimap's radius (64 m), texel (1 m), size (168 px), redraw
   thresholds (a texel's move, 2° of turn), the full re-read interval (2 s, for edits), the scan
   window above and below the eye, the dots' colours and size; the height shading's default (on), its
   range and strength; the explored cell (8 m); the big map's zoom range and step and fog colour.
   `shared/ui/gen.mjs` (with the HUD's parts) validates it (ranges, colours, unknown and missing
   fields, a message naming the entry) and emits `crates/dwell-game/src/map.gen.rs` and
   `client/src/ui/map.gen.ts`; CI regenerates and fails on a diff; `gen.test.mjs` changes a look by
   data alone.
2. **The minimap is the client crate's** (`dwell-game` `minimap.rs`, platform-free and tested
   natively): it reads each column's top within the disc from the client's streamed voxels (its
   colour: the block's LOD colour, `dwell_mesh::lod_color`; water darkened by its depth; a hill over
   the scan's top followed up to its surface), on the player's own face through `FaceFrame` (face B's
   ground on face B), into a face-local grid; a move of a texel reads it again starting a little over
   each column's last top (a whole read every 2 s), and the round RGBA disc is drawn from the grid
   turned so the heading is up and the player's right is right (on face B the mirror makes the
   face-local frame's left the player's right). The shell (`client/src/ui/minimap.ts`) puts it on a
   canvas with the dots (the player red; others, on the same face and inside the disc, blue).
3. **The map ability.** `map` joins the modes' fixed vocabulary (`dwell_protocol::modes::Ability`);
   creative and adventure both have it. The client hides the minimap and the big map in a mode
   without it (`Client::mapAllowed`); nothing on the server. A server that names no mode shows them.
4. **Explored cells are kept by the client, in this browser** (not the server). A cell (8 m, per
   face) is explored once its centre has been inside the disc; the client reports new and visibly
   changed cells with their average colour (`takeExplored`), and the shell keeps them in IndexedDB
   (`dwell-explored`, one record per world: a local world by its file, a remote one by how it was
   reached, the same key the registry's trust uses), saved every 10 s and on leaving the page. The
   record is **append-only** (`format`, `cellM`, `cells` by `"face,i,k"`): unknown fields and other
   builds' cells are kept on rewrite, a record of another cell size is left and ours kept beside it.
   *Why the client:* the minimap is the client's (it draws what the client has streamed); per-player
   storage on the server would need player identities that persist across sessions and hosts (a
   player's device key is per browser anyway), new messages and a world-format table, and a
   dedicated server would hold every visitor's map. In the browser it costs nothing on the wire or in
   the save, survives version changes (the store is shared and append-only), and the map works the
   same on a friend's or a dedicated server. *Cost:* the map is per browser — another device starts
   blank — and lost with the browser's storage. If the map ever needs to follow a player across
   devices it moves to the server as an optional world table keyed by the player's identity.
5. **The big map** (`MapUi`): M or a tap on the minimap opens it (pointer lock released), M or Esc
   closes it; the explored cells drawn as squares of their colour on the fog, the player an arrow at
   their heading; the wheel zooms about the pointer, a drag pans. It is drawn with the same handedness
   as the minimap (a yaw-0 player's right is right), so face B reads as the player sees it.
6. **Settings.** The minimap is a part of `shared/ui/hud.json` (Settings → On screen hides it); the
   height shading is a toggle there (`dwell.hud`'s `mapShading`, pinned only when changed).

## Consequences

- The minimap's cost, measured in the browser (Chrome, a Mac's GPU; the bench flying at 8 m/s for
  12 s, DEV_TOOLS.md `minimap.scan`): 152 reads, mean 1.0 ms each with its draw (p50 0.6, p95
  1.7), ~13 ms a second in all. A move keeps the columns it had and reads only the strip it
  uncovers; the refresh every 2 s reads every column from its last top, ~5–9 ms on its frame (a whole
  read from the scan's top was ~6 ms, every texel moved, before). A draw alone (a turn) is under
  1 ms. A slower device could refresh less often by data (`refreshSeconds`).
- What the minimap shows is what the client has streamed: unloaded ground is empty, beyond the view
  nothing. The LOD colour map (`?lodmap=on`) could later feed a wider map.
- A compass, waypoints or more layers are additions to the data and the shell's map; a mode without
  the map is a data change.
