# Water grid benchmark (Phase 53 G0)

The surface grid of [ADR 0060](../../adr/0060-water-surface-grid.md) in native Rust, run on the game's
own terrain (`dwell-worldgen`): sparse 16 × 16 chunks, rayon, sleeping chunks with the band and smooth
waking, still water as pools, held sea outlets. Not part of the game or its workspace; results in
[`WATER_MODEL_EVALUATION.md`](../../proposals/WATER_MODEL_EVALUATION.md) §5.9.

    cargo build --release --bin world          # RUSTFLAGS="-C target-cpu=native" for speed

`world SEED SECONDS THETA [lakes|nolakes] [skipA] [STATE] [MODE]`, settings by environment:

- `REAL=1` the game's terrain (face A); `REGION=x0,z0,x1,z1` a window of it (m); `HOLD_SEA=1` sea held.
- `BAND=0.05 SMOOTH=1 EMA_SECS=5 WIN_SECS=10` the band, smooth waking, averaged faces, calm window.
- `POOLS=1` still water as pools (`POOL_V`, `POOL_BAND`).
- Modes: `short N` (N s sleeping against N s stepping everything), `who` (which chunks keep waking),
  `probe` (what moves), `diag` (swing and drift), `edit` (a dam and a flood). `TRACK="x,z;…"` traces columns.

The window used in G0 so far: `REGION=-10000,-8000,-2000,-1008 HOLD_SEA=1 REAL=1`, settled with
`world 42 900 0.95 lakes skipA win.bin`, then `… win.bin short 180`.
