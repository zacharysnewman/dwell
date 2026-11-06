# 0062. The water runs off the server's tick thread, by message passing

- Status: Accepted (the owner, 2026-11-06: "Use message passing for now, make a note about future possibly
  using shared memory threads"; the water stays saved with the world); built, Phase 53 G5 (WATER_SURFACE_GRID.md §2d, as built)
- Date: 2026-11-06
- Design: [`WATER_SURFACE_GRID.md`](../WATER_SURFACE_GRID.md) §2d
- Amends: ADR 0060 (where the grid runs)

## Context

The water grid (ADR 0060) steps inside the server's tick (`Server::flow_water`), on the thread that steps
the players and bodies. Water runs on game time, so a water tick is seconds of water; with much water
awake (a dammed river) its substeps took up to 50 ms, and the game stopped and started (BUGS.md #33). The
fix in 0.49.7 caps a tick's work (`stepBudget`): physics keeps 60 Hz and the water runs slower than game
time instead. The owner, 2026-11-06: "it's not really acceptable to slow down water sim all the time".

The water reads the world through a few questions (is a loaded cell solid, open or liquid, and its fill;
which chunks are loaded) and writes back water cells. Nothing else in the tick waits on it: its writes
go out as one modification, its currents are read by swimmers and bodies a tick later at no cost.

In the browser, the local server is one Web Worker (one thread). WASM threads with shared memory need the
page cross-origin isolated (COOP/COEP headers), which GitHub Pages can't set; a second worker with
message passing works everywhere.

## Options

1. **Keep it on the tick thread; a native build only.** Native code is faster per thread than WASM, but
   water still competes with physics; browser worlds (most play) gain nothing.
2. **Its own thread, by message passing** (native: a thread with channels; browser: a second worker).
   Physics never waits for water; water has a core to itself. Edits and writes cross a channel, a tick
   apart. The water keeps its own copy of what it reads of the world (cell kinds), fed by the server.
3. **Its own threads with shared memory** (WASM threads, `SharedArrayBuffer`). No copy of the world, the
   grid could be split across cores; but the page needs cross-origin isolation (a service-worker shim on
   GitHub Pages; native in Electron and Capacitor), and every world access becomes synchronised.
4. **The grid split across many threads by chunk**, on top of 2 or 3: scales with cores for large floods;
   each substep exchanges chunk edges.

## Decision

Option 2. The grid runs behind a **water host** with one interface and three placements: **inline** (on
the calling thread: tests, the world-creation fill, and a fallback), a **native thread**, and a **browser
worker**. The server sends it what changed (chunks loaded and unloaded, as cell kinds; cells edited;
the players; the water time to run) and applies what comes back (water cells written, currents, the
chunks it wants loaded, its saved record) at its tick's start. A write the world has since overtaken (the
cell no longer open or water) is dropped; the edit that overtook it already reached the water. Water
falls behind instead of slowing physics: the time it hasn't run piles up and is run when it catches up,
up to a cap past which it is dropped as now (the budget stays as the worker's own guard, raised). The
water's record stays saved with the world: the server asks for it before a save and writes the newest it
has; closing a world waits for it.

Shared memory (option 3) and splitting the grid by chunk (option 4) are noted as later steps, if one
thread for water isn't enough or the copy of the world costs too much memory.

## Consequences

- Physics keeps 60 Hz whatever the water does; a big flood runs at the water thread's speed.
- The water holds a second copy of what it reads: a byte a cell (solid, open, or water and its fill) per
  loaded chunk, a uniform chunk (all stone, all air) as one value.
- Water writes reach the world a tick (17 ms) later than now; edits reach the water a tick later.
- The world-creation fill (G1b) can run on the same host.
- Reversing it is placing the host inline.
