---
name: bench
description: Run Dwell for real in Chrome on the GPU and read its telemetry — frame-phase timings, job latencies, uploads, the player controller's per-tick trace, long tasks, screenshots and Chrome traces. Use to reproduce a bug in the browser, benchmark a change before/after, or check what the game is doing; extend it (new scenario, step or metric) when it can't answer the question yet.
---

# Dwell bench

Full reference: `docs/DEV_TOOLS.md`. Quick path:

1. Toolchain (once per machine): `. ~/.cargo/env` (rustup; `rust-toolchain.toml` pins the version),
   `rustup target add wasm32-unknown-unknown`, `cargo install wasm-bindgen-cli --version <the
   version in Cargo.lock> --locked`, `npm ci` in `client/`.
2. Build: `cd client && npm run build:wasm && npm run build`. Rebuild both after any Rust change
   (`npm run build` alone after shell-only changes). A `vite preview` already on :4173 serves the new
   `dist/` without restarting.
3. Run: `npm run bench -- <scenario…> [--out DIR] [--unthrottled] [--swiftshader] [--file FILE]
   [--base URL]` (`--base` for a build already served, e.g. another worktree's on its own port).
   Put `--out` and throwaway `--file` scenarios in the session's scratchpad, not the repo.
4. Read the printed summary; for detail, `OUT/<scenario>/report.json` (`records.<name>.ticks` for the
   controller trace, `.frames` for raw per-frame values, `.telemetry.samples` for job timings), the
   PNGs (Read them to see the frame) and `<name>.trace.json` (DevTools Performance).

5. Renderers side by side: `npm run bench -- --compare <scenario…> [--configs a,b]` runs each
   scenario once per configuration in `client/bench/configs.json` (renderer, scale, upscaler) and
   writes `OUT/compare.json`, a printed table, and per `shot` a `<shot>.<config>.side.png` (baseline
   left) and `.diff.png` (ΔE heatmap) with % of pixels over the threshold, mean and max ΔE.
   Configurations the build can't run (no such renderer from `__dwell.renderers()`; today every
   `march` one) are skipped with a note. Read the side and diff PNGs to see the difference.

Tips:

- On a Mac, frames are vsync-locked at 16.7 ms: use `--unthrottled` to see real frame cost, or read
  `frame.cpu` and the per-phase series.
- Compare a change by running the same scenario before and after into two `--out` dirs.
- A scenario for a one-off question goes in a scratch `--file`; one worth keeping (a benchmark, a
  bug repro) goes in `client/bench/scenarios.json`.
- If a question needs a metric that isn't there, add a `telemetry::span/add/sample` at the call
  site (and to the DEV_TOOLS.md table) rather than ad-hoc logging.
