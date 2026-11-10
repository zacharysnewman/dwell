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
   window above and below the eye, the dots' colours and size; the height shading's default (on); the
   height bands' colour stops (0.50.52, below); the explored cell (8 m); the big map's zoom levels and fog colour.
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
   face) is explored once its centre is within the disc's radius of the player, a **2D distance on
   their face alone** (owner, 2026-11-09: "I just want it as a simple 2D exploration"; 0.50.47): the
   height between them and whether its terrain is loaded do not matter. Before 0.50.47 a cell was
   marked only where the disc's tops found ground inside the scan's height window, so a valley more
   than `scanDownM` below a hilltop or a flight was left a gap. A cell's colour has three sources,
   better replacing worse (`minimap::Source`): the loaded terrain's tops under the disc, averaged;
   else the generator's column top at its centre, from the revealed map's tiles (below; asked for by
   `Explored::ask`, at most `revealed.jobsInFlight` at once, run in the job workers, never on the
   main thread, kept for the session); else `explored.pendingColor`. The disc itself draws a texel
   its scan found nothing in from its explored cell's colour (unless pending; in the height bands by
   the cell's height when the toggle is on),
   so a valley below shows there too. A world
   the client does not generate keeps its pending cells until the terrain is seen. The client
   reports new and visibly changed cells with their colour (`takeExplored`; again as a better
   source changes it), and the shell keeps them in IndexedDB
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
   **Revealed in creative** (owner, 2026-11-08: "big map should be fully explored on creative";
   0.50.46): a mode with the `mapRevealed` ability shows the whole map, no fog. Unexplored cells
   take the terrain generator's colour for their column (the client already runs the generator from
   the seed for its LOD): tiles of `revealed.tileCells`² cells generated in the job workers by the LOD
   colour map's raster (`crates/dwell-client/src/map_reveal.rs`), asked for nearest the view's centre
   first, at most `jobsInFlight` at once, taken at most `budgetMs` a frame (`RevealCache`), so opening
   or zooming the map never hitches. Explored cells draw over them (edits show once explored).
   Chosen over the LOD sections' colours (only near the player) and a coarse world map (too coarse
   close up): the generator covers every distance at the cell's resolution and agrees with the
   explored colours, which come from the same `lod_color`. The tiles are kept for the session only.
   A world the client does not generate (a flat world) has nothing to reveal and stays fogged.
