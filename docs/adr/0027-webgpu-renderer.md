# 0027. WebGPU renderer: Three's `WebGPURenderer` with TSL materials, a WebGL2 fallback, and a look registry

- Status: Superseded by [0028](0028-rust-port.md)
- Date: 2026-10-12
- Resolves: Phase 14 of the implementation plan (the baseline's decision D4,
  [`BASELINE_EVALUATION.md`](../BASELINE_EVALUATION.md) §7)
- Supersedes: [0002](0002-client-renderer.md)'s backend (`WebGLRenderer` on WebGL2); Three.js and
  Dwell's thin `client/render` interface stay
- Builds on: [0015](0015-block-registry.md) (blocks name their look), [0025](0025-baseline-world.md)
  (the baseline)

## Context

The client draws with Three.js 0.186 `WebGLRenderer` behind Dwell's `client/render` interface,
which game code uses instead of Three.js and which was designed for a backend swap (ADR 0002 named
`WebGPURenderer` as the upgrade path). The baseline (B8) asks for **WebGPU**, for performance and for
future shaders. What is not a system today is how blocks are drawn: a block's `look` is one of a
closed list branched on in `mesher.ts`, the shading (height fog, per-face light, sky) is GLSL
spliced into Three's shaders with `onBeforeCompile` or a `ShaderMaterial`, and textures are a
closed `TileName` list with one painter function per tile. A new kind of surface is a code change in
several places.

WebGPU runs in current Chrome and Edge, Electron, Safari 26 (macOS and iOS, so Capacitor's
WKWebView) and Android WebView (Chrome 121+); Firefox's support varies by platform and older
devices have none.

## Options considered

1. **WebGPU only**, with an "unsupported browser" screen. One backend to test, but players on
   older devices and some browsers can't play.
2. **WebGPU first, Three's WebGL2 backend as an automatic fallback**, all shaders in TSL (chosen,
   D4). `WebGPURenderer` runs the same TSL node materials on either backend, so there is still one
   shader source; the fallback costs a second CI configuration, not a second renderer.
3. **Keep `WebGLRenderer`** and port later. Fails B8, and every shader written meanwhile in GLSL is
   work to redo.

## Decision

1. **`WebGPURenderer`** (from `three/webgpu`, initialised asynchronously) replaces `WebGLRenderer`
   behind `client/render`. It uses WebGPU where the device offers it and otherwise **falls back
   automatically to Three's WebGL2 backend**. Game code does not change; the render interface
   does not leak which backend runs.
2. **All shaders are TSL node materials**: height fog and per-face light, the sky, the chunk and LOD
   materials, tone mapping and exposure. No GLSL and no `onBeforeCompile` remain, so both backends
   run one source.
3. **The player can see which backend runs** (owner, 2026-10-12): the **settings menu** shows the
   active renderer — WebGPU, or WebGL2 (fallback) — and the F3 overlay reports it too. It is shown,
   not chosen: there is no switch to force a backend for players (a `?renderer=` URL parameter may
   force one for tests).
4. **A look registry.** Each look (`cube`, `shaped`, `water`, `ladder`, …) is a module that
   registers, under its name, a TSL material and a meshing style. Block data names its look; the
   block registry's generator validates the name against the registry; `mesher.ts`'s branches on
   look names are replaced by lookups. A future shader is one module plus a data reference.
5. **Textures as data.** Block textures are described in data next to the block data (parametric
   noise recipes — palette, scales, features — or image files), named by block data; the atlas is
   built from them and sized to fit. `TileName` and the painter functions are removed.
6. **CI renders WebGPU.** Playwright's headless Chromium runs with WebGPU enabled (flags and a
   software adapter) for the e2e suites and the screenshot script, and the suites also run on the
   forced WebGL2 fallback.

## Consequences

- The Phase 7 look (palette, face tints, sky, tone mapping, exposure) is ported, not redesigned;
  before/after screenshots are compared by the owner. The biome-tint path is not ported (it is
  removed by [0025](0025-baseline-world.md)).
- Batched terrain, the near/far depth passes and the LOD's draw path are re-verified on the new
  backend; frame time is measured on a desktop and a phone against the WebGL build.
- **Risk — CI.** Headless WebGPU on CI runners is proven first, before any port; if it can't be made
  reliable the e2e suites run on the fallback and WebGPU is covered by a smoke test and manual checks.
- Adding a texture is a data entry and adding a look is one module plus data; a test adds a fixture
  look and texture without touching the mesher.
- No public API changes (worlds, terrain, protocol, storage): no new compatibility line.
- Reversal: forcing the WebGL2 backend is one renderer option; returning to `WebGLRenderer` would
  mean rewriting the TSL materials in GLSL.
