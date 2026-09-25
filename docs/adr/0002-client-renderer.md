# 0002. Client renderer: Three.js on WebGL2, behind a thin render interface

- Status: Accepted
- Date: 2026-09-25
- Resolves: ARCHITECTURE.md Open Decisions #2

## Context

The client only renders: physics, prediction, and worldgen run in the sim-core WASM module
(ARCHITECTURE.md §2.1, §5). Rendering needs are voxel-specific: custom chunk meshes with compact
vertex formats, block texture arrays, custom shaders (ambient occlusion, lighting), many chunk
draw calls with culling, camera-relative (floating-origin) rendering, and dynamic meshes for
Tier 1 clusters and players. Targets are browsers (GitHub Pages), Electron, and Capacitor on
iOS (WKWebView) and Android.

## Options considered

1. **Three.js, WebGL2 first.** Large ecosystem, custom `BufferGeometry` + shader materials fit
   voxel chunks, built-in loaders, post-processing, shadows, and debug helpers. WebGL2 runs on
   every target. `WebGPURenderer` with TSL shaders is an upgrade path. Cost: scene-graph overhead
   per draw; peak performance may need bypassing parts of it.
2. **Babylon.js.** Full engine with mature WebGPU; much larger bundle and overlapping systems
   (physics, loop, input) we already own.
3. **Custom WebGL2/WebGPU renderer.** Maximum control and smallest bundle, but cameras,
   materials, shadows, loaders, and debug drawing must all be built first.
4. **PlayCanvas engine.** Lean and mobile-friendly; strengths are its editor/asset pipeline,
   which we would not use.

## Decision

Use **Three.js** with **`WebGLRenderer` (WebGL2)** as the initial backend.

- Chunks use our own geometry (packed vertex attributes) and our own shader materials; block
  textures come from a texture array.
- All rendering goes through a thin `client/render` interface owned by Dwell: chunk meshes,
  dynamic body meshes, player/remote-player views, camera rig, debug draw, and frame submit.
  Game code never touches Three.js objects directly.
- Rendering is camera-relative: world positions are rebased around the camera before upload.

## Consequences

- Fast start with mature tooling; the WebGL2 baseline covers every target platform.
- Hot paths (chunk culling, batching/multi-draw, GPU-driven rendering) can move to custom code
  behind the render interface without affecting the rest of the client.
- Moving to `WebGPURenderer` (TSL shaders) later is a backend swap behind the same interface;
  writing chunk shaders so they can be ported to TSL keeps that cheap.
- Three.js is a runtime dependency of the client bundle, pinned and updated deliberately.