6. **Settings.** The minimap is a part of `shared/ui/hud.json` (Settings → On screen hides it); the
   height shading is a toggle there (`dwell.hud`'s `mapShading`, pinned only when changed).

### The big map panel and elevation shading (0.50.48; owner, 2026-11-09)

"The big map sucks. It takes up the whole screen, doesn't have an obvious close button, and
unintuitive zoom/move controls." The owner's answers: a **framed panel** about 80% of the screen over
the game (the world visible round it) with a title bar; an obvious **✕ close** (plus M and Esc); **+ / −
zoom buttons** (scroll and pinch kept); **"Center on me"**; a **scale bar** in metres; **drag to pan**
with a grab / grabbing cursor. Mouse, touch (pinch, drag, tap) and the controller (stick / D-pad pan,
triggers / bumpers zoom, B closes; View opens and closes it, no longer the F4 debug map). On a phone
the panel is nearly full screen inside the safe area, its buttons big enough to tap. The sizes,
colours and controller buttons are data (`bigMap.*` in `shared/ui/map.json`); the controls' logic is
a page-free `BigMapState` / `PadMap` (`client/src/ui/minimap.ts`), tested without a page.

"The world map also is almost all one color, we should do shading based on elevation." Each explored
cell carries a height from the same source as its colour (the loaded terrain's tops averaged, the
generator's column top in the reveal/explore tiles — `Job::LodMap { heights }` appends i16 height
codes — else none, flat): a code of half-metres times two plus a water bit. The big map's tiles are
shaded by `dwell_game::minimap::shade_rgb`, the one shading function the minimap's height-shading
toggle also uses: a Lambert hill-shade of the screen gradient from neighbouring cells, lit from a
light direction that is data (`shade.lightAzimuthDeg`, `lightElevationDeg`, `exaggeration`,
`strength`; flat ground unchanged), then a gentle tint by height above sea level (`shade.tint*`);
water keeps its colour. The gradient is in screen terms, so the light comes from the screen's top left
on both faces. Stored records gain an optional `heights` map beside `cells` (append-only).

### Height bands instead of the hill-shade (0.50.52; owner, 2026-11-09)

After seeing 0.50.48: "That looks like lighting, not elevation shading? Is that a height map? It
doesn't look like one." The owner's answers: **colour bands by height** (a classic topographic,
hypsometric tint: low ground dark green, rising through lighter greens and tans to grey and white
peaks; water in blues by depth) and **no lighting**. So the hill-shade and the tint (`shade.*`,
`relief`, `tint`, `shade_rgb`) are gone, and the eye-relative brightness of the minimap's toggle
with them (`heightShading.rangeM`, `darkest`, `brightest`). `dwell_game::minimap::band_rgb` colours
a cell by its height alone: land by its height above sea level through `heightBands.land`, a list of
`{heightM, color}` stops, interpolated between them and held past the ends, the height first
rounded down to a multiple of `heightBands.stepM` (8 m: gentle steps, so a slope reads as contour-like
bands); water by its bed's depth below sea level through `heightBands.water` (`{depthM, color}`,
`waterStepM` 0, smooth), water above sea level (a river or a lake in the hills) the shallowest stop.
The block colour and the slope play no part. A water cell's height code is now its **bed's** (the
generator's column ground, a river's channel bottom; on the loaded terrain the scan's surface less
the depth it followed, up to `heightShading.waterDepthM`), so the depth comes from the one height;
codes stored by 0.50.48–0.50.51 (the water's surface) read as shallow water. The big map uses the
bands always; the minimap keeps its blocks' colours, and its height-shading toggle switches it to the
same bands, to compare. `shared/ui/gen.mjs` validates the stops (at least one, rising, colours,
ranges, unknown fields) and emits them as Rust slices and TS arrays; a stop is added by data alone.
The stops were picked for the generator's range (sampled at seeds 0 and 7, face A): the ground round
spawn is mostly 280–330 m above sea level (seed 7's hills to ~850 m), the whole disc 0–2600 m, the
sea at most ~22 m deep; so the stops are dense between 200 and 400 m (a change every 40–60 m), then
wider to the peaks.

### Five zoom levels, tiles built once, block colours tinted by height (0.50.61; owner, 2026-11-09)

"The big map should have at most 5 zoom levels. The most zoomed out should be the entire
world/island." "The closest zoom should be the mini map size." "If the map loads I'd love if it only
did it once. And preferably invisibly to you." And, after the bands hid the sand: block colours,
lightly tinted by height. So:

- **Levels as data** (`bigMap.levels`, 2–5 `{cellM}`, closest first; `startLevel`): each level shows a
  fixed span across the map's short side, computed by `shared/ui/gen.mjs` — the closest the
  minimap's diameter (128 m), the widest the island's (`islandRadius` × 2, 50 km), the rest spaced
  geometrically (569 m, 2.5 km, 11 km) — and has its **own cell size** (1, 2, 8, 32, 128 m: 1 m a cell
  up close; a whole power of two, rising). + / −, the keys, the wheel (`wheelStepPx` of scroll a
  level), a pinch (`pinchStep` of spread a level) and a controller zoom-button press step one level,
  clamped at the ends (no free zoom; `padZoomPerS` and the px-per-cell range are gone). The widest
  level is centred on the island.
- **Tiles per level, built once.** A tile is `revealed.tileCells`² cells of one level, the same
  `Job::LodMap` raster as before at the level's cell size (`requestReveal(face, ti, tk, cellM)`; the
  tile's bytes now carry its cell size). `RevealCache` keeps them per world **and compatibility
  line** in this browser (IndexedDB `dwell-map-tiles`, a new database, key `<world>|<line>|<tileCells>|<cellM>|<face>,<ti>,<tk>`,
  each tile written once: append-only; a new line builds its own once, since the generated terrain
  is the line's. Amended 2026-11-09, 0.50.75, owner: "rebuild once per compatibility line change";
  from 0.50.61 to 0.50.74 the key held the build's version, and those keys are left in the store). Once the kept tiles are read, the **background** builds each level's
  tiles within `prebuildSpans` (2.5) of its span round the player (the widest level's the whole
  island), clipped to the island, the start level first, `backgroundJobs` (1) at a time in the job
  workers, while the map is shut or its view is whole; the open view's missing tiles still come
  first (`jobsInFlight`). A world the client does not generate (a native server) stops the
  background at its first empty tile. Measured (Chrome, a Mac): a fresh seed 0 world builds its
  ~1600 tiles in ~65 s of background; a reload builds nothing (`__dwell.mapTiles()`).
- **Fog per level** (`ExploredLevels`): adventure fogs a level's cell unless an explored 8 m cell
  covers it (a finer level) or lies in it (a coarser one). Explored ground shows the generator's
  detail at levels finer than the explored cells and the explored cell's own colour (the loaded
  terrain's, with edits) at 8 m and coarser; an explored cell's change makes its tiles at every
  level stale (redrawn, not regenerated). Edits finer than 8 m do not show at the 1 and 2 m levels
  (the generator's ground stands there).
- **The look** (`heightTint`, replacing `heightBands`): each cell its block colour scaled in
  brightness by its height above sea level, `darkest` (0.8) at `lowM` (0) to `brightest` (1.15) at
  `highM` (400 m), linear between, so sand stays sand and grass grass; water keeps its colour (already
  darkened by depth). `dwell_game::minimap::tint_rgb`; the minimap's height-shading toggle shows the
  same tinted look (off: plain block colours). Minecraft's per-pixel stair-step shading is left for
  later (docs/FUTURE.md).

### The widest level is the whole world, its sea included (0.50.63; owner, 2026-11-09)

"The full zoom doesn't show the whole world surface" … "Including all the water". 0.50.61's widest
level spanned the island's *mean* diameter (`islandRadius` × 2, 50 km), but the island's coast reaches
well past it (the radius noise, the warp and the coast detail add up to ~10 km), so the island ran off
the panel; and its tiles were built only inside the 25 km circle, so the drawn ground ended in square
steps against the fog where the sea should go on. So:

- **The widest span is the world disc's** (`worldRadius` × 2, 100 km, `shared/protocol/constants.json`),
  computed by `shared/ui/gen.mjs` (`levelSpans`, `WORLD_RADIUS_M`), across the panel's **short side**
  (`levelPxPerM`), so the whole disc fits a wide desktop panel and a phone either way up; centred on
  the world. The levels between stay geometric (677 m, 3.6 km, 18.9 km).
- **Tiles are clipped to the world disc, not the island** (`RevealCache.onWorld`): the sea's tiles are
  built like the land's, at every level, so no level's view or background square has holes inside the
  world. Off the disc the panel shows the fog colour (`bigMap.fogColor`), the world's edge reading as
  a dark rim round the round sea.
- **Coarser cells to keep the build small** (`bigMap.levels`, 1, 2, **16, 64, 256** m, from 1, 2, 8, 32,
  128): the widest level at 256 m covers the whole disc in ~150 tiles (8 km a tile), as many as the
  island took at 128 m; the middle levels' longer spans at 16 and 64 m keep ~220–300 cells across the
  short side. A background build at the origin is ~1950 tiles (it was ~2300 clipped to the island).
  Kept tiles of the old cell sizes are never read again (their key carries the cell size); a new build
  makes its own anyway.

### Amendment (0.50.76): the closest levels' real blocks

Owner, 2026-11-09: "Yes big map closest zooms should use 1m". The 0.50.61 report found levels 0–1 almost
flat green: the generator's map colours have no trees, no single blocks and no edits.

- **Loaded terrain first.** A level whose `cellM` is at most `revealed.liveMaxCellM` (2: the 1 m and
  2 m levels) is drawn, while the map is open, from the **loaded terrain** as the minimap reads it:
  `dwell_game::minimap::scan_tile` (the minimap's column read, `column_top`, shared) gives each cell
  its column's top block at the cell's centre, its LOD colour (grass, sand, water darkened by depth,
  leaves, paths, the player's builds and edits), and its height code, so the light height tint
  (`heightTint`) applies as before. The wasm `liveTile(face, ti, tk, cellM)` returns it in
  `takeReveal`'s bytes, alpha 0 where nothing is loaded; `map_reveal::live_tile`.
- **The generator's tile shows elsewhere.** `RevealCache.live` holds those tiles in memory only (never
  the store: they change with edits); a cell with a loaded top shows over the generator's tile, the
  rest (not loaded, beyond the streamed chunks) is the generator's tile at that cell size as before.
  A top whose cell above starts a chunk not held is **unknown**, not the top (the browser check showed
  a hill's stone where its grass chunk had not streamed yet), so it falls back too.
- **Budget.** `RevealCache.pumpLive` reads at most `revealed.liveTilesPerFrame` (2) tiles a frame, the
  nearest the view's centre first, each again once `revealed.liveRefreshMs` (2 s) old, which is how an
  edit shows; a tile that reads the same is not redrawn. A 32² tile is 1024 columns, the minimap's
  read is ~16k (~5 ms in the browser), so a frame's reads are well under a millisecond
  (DEV_TOOLS.md `map.liveTile`). Nothing is read while the map is shut or at the coarser levels.
- **Fog is unchanged**: adventure fogs unexplored cells at every level; creative reveals all.
- **Not done:** the generator's fallback at these levels is still its map colour (the LOD raster's
  column top), without trees; a top-block sampler for the tree layer was not cheap enough to add in
  this change. Beyond the loaded chunks the 1–2 m levels still read as plain ground.

### Amendment (0.52.10): the minimap always 1 m and whole; the open map keeps building

Owner, 2026-11-10: "The mini map gets all fragmented when moving fast, it should always show the full
width and height of what's around you, it's leaving black edges", "the mini map is showing a coarse
view sometimes. It shouldn't ever be coarse", and of the big map: "Preferably it should do that until
it's done, while the map is open."

- **The minimap's fill is the generator's 1 m tops.** Where the scan found no top (columns not
  streamed yet when moving fast, LOD-only ground, or ground past the scan's reach) the disc drew the
  8 m explored cell's colour (coarse), or nothing (black) until that cell's 8 m tile came. It now draws
  `dwell_game::minimap::Generated`: the big map's closest level's tiles (`bigMap.levels[0]`, 1 m cells,
  the generator's top block and height, the tiles the big map already builds once per world and line
  and keeps in `dwell-map-tiles`). The client names the tiles it lacks within the disc's radius and one
  tile (32 m) of the player, nearest first (`minimapWants`), and lets go of those three tiles past it;
  the shell hands each over from its cache (`minimapTile`) or asks for it before the open view's and
  the background's (`RevealCache.feedMinimap`, within `revealed.jobsInFlight`). A tile's arrival
  redraws the disc. Loaded columns override it as before (real blocks, edits); the explored cells are
  still marked and coloured as before for the big map's fog, but the disc never reads them.
- **Fog** on the minimap is unchanged: the disc is always explored ground (its cells are explored as it
  covers them).
- **The open map keeps building.** Before, with the map open, the background ran round the player
  only, one job at a time, the start level first. Now, once the view's tiles are asked for, it builds
  every level's tiles within `prebuildSpans` round the view's centre (the open level first, then
  outward to the finer and coarser levels) and then round the player, at `jobsInFlight`, until none in
  range is left, with a quiet "Mapping · N tiles" count in the panel's corner while any are.
- **Tests.** `minimap_tests.rs` `the_disc_shows_unloaded_ground_at_one_metre_never_coarse_or_black`
  (both faces: a disc with 20 m loaded, every pixel inside drawn, each unloaded one its own metre's
  generated colour, none an explored cell's; red on the old fill), `minimap.test.ts` "the minimap and
  the open map keep building" (the 1 m tiles asked first and handed over, kept ones without a build;
  every level's tiles round a panned view built over frames until none remain; red before).
- **Cost.** The bench flying at 64 m/s (scratch scenario): `minimap.scan` p50 1.2 ms after vs 1.5 ms
  before, `minimap.draw` ~0.5 ms; frame CPU p50 3.6 vs 3.5 ms (noise). The fill is a hash lookup a
  pixel where the scan found nothing.

### Amendment (0.52.13): three levels, the coarse ones built whole

Owner, 2026-11-10, of 0.52.10's open-map build: "It's not continuing to map sections you can't see
while you're not there. Ideally it should expand out from where you are looking. Or if you're on a
higher zoom level that is already loaded, it should be loading the finer ones." A tandem build by
shares across five levels was agreed, then replaced the same day: "Maybe we just have 3 zoom levels.
The two coarse ones load along with the area around you in 1m. The 1m is always streamed."

- **Three levels** (`bigMap.levels`, data): 1 m (the minimap's area), 16 m (3.6 km across, geometric
  between) and 256 m (the whole world disc). The 2 and 64 m levels are gone; their kept tiles stay in
  the store unread (keyed by cell size, append-only). `startLevel` 1 (16 m); `liveMaxCellM` 1.
- **Each level's `reach`** (data: `player`, `island`, `disc`; `shared/ui/gen.mjs` validates it)
  says where the background builds it, map open or shut: the 1 m level within `prebuildSpans` of its
  span round the player (always streamed; while the map is open the loaded terrain shows over it,
  0.50.76), the 16 m level every tile touching the island (`islandRadius` 25 km: ~7 500 tiles; 32 m
  was not needed), the 256 m level every tile touching the world disc (~120 tiles).
- **Order.** The open view's tiles are all asked for before anything else; then the player level, then
  the coarse levels coarsest first (the whole world in ~120 tiles, then the island's detail), each
  nearest the open view's centre (else the player) first, listed again when that centre moves four of
  the level's tiles (panning re-centres it). `jobsInFlight` at once while the map is open,
  `backgroundJobs` while it is shut; at most 4 096 kept tiles passed over a frame (a reopened world).
- **The count** ("Mapping · N tiles") is each coarse level's reach total less its kept tiles (counted as
  they arrive or are read, no scan) plus the player level's missing ones.
- **Tests.** `minimap.test.ts`: three levels and their reaches (the 1 m round the player, the coarse
  ones the whole island and the whole disc, in that order); with the map open the view's tiles all
  asked first, then the 1 m round the player, then the disc, then the island nearest the view, until
  none is left; panning re-centres the island build; shut, it carries on round the player a job at a
  time. `gen.test.mjs`: a bad `reach` fails naming it.
- **Cost** (bench, M1 Air, seed 7, a fresh world and tile store, unthrottled; 60 s walking with the map
  shut, default against `?mapbuild=off`): frame interval p50 3.5 / 3.4 ms, p95 and p99 18.7 / 18.8 ms
  both, max 35.2 / 18.8 ms (one frame with the build on, its time in `ticks`, 32.7 ms, not the map's
  pump); `frame.cpu` mean 1.92 ms both, p95 3.2 both. In the workers a tile is `job.reveal.run` p50
  6.1 ms (2 766 built in the run); chunk generation ran p50 13.9 vs 11.8 ms (sharing the cores) but
  waited no longer (queued p95 15.4 vs 15.6 ms), meshing the same (queued p95 13.7 vs 14.3 ms). The
  whole island takes ~3.5 min with the map shut (one job at a time, ~35 tiles a second) and under a
  minute open (two at once: 7 317 left at opening, 2 636 after 30 s). Building the 256 m level on the
  loading screen from the world map was weighed and not done: it is ~120 tiles, built a few seconds
  after the join behind the 1 m area, and the generator's tiles already read its LOD columns.

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
