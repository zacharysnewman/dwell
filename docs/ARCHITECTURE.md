# Dwell — Game Architecture

> **Living document.** This file describes the architecture as it is *intended and
> built*. Any change that adds, removes, or alters an architectural element
> (components, process boundaries, protocols, message formats, data formats,
> physics tiers, tunable thresholds, deployment topology, third-party engines)
> must update this file in the same change. See `CLAUDE.md`.
>
> Status legend used below: **[planned]** not yet built · **[in progress]** · **[built]**
>
> **The build is one Rust workspace** ([ADR 0028](./adr/0028-rust-port.md), Phase 24): the dedicated
> server, the authoritative sim core, physics (Rapier) and the browser client (wgpu, WebGPU only) are
> Rust crates in one Cargo workspace (§3); the client's DOM shell, the master server and the release
> scripts are TypeScript. The C++ server and core, CMake, Emscripten, Jolt, SQLite, zstd, Monocypher
> and the Three.js client were removed at the cutover (24f).

---

## 1. System Overview

Dwell is a **server-authoritative, multiplayer-first voxel sandbox** that supports both
small- and large-scale dynamic physics (collapsing structures, explosions, debris).

| Concern | Choice |
|---|---|
| Authoritative server | Rust (`dwell-sim` core, `dwell-server` binary), Rapier physics (`rapier3d-f64`); **hosted by players** — dedicated servers or friend worlds (§10, ADR 0003) |
| Client | Rust compiled to WebAssembly (`dwell-client`, `wasm-bindgen`): session, prediction with the same controller and physics as the server, terrain streaming, meshing, LOD and rendering; a TypeScript shell for the DOM UI (ADR 0028) |
| Renderer | wgpu, WebGPU only, WGSL shaders (ADR 0028); no WebGL fallback |
| Client shells | Browser (GitHub Pages) → Electron (desktop) → Capacitor (iOS/Android) |
| Transport | Dedicated servers: WebTransport (HTTP/3 / QUIC), WebRTC fallback. Friend worlds: WebRTC data channels (ADR 0008) |
| Services | **[built, Phases 5b–5e]** Master server on Cloudflare Workers + Durable Objects: server listing, join codes, cert-hash distribution, WebRTC signaling, TURN credentials (Cloudflare TURN) (§10.3, ADR 0013) |
| Identity | Device keys (Ed25519) now; optional accounts later (§10.4, ADR 0004) |
| Simulation | 60 Hz internal physics step, 20 Hz network snapshots |
| Players | Dynamic-body, velocity-layer controller ported from the Physics Player Controller, client-predicted (§9, [`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md)) |
| World | **[built, Phase 3c]** A disc of radius 8,192 km, 16,384 m tall with two faces (Phase 13), double-precision physics (§6.3, ADR 0011) |
| Terrain | Seeded deterministic procedural generation, the same Rust crate (`dwell-worldgen`) on server and client (§6.3) |
| Whole-world view | **[built, Phase 4]** 3D level-of-detail octree around the camera, generated on the client from the seed; only modified sections are sent (§6.6, ADR 0012) |
| First deployment target | **GitHub Pages** at `https://dropkickarcade.com/dwell/` (ADR 0005) |

```
                    ┌───────────────────────────────────────────────┐
                    │          Authoritative Server (Rust)          │
                    │                                               │
  inputs (dgram) ──▶│ Input validation ─▶ Player controllers        │
                    │ Worldgen (seed) · LOD propagation (off tick)  │
                    │        │                                      │
                    │        ▼                                      │
                    │ Master voxel grid ─▶ Structural integrity     │
                    │        ▲                 │  (flood-fill)      │
                    │        │ re-bake         ▼                    │
                    │ Sleep monitor ◀── Tier 1 bodies (Rapier, 60Hz)│
                    │                                               │
                    │ Replication: snapshots (dgram, 20Hz)          │
                    │              voxel deltas / events (reliable) │
                    │              LOD sections (reliable, `lod`)   │
                    └───────────────▲───────────────┬───────────────┘
                                    │ WebTransport  │
                                    │  / WebRTC     ▼
┌──────────────────────────────────────────────────────────────────────┐
│        Client (Rust WASM: dwell-client, wgpu; TS DOM shell)          │
│  Input → Local prediction (player controller) → Reconciliation       │
│  Chunk store (+ generation workers) → Mesher (workers) → Renderer    │
│  LOD octree (+ generation / meshing workers) → far render pass       │
│  Snapshot buffer → Interpolation of Tier 1 bodies                    │
│  Local-only Rapier world → Tier 2 cosmetic debris                    │
└──────────────────────────────────────────────────────────────────────┘
   Shells: Browser (GitHub Pages) │ Electron │ Capacitor (iOS / Android)
```

---

## 2. Deployment Topology

### 2.1 GitHub Pages (first target) **[built]** (items 1–4)

GitHub Pages only serves static files. Consequences that shape the architecture:

1. **The authoritative server cannot run on GitHub Pages.** The static client is served
   from Pages; servers are hosted by players (§2.3, §10). The client finds them through the
   master server, a join code, or an invite link (§10.1).
2. **No custom HTTP headers** → no `Cross-Origin-Opener-Policy` / `Cross-Origin-Embedder-Policy`
   → no `SharedArrayBuffer`. The browser build (`dwell-client` for `wasm32-unknown-unknown`, with
   the sim core and Rapier inside it) is therefore **single-threaded** (no WASM threads); parallel
   work runs in **Web Workers**, each with its own instance of the same WASM module, exchanging
   transferred byte buffers (§5.1, [ADR 0007](./adr/0007-threading-model.md)). No
   `coi-serviceworker`. Electron loads the same single-threaded build; the native server is
   multithreaded (worldgen, LOD propagation, the save thread, networking).
3. **Default project path.** The site is served at `https://dropkickarcade.com/dwell/` (ADR 0005).
   The origin is shared with other `dropkickarcade.com` games, so all browser storage is
   `dwell`-namespaced, the device key is non-extractable, and any service worker is scoped to
   `/dwell/`. **[built, Phase 6]** `/dwell/` is a **launcher** and every build is served from its own
   directory, `/dwell/v/<version>/` (Vite `base` per build; `/dwell/` for a local build, the preview
   and the e2e suite), so players can join servers and open worlds that have not moved to the
   newest version ([ADR 0014](./adr/0014-versioned-releases.md), [`RELEASES.md`](./RELEASES.md);
   see "Deployment" below). A move to its own subdomain is a documented future option (ADR 0005).
4. **Local mode** **[built]**. So the Pages deployment is playable with no hosted server, the
   server's simulation core (`dwell-sim`) runs in the browser: `LocalWorld`
   (`dwell-client` `web/local_worker.rs` over `local.rs`'s `LocalCore`) in a module Web Worker
   (`client/src/local/worker.ts`), connected through the TS `LoopbackTransport`, which the shell
   hands to the Rust client like any transport (§8.1). Same code, same protocol, same device-key
   handshake, no network. **[built, Phase 5a]** A page with no world or server in its
   address opens the **main menu** (`ui/mainMenu.ts`: your worlds, and playing with others — join, nearby, recent, the server browser), which the **launcher** shows at `/dwell/` (RELEASES.md §5: every choice, creating and regenerating worlds included, is a route to the build that carries it out; a build with no route hands over to the launcher);
   choosing a world navigates to `?play=<world id>` and joining to the invite, so a reload
   continues the same game and Back returns to the menu (`ui/launch.ts`). **[built, Phase 6]**
   Those navigations go to `/dwell/` (`launcherHref`, `launcherPath.ts`), never to a version's own
   path: the launcher opens the build the choice needs. Links still open
   directly: an invite, a friend world's join code (`?code=`, Phase 5c, §10.2), or a local world
   by `?local=1`, `?world=` or `?seed=`. Local mode and
   dedicated servers generate the **procedural terrain** world (generator version 12, §6.3) by
   default; `?world=flat`, `?world=sandbox` and `?seed=N` (local mode) or `--generator N` and
   `--seed N` (`dwell-server`) pick another generator or seed. (The movement playground, version 1, became a
   test structure file in Phase 15, §6.3; old playground worlds open in their own versions.)
   **[built, Phase 3e]** Local worlds are saved in the browser: one world file per generator and
   seed in OPFS (`dwell/worlds/local-g<generator>-s<seed>.dwellworld`, §6.4), at every autosave
   (`AUTOSAVE_SECONDS`) and when the page is hidden or closed; a reload continues the same world. **[built, Phase 5a]** Worlds
   are named and managed from the menu: create (name, seed — a number, or text hashed with
   FNV-1a, blank for random — and type: Terrain, Flat or **[built, Phase 27 S2a]** Sandbox, also
   `?new=1&type=sandbox`; an unknown type makes terrain), play, regenerate (the files deleted and recreated from the
   seed with the type's current generator) and delete. Menu-created worlds use id-based files
   (`dwell/worlds/w<10 base-36 chars>.dwellworld`); links keep opening the per-generator-and-seed
   files. An index in local storage (`dwell.worlds`, `local/worldIndex.ts`) holds each world's name,
   type, seed, generator, when it was last played and **[built, Phase 6]** the app version that
   last played it (§6.4); per-seed files it doesn't list (saved before the menu) are adopted when
   the menu opens, as worlds saved before versioned releases, which are listed only to be deleted. The game menu's Quit to main menu asks the worker to
   save (answered with `saved`) before leaving.
5. **One Rust codebase, one WASM module.** `npm run build:wasm` builds `dwell-client` for
   `wasm32-unknown-unknown` (release) and runs `wasm-bindgen --target web` into `client/src/pkg`
   (not committed); Vite bundles that one module, and the page (the game client), every job worker
   and the local-mode worker each instantiate it (`client/src/engine/wasm.ts`). Every piece of
   simulation logic (player controller, worldgen, physics setup, protocol codec) has exactly one
   implementation, shared by the native server and the browser.

**Deployment** **[built, Phase 6]** (the repository's Pages source is "GitHub Actions"):

- **Builds are releases** (`.github/workflows/release.yml`). A push to `main` builds a **dev** build,
  `v<next version>-dev.<run>` (a GitHub pre-release) — unless it leaves `client/package.json` above
  the newest release, which releases that version as **stable** (the baseline, a new line); a pushed `v<version>` tag, or the workflow run
  by hand, builds the **stable** release. `client/scripts/release.ts` decides which version: the next
  patch after the newest published release, or `client/package.json`'s version if that is higher
  (`package.json` is a floor, raised by hand only to start a new line; nothing is bumped after a
  release), or one named in the manual run's *version* input or by the tag, which must be newer than
  the newest release. Each build is made with `DWELL_VERSION` (read by
  `client/vite.config.ts` and `crates/dwell-sim/build.rs`), with the Rust client's WASM built for it,
  for the base `/dwell/v/<version>/`, and attached to its GitHub Release as `dwell-<version>.tar.gz`
  with its `build.json` (version, channel, date, commit, `protocolVersion`, generator versions,
  `minLauncher`). The version is Semantic Versioning 2.0.0, one for the whole app; before `1.0.0`
  breaking changes bump MINOR and compatible ones PATCH (§10.5).
- **The site is assembled from the releases** (`.github/workflows/pages.yml`, called at the end of
  each release and runnable by hand; `client/scripts/site.ts`): `index.html` (the launcher),
  `versions.json` (generated from each `build.json`) and `v/<version>/` for every stable release and
  the newest 10 dev builds, each unpacked and immutable; older dev releases and their tags are
  deleted. It is deployed with `actions/deploy-pages`, and its size is reported against Pages'
  1 GB limit. Builds are same-origin directories because release assets carry no CORS headers or
  script content types, the CSP is `script-src 'self'` / `worker-src 'self'`, and all versions must
  share the origin's storage.
- **The launcher** (`client/launcher`, its own small page and Vite build) reads `versions.json`,
  chooses a build and replaces itself with it, keeping the query: for `?play=<id>` the newest
  published build on the world's compatibility line and not older than the version that last played
  it (read from the world index, shared by every version); for an invite or code with `v=<host
  version>` the newest build on the host's line; otherwise the latest stable build (the latest dev
  build if the player ticked "Show dev builds" on the version page, `dwell.channel`), falling back to the
  other channel. A missing version, or a request no published build can serve, shows a message
  offering the latest. `?version=<v>` pins a build (developers).
  `/dwell/?versions` is the **version page** (Phase 6b): every published build, newest first, with its
  compatibility line and the player's worlds it can open, and the "Show dev builds" tick
  (`dwell.channel`). Playing a build opens it for that visit only. A plain visit to `/dwell/` lands on a launcher
  screen (Play latest stable / Choose version, waiting for a click) so the page is reachable whatever
  build was last played; the menu's Versions link goes there too.
- **Licence notices** ship in every build (`THIRD_PARTY_NOTICES.txt`, linked from the menu's
  About); `THIRD_PARTY_NOTICES` also covers the crates linked into `dwell-server`, to ship beside
  its binary when it is packaged (Phase 23).

The page carries a strict Content-Security-Policy `<meta>` tag, since Pages cannot send headers (§11).

### 2.2 Desktop / Mobile shells **[in progress]**

The same Vite build output is wrapped by:
- **Electron** (`platforms/electron`) **[built: shell]** — Chromium, full WebTransport support.
  `main.js` serves the client build from a privileged, secure `dwell://` protocol (so WebCrypto,
  IndexedDB, WebTransport and WebGPU work) with COOP/COEP headers (cross-origin isolated, though
  the build it loads is the same single-threaded one as the web's, ADR 0007). WebGPU is turned on
  where Chromium does not enable it by default (Linux: `enable-unsafe-webgpu` and Vulkan); with
  `--smoke` it runs on SwiftShader's Vulkan in software. Invite queries are passed with
  `--join=<query>`; `--smoke` exits 0 once joined (used by CI under Xvfb). Packaging and "Host
  world" come in Phase 23.
- **Capacitor** (`platforms/capacitor`) **[planned, Phase 23]** — Android System WebView (Chromium)
  and iOS WKWebView, wrapping the same web build. They need WebGPU (ADR 0028: Android WebView from
  Chrome 121, iOS 26). Where WebTransport is unavailable (possibly WKWebView), the client uses
  WebRTC.

### 2.3 Server hosting **[planned]**

There are **no official game servers**; players host (ADR 0003, details in §10):
- **Dedicated servers:** the native server binary (Windows/macOS/Linux) or Docker image on a
  player's machine or rented VM, or launched by the Electron app ("Host world"). Needs inbound
  UDP (QUIC and WebRTC); UPnP/NAT-PMP is attempted, otherwise the host
  forwards the port.
- **Friend worlds:** any client (browser, phone, desktop) hosts its integrated server (the sim
  core in a worker) over WebRTC.
- **Certificates:** dedicated servers generate a self-signed ECDSA P-256 certificate at startup
  (13-day validity, by `dwell-net`) used by **both** WebTransport and WebRTC DTLS, and
  publish its SHA-256 through the master server or the invite link; clients pass it as
  WebTransport `serverCertificateHashes` or pin it as the DTLS fingerprint. Hosts with their own
  domain may use a publicly trusted certificate instead. Local development uses the same
  mechanism on `127.0.0.1`. **[built]** (rotation before expiry: Phase 23)
- **Our infrastructure** is limited to the static site (GitHub Pages), the master server, and a
  TURN relay — **[built, Phase 5b–5d; TURN key outstanding]** both on Cloudflare: a Worker with Durable Objects and
  Cloudflare's managed TURN, on the free plan (§10.3, ADR 0013).

---

## 3. Repository Layout **[built]**

```
/Cargo.toml          The Rust workspace (ADR 0028): every crate in crates/; edition 2024, Rust 1.94
                     (rust-toolchain.toml pins 1.94.1); `unsafe_code = "forbid"` workspace-wide. The
                     dev profile builds Dwell at opt-level 1 and dependencies at 3.
/.cargo/config.toml  The wasm32-wasip1 test runner (tools/wasi-run.mjs) and web-sys's unstable-API
                     flag for the browser build (WebTransport).
/tools               wasi-run.mjs: runs `--target wasm32-wasip1` test binaries as WASM under Node's WASI.
/crates
  /dwell-protocol    Protocol constants (`constants.gen.rs` from shared/protocol), the message codec
                     (every message, chunk palette + RLE, posfix; pinned by shared/protocol/vectors.txt)
                     and Ed25519 verification (`auth.rs`, ed25519-dalek).
  /dwell-blocks      The block registry (`blocks.gen.rs` from shared/blocks), looked up by name; state
                     strings and properties; shape geometry (`shape.rs`).
  /dwell-world       Chunks and the voxel grid (`VoxelWorld`), world bounds and the two faces, LOD
                     sections (`lod.rs`: grid, downsampling, bounds), structures (`structure.rs`, tables
                     `structures.gen.rs` from shared/structures), block edits
                     (`edit.rs`: targeting and the server's edit check).
  /dwell-worldgen    Noise, the interim island, the terrain generator, the bifacial world; generator
                     and air-test lookup by version.
  /dwell-storage     World files on redb (LZ4 blobs, world state ids by string), the save thread
                     (`WorldStore`), `BytesFile` (a world file held in memory).
  /dwell-physics     Rapier (`rapier3d-f64`, enhanced determinism) behind Dwell's types (`Physics`,
                     an opaque `BodyHandle`); the terrain voxel collider; the player controller, its
                     voxel probes, wire conversions (`net.rs`) and the client's predictor; the
                     controller suites (tests/, golden trace in tests/golden/).
  /dwell-sim         The authoritative server core (no sockets or OS calls): sessions and the
                     handshake, the tick, snapshots, streaming, LOD propagation, edits, hosting,
                     persistence; the worldgen pool, the fixed step, the app version (build.rs).
  /dwell-net         WebTransport (wtransport) and WebRTC (str0m) behind one event queue on their own
                     tokio threads; master-server registration (ADRs 0001, 0008).
  /dwell-server      The dedicated server binary `dwell-server`: launch options, world file settings
                     and permissions, the version lock (read-only peek), master heartbeats, the
                     fixed-step loop (tests/host.rs runs the binary).
  /dwell-mesh        Chunk and LOD-section meshers, the block texture atlas and hotbar swatches.
  /dwell-march       [in progress] The ray-marched renderer's CPU core (RAY_MARCHED_RENDERER.md,
                     Phase 25 A3a): the GPU brickmap layout as u32 words, chunk/section builders, the brick
                     pool, the reference marcher, its brute-force oracle and steps-per-ray stats
                     (examples/march_steps.rs). Not used by the client yet.
  /dwell-look        The look as data: palette, face tints, lights, the sky (and its generated WGSL),
                     the sky frame, height fog, exposure.
  /dwell-game        Client game logic: input and touch state machines, flight, the eye and face
                     flip, remote players, block interaction, the chunk streamer, HUD text.
  /dwell-lod         LOD selection (§6.6): the traversal, cache, stand-ins, requests, frustum, detail
                     settings.
  /dwell-client      The browser client (cdylib, wasm-bindgen; also an rlib tested natively):
                     net/ (session, framing, datagram sender, simulated conditions, the WebRTC
                     answer), local.rs (LocalCore), jobs/ and pool.rs (background jobs and their byte
                     codec), game.rs (the game loop behind a `Scene` trait), render/ (the wgpu
                     renderer, camera, WGSL), web/ (the `Client` app, browser transports, job
                     workers, `LocalWorld` on OPFS, the JS API); www/ holds the WebGPU probe page
                     (`npm run build:probe`, `npm run e2e:webgpu`).
/client              TypeScript shell (Vite): the DOM UI over the Rust client (§5).
  /src/engine        wasm.ts (loads the module, once per context), jobWorker.ts (a job worker).
  /src/pkg           wasm-bindgen output of `npm run build:wasm` (not committed).
  /e2e               Playwright end-to-end tests against `dwell-server` and the built client
                     (launcher.spec.ts: against a locally assembled site, playwright.site.config.ts).
  /e2e-webgpu        The WebGPU probe and the renderer's smoke scene in Chromium.
  /launcher          The launcher page at /dwell/ (§2.1, Phase 6): versions.json, version choice.
  /scripts           site.ts (plan, manifest and size of the Pages site), release.ts (what a release
                     run builds), localSite.ts (a local multi-version site for the e2e tests),
                     shots.ts (screenshots of the named views), views.ts (the views' format and
                     checks), bench.ts and scenarios.ts (the dev bench, DEV_TOOLS.md), compare.ts,
                     configs.ts, compareReport.ts and compareHtml.ts (the bench's renderer comparison,
                     `--compare`, and its HTML report); run with Node's type stripping.
  /views             views.json: named views (camera, window, settings) for shots.ts and e2e specs.
  /bench             scenarios.json: scripted scenarios for the dev bench (bench.ts); configs.json:
                     renderer configurations for `--compare`; out/ (ignored) gets its reports.
/shared/protocol     constants.json (single source of protocol constants) + gen.mjs (→ Rust for
                     dwell-protocol, TS constants for the shell), make_vectors.py (independent
                     reference encoder) → vectors.txt (golden bytes the codec must match).
/shared/blocks       Block data (dwell.json) + gen.mjs and shapes.mjs (→ Rust for dwell-blocks, TS for
                     the shell's block dump) and vectors.txt.
/shared/worldgen     golden/: chunk and LOD-section hashes of generator version 12 (§6.3).
/shared/structures   Structure files (*.json; test/ holds fixtures) + gen.mjs (→ Rust tables for
                     dwell-world) and gen.test.mjs; playground.json is the movement playground the
                     tests stamp (§6.3).
/shared/version      vectors.txt: version precedence, compatibility lines and "may open" cases that the
                     TypeScript (client/src/version) and Rust (dwell-sim app_version.rs)
                     implementations both pass.
/shared/master       vectors.json (+ make_vectors.mjs): the master's request-signing format.
/shared/licenses     gen.py → THIRD_PARTY_NOTICES (root): third_party.json + texts/ for components
                     that are not Rust crates, `cargo metadata` for the crates linked into
                     `dwell-server` (native) and `dwell-client` (wasm32).
/services/master     Master server: Cloudflare Worker + Durable Objects, TypeScript, Wrangler
                     (listing, join codes, signaling, TURN credentials; ADR 0013).
/platforms/electron  Electron shell ("Host world" launching the native server: Phase 23).
/platforms/capacitor Capacitor shell (Phase 23; a placeholder today).
/docs                ARCHITECTURE.md, PLAYER_CONTROLLER.md, IMPLEMENTATION_PLAN.md, FUTURE.md and the
                     design docs; prototypes/ (saved design prototypes).
  /adr               Architecture decision records.
/.github/workflows   ci.yml (protocol, master, client-lint, client, e2e shards, rust, and the `CI`
                     gate), release.yml (builds as GitHub Releases), pages.yml (the site from the
                     releases), master.yml.
rust-toolchain.toml  Pinned Rust toolchain.
LICENSE              All rights reserved (the source is public for reference only).
THIRD_PARTY_NOTICES  Licenses of the third-party components in Dwell's builds (generated).
```

The crates depend downward: protocol and blocks at the bottom; world on them; worldgen, storage
and physics on world; sim on all of those; server on sim, storage and net; mesh, look, game and
lod beside sim for the client; dwell-client on everything but net and server. Apart from
`dwell-net`, `dwell-server` and `dwell-client`'s `web/`, the crates do no I/O beyond the world file
and build both natively and for `wasm32-unknown-unknown`; the client-logic crates take time as an
argument and their collaborators as traits, so they run natively under test as in the browser.

### 3.1 Toolchain & CI **[built]**

| Area | Choice (pinned) |
|---|---|
| Rust | Toolchain 1.94.1 (`rust-toolchain.toml`), edition 2024, one Cargo workspace; targets: native (the server, tests), `wasm32-unknown-unknown` (the browser client, `wasm-bindgen` 0.2.129 with `--target web`), `wasm32-wasip1` (deterministic suites under Node's WASI). Key crates: `rapier3d-f64` 0.36 (`enhanced-determinism`), `wgpu` 30 (WebGPU backend, WGSL; `naga` validates the shaders in tests), `redb` 4.3, `lz4_flex` 0.14, `ed25519-dalek`, `wtransport` 0.7.2 (ring), `str0m` 0.23.1 (aws-lc-rs), tokio, `ureq` (rustls), `web-sys`/`js-sys`, `libm` (transcendentals identical natively and in WASM). Tests with `cargo nextest`; `cargo fmt`, `clippy -D warnings` |
| Client shell | Node 22, Vite 8 (module workers), TypeScript 6.0 (strict), ESLint (typescript-eslint strict, type-checked), Prettier, Vitest, Playwright 1.56.1 (e2e) |
| Desktop | Electron 44.4.5 (`platforms/electron`) |
| Releases | `release.yml` and `pages.yml` (§2.1): Node 22 type stripping runs `client/scripts/*.ts`; the Rust client is built with `DWELL_VERSION` (`npm run build:wasm`); `gh` publishes releases; the site is deployed with `actions/deploy-pages` |
| Master server | TypeScript 6.0 on Cloudflare Workers (`services/master`, compatibility date 2026-08-15), Wrangler 4.124.0, `@cloudflare/workers-types`, Vitest 4.1 with `@cloudflare/vitest-pool-workers` 0.22.0 (tests run in workerd), ESLint/Prettier as the client; `legacy-peer-deps` in its `.npmrc` |
| CI (`ci.yml`) | Protocol: the generators' Rust and TS output and the golden vectors (including `shared/master/vectors.json`) must match the committed files. Master: format, lint, typecheck, Vitest in the Workers runtime, `wrangler deploy --dry-run`. Client-lint (its own fast job): builds the Rust client's module and types (`npm run build:wasm`), then format, lint, typecheck. Client: `npm run build:wasm`, Vitest, build, launcher build. E2E (the browser suite is serial, so it runs as four `--shard`s on separate runners, each building `dwell-server` in Release and the client): Playwright Chromium **headed under Xvfb** on SwiftShader's WebGPU — headless Chromium's software adapter loses the WebGPU device as soon as a canvas presents — against the native server and the built client (WebTransport, WebRTC, local mode, session replacement, movement, two clients seeing each other move with one over a simulated 150 ms link, breaking and placing every palette block, an edit seen by a second client, touch edits, a local world surviving a reload, the whole-world view, terrain drawing, flight speed, the block dump, menus, settings, friend worlds through a local `wrangler dev` master, the lobby, the version lock); shard 1 also runs the launcher's own e2e (`npm run e2e:site`: three versions assembled locally — which build each address opens, worlds in their own line, version badges, errors) and the Electron smoke test under Xvfb (a dedicated server over WebTransport, and local mode). Rust: `cargo fmt --check`, clippy `-D warnings` natively (all targets) and for `dwell-client` on wasm32, the third-party notices are current (`shared/licenses/gen.py --check`), `cargo nextest` over the workspace, the controller suites again ~8,000 km from the origin (`DWELL_ORIGIN_X=far`) and on face B (`DWELL_FACE=b`) and the netcode suite far from the origin, the deterministic suites (blocks, protocol, world, worldgen goldens, the controller's golden trace as the native↔WASM divergence check) again under `wasm32-wasip1`, the 64-player controller gate in Release, then the WebGPU probe and the renderer's smoke scene in headless Chromium (`--use-webgpu-adapter=swiftshader`, offscreen first). The `CI` job needs all of the above and fails if any did not succeed: it is the one check to require in branch protection |
| Enforced boundaries | Workspace lints forbid `unsafe` code; only `dwell-physics` uses Rapier types (game code holds opaque `BodyHandle`s); ESLint forbids `node:` built-ins outside tests in the client |

---

## 4. Server (Authoritative) **[in progress]**

### 4.1 Responsibilities
- Validate player input (rate, magnitude, reach for block edits, anti-teleport).
- Own the **master voxel grid** — the single source of truth for terrain — and generate
  unmodified chunks from the world seed (§6.3).
- Compute **structural integrity** and run flood-fill clustering on detached voxels.
- Simulate all **Tier 1** dynamic bodies and all player characters.
- Replicate state: snapshots (unreliable) and voxel deltas / events (reliable).
- Run the **sleep / re-bake** cycle to keep active body count bounded.

### 4.2 Main loop

```
every 16.67 ms (60 Hz):
    drain & validate inputs (per player, ordered by input sequence)
    integrate completed worldgen jobs (thread pool, budgeted)
    apply queued voxel edits → integrity → clustering → awaken bodies
    player controller pipeline per player (probes → layers → set body velocity; PLAYER_CONTROLLER.md §4)
    physics.step(1/60)                 (Rapier)
    sleep monitor → re-bake queue → apply re-bakes
every 3rd step (20 Hz):
    build per-client snapshot (interest-managed, prioritized) → send datagrams
    flush reliable queues (voxel deltas, events) in order
```

The loop uses a fixed timestep with an accumulator; the server never steps with a variable dt.

**Built (Phase 1; Rust, Phase 24):** `dwell-server` (`crates/dwell-server/src/main.rs`) drains
`dwell-net`'s events and feeds them to the core, flushes the core's outbox immediately (so
request/response latency doesn't wait for a tick), runs the due 60 Hz steps (`FixedStep` in
`dwell-sim`: carries remainders, drops backlogs beyond 8 steps), and sleeps until the next step or
at most 2 ms. Rapier steps on the tick's thread. **Built (Phase 2):** each `Server::step` takes one
input per joined player from its jitter-buffered queue (§9.3), runs the player controller pipeline,
applies its server-side consequences (fall damage; there is no void: a body that leaves the disc
falls toward the midplane and is carried across the midplane by the flip band, §6.3), steps
physics, respawns dead players after `RESPAWN_SECONDS`, and every 3rd tick sends each client a
`PhysicsSnapshot`. Sessions are kept in id order, so every pass over them is deterministic.
**Built (Phase 3a):** the world generates from `(worldSeed, generatorVersion)` (§6.3); players
spawn at the generator's spawn point. **Built (Phase 3b):** each step first integrates the chunks
the worldgen pool finished, and ends by streaming chunks to each client (interest-managed, within
its bandwidth budget), telling the pool what to generate next, and (every 64 ticks) evicting
unmodified chunks far from players (§6.3). **Built (Phase 3d):** next, the block edits queued since
the last step are validated and applied, and one `VoxelModification` goes to every client streaming
a changed chunk (§6.5); terrain collision re-syncs in the controller pass of the same step.
**Built (Phase 3e):** the step starts by collecting saves the I/O thread finished, and ends
with the autosave when due (§6.4). Integrity and re-bake arrive with later phases.

### 4.3 Simulation core vs. network front-end
`dwell-sim` has no sockets and makes no OS calls of its own: the world file goes through
`dwell-storage` (§6.4), nonces come from an `Entropy` the host supplies (the OS generator natively,
`crypto.getRandomValues` in the browser), and time is the host's (`Server::step` per tick; the
fixed step is fed elapsed seconds). Its only threads are the optional worldgen pool's
(`ServerConfig::worldgen_threads`; the dedicated server runs `cores − 2`, at least 1, the browser's
single-threaded local mode none, §6.3), the LOD propagation thread (one natively, none in the
browser, §6.6) and the world store's save thread (native only). `std::time` panics on
`wasm32-unknown-unknown`, so there the tick's time budgets count work items instead of measuring
time (`clock.rs`: about one chunk or section per millisecond of budget). It consumes decoded
transport events and emits encoded messages **[built]**: `dwell_sim::server::Server` —
`on_connected(session, transportKind, binding)`, `on_reliable`, `on_datagram`, `on_disconnected`,
`step()`, and `take_outbox()` returning `Outgoing { session, kind: Reliable(channel) | Datagram |
Close, bytes }`. The identical core runs in the dedicated server and in the browser's local mode
(§2.1); the client's prediction uses the same controller and physics crates (§9.3). `dwell-net`
owns sessions and stream management for both server transports.

**WebTransport stack (ADR [0001](./adr/0001-webtransport-server-library.md)) [built].** WebTransport is
provided by the Rust crate `wtransport`, wrapped in `dwell-net`, which the server calls directly
through a safe API: `NetServer::start(StartOptions)` / `shutdown`, `poll()` returning `Event`s
(Connected / Disconnected / Reliable / Datagram), `send(session, Command)` (Reliable on a channel,
Datagram, or Close, which flushes the control stream first), plus accessors for the certificate
hash, ports and ICE credentials. Its tokio runtime (2 workers) runs on its own threads; events
reach the main loop through a queue it drains, so no network callback ever enters the simulation.
Sockets bind dual-stack `[::]` and fall back to IPv4 where IPv6 is unavailable.

**WebRTC fallback (ADR [0008](./adr/0008-dedicated-server-transports.md)) [built].** The same
crate hosts an ICE-lite WebRTC endpoint built on `str0m` (sans-I/O, one tokio task driving every
WebRTC client), on its own UDP port (default: WebTransport port + 1), behind the same event queue.
**No signaling:** the server has fixed ICE credentials (random per start) published
in the invite link; it learns each client's ICE username from the client's first STUN binding
request and then creates a `str0m::Rtc` for it (DTLS server role, SCTP, four pre-negotiated data
channels, §8.1). DTLS uses the WebTransport certificate, so both transports share one fingerprint.
Client certificates are not verified (str0m fingerprint verification off): the client pins the
server fingerprint and proves its identity with the device-key handshake, which is bound to that
fingerprint. Clients that don't open all channels within 10 s are dropped, and half-open clients
are capped at 512. Reliable writes queue while SCTP buffers are full.

---

## 5. Client **[in progress]**

The client is the Rust crate `dwell-client` compiled to WebAssembly, driven by a TypeScript shell
(ADR 0028). Rust owns everything that runs per frame or per tick: the session and its transports
to dedicated servers, prediction and reconciliation, remote players, terrain streaming, meshing,
the whole-world view, the renderer and the meaning of input. TypeScript keeps the DOM: menus, the
HUD's elements, settings, the server browser, the master client, rooms and peer signalling, the
hosting relay, local mode's worker and loopback, identity and the launcher; it forwards input
events to the Rust `Client`, calls its `frame` from `requestAnimationFrame` and draws what `hud()`
returns. Without WebGPU (no `navigator.gpu`, no adapter or device) the page shows an
"unsupported" message instead of the game (`ui/startupError.ts`).

**Rust (`crates/dwell-client` and the client-logic crates):**

| Module | Responsibility |
|---|---|
| `web/app.rs` **[built]** | `Client`, the wasm-bindgen API one page holds: `Client.create(canvas, options)` starts WebGPU on the canvas (rejecting without it), takes the identity (`setIdentity`: the device's public key and a JS signing function, ADR 0004), connects (`connectWebTransport`, `connectWebRtc`, or `connectTransport` for a JS transport object the shell made), runs `frame(now)`, takes keyboard, mouse, touch and wheel events, block edits and palette selection, flight and its speed, the fog, exposure, sky rotation and detail settings, and reports `sessionState`, `hud`, `debugState` (what the `window.__dwell` test hooks read), `renderStats`, the workers' memory and the dev telemetry (`telemetry`, `telemetryFrames`, `telemetryTicks`, `telemetryReset`). |
| `dwell-world::hash` **[built]** | `HashMap`/`HashSet` with FxHash for the integer keys of the hot maps (chunk coordinates, LOD section ids, job ids) in the voxel world, the LOD system, the chunk streamer, the renderer, the job pool, physics terrain and the server sim: std's SipHash was a third of the client's main-thread time when flying (DEV_TOOLS.md). Not DoS-resistant, so never for keys a peer chooses. |
| `telemetry.rs` **[built]** | Dev telemetry ([DEV_TOOLS.md](./DEV_TOOLS.md)): per-frame series (named spans and counts: the frame's phases — network, job polling and applying, ticks, chunk meshing, the LOD update, encoding and presenting — and GPU uploads), event samples (each job kind's queue and run times, from the pool) and a trace of the predicted controller's ticks, kept for the last 600 frames and summarised as percentiles. Always on, one per thread; the bench (`client/scripts/bench.ts`) drives scenarios in a real browser and reads it. |
| `web/transport.rs`, `net/` **[built]** | Browser transports over `web-sys`: WebTransport to dedicated servers (certificate-hash pinning, the §8.1 framing; a closing connection first reads the control stream to its end so the server's last message — a `Reject` such as `Replaced` — is not lost; datagram writes never queue — one in flight and only the newest waiting per message type, `net/datagram.rs`, so slow frames cannot build input latency) and WebRTC to their ICE-lite endpoint (the server's answer built locally from the invite, `net/sdp.rs`); a JS transport object (friend-world peers, local mode's loopback) wrapped as a third kind. All deliver into one event queue the frame drains. `net/session.rs`: the join handshake and connection health as a poll-driven state machine (signing is the host's: the session asks the shell's function on a `Challenge`), reliable and datagram RTT. `net/netsim.rs`: `?netsim=rtt,jitter,loss%`. `web/mod.rs` `pingServer`: a lobby entry's `StatusRequest` round trip over a fresh connection (5 s timeout). |
| `game.rs` **[built]** | `Game`: the fixed 60 Hz loop — samples input, predicts with `dwell-physics`'s `Predictor` (the same controller and Rapier setup as the server, against the client's streamed `VoxelWorld`), sends `PlayerInput` (newest 4), feeds snapshots and knockback events to the predictor, nudges its tick rate from the server's input buffer, streams terrain around the player (`dwell-game`'s chunk streamer, mesh jobs within a 4 ms frame budget), drives the first-person camera (per-tick eye height with crouch and step-up/down smoothing, and the turn-over across the midplane), block targeting and its outline, the whole-world view (`dwell-lod`), the HUD text and the F3 overlay. A tool can place the camera (`set_view`, a `ViewOverride`): the renderer and the LOD view from it while the player stays put (named views, §6.3). Platform-free behind a `Scene` trait (the renderer) and a job pool, so `tests/game_local.rs` plays a local world natively through the real session and handshake. |
| `dwell-game` **[built]** | `remotes.rs`: snapshot buffer and interpolation `INTERP_DELAY_MS` in the past. `input.rs`: keyboard + pointer-lock input (WASD, Space, Shift, C/Ctrl, F3, F4 and its zoom `-`/`=`; while the pointer is locked, left/right click break/place, number keys and the wheel pick a block) and input quantization. `flight.rs`, `flight_speed.rs`: the creative-flight toggle (Phase 4: double-tap Space or Jump, or the touch Fly button; only if `Welcome` allows it; §9.1) and the speed level. `touch.rs`: touch controls for phones and tablets (floating left-half joystick, drag-to-look right half — a short, still touch there is a *tap* that breaks or places — held Jump and Crouch buttons, latching Run and Break/Place buttons, a Fly button, an ⓘ button toggling the F3 overlay, one captured pointer per control, merged into the same sampled input; the DOM elements are the shell's `ui/touchControls.ts`). `eye.rs`, `face_flip.rs`: the eye and the camera's turn-over between the faces. `block_interaction.rs` (§6.5): the target under the crosshair (`raycast_block` from the eye, `REACH_DISTANCE`), the palette (every placeable state, ladders as one slot whose facing follows the placement) and its selection, and break/place actions turned into `BlockEditRequest`s at most once per `BLOCK_EDIT_INTERVAL_MS`. `chunk_stream.rs`: applies `ChunkData` (Generated through the generation workers, Explicit decoded) and `ChunkUnload` to the streamed world, applies `VoxelModification`s in revision order (holding those of chunks still generating; a gap sends `ChunkResync`), starts mesh jobs for changed chunks nearest first (each job's padded voxels copied row by row from the chunk and its 26 neighbours, each looked up once), and tells the game when the terrain around the player is loaded (`gate.rs`, §6.3). `chunk_diff.rs`: the F3 overlay's chunk regenerated and diffed. `hud_text.rs`: the HUD's lines (background work such as terrain loading is a small status in the bottom-left corner, never over the view). |
| `jobs/`, `pool.rs`, `web/jobs.rs` **[built]** | Background jobs (§5.1): chunk generation (and the verification hash), LOD bounds and sections, the F4 map's height samples, chunk and section meshing. `jobs/codec.rs` encodes jobs and results as bytes (one transferred buffer each way); `pool.rs` keeps at most the workers' capacity in flight per kind (generation, meshing), cancels queued chunk jobs on unload and routes results to the streamer, the LOD system and the debug tools; `web/jobs.rs` posts to the least busy worker. Natively the same jobs run inline (tests). |
| `dwell-mesh` **[built, Phase 3d; LOD 4c]** | Greedy chunk mesher: a chunk's voxels with a one-voxel apron (34³) in; positions, normals, face-tinted colours, texture coordinates in blocks and per-vertex atlas tile rectangles out, an opaque and a transparent (water) mesh. Faces are culled like collision (hidden by full cubes; water by water; a ladder draws only its facing plate); faces of full cubes and water merge into rectangles of one material per slice, ladders stay one quad per face. Render meshes only: collision stays in `dwell-physics` (§6.1), so prediction collides with exactly the server's geometry. LOD sections (`lod_mesher.rs`, §6.6): 34³ cells in, flat-coloured greedy meshes in cell units plus per-side skirts out. **Block textures** (`textures.rs`): generated from tiled noise — periodic value-noise fBm whose lattice wraps at the 32-texel tile, so every tile is seamless across blocks — for grass (top, a side with a grass fringe), dirt, stone, sand, rippled water, ladders (rails and rungs) and **[built, Phase 27 S2a]** `grid` (white with a one-texel line along two edges, so every block edge gets a faint line; the sandbox floor's), named by the registry's texture names; packed in a 512² atlas (8 × 8 cells) with 16-texel wrapped gutters, mipmapped down to the level where the gutter is one texel wide (five levels, two texels per tile at the last): filtering a smaller level at a tile's edge would reach past the gutter into the neighbouring tiles and the atlas's empty space, which a grazing view of distant ground samples; the hotbar's swatches are cut from it (`hotbarSwatch`). Each tile's average colour in linear light is the flat colour of LOD faces. |
| `dwell-look` **[built, Phase 7]** | The look as plain data shared by both meshers, the lights, the sky and the fog (WORLD_GENERATION.md §1): one per-face RGB tint table — warm on top, cooler on the sides, blue underneath — in the chunk and LOD meshers alike (`normal_tint` for shaped faces); a warm sun and a green-bounce hemisphere light per face; the exposure (`DEFAULT_EXPOSURE`); the **sky** (`sky_color` and its generated WGSL twin, `sky_wgsl`), the sky frame (`sky_frame.rs`) and the height fog's model (`fog.rs`, unit-tested). Block colours and tiles are tuned to the measured palette; water is turquoise. |
| `render/` **[built]** | The wgpu renderer (WebGPU only, WGSL; [ADR 0028](./adr/0028-rust-port.md)): terrain chunks (a texture repeated once per block across merged quads — `tile.xy + fract(uv) × tile.zw` sampled with explicit gradients so mip selection has no seams — from a mipmapped sRGB atlas, averaged in linear light, crisp up close), LOD sections (§6.6: a section's surface and its six skirts in one vertex and index buffer, each skirt a range drawn when its side shows; stand-ins clipped to their box in the fragment shader), players as capsules with a visor, turned to their heading (lying down and grey when dead; `geometry.rs`), the targeted block's outline, debug line segments, and the sky. Lighting replicates the Three.js build's look: Lambert shading with a sun or moon and a hemisphere light, each weighted to the fragment's own face; then ACES filmic tone mapping with the exposure, a manual sRGB encode (the canvas is a non-sRGB format), then the height fog mixed toward the sky's colour in output space. **[built, Phase 27 S2a]** Terrain faces of a block with the registry's `grid` look option (below) are darkened along world-aligned lines every `every` blocks (`grid_shade`: 0.12 blocks wide, antialiased by the pixel's footprint, its coverage spread over the footprint once that outgrows the line and faded to its average beyond half a period, so distant grids neither shimmer nor alias), before lighting. The vertex carries nothing for it: the shader finds a face's grid from its tile rectangle (the atlas cell it starts in) in a 64-entry uniform table (`tile_grids`, frame group binding 3, written once from `dwell_mesh::tile_grids`), so the terrain vertex stays at 15 floats. Water (opacity 0.55, visible from both sides, no depth writes) draws after the opaque meshes, far to near. Everything is drawn **camera-relative**: the eye is f64, each mesh's origin offset from it is computed in f64 and passed as f32, so precision holds ~8,000 km out. Two depth passes split at `LOD_NEAR_SPLIT_M` (§6.6), 4× MSAA (none on a CPU adapter such as SwiftShader, `samples_for`), a 32-bit float depth buffer. **The shared frame [built, Phase 25 A1]** (`frame.rs`, [ADR 0029](./adr/0029-ray-marched-renderer.md) step A1 = water's W2): the two depth passes draw into offscreen targets, not the canvas — lit, *linear* colour into an `rgba16float` target, premultiplied by coverage (cleared to 0, so alpha is how much of the pixel something covers; water blends into it in linear light), multisampled and resolved by the near pass; and each opaque fragment's **view depth** (metres along the view axis) into an `r32float` target both passes share (the far pass clears it, the near one draws over it: one meaning across the split, where the depth buffer is cleared between the passes and holds each pass's own projection). WebGPU can't resolve `r32float`, so the view depth is multisampled like the scene and stored, and its readers load its samples; the depth buffer stays attachment-only and discarded. A **composite** pass (a full-screen triangle into the canvas) then tone-maps and encodes the resolved colour, fogs it by the mean view depth of the samples it covers, and lays it over the sky of the pixel's view direction by its coverage — the sky is no longer drawn into the scene, and tone mapping and fog run once per pixel instead of in every lit fragment shader. The outline and debug lines draw last, over the composite in output colour (unlit and unfogged, as before), with the near pass's projection, single-sampled, depth-tested in the shader against the stored view depth sample by sample (the debug lines untested). Tone mapping after the MSAA resolve and water blended in linear light change the picture slightly (antialiased edges, the sea's colour), and the outline is no longer multisampled. **GPU pass timing** (`timing.rs`): with `Features::TIMESTAMP_QUERY` (requested when the adapter has it), each pass (far, near, composite) writes timestamps at its start and end, resolved into one of a ring of three `MAP_READ` buffers, mapped after the submit and read on a later frame (a frame finding all three still in flight goes untimed: reading never waits on the GPU), and reported as telemetry samples `gpu.far`, `gpu.near`, `gpu.composite`, `gpu.frame` (DEV_TOOLS.md). `camera.rs`: 75° vertical field of view, capped at 100° horizontal on wide screens, and the view turned over across the midplane; `display.rs`: `?scale=` (0.25–2) renders at a fraction of the resolution. `stats()` reports the frame's draw calls, triangles and GPU memory (F3). Each chunk and section is its own draw. Meshes live in **arena pages** (`arena.rs`): a few 32 MiB GPU buffers per vertex format (terrain, flat, lines), each holding many meshes' interleaved vertices and u32 indices, allocated first-fit from a coalescing free list and written with `Queue::write_buffer` (ordered after the frames already submitted, so a reused region never changes an earlier frame); a run of draws from one page binds its vertex and index buffers once and each draw is one `drawIndexed` (its first index and base vertex). Each draw's uniforms (offset, clip box, tint, turn) are an element of one storage array the shaders index by the draw's instance (draw k is drawn as instance k: no bind per draw, 80 bytes uploaded per draw). The opaque draws are ordered by pipeline and page and then near to far (each pipeline and page bound once; nearer meshes reject what they hide before it is shaded), and a section's surface and the skirts drawn with it, consecutive in its index buffer, draw as one call — each WebGPU call crosses from WASM to JS, which was most of the frame's encoding time (DEV_TOOLS.md). A replaced or removed mesh returns its regions to its page; pages, targets and the draw uniforms are **destroyed** explicitly when they go: WebGPU frees them on `destroy()` or whenever the page's garbage collector gets to them, and wgpu's WebGPU backend does nothing on drop. Native tests run the renderer on wgpu's noop device. |
| `render/` compare **[planned, Phase 25]** | Over the shared frame (built, A1: the row above). **A second, ray-marched terrain renderer** beside the mesher, at every distance ([`RAY_MARCHED_RENDERER.md`](./RAY_MARCHED_RENDERER.md)): `Scene`'s terrain calls become payload-generic (meshes for raster, bricks for the marcher; the workers run the matching job kind), bricks are a fixed-depth brickmap (one 64-tree node of 8³ bricks per chunk, clipmap levels for LOD sections) built in the workers, and compute passes march it with empty-space skipping (cone LOD, masks, column heights, brick distances, a beam prepass), writing a G-buffer the shared composite shades; overlays raster on top through a `frag_depth` pass. `?renderer=raster|march|compare` and compare mode (split, blink, difference) and `bench --compare` exist in **local builds only**; deployed builds draw with the mesher until the owner's decision (A7). |
| `dwell-march` **[in progress, Phase 25 A3a: the CPU core]** | The alternative ray-marched terrain renderer's CPU side, pure data (no wgpu; native and `wasm32`). **Layout** (`layout.rs`, documented word by word for the WGSL port): a clipmap of levels (one per LOD level, cell 2^L m) of slots in the *grid frame* (world metres + 2²³, so every coordinate is in 0..2²⁴); a slot is NONE, ALL_AIR, ALL_SOLID(state) or a node; a node (272 B) is one chunk or LOD section as 4³ bricks of 8³ cells with 64-bit `mixed`/`uniform` masks and 64 brick words (pool offset, uniform state, or — for an empty brick — its Chebyshev distance in bricks to the nearest non-empty one); a brick is a header, a 512-bit occupancy and either raw `u16` states (level 0, 1,092 B; +512 B shape plane when it holds shaped cells) or 1/2/4-bit palette indices (LOD, 136–356 B, raw past 16 states); a node's column grid is a 32² min-max mip of its occupied rows (2.7 KiB). **Occupancy comes from registry data** (`state.rs`): a state is `Empty`, `Transparent` (`liquid`: water stays a raster pass, so it never sets occupancy), `Solid` (`shape: Full`) or `Shaped` (`shape: Shaped`, or a drawn thin look such as a ladder; the shape hook hits the cell's cube until A5's analytic shapes). **Builders** (`build.rs`, deterministic, for the workers): chunk → raw bricks, section → palette bricks, both with column grids (measured: they cut horizon p99 from 455 to 83 steps). **Pool** (`alloc.rs`): one growable word buffer with an O(1) free-slot stack per size class. **Reference marcher** (`march.rs`): camera-relative integer + fraction ray state (plane distances from exact integer differences, so a hit 4,000 km out has the same bits as at the origin), cone LOD by pixel footprint, slot → node → brick → voxel with column-region and brick-distance skips, a re-entry rule after every skip that lands in the cell a voxel DDA would (no cracks; ties to the lowest axis), hit position from cell + face + `t`, a step cap. Tested against a brute-force DDA on random scenes, both faces and far from the origin (`tests/march.rs`). The GPU marcher (a WGSL port over the same words), worker brick jobs and uploads are A3b; the client does not use the crate yet. |
| `render/` water **[planned, Phase 26]** | A dedicated water shader over the shared frame ([ADR 0030](./adr/0030-fluid-looks-and-water-rendering.md), [`WATER_RENDERING.md`](./WATER_RENDERING.md)): normal maps, Fresnel and a glint per fluid look (§6.1), refraction and Beer–Lambert absorption from the scene depth behind the surface, shoreline foam, underwater fog, the water line, Snell's window and caustics, screen-space reflections into the sky; quality tiers (Low to Ultra) in the settings menu, Low on phones. The surface geometry stays still; an eye-submerged test that matches the drawn surface (0.875 of a cell for water) drives the underwater view. |
| `local.rs`, `web/local_worker.rs` **[built]** | Local mode (§2.1): `LocalCore`, the `dwell-sim` server with its world file, stepped by elapsed time; `LocalWorld`, its wasm-bindgen face for the local-mode worker, with the world file on an OPFS sync access handle (§6.4); `worldFileVersion` / `setWorldFileVersion` read and rewrite a world file's version (debug tools and the version-lock e2e test). |
| `physics/` | **[planned, Phase 21]** The client's debris world (a local Rapier world); the prediction world is the predictor's (§9.3). |
| `interp/` | **[planned, Phase 20]** Tier 1 transform interpolation (and bounded extrapolation); player interpolation is in `dwell-game`'s `remotes.rs`. |
| `debris/` | **[planned, Phase 21]** Tier 2 cosmetic debris spawn, simulation, and cleanup. |

**TypeScript shell (`client/src`):**

| Module | Responsibility |
|---|---|
| `main.ts`, `engine/` **[built]** | Creates the `Client` on the canvas, forwards DOM input (keys, pointer lock, mouse, wheel, touch) to it, runs the frame loop, sizes the drawing buffer, applies the settings, and shows the HUD, hotbar and debug tools; `engine/wasm.ts` loads the module once per JS context, `engine/jobWorker.ts` is a job worker (§5.1). |
| `net/` **[built]** | `connect.ts`: chooses how to reach a server — a dedicated server by invite (the Rust client opens the connection: `connectWebTransport`, falling back to `connectWebRtc`; `?transport=` forces one), a friend world through its room, or local mode — and hands friend-world peers and the loopback to the client as `Transport` objects (`Transport.ts`: `sendReliable`, `sendDatagram`, handlers, `binding`); invite parsing (`invite.ts`). `LoopbackTransport` (`loopback.ts`): the local-mode worker's messages. **[built, Phase 5b–5c]** The master client (`master.ts`: signed requests, rooms, TURN credentials, the room socket URL; `roomSocket.ts`: the signaling WebSocket); `peer.ts`: `PeerTransport`, full WebRTC to a browser-hosted friend world negotiated through the room, and `HostPeer`, the host's side of one guest; `hosting.ts`: the host's relay between the room, its guests' peer connections and the local-mode worker (§10.2); `connectToRoom` joins by code (`joinCode.ts`). **[built, Phase 5d]** `connectToCode` resolves a code first: a dedicated server is joined like an invite link (`serverInvite`), a friend world through its room; the master client also resolves typed addresses and lists games on the player's network. **[built, Phase 6]** The master reports the host's app version: `connectToCode` throws `HostVersionError` for a host on another version line (§10.5), and the page then opens the launcher with `?code=…&v=<host version>`. **[built, Phase 5e]** The lobby list (`MasterClient.lobby`, with search and filters) and join receipts (`connectToCode` posts one once it has joined a dedicated server it resolved). |
| `identity/` **[built]** | Device key (§10.4): non-extractable Ed25519 WebCrypto key in IndexedDB; it signs the handshake for the Rust session. |
| `local/` **[built]** | Local mode: the module worker (`worker.ts`) running the Rust `LocalWorld` (`wasmCore.ts`), stepping it every 4 ms of real time and posting its outbox; `world.ts` reads `?world=` and `?seed=`. **[built, Phase 3e]** `worldFiles.ts`: the world file's OPFS sync access handle, opened by the worker before the core starts; the page asks for a save when hidden or closed. **[built, Phase 5a]** `worldIndex.ts`: the world list (names, seeds, types, last played) in local storage, seeds from text, adoption of per-seed files; `worldFiles.ts` also lists and deletes world files. **[built, Phase 6]** The index records each world's `appVersion` and is a cross-version contract (append-only; unknown fields and unreadable records are preserved on rewrite); worlds without a version are listed apart; `worldAccess.ts` says whether this build may open a world (§6.4); `LocalWorld` throws when the world file is locked to another version. |
| `ui/` **[built]** | Connection status overlay (transport, player id, RTTs, server tick, frame rate — `fps.ts`, per full second); HUD (`hud.ts`: crosshair, health, death message, the texts the client returns) and the F3 debug overlay (a capped, scrolling panel headed by its tools with their keys — Copy block info F6, Map F4, Close F3; on touch screens the connection status and the overlay stack below the top hotbar) and a hint of the keys when a game starts (PLAYER_CONTROLLER.md §9; Phase 3e adds the player's chunk regenerated and diffed against the world's: its revision and how many voxels differ from generation; the memory line, `memory.ts`); the F4 terrain map (`mapOverlay.ts`, Phase 3e: 128² columns around the player sampled by a generation worker, coloured by height and hill-shaded, with the player's heading; `-` and `=` zoom from 8 m per column out to the whole disc at 128 km, centred on the origin, with the player a dot on it); the block hotbar (`hotbar.ts`, §6.5: a swatch per palette slot, the selected one highlighted and named; tapping a slot selects it); the flight speed slider (`flightSpeedControl.ts`); the game menu (`settingsMenu.ts`, `viewSettings.ts`, `detailSettings.ts`: a ☰ button in the top-left corner, or Esc, opens a full-screen menu over the dimmed game, with pages — **Settings**: sliders for the height fog's distance, density and height, the full-detail distance, distant detail and memory, §6.6, the tone-mapping exposure and the sky rotation, applied live and kept in local storage; Reset, and Copy JSON to share them — selected in a text box where the clipboard is unavailable; **Controls**: every key and touch control from `controls.ts`, one data table that also feeds the Debug page, the overlay's key hints and the start hint; **Debug**: the debug tools as buttons with their keys; **Host**, in a local world — and Quit to main menu and Resume at its foot; the touch controls are hidden while it is open, so they never take its taps). The screen stacks, bottom to top: the canvas, the touch zones, the HUD (its buttons above the zones), the HUD widgets, the menu button, the menus; menus and play work upright and sideways on phones (no rotate prompt). The look (`style.css`): a daylight sky over terraced voxel hills; main buttons are grass blocks, the title is extruded like a block. **[built, Phase 5a]** The main menu (`mainMenu.ts`: world list with create / play / regenerate / delete — the last two ask to confirm — and Join: paste an invite link, or pick a recently joined server, `recentServers.ts`); in a game the ☰ panel is also the game menu (Resume, Quit to main menu; opened when the pointer is released with Esc); `launch.ts` decides what the page opens. **[built, Phase 5c]** Host… in the game menu of a local world (`hostPanel.ts`: guest limit by platform, who may build and fly; then the join code, invite link, QR code, guests playing and Stop hosting; wired to the page's lifecycle in `src/hostWorld.ts`), and join codes in the Join box and as `?code=` links. **[built, Phase 5d]** The Join box also takes a server address (`host[:port]`, looked up through the master); "On your network" on the join screen lists servers and friend worlds hosted on the player's network; the host dialog's visibility (code only / code + same network). **[built, Phase 5e]** The server browser (`serverBrowser.ts`, in the main menu below Join): the lobby list with a search box and a "New servers" filter; each dedicated server is pinged (`pingServer`, at most four at a time) and one running another protocol version is marked with the handshake's reason and can't be joined; friend worlds show their players; Join opens it by code. The host dialog's third visibility, Public (server list). **[built, Phase 6]** The menu shows the app version and, under About, the licence notices, and a Versions link to the launcher's version page (Phase 6b); each world shows the version that last played it; worlds saved before versioned releases are listed apart to delete; every route opens through the launcher; the server browser labels a host on another version line and joins it through the launcher (`otherLineNote`). |
| `debug/`, `world/`, `protocol/`, `version/` **[built]** | The block dump (`debug/blockDump.ts`, §6.3) over the generated TS block tables (`world/blocks.gen.ts`, `blocks.ts`: canonical state strings); the generated protocol constants (`protocol/constants.gen.ts`); the app version's rules (`version/semver.ts`, §10.5). |

The client never mutates the voxel grid on its own authority. Block edits are sent as
requests; the visual change is applied when the server's reliable delta arrives (an optional
optimistic "ghost" may be shown meanwhile).

### 5.1 Threads and workers

Per [ADR 0007](./adr/0007-threading-model.md):

| Context | Web (Pages) / Capacitor / Electron | Native server |
|---|---|---|
| Main thread | The Rust `Client`: input, prediction, networking, streaming, LOD selection, rendering; the TS shell's DOM UI | Main loop (tick); Rapier steps on it |
| Sim core (integrated server) | **[built]** `LocalWorld` in a module worker (local mode and friend worlds), single-threaded | Native, on the main loop |
| Generation (chunks, the verification chunk, LOD bounds and sections, the F4 map) | **[built]** Job workers (cores − 2, 1–4; at most 2 on phones), each its own WASM instance; LOD jobs queue behind chunk jobs. The local-mode server generates on its tick within a budget | **[built]** Thread pool (cores − 2, at least 1) |
| Meshing (chunks, LOD sections; lighting later) | **[built]** A second set of job workers, as many | — (the server does not mesh; it downsamples LOD sections on its LOD thread, §6.6) |
| Storage (redb) | **[built]** Inside the local-mode worker (an OPFS sync access handle), committed on the tick | **[built]** Save thread |
| Networking | Browser APIs (WebTransport, WebRTC) on the main thread | **[built]** `dwell-net`'s tokio threads |

Workers share no memory: every job and result is one transferred `ArrayBuffer` (`jobs/codec.rs`), and
each worker keeps two jobs (one running, one waiting). Pool sizes are capped, lower on mobile.

---

## 6. Voxel World

### 6.1 Grid & chunks **[in progress]**

> **Prototype content.** Every material that exists today — its name, look, and role (the
> grass/dirt/stone layering, sand at the shore) — and the terrain's style (§6.3: one island) are
> **placeholders** that exercise the systems: meshing, textures, collision shapes, streaming,
> generation, and editing. They are not Dwell's block set or world design and deliberately do not
> constrain it. The architecture fixes only the *mechanisms*: a `u16` block-state id with per-state
> properties (shape, liquid, climbable, strength, density, render style) and one block registry
> generated from data files shared by server and client. Real content replaces the prototype set
> later, through the generator version and the block data files, without architectural change.

Built (Phases 1–3a; registry Phase 8; cut to the baseline in Phase 15, ADR 0025): the prototype
blocks — **air, stone, dirt, grass, sand, water and ladders**, exactly (a test lists the placeable ones), and since Phase 27 S2a the sandbox floor — where
each state has a collision **shape** (`Empty`, `Full`, or `Shaped`, below), `climbable` + facing,
`liquid`, `flooded` and `indestructible` (block data `"indestructible": true`; PLAYER_CONTROLLER.md §6).
**[built, Phase 27 S2a]** One block is indestructible: `dwell:sandbox_floor` (prototype content:
full, white with grid lines, not in the palette, not adaptive), the structure sandbox's ground — the
property bedrock had, reused rather than a new flag; it stays what the structural anchor of
Phase 20 may use. Phase 15 removed
bedrock, the debug launch pad (and its `launch_speed` property, decision D6), the stone slab and
every slope and slab family, sandstone, gravel, snow, ice, log, leaves and the three ores, with
their texture tiles. 32³ chunks with revisions (generated chunks start at revision 0);
`VoxelWorld` (`dwell-world`), generate-on-access with eviction of unmodified chunks on the server, and *streamed*
(no generator; missing chunks read as air) in the client; a flat test world (grass top face at
y = 0, stone down to the midplane, its core — no bedrock), whose layers are a parameter
(`FlatLayers`: top, the three under it, the rest; the voxel and LOD generators take them), so
**[built, Phase 27 S2a]** the **structure sandbox** (generator **100**, `GENERATOR_SANDBOX`) is the
same world made of `dwell:sandbox_floor` alone (`dwell-worldgen` resolves it by name). The movement playground is a structure
file the tests stamp over the flat world (§6.3). **Terrain collision** (`dwell-physics`
`terrain.rs`; Rust port, Phase 24): one static Rapier **voxel collider** (Parry's `Voxels` shape)
holding the solid cells of the chunks around players. Parry classifies each voxel by its
neighbours, so a capsule slides across voxel edges and chunk borders without catching on internal
edges; with f64 positions it is exact anywhere in the 8,192 km world, so it needs no regions or
anchors and there is no seam under a player (PLAYER_CONTROLLER.md §5). Chunks are added on demand
around each player (out to 3 m plus twice a tick's travel, capped), re-synced in the same tick as
an edit or a streamed chunk, and dropped every 64 ticks when more than 2 chunks from every player;
a chunk with no solid cell holds nothing, so open sky costs no collision memory. Every solid state
collides as its full cell: the baseline has only cubes (Phase 15); shaped blocks (Phase 18) will
need their own colliders. The procedural terrain generator and streaming are built (§6.3), and so
are block edits (§6.5, Phase 3d): the placeable set is the registry's `palette` data (a state's
`placeable` flag; not air, liquid or indestructible), read from the same generated Rust registry by
the server and the client.

**Block registry [built, Phase 8]** ([ADR 0015](./adr/0015-block-registry.md),
[`BLOCK_REGISTRY.md`](./BLOCK_REGISTRY.md)). Voxels are runtime **state ids** of a registry of
namespaced blocks with typed properties; a state's canonical string is
`ns:name[k=v,…]` (all properties, keys alphabetical: `dwell:ladder[facing=north,flooded=false]`).
Blocks are data (`shared/blocks/*.json`: properties, shape, density, liquid, climbable, palette
membership, look, colour, textures); `shared/blocks/gen.mjs` generates the Rust tables
(`crates/dwell-blocks/src/blocks.gen.rs`: per-state info, block, property and shape tables,
`REGISTRY_HASH`) and the TypeScript tables the shell's block dump reads (`client/src/world/blocks.gen.ts`,
with `world/blocks.ts` for canonical strings), plus a shared vector (`vectors.txt`, checked by
both); CI fails on stale output. Code names blocks by their string (`dwell_blocks::require("dwell:stone")`),
resolved once at startup; the few the engine itself needs are resolved into `dwell_world::M`. State ids are dense `u16`, assigned in declaration order
(air = 0), each block's states in property order (keys alphabetical, the last varying fastest). The
**registry hash** (FNV-1a 64 over the canonical strings in id order) is carried by `Welcome` (§8.3)
and checked by the client. Runtime ids stay on the wire and in memory (a `u16` per voxel: a
bit-packed chunk-local palette would be 7× smaller but 2.7× slower to scan with the current access
pattern, ADR 0015 — deferred); on disk, chunk palettes hold **world state ids** mapped to strings
(§6.4).

**Fluid looks [planned, Phase 26]** ([ADR 0030](./adr/0030-fluid-looks-and-water-rendering.md)). A
`fluids` table in the block data names each fluid's look (absorption, scatter, roughness, IOR, waves,
foam, caustics, surface drop); a liquid block references one by name (`"look": "fluid", "fluid":
"dwell:water"`), `gen.mjs` validates it and emits a fluid table the renderer uploads, and the mesher
takes the fluid id and surface drop from it (no hardcoded water opacity or drop). Fluids are per
build and looks only: no flow or other behaviour.

**Unbreakable blocks [built, Phase 27 S2a]** ([ADR 0031](./adr/0031-data-driven-structures.md)). The
block data's existing `indestructible` property (bedrock's, kept for Phase 20's structural anchor)
is the flag: no new `breakable` flag was added. The server's edit validation rejects breaking an
indestructible state, and placing into its cell (which would replace a liquid). The structure
sandbox's white floor block (`dwell:sandbox_floor`) is the one block that uses it.

**Grid lines [built, Phase 27 S2a]**. Two data-driven parts draw the sandbox floor's grid, both
usable by any block: the `grid` texture tile (a faint line at every block edge, from the tile; the
mipmaps fade it), and the **`grid` look option** in the block data, `{ "every": N, "strength": s }`
(`N` divides the chunk size, 32, so chunk-local uvs line up across chunks; `s` in (0, 1]; `gen.mjs`
rejects anything else), which the terrain shader draws from world position (§5 `render/`). A
face's grid is found through its texture tile (a per-tile table built from the registry), so every
block drawn with a tile must want the same grid, or none — `gen.mjs` rejects a tile whose blocks
disagree, an untextured block counting as the `plain` tile. The
floor uses the tile and `{ "every": 8, "strength": 0.3 }`. LOD sections draw the tile's average
colour and no lines. The registry hash covers state strings only, so look options do not change it.

**Adaptive natural blocks [planned, Phase 18; formerly 9e]** ([`ADAPTIVE_BLOCKS.md`](./ADAPTIVE_BLOCKS.md),
[ADR 0024](./adr/0024-adaptive-natural-blocks.md)). Natural blocks (block data `"adaptive": true`)
will be stored as cubes and shaped at use from the 3 × 3 columns around them by a rule table kept as
data in `shared/blocks/`, read by the server and the client alike, on the shape mechanism below.

**Shapes [built, Phase 9; no shaped block since Phase 15]** ([ADR 0016](./adr/0016-slope-blocks.md),
[`SLOPE_BLOCKS.md`](./SLOPE_BLOCKS.md)). Every solid state has a **shape**: its top surface is given
by four corner heights in halves of a cell (NW, NE, SE, SW), split into two planar triangles along
one diagonal, hanging from the floor (upright) or the ceiling (inverted); a cube is (2, 2, 2, 2). The
mechanism stays: `shapeFamilies` in the block data generates a material's slope and slab families
(nine slope shapes in four facings and two halves, and the slab; `flooded` means water fills the
shape's open part) — but since Phase 15 no block uses it (the list is empty), so the shape table
holds the empty shape and the cube until Phase 18. The geometry is baked once by
`shared/blocks/shapes.mjs` (`gen.mjs`) into the generated tables: each shape's closed, outward-wound
polygons tagged with the cell face they lie on (or the sloped surface), its side profiles for
culling, volume, convexity and extent — one table, so server, client and meshers agree.
`dwell-blocks` `shape.rs` (`shape_of`, `solid_span_at`/`surface_height_at`, `face_covered`,
`ray_enter_shape`, `vertical_segment_distance_sq`; `+ − × /`, `min`, `max` only, ADR 0010) answers
every point query: **block targeting** (`raycast_block`, the controller's ray probes) enters the
exact shape (a block without a solid, a ladder, is targeted and seen as its whole cell), the
**edit check** tests the capsule against the placed shape's true volume, and the controller's
overlap test (uncrouching) uses the true surface; the terrain collider treats every solid as a
full cell until Phase 18 (above). The exhaustive adjacency test checks every pair of shapes in the
table. The chunk mesher draws shaped polygons with their true normals and a face tint interpolated
by the normal (`dwell-look` `normal_tint`).
A state with a `flooded` property (today the ladder) takes it from its cell on placement (placed
into water: flooded; anywhere else: dry — no water from nothing), breaking a flooded state leaves
water, and a flooded cell counts as water for swimming. Shape picking in the hotbar (the R key, the
placement preview) went with the placed families.

- Voxel = 1 m cube; `uint16` block-state ID (0 = air). The registry defines each state's density,
  strength, and render properties and is shared by server and client (generated from one data set).
- Chunk = **32 × 32 × 32** voxels, addressed by `ChunkCoord(int32 x, y, z)`. The grid is 3D in
  every respect — generation, storage, streaming (§6.3) and LOD (§6.6). The world is 256 chunk
  rows (8,192 m) tall; `int32` coordinates reach the rim of the 8,192 km disc (chunk ±256 000) with
  room to spare.
- Wire / storage encoding **[built]**: per-chunk **palette + run-length encoding**
  (`ChunkData Explicit`, §8.3). Voxels are taken in layer order (x fastest, then z, then y) so
  horizontal strata make long runs; the palette lists materials in order of first appearance; each
  run is a LEB128 length and a palette index (u8, or u16 above 256 entries). The ids are runtime
  state ids on the wire and **world state ids** in the file (§6.4). The encoding is
  canonical: `dwell-protocol` reproduces the golden vectors (made by the independent Python
  encoder) byte for byte and rejects every malformed one.
  In the world file the same bytes are LZ4-compressed (§6.4); the wire does without general-purpose
  compression (QUIC and SCTP do not compress, but Explicit chunks are rare in generated mode).
- Each chunk carries a monotonically increasing `revision` so clients can detect and
  discard stale or out-of-order updates and request resync. **[built, Phase 3d]** Generated chunks
  are revision 0; every `VoxelModification` that touches a chunk advances it by exactly one
  (`Chunk::set_at` + `bump_revision`), so the next expected revision is always current + 1.

### 6.2 Voxel states (the three physics categories)

| State | Where it lives | Physics cost | Synchronized |
|---|---|---|---|
| **Static terrain** | Master grid; the solid cells of chunks near players in one static Rapier voxel collider | ~0 (static) | Reliable chunk data + deltas |
| **Tier 1 dynamic** (gameplay-critical) | Server Rapier world as one compound-shape body per cluster | Active | Unreliable snapshots, 20 Hz |
| **Tier 2 dynamic** (cosmetic debris) | Client-local Rapier world only | Client only | Not synchronized; derived from a reliable event |

Static collision is one voxel collider of the solid cells (§6.1; a height field cannot represent
overhangs or caves). Collision exists only for full-detail chunks near players; LOD terrain (§6.6)
is render-only. Server and client build the same collider from the same chunk data with the same
code (`dwell-physics`), so client prediction collides with the same geometry the server does.

### 6.3 Terrain Generation **[in progress]**

Terrain is **procedural, seeded, and deterministic**: an unmodified chunk is a pure function
`generate(worldSeed, generatorVersion, ChunkCoord)`. The server is authoritative, but because
generation is deterministic, the network and disk only need to carry *differences* from the
generated baseline.

The pipeline structure below (deterministic stages, lattice-sampled fields) is architecture; its
current *content* — one island, its heights and surface materials — is prototype (§6.1).

**[built, Phase 15]** The **baseline rework** (`IMPLEMENTATION_PLAN.md` Phases 14–19,
[`BASELINE_EVALUATION.md`](./BASELINE_EVALUATION.md), [ADR 0025](./adr/0025-baseline-world.md)) cut
the generator down to an **interim** one: one island at the origin, a 2D height field and a fill by
depth — no continents, plates, rivers, lakes, climate, biomes, tints, mountain cascade, 3D density,
caves, ores, trees, boulders or slope stage (their ADRs 0017–0022 are superseded). Its parameters
are code constants: Phase 17 replaces it with an evaluator of terrain definitions (data,
[ADR 0026](./adr/0026-terrain-definitions.md)); Phase 16 resizes the world to the 50 km disc
(the island is already sized for it: 20–30 km in radius). The colour pass of Phase 7
([`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §1) stays.
**[built, Phase 13]** The world is **bifacial**: a second face on the disc's underside, mirrored
about the midplane (y = −2,048), with its own terrain, and gravity toward the midplane on both
sides ([ADR 0023](./adr/0023-bifacial-world.md), [`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md);
*Bifacial world* below). Each phase updates this section, §6.6 and §5 as it lands.

**Built (Phases 3a, 3c, 13, 15; Rust, Phase 24):** the generator (`dwell-worldgen`: `terrain.rs`,
`island.rs`, `noise.rs`, `bifacial.rs`) is **generator version 12** (Phase 15's interim baseline generator; versions 2–11
were the earlier terrains — planet-scale relief, slopes, continents, rivers, climate and biomes, face
B — and are retired: a world saved with one loads as the flat world; the version launcher opens the
build that saved it, §2.1). It is announced in Welcome as generator **8** (`GENERATOR_TERRAIN`), the
default for dedicated servers and local mode; version **0** (flat) remains for tests, and
**[built, Phase 27 S2a]** version **100** is the structure sandbox (the flat world in its white,
indestructible floor block, `docs/STRUCTURES.md`; numbered away from the terrain's
versions, which grow; 1–7 are retired and load as the flat world, as any unknown version does).
Creative flight is the server's flight policy (everyone by default, so on in every local world,
the sandbox included). The **movement
playground** (generator 1 until Phase 15) is now a **structure file**
(`shared/structures/playground.json`, below) that the controller and netcode tests stamp over
the flat world; no world uses it. Players spawn on the first dry, level 9 × 9 patch (top voxels at
one height, at least 2 m above sea level; `SPAWN_CLEAR`, wide enough for the spread of spawning
players to build beside themselves) found in an 8 m spiral from the origin. Debug tooling: **named
views** **[built]** — `client/views/views.json` lists views by name: a seed, a camera (a world
position and a look, or none for the player's eyes at the spawn), a window size, settings-menu
values (fog, detail, exposure), the F3 overlay and LOD level tints, a resolution scale and how long
to wait for the LOD. `client/scripts/views.ts` checks the file (unknown fields, ranges and
duplicate names fail with the entry and field named; Vitest adds a fixture view by data alone);
`client/e2e/views.ts` `openView(page, view)` opens one in a page, places the camera
(`__dwell.placeCamera`, the client's `Game::set_view`: the renderer and the LOD view from there
while the player stays at the spawn, so away from it the view is the LOD's; no protocol change),
and waits until the LOD has nothing left to generate, mesh or ask for; `client/scripts/shots.ts`
shoots any of them into PNGs with their LOD stats, for comparing builds (`--views FILE` for another
views file). Adding a view is adding an entry. Under software rendering a high view at fine
settings takes minutes to settle: the LOD starts jobs once a frame, at a few frames a second. Decided with the owner (2026-10-14): a client-only placed camera rather than a server
teleport (a protocol change), and views as a data file with a runner rather than helpers alone;
the views are per build (checked in), added by hand or by a tool. **[planned]** The
C++ build's command-line tools — `dwell_worldgen_inspect` (ASCII and image height maps, the
island's extent over many seeds, generation timings, first-person terrain views) and `dwell_world`
(world file info and regenerate-and-diff) — went with the C++ tree and have no Rust counterpart
yet (a `dwell-tools` crate, IMPLEMENTATION_PLAN.md Phase 24a). In game (Phase 3e), F4 shows the terrain's
**height map** around the player (the biome map went with the biomes) — `-` and `=` zoom it out and
in, from 8 m per column (1 km across) to the whole disc — and the F3 overlay the player's chunk
regenerated and diffed against the world's; the overlay's **Copy block info** button (or F6) copies
the targeted block and the 5 × 5 × 5 cells around it as JSON (`debug/blockDump.ts`: canonical state
strings by palette index, with the world's seed and generator version, the build, and the player's
position and view) — or shows it to copy by hand where the clipboard is refused — so a spot can be
regenerated and examined exactly.
**Built (Phase 3b):** streaming, the verification chunk, and the generation pools (below).

#### World bounds **[built, Phase 3c]**
A planet-scale world ([ADR 0011](./adr/0011-planet-scale-world.md)):
- **Horizontal:** a disc of `WORLD_RADIUS` = 8 192 000 m (8,192 km, ≈ 2.108 × 10⁸ km²) centred on
  the origin: a column (x, z) is inside when x² + z² < `WORLD_RADIUS`² (`InsideWorldDisc`).
  Columns outside it generate nothing, in every generator: the rim is a cliff about 4 km tall
  between the two seas, with nothing in front of it, and a body that goes over it falls toward the
  midplane and is carried across it by the flip band (*Bifacial world* below).
- **Vertical:** `WORLD_BOTTOM_Y` = −10 240 to `WORLD_MAX_Y` = 6 144 (16,384 m, 512 chunk rows):
  **face A** (the top, today's terrain) from the midplane `MIDPLANE_Y` = −2 048 to `WORLD_MAX_Y`, with
  `SEA_LEVEL` = 0, and **face B** its mirror image below the midplane, [−10 240, −2 048). There is
  no void below the world and no bedrock (below).
- **Precision:** float32 steps by 0.5 m at 8,192 km, so physics is Rapier's f64 build
  (`rapier3d-f64`) on the server and in the browser (f64 world positions through the controller,
  probes, the terrain collider and prediction, §6.1); wire positions are f64 or 1/256 m fixed point
  (§8.3); worldgen never converts a whole world coordinate to float (see Determinism). The client
  keeps positions as f64 and the renderer draws camera-relative (§5). The player controller
  suites and the netcode tests run both at the origin and ~8,000 km from it
  (`DWELL_ORIGIN_X=far`), with the same results; the golden trace is held bit-exactly in the
  origin frame, natively and in WASM.
- **Scale of terrain:** (prototype content, §6.1) since generator version 12 (Phase 15) **one
  island** 20–30 km in radius at the origin (*Interim island layout* below), land up to ~130 m above
  sea level and a sea floor 20 m deep; the rest of the 8,192 km disc is open sea until Phase 16
  shrinks the world to 50 km. The full-detail world is never generated wholesale — distant terrain
  comes from the LOD system (§6.6).

The terrain generators leave everything below `WORLD_BOTTOM_Y` and at or above `WORLD_MAX_Y`
empty. The terrain has **no bedrock**: the rock at the midplane is ordinary stone and can be dug
through, in the flat test world too (its `BEDROCK_LAYERS` went in Phase 15), which has no face B. The anchor for structural integrity (§7.1) is the core zone
of `CORE_ANCHOR_LAYERS` (8) either side of the midplane, by position (Phase 20).

#### Bifacial world **[built, Phase 13]**
([ADR 0023](./adr/0023-bifacial-world.md); design in [`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md).)
The disc has a second inhabited face on its underside; **down is always toward the midplane**, so a
player on face B stands upside down in the world.
- **The mirror.** A voxel at height `y` on face B is the face-local voxel `h = −4 097 − y`
  (`mirror_y`), a chunk row `cy` is face-local row `−129 − cy` (`mirror_chunk_y`): voxels map onto
  voxels and chunks onto chunks. Face B's sea surface is `y = −4 096`. `dwell_world::Face`,
  `face_of_y` and `face_local_chunk` (`voxel.rs`) are the one mapping, used by server and client.
- **Generation.** `dwell_worldgen::BifacialTerrain` (`bifacial.rs`) holds two `TerrainGenerator`s working
  in face-local coordinates: face A's is seeded by the world seed as ever, face B's by
  `face_seed(seed, B)`, a hash through streams no stage uses — its own island and hills. A chunk of
  face B is the face-local chunk flipped vertically (`dwell_blocks::mirror`: a state with a `half` swaps it
  — none since Phase 15 — every other state is its own mirror). Generation stays a pure function of
  `(seed, version, chunk)`. Air chunks, the sky floor and the disc are per face; spawn is on face A;
  streaming, edits and the chunk rows cover both (`kMinChunkY` = −320).
- **Gravity.** The controller's `face` (±1) is the side of the midplane the body is on; the
  controller works in the face-local frame (`dwell-physics` `frame.rs`, PLAYER_CONTROLLER.md §11)
  and gravity points to −y in it. Tier 1 bodies (Phase 20) get a Rapier gravity scale of their side
  (`Physics::step`). **The
  flip band:** within `FLIP_BAND` (4 m) of the midplane a player moves as when swimming, without
  buoyancy or drag; a body moving toward the midplane is **accelerated** (2 g extra), carrying it
  across rather than letting it hover, and moving away the pull fades linearly to zero at it. Nothing
  brakes a body, so a fall of the rim's 2 km passes through at speed and gravity (toward the
  midplane on both sides) draws it back: it swings across the midplane (BIFACIAL_WORLD.md §10).
- **Crossing.** There are no crossing routes: a player digs down through the core or goes over the
  rim, and the face switches (with 5 cm of hysteresis) when the body is across the midplane; the
  controller state is mirrored with the frame and the camera turns over smoothly (a half forward
  somersault about its right axis, the heading turned by 180°; PLAYER_CONTROLLER.md §9).
- **Light and sky.** One sky sphere (`dwell-look` `sky_color`, `sky_frame.rs`): a function of the
  world direction, the viewer's face and a **sky frame** — the day sky's zenith, the sun and the
  moon (three world unit vectors) — graded from day to night by the dot with the zenith, with a haze
  at the viewer's horizon and the sun's and moon's glows. The frame turns as a unit about
  `SKY_AXIS` (horizontal, ⟂ the sun's azimuth) by an angle that is client-only for now (not
  synchronised between players): the settings slider "Sky rotation", `?skyrot=<degrees>` and
  `?skyspin=<degrees per second>`; at angle 0 it is the static sky (sun up on face A, the counter-angled
  moon up on face B). The lights follow the frame, each weighted per fragment by its height over
  that fragment's face's horizon (smooth, `own` in the renderer's WGSL), so a ceiling is not
  lit by the other face's light (§5). The meshers' baked face shading (warm tops, cool sides) does
  not follow the frame: moving the sun and moon separately is phase 2 (`FUTURE.md`).

#### Interim island layout **[built, Phase 15; replaced by the terrain definition's layout in Phase 17]**

`dwell-worldgen` `island.rs`: the signed distance to the coast (m, > 0 on land) of one island at the
origin — `radius − |p + warp(p)| + detail(p)`, where the radius is 25 km plus a smooth 20 km-wavelength
noise of up to ±5 km, the warp a 6 km two-octave vector noise of up to ±3 km, and the detail five
octaves of fractal noise from a 4 km wavelength (±1.6 km). The distance uses the correctly rounded
double square root (deterministic, as ADR 0017's bisectors did). Every noise field is read at
coordinates shifted by a hashed per-seed offset (`noise_offset`, `noise.rs`): gradient noise is zero on
its lattice, which the origin and the level of detail's cell centres would otherwise sit on. Over
seeds the island reaches 28–32 km and the sea starts 19–22 km from the origin
(measured with the C++ build's inspect tool; the Rust generator reproduces its output exactly). The level of detail keeps the coast detail's octaves its cell
resolves.

#### Structure files **[in progress, Phase 27: the format and loader built (S1)]**

Hand-built voxel geometry as data ([ADR 0031](./adr/0031-data-driven-structures.md),
[`STRUCTURES.md`](./STRUCTURES.md) §3). The source of truth is one JSON file per structure in
`shared/structures/` (test fixtures in `shared/structures/test/`, namespace `test`): a format
version, a namespaced `name` matching the file name, a `kind` (`"voxels"` is the only one), `size`
(each edge 1–32), an `anchor` cell inside it, a `palette` from single printable characters to block
state names (`null` keeps what is there, explicit `dwell:air` carves), `layers` (y-slices bottom up
of z-rows of x characters) and optional `variants`, named operations applied in a fixed order:
`rotate` (quarter turns about +Y, clockwise from above), `mirror` (`x` or `z`) and `choices`
(per-voxel weighted blocks for a palette character). Kinds and variant operations are open lists: a
new one is a name the generator validates plus its engine code, with no change to existing files.
`shared/structures/gen.mjs` (run after the block generator: names resolve against
`shared/blocks/vectors.txt`) validates every file, naming the file and entry on any error, and
emits each structure's canonical form and its hash (FNV-1a 64 over a canonical text) as Rust tables:
`dwell-world` `structures.gen.rs` for the build, `tests/fixtures/structures.gen.rs` for fixtures;
CI regenerates them and fails on a diff, and runs the generator's own tests
(`gen.test.mjs`). `dwell-world` `structure.rs` loads a structure by name (`load`), rebuilds the
canonical text to check the hash and resolves names once; `place(at, seed)` puts the anchor at
`at` and picks the orientation and the block choices from the seed with integer hashing (ADR 0010;
choices hash the unturned cell, so they don't depend on the orientation); facing blocks turn with
the geometry through the registry's facing property. The result is stamped over a base generator
(`with_structure`, with a matching air test, `with_structure_air`). The movement playground the
controller and netcode tests walk — a block step, a doorway, a crawlspace, a ladder to a ledge and
a pool — is one (`shared/structures/playground.json`, decision D7), placed with its anchor at the
origin; a test proves it stamps exactly as the old `fill` text file did, which was removed. Worldgen
places no structures (baseline B10; placement is deferred, and would ship on a new compatibility
line).

**The structure sandbox [planned, Phase 27]**. Structures are per build and added only by the
owner, built in a **`sandbox` world type** (flat unbreakable white floor, flight on) with region
tools (box select, fill, copy/paste with rotate and mirror, undo) over a server-validated region
edit, saved to a separate OPFS store (`dwell/structures/`, append-only) and exported as JSON.

**Tree growth [planned, Phase 30]** ([ADR 0033](./adr/0033-tree-growth.md),
[`TREE_GROWTH.md`](./TREE_GROWTH.md)). A tree is a structure whose model is the mature tree: its
palette marks roles (`wood`, `leaf`, optional `trunk`, `root`, `core`) and a `growth` block gives the
engine (`skeleton_reveal`) and rates. `gen.mjs` extracts a skeleton graph from the wood, and one Rust
`stage_voxels(structure, variant seed, age)` reveals it: the trunk 1 m per 30 in-game days, branches
at `branch_rate` of that (default 1/3), scaled by the world's growth speed; the final stage is the
model. Variants jitter the skeleton (branch positions, lengths, droop). Trees are **a layer composed
at chunk load**, not voxels in the saved chunk: generated terrain and its saved edits, then
procedural trees not tombstoned, then planted trees from `growers`, each at its stage for the world
time; growth writes only into air or its own leaves, and a blocked node prunes its subtree. The
server and the client compose the same layers from the same records. Breaking a tree's wood prunes
that subtree (or tombstones the tree); the cut-off part is a Phase 20 body (§7.1). Procedural trees
wait for structure placement in worldgen (Open Decision #24).


#### Generator pipeline **[built, Phase 15: generator version 12]**
Executed per chunk. Every stage reads only noise of world coordinates, never another chunk's data,
so chunks can be generated in any order and in parallel. The 2D fields are sampled on a 4-column
lattice and interpolated bilinearly; the chunk path and the point queries (`column_at`, `solid_at`,
`ground_y`) share that arithmetic.

1. **Layout (2D).** The coast distance of the interim island (above).
2. **Height (2D).** On land the ground rises from sea level at the coast toward 90 m inland
   (`90 · c / (c + 6 km)` for coast distance `c`), with hills of ±30 m (a 2 km, four-octave fBm)
   faded in from 0.5 to 4 km inland; at sea the floor falls linearly to −20 m over 2.5 km; small
   bumps (±2.5 m, a 128 m three-octave fBm) everywhere. Heights over the island span about 0–130 m
   above sea level (sea level is face-local 0; Phase 16 moves it to `h` = 32 over a 10-layer core).
3. **Fill.** A column is solid from the face's bottom (the midplane) to its top voxel
   (`ceil(height) − 1`): stone, under a skin of grass over three of dirt on land, or three of sand
   at the shore (a top voxel less than 2 m above sea level) and under water. Open space below sea
   level is water. There is no 3D density: no overhangs, caves, floating pieces or stability pass,
   and no ores or features. Chunks above every column's height (and above sea level) are air,
   known without generating them (`is_air_chunk`).

A chunk took ~0.07 ms to generate natively and a LOD section 0.55–0.85 ms in the C++ build
(Release), against ~1.05 ms and 0.9–2.1 ms for generator version 11; the Rust generator has not
been benchmarked yet.


#### Determinism **[built]**
Server (native), local mode (WASM), and client (WASM) must produce **bit-identical** chunks
([ADR 0010](./adr/0010-worldgen-noise-numerics.md)):
- The generator is one Rust crate (`dwell-worldgen`), and the client runs the **same code compiled
  to WASM** — there is no second implementation.
- Noise uses integer hashing for gradients and evaluates in strict IEEE float: only `+ − × /`
  and comparisons (Rust never contracts them into fused multiply-adds, and nothing uses
  `mul_add` or fast-math), no library calls — but for the correctly rounded `sqrt` of the island
  layout, which IEEE 754 fixes to the last bit (x86-64 `sqrtsd`, WebAssembly `f64.sqrt`;
  [ADR 0017](./adr/0017-continents-from-voronoi-plates.md) amends ADR 0010).
- A golden test (`shared/worldgen/golden/chunk-hashes.txt`, `dwell-worldgen` `tests/golden.rs`) hashes chunks across the
  pipeline for two seeds — the surface at the spawn, deep rock, the core (the rows either side of the
  midplane), sky, the top of the world, the coast of each face's island where it crosses the +x
  axis (and just inland and offshore), the open sea, the rim and the sea ~8,000 km out — and chunks
  of face B (the rows either side of the midplane and its island's surface); CI runs it natively and under
  WASM (`wasm32-wasip1` on Node's WASI). The hashes were recorded by the C++ generator; the Rust
  port reproduces them exactly, so none were re-recorded.
- `generatorVersion` is bumped for any change that alters output (and the golden hashes are
  regenerated); saved worlds record it.
- **[built, Phase 3c]** Planet-scale coordinates (ADR 0011): noise never converts a whole world
  coordinate to float. World coordinates are integers and wavelengths integer metres; each octave
  splits `x · 2^octave` (64-bit) into an integer lattice cell (feeding the hash) and the offset
  within it, `remainder / wavelength` — one correctly rounded float division (`lattice`,
  `noise.rs`). A test checks that noise is as detailed ~8,000 km out as at the origin.
- **[built, Phase 4a]** LOD generation (§6.6) follows the same rules and has its own golden
  hashes (`shared/worldgen/golden/lod-hashes.txt`: the surface near the spawn, mountains,
  ocean, the rim, terrain ~8,000 km out, the index level and the root, and — Phase 10 — a coast, an
  ocean gap, an island, an interior and the abyss at several levels), checked natively and under
  WASM (`wasm32-wasip1`).

#### Authority, storage, and streaming **[built]**
- **Server storage.** The server's `VoxelWorld` holds chunks only while they are needed: every 64
  ticks, chunks more than 3 chunks (Chebyshev) from every player are evicted if they can be had
  again — unmodified ones (revision 0) by generating them, modified ones once saved (their revision
  in the world file) by reading them back; modified chunks not yet saved stay. With a world file
  (§6.4, Phase 3e) the server knows every saved chunk's coordinates and revision from startup, reads
  saved chunks around players from the file (≤ 16 per tick) instead of generating them, never treats
  them as air, and streams them Explicit. Terrain collision likewise drops chunks more than 2 chunks from
  every player (`TerrainCollision::retain`, every 64 ticks), so memory stays bounded while players move.
- **Server generation** (`dwell-sim` `worldgen_pool.rs`). Each tick the server asks a `WorldgenPool` for
  the missing, non-air chunks within 2 chunks of each player (nearest first; collision reaches 1 chunk and
  meshing reads its neighbours), then those full-mode clients wait for; results are collected at
  the start of the next tick. The dedicated server runs `cores − 2` generation threads (at least
  one); the browser's local mode has no threads and generates on the tick within a budget (4 ms
  natively; in the browser, which has no clock in `std`, about four chunks a tick, §4.3). A chunk
  still missing when collision needs it is generated on the spot (counted as
  `generated_on_access`; a test checks walking never needs it). The spawn region (radius 2
  chunks) and the verification chunk are generated at startup.
- **Air chunks** **[built, Phase 3c]**. A generator's air test (`air_test_for`; for terrain
  `is_air_chunk`, exactly its own "sky above everything" shortcut, cached per chunk
  column) names unmodified chunks that are all air: outside the world's rows or disc, or above the
  highest reach (height + overhang + tree margin) of every column around them. The server never
  generates or stores them — a generated `VoxelWorld` reads them as air — and streams them as
  payload-free `Air` messages in either mode, so open sky costs neither side generation, memory or
  meaningful bandwidth (a test streams a view high in the sky with no chunk generated).
- **Verification.** `Welcome` carries `worldSeed`, `generatorVersion`, and the **verification
  chunk** (the chunk under the spawn). The client generates it in a generation worker and replies
  `WorldgenCheck(hash)` (FNV-1a 64 of its voxels, `chunk_hash`). A matching hash puts the session in
  **generated mode**; a mismatch, or 0 (`?chunks=full`, or no working worker), puts it in
  **full-chunk mode**, where every chunk is sent explicitly. Nothing is streamed before the check.
- **`ChunkData`** has three forms: `Generated(coord, revision)` — "generate this yourself, no
  changes" (18 bytes); `Air(coord)` — an unmodified all-air chunk: nothing to generate or store
  (18 bytes); and `Explicit(coord, revision, palette+RLE)` for modified chunks and for
  full-chunk mode (§6.1 encoding; for terrain chunks near the surface, median ~2 KB, up to ~8 KB;
  all-air or all-stone ones 26 bytes). `ChunkUnload(coords)` tells the
  client to drop chunks.
- **Interest management** (per client, `stream_chunks` in `dwell-sim` `server.rs`). The view is a **sphere** of
  `VIEW_RADIUS_CHUNKS` around the player's chunk (`x² + y² + z² ≤ r² + r`), clipped to the world's
  rows. Chunks the client lacks are sent nearest first, at most
  `MAX_CHUNKS_PER_TICK` per tick and within `CHUNK_BYTES_PER_SECOND` (a byte credit refilled each
  tick; one chunk may overdraw it). When the player's chunk changes, chunks beyond the view plus
  `UNLOAD_MARGIN_CHUNKS` (hysteresis) are unloaded (outside the sphere of r + margin). Dead players
  stream around their body. Beyond the view, the LOD system (§6.6, Phase 4) takes over.
- **Requested chunks** **[built, Phase 4]** (`request_chunks`, `stream_requested`). Beyond
  the view, the client asks for the chunks it will draw at full detail (`ChunkRequest`, from the
  LOD system, §6.6), within `RENDER_RADIUS_CHUNKS` (12, 384 m) of the player's chunk. They are
  queued (at most 1,024, `CHUNK_REQUESTS_PER_SECOND` a token bucket; the rest dropped and counted
  in `StreamStats`) and sent after the view's own chunks within the same byte and per-tick
  budgets, in the forms above — so unmodified ones cost a `Generated` marker, modified ones arrive
  explicitly, and edits reach them like any streamed chunk. A requested chunk (view chunks the
  client asks for included) stays until it is beyond the render radius plus
  `UNLOAD_MARGIN_CHUNKS`. In full-chunk mode only stored chunks are answered (the server does not
  generate for rendering alone).
- **Client** (`dwell-game` `chunk_stream.rs`, `dwell-client` `pool.rs`). Generated chunks go to the
  generation workers (§5.1: cores − 2, 1–4 — at most 2 on phones — each with its own instance of the
  client's WASM module; two jobs per worker; voxels come back as transferred buffers) and then
  into the client's streamed world; Explicit chunks are decoded on the main thread; Air
  chunks only count as loaded (nothing stored or meshed). Pending
  generations still queued are cancelled on unload. Render meshes are built by the meshing workers (§5
  `dwell-mesh`): changed chunks are sent nearest first, a few jobs per worker in flight, their voxels
  (with the apron) copied out of the client's world within a 4 ms frame budget; a chunk arriving marks
  its loaded neighbours for re-meshing, and a chunk waits while a neighbour is still being generated.
  Voxel modifications (§6.5) apply in revision order: those of a chunk still being generated are
  held until it arrives, one for a chunk held as Air creates it, stale ones are dropped, and a gap
  sends `ChunkResync` for the chunk (answered with its current state) and ignores it until then. Prediction starts once the chunks within 1 of the player
  are loaded ("Loading terrain…"). The client treats missing chunks as air, and its terrain
  collider re-syncs a chunk when it arrives or changes.

### 6.4 World Persistence **[built, Phase 3e; LOD cache Phase 4b; block states Phase 8; redb, Phase 24]** (bodies, backups, export UI: later)

Decisions: [ADR 0006](./adr/0006-world-persistence-sqlite.md) (one file per world holding all of
its data) with the storage engine replaced by [ADR 0028](./adr/0028-rust-port.md) §8. Each world
is **one `redb` database file** (`.dwellworld`; redb is a pure-Rust embedded key-value store with
ACID transactions and copy-on-write B-trees) holding **all** of its data; nothing about a world
lives in side files. Chunk and LOD blobs are the protocol's palette + RLE compressed with **LZ4**
(`lz4_flex`, the raw size prepended and checked before decompressing).

| Table | Contents |
|---|---|
| `meta` | Scalars by name: format version, block registry hash, world seed, generator version, spawn, world time, timestamps, **[built, Phase 6]** the app versions that created and last saved the world; **[planned]** preview image |
| `settings` | Name, MOTD, max players, edit and flight policies, autosave interval, allow-list switch, the server key **[built]**; **[planned]** icon, visibility, password hash, online/offline mode, physics/view caps, backup policy; **[planned, Phase 30]** the growth-speed multiplier (default 1), applied by the server to tree growth |
| `block_states` | **[built, Phase 8]** world state id → canonical string (§6.1), ids assigned densely in order of first use and stable for the life of the file |
| `chunks` | Modified chunks only: `(cx, cy, cz)` → revision, generator version, LZ4 palette + RLE blob (same encoding as `ChunkData Explicit`, but the palette lists **world state ids** of `block_states`, not runtime ids) |
| `lod_sections` | **[built, Phase 4b]** Cache of modified LOD sections (§6.6): `(level, i, j, k)` → `lodRevision`, dirty flag, generator version, LZ4 palette + RLE blob of the 34³ cells (empty while dirty and never computed); derivable from `chunks` (its cells are runtime ids: dropped when `meta`'s registry hash changes), rebuilt on a generator or registry change |
| `players` | Keyed by device public key: display name, state blob (version 1: feet `f64×3`, health; later inventory), first/last seen |
| `permissions` | `(public key, kind)` with kind `op` / `ban` / `allow` → reason, granted by, when |
| `bodies` | **[planned, Phase 20]** In-flight Tier 1 clusters (voxel layout, transform, velocities) |
| `tree_tombstones` | **[planned, Phase 30]** Per chunk: the sparse set of chopped procedural tree ids `(chunk, procedural index)` and the pruned node sets of partly cut ones (§6.3 tree growth) |
| `growers` | **[planned, Phase 30]** Player-planted trees, keyed by the chunk holding the root: structure, variant seed, root, planted world time, pruned nodes |

**Built (format 1, `dwell-storage` `db.rs`):** the tables above but `bodies`, `tree_tombstones` and `growers`. Format 1 is the first
Rust format: the port started a new compatibility line (ADR 0028), so files of the C++ build's
SQLite formats are not read (they are version-locked, ADR 0014, and open in their own builds).
The server restores the LOD cache at startup (dirty rows are recomputed), drops and rebuilds it
from the chunks when its rows belong to another generator version, and derives it for saved chunks
it does not cover; each autosave writes the sections written since the last save and those still
to compute (flagged dirty) in the same transaction as the chunks. `WorldDb` translates runtime ids
to world state ids on save (a state new to the file gets the next id and its string is stored, in
the save's transaction) and back on load through the canonical strings, so a world outlives
changes to the code's runtime ids; a chunk holding a string the build's registry does not know is
unreadable and is generated again (aliases, upgrade rules and an unknown-state placeholder are
deferred to world upgrades, `FUTURE.md`). `bodies` (Phase 20) arrives with a format bump.

- **Same code everywhere:** `dwell-storage` (`WorldDb`, the file's tables and records;
  `WorldStore`, the server's handle on it) runs natively on a file and in the browser on a redb
  storage backend over an OPFS `FileSystemSyncAccessHandle` (`dwell-client`
  `web/local_worker.rs`, inside the local-mode worker: no cross-origin isolation needed; files
  under the `dwell/worlds/` OPFS directory, opened by `client/src/local/worldFiles.ts`). One
  format and one code path on every platform. redb wants a `Send + Sync` backend and a JS handle is
  neither (and `unsafe` is forbidden), so the backend holds an id and the handle stays in the
  worker's thread-local table. Handles are exclusive: a second tab on the same world runs without
  persistence. `BytesFile` holds a world file in memory (tests; the client's `setWorldFileVersion`).
  Capacitor will use the OPFS backend or a native-file backend per platform.
- **Loading:** a saved world's seed, generator version and spawn replace the launch options'; its
  world time resumes; the saved chunks' index is read at startup and chunks are read on demand
  (§6.3); a returning player (by device key) spawns where it was saved with its health, unless it
  was dead.
- **Saving:** every `AUTOSAVE_SECONDS` (30 s; the world's `autosave_seconds` setting), the dirty
  chunks (copied), every joined player, the players who left since, and meta are prepared on the
  tick and committed in one write transaction — natively by the `WorldStore`'s save thread (redb's
  readers never wait for the writer, so the tick keeps reading), in the browser inline (no
  threads). A chunk counts as saved at the revision that committed; a failed save leaves its
  chunks dirty for the next. Also on shutdown (`dwell-server` saves on SIGINT/SIGTERM and waits for
  the commit) and when the local-mode page is hidden or closed. A new world's meta is written at
  once. Unix times come from `std::time` natively and from JS (`Date.now`) in the browser.
- **Crash safety:** a save is one redb write transaction: all of it commits or none of it (a test
  aborts a save just before its commit and finds the previous save intact, block states included,
  and redb's integrity check passing).
- **Backups** **[planned]:** dedicated servers take rotating backups.
- **Portability:** a `.dwellworld` file is the database itself, in the same format natively and in
  the browser; **[planned]** export/import UI across dedicated servers, browsers and apps.
- **Version lock** **[built, Phase 6]** ([ADR 0014](./adr/0014-versioned-releases.md), §10.5): every
  save writes `app_version_last` (this build's app version) and, for a new file, `app_version_created`
  into `meta`. A build opens a world only on its **compatibility line** — MAJOR from `1.0.0`,
  MAJOR.MINOR before, the exact version for a pre-release (dev build) — and at or after
  `app_version_last` (a compatible release may add data an older build would not understand).
  `dwell-server` refuses a world it may not open, with a message naming the version to use, before
  writing anything: it reads the meta through redb's read-only database (`WorldDb::peek_meta`),
  since opening a redb file for writing rewrites its header. The local-mode core refuses it too
  (`LocalWorld` throws). A file with no app version was saved before versioned releases: no build
  opens it, nothing migrates it, and the menu offers to delete it. The rule is implemented twice
  (`client/src/version/semver.ts`, `crates/dwell-sim/src/app_version.rs`) and both pass
  `shared/version/vectors.txt`. The browser's own stores shared by all versions (the world index,
  settings) are append-only and keep what they don't understand when rewritten.
- **Format versions:** `meta.format_version`; opening creates any missing table and drops the LOD
  cache when the registry hash changed, in one transaction (a file already current commits
  nothing); a file from a newer build is refused. Later formats add ordered migrations on open.
- **Settings and permissions** **[built: launch options]:** `dwell-server --world FILE` (default
  `world.dwellworld`; `""` keeps the world in memory) opens the world; `--name`, `--motd`,
  `--max-players`, `--edits everyone|ops|nobody` and `--flight everyone|ops|nobody` (creative
  flight, §9.1; default everyone) are saved into `settings`, and `--op KEY` /
  `--ban KEY` into `permissions`; stored settings and permissions apply at startup (ops may edit
  under `--edits ops` and fly under `--flight ops`, banned keys are refused with `Reject(Banned)`, and with `allow_list` set only
  `allow` keys may join, else `Reject(NotAllowListed)`). **[planned, Phase 23]** Admin commands.
- **Tooling** **[planned]:** world file info and regenerate-and-diff (the C++ build's `dwell_world`
  tool) have no Rust counterpart yet (§6.3).

### 6.5 Block Interaction & Inventory **[built, Phase 3d]**

Players break and place blocks. Server-authoritative like every voxel change
(`dwell-world` `edit.rs`, `dwell-game` `block_interaction.rs`):

- **Targeting (client):** each frame `raycast_block` (a DDA from the eye along the view,
  `REACH_DISTANCE` long, in float relative to the eye's cell so it is exact anywhere in the world)
  finds the first *targetable* cell — anything but air and liquids — by entering its exact shape
  (a slab, a slope: `ray_enter_shape`; a ladder, which has no solid, as its whole cell — the server's
  line-of-sight check sees it the same way), and the face it entered through (a sloped surface counts as
  the face its normal mostly points through, the top on a tie); the renderer outlines the cell. The
  client runs the same Rust as the server.
- **Actions:** desktop — left click breaks the targeted block, right click places the selected
  block against the targeted face (only while the pointer is locked; the first click locks it).
  Touch — a tap on the view (a touch on the look half that moves ≤ 12 px and lifts within 500 ms)
  acts, and a latching Break/Place button picks which action. The client sends at most one
  `BlockEditRequest` (§8.3) per `BLOCK_EDIT_INTERVAL_MS`.
- **Validation and application (server):** requests queue per session (≤ 8 per tick) and apply at
  the start of the next step, in order: the player must be alive and allowed to edit
  (`ServerConfig::edits`: everyone, ops only, or nobody), within a token-bucket rate (one edit per
  `BLOCK_EDIT_INTERVAL_MS`, bursts of 3, so edits bunched by the network pass), then
  `check_block_edit`: the target cell within `REACH_DISTANCE` + 1 m (latency slack) of the server's
  view of the eye (checked first, so a far request reads nothing); a targetable cell; the eye in
  front of the targeted face and a ray to its centre or one of four points near its corners
  reaching the target first (line of sight; on a shape, the sample points lie on the polygons whose
  normal points through the requested face); Break — not indestructible (block data; else
  `Unbreakable`), leaving water where a flooded state stood; Place — a placeable state, into an air
  or liquid cell (not an indestructible one: placing would replace it) inside the world's rows and disc (its `flooded` follows the cell), and (solid blocks) not overlapping any
  player's capsule, tested against the shape's true volume. Accepted edits of a step are batched into one
  `VoxelModification` (one entry per chunk, its revision + 1) sent to each client streaming a
  changed chunk; clients streaming it later get the chunk Explicit. The client does not predict
  edits: the change shows when the modification arrives (one RTT), and terrain collision — server
  and client alike — rebuilds before the next tick's probes (PLAYER_CONTROLLER.md §5). Rejections
  are counted per session (`SessionStats`), with the last reason.
- **Inventory:** creative-style and infinite — every *placeable* material in the (prototype, §6.1)
  material table is always available: all but air and liquids (water) — since Phase 15 stone, dirt,
  grass, sand and ladders, five hotbar slots; nothing is consumed or collected. Ladders are one hotbar slot: placed against a side face the ladder is
  mounted on that block, facing out of it; on a top or bottom face it faces the player. A hotbar
  HUD shows the palette with the selected block highlighted; number keys (1–9, 0) and the scroll
  wheel (desktop) or tapping a hotbar slot (touch) change the selection. Selection is client-side
  UI state and travels in each `BlockEditRequest`. The hotbar has one slot per block (ladders as
  one); the shape key and the placement preview of Phase 9b went with the placed shapes (Phase 15).
  Collected, finite inventories are out of scope for now.

### 6.6 Level of Detail: the Whole-World View **[built, Phase 4]** (frame-rate check on real hardware outstanding)

Decision: [ADR 0012](./adr/0012-lod-octree.md) (concepts from the Distant Horizons mod, adapted
to 3D). Everything that should be visible from the camera — on a mountain, in the air, or flying
high above the disc — is drawn, at a detail that drops with distance.

**Grid.** A 3D octree of **sections**. At level L a cell is a 2^L m cube and a section is 32³
cells (32 × 2^L m on a side); **level 0 is the chunk grid**. Section coordinates `(L, i, j, k)`
count from the corner (−2²³, −2²³, −2²³), so cells of consecutive levels nest exactly and one
root section at `LOD_MAX_LEVEL` = 19 (16 777 km) holds the whole disc. The world's
height is 16,384 m: from level 9 a section spans all of it (one row of the octree). **Phase 13:**
sections up to level 6 (2,048 m) lie wholly on one face of the midplane, a row belonging to the face
of its centre; coarser ones straddle it, and each cell takes the face of its centre. A face-B section
is generated as the mirror image of a face-local section (rows reversed, `generate_lod_at` on face B's
generator), downsampled with the block read upside down, and meshed as its mirror image
(`mesh_section` with `mirror`) then turned back, so the surface it draws is the one facing down.
Column bounds carry both faces (`LodBounds`: `lo`/`hi` and, in face-local terms, `lo_b`/`hi_b`);
`kind_from_bounds` judges each face over the rows it owns.

| Level | Cell | Section | Drawn at distances of about (1080p, 75° FOV, 4 px) |
|---|---|---|---|
| 0 | 1 m | 32 m | The streamed chunks around the player (§6.3) |
| 1–4 | 2–16 m | 64–512 m | From the edge of the streamed view to ~5.6 km |
| 5–8 | 32–256 m | 1–8 km | 5.6–90 km; level 8 is the LOD index level |
| 9–19 | 512 m–524 km | 16–16 777 km | Beyond 90 km, out to the whole disc from altitude |

(Level L is drawn from about 176 × 2^L m under those settings; the world is flat, so from the
ground the view reaches the rim wherever terrain does not block it. `LOD_PIXEL_ERROR` was 2 px
in ADR 0012; at 2 px a view from the ground draws ~8,200 sections, at 4 px ~2,800 — measured in
the LOD system's scenario test — so the defaults are 4 px desktop, 8 px mobile.)

**[built, Phase 4a]** The grid and coordinates (`dwell-world` `lod.rs`, shared by server and client;
`dwell-lod` `grid.rs` adds the client's float helpers), downsampling, LOD generation for every
generator (the terrain's in `dwell-worldgen` `terrain.rs` and `bifacial.rs`; the flat world's in
`dwell-world` `lod.rs`) and the column bounds that classify sections without generating them
(`lod_bounds_at`, `kind_from_bounds`). The client runs them in its generation workers (§5.1).

**Content.** A section is 34³ `u16` materials: its 32³ cells and a one-cell apron from its
neighbours for culling border faces, in layer order (x fastest, then z, then y — the chunk
mesher's padded layout), encoded with the chunk palette + RLE codec taking the cells in that
order (`dwell-protocol` `encode_lod_cells`); a terrain section near the surface is
~1.5–6 KB.
- *Unmodified* sections come from LOD generation (`generate_lod(seed, generatorVersion, L, i, j, k)`
  in `dwell-worldgen`): the generator evaluated per cell, with noise octaves and features
  smaller than a cell dropped (no aliasing). Deterministic native vs WASM (§6.3 Determinism), so
  the client generates them itself. Each cell samples the pipeline at its **centre column and
  bottom voxel** — the voxel whose solidity decides a floor under downsampling (below) — so
  generated and downsampled sections agree: fractal octaves whose lattice is finer than a cell
  are dropped (still normalised by the full amplitude), and surface materials follow the depth in
  metres of the cell's top voxel below the column's surface. The apron below a face reads as stone
  (the other face's rock), so the midplane is never drawn. Measured against the downsample of
  generated chunks at the spawn, the coast, a hillside and the sea (levels 1–3)
  (`lod: generation` tests the tolerance: 95%, 95%, mean under half a cell).
- *Modified* sections (any modified chunk below them) are the downsample of their 8 children,
  recursively; unmodified children come from LOD generation, level-0 children are chunks. A 2×2×2
  block becomes filled if ≥ 4 cells are not air — solids and liquids alike (a one-voxel wall or
  floor survives a level; a 1 × 1 pillar does not; a sea keeps its surface at levels whose cells
  are deeper than the sea instead of showing its floor). The material is the most common one
  among the **top filled cell of each of the block's four columns** (the surface seen from
  above), ties to the upper cells — so surface materials survive to the coarsest levels instead
  of the rock beneath them (`downsample_block`, tested on crafted layouts). The flat world's
  generated sections equal the downsample of its chunks exactly (levels 1–3, tested).
- Sections that the generator's column height bounds prove all air, or buried with no exposed
  face, are skipped without generating anything: the bounds of a column of sections
  (`lod_bounds_at(L, i, k)`, from the 34 × 34 columns' 2D fields) give the heights above which
  every cell is air or sea and below which every cell is solid; LOD generation classifies by the
  same bounds, so it returns `Empty` or `Buried` exactly when the bounds say so.

**Server** **[built, Phase 4b]** (`dwell-sim` `lod_propagation.rs`, `Server::update_lod`). A chunk edit (any
source: edits now; collapses and re-bakes later) marks its level-1 section dirty. Each tick at most
`LOD_PROPAGATION_SECTIONS_PER_TICK` dirty sections — lowest level first (children before parents),
then nearest to a player — are snapshotted on the tick (the content of their modified children:
chunks for level 1, cached sections above) and computed off it: the generated section with
the octant of each *modified* child replaced by that child's downsample (unmodified octants keep
the generated cells, so a build changes only the octants above it). Writing a section gives it the
next `lodRevision` from a server-wide counter and marks its parent dirty, up to the root (19
sections per edited chunk; a section dirtied again while being computed is recomputed after).
The dedicated server computes on one thread; the browser's local mode on its tick within a 2 ms
budget (about two sections a tick in the browser, §4.3).
A modified section's stored apron is generated; the server fills it from its modified same-level
neighbours' borders when it sends the section. Sections are cached in `lod_sections` (§6.4). In a
test, an edit reaches the root within 19 ticks and a far client's index 8 ticks after the edit.

**Streaming** (§8.3) **[built on the server, Phase 4b; client 4c]**. After `WorldgenCheck`, the server sends the **LOD index**: every modified
section at `LOD_INDEX_LEVEL` = 8 with its revision (`LodIndex`), then changes as propagation writes
them (`LodIndexUpdate`, coalesced to at most one per `LOD_INDEX_UPDATE_MS`). From the index (entries carry the row `j` since Phase 13: a level-8 column of the world is three
rows) the client knows exactly which sections at level ≥ 8 are modified, and that any section below an
unindexed level-8 section is not; it generates all unmodified sections itself. For the rest it
sends `LodRequest(L, coord, knownRevision)` and receives `LodData` — `Generated`, `Explicit`, or
`Unchanged` — on the `lod` stream within `LOD_BYTES_PER_SECOND` (a byte credit per tick, like
chunks). `Generated` means nothing under the section is modified *yet*: a section still being
computed answers `Generated`, and the index update that follows its level-8 ancestor's write makes
the client ask again. Requests are limited to `LOD_REQUESTS_PER_SECOND` per client (a token
bucket; the rest are dropped and counted) and at most 256 queued. When an index entry's revision
changes, the client re-requests the sections it holds below it. In full-chunk mode (§6.3) every
request is answered `Explicit` (unmodified sections generated by the server's LOD thread, 256 kept).
Every player's builds are therefore visible from anywhere.

**Client** **[built, Phase 4c]** (`dwell-lod` `system.rs`, `dwell-mesh` `lod_mesher.rs`, `dwell-client` `render/`).
Each frame the octree is walked from the root around the **camera** (the eye):
- A node is refined while its cells project larger than `LOD_PIXEL_ERROR` **CSS** pixels, times
  the error scale that keeps the view within the cache (below) (screen-space error; equivalent to
  a log-distance rule at a fixed field of view, and right for altitude and zoom). CSS pixels, not
  device pixels: counted in device pixels, a 2× screen asked for four times the sections (a
  phone's 3×, nine) — in every direction: detail depends on distance only, not on where the camera looks,
  so turning shows what is already loaded instead of popping in (playtest feedback). The view
  decides only the load order: work outside it (and not closer than its own size) ranks 8× lower,
  so what the camera faces loads first and the ring around it after. Level-0 nodes are the streamed chunks: a level-1 section refines into its 8
  chunks as soon as all are loaded and meshed (or air); chunks whose level-1 section is not refined are
  hidden, since LOD draws there (all chunks show until the root is ready). **Full detail beyond
  the view [built, Phase 4]:** the chunks a refined level-1 section needs that the view does not
  stream are asked of the server (`ChunkRequest`, within `RENDER_RADIUS_CHUNKS` less one, nearest
  first, paced under `CHUNK_REQUESTS_PER_SECOND`, re-asked after 10 s if they never came), and the
  section stays drawn until they arrive; modified chunks and structures therefore show at full
  detail out to the **full-detail distance**, a setting in the settings menu (`dwell-lod` `detail.rs`, `ui/detailSettings.ts`:
  96 m, the streamed view, to 352 m, the request radius; default 256 m on desktops, 128 m on
  phones, for memory): level-1 sections within it refine into chunks whatever the pixel error
  (and every level above refines down to them), and beyond it the finest level drawn is 1 — except
  the streamed view's own chunks around the player, which always show. Chunks the client already
  holds are asked for once as well, so the server keeps them after they leave the view.
  Distances for refinement and load order are from the camera's position alone: detail does not
  load ahead along its velocity (it did, and looked bad; removed 2026-10-14). **The player's surroundings never wait for coarse levels:** a level-1
  section around the camera whose chunks have been drawable for `FORCE_CHUNKS_AFTER_MS` (1 s)
  is always reached — the walk descends to it through ancestors whose children are not all
  ready, drawing the ready siblings and drawing, for each unready one, its nearest ready ancestor's mesh
  **clipped to the unready section's box** (a *stand-in*: a clip box tested per fragment in the
  renderer's shader, at most 64 a frame, so a loaded peak is never hidden and no flat cut shows;
  drawn without water or skirts, so a lake may be missing for the moment its section takes to
  load; a buried region gets none), never drawing a coarse section over the chunks. A device that keeps up never takes this path;
  one whose LOD generation is slow or stalled still shows the world around the player.
- **The other face is not loaded or drawn:** the traversal culls sections of level ≤ 6 that lie
  wholly on the face the camera is not on and are farther than `OTHER_FACE_NEAR_M` (512 m, so a
  shaft through the core still shows), except within `RIM_VIEW_M` (16 km) of the rim, where the
  other face can be seen past the edge. A culled section is never scheduled, never makes its parent
  wait and is evicted like any unused one. Sections of level ≥ 7 hold rows of both faces, so they
  are meshed for the viewer's face: the other face's rows are filled with stone (nothing is drawn
  against stone), and seen from face B they are meshed without column surfaces (face A's would be
  wrong there); they are meshed again when the camera's face changes.
- A parent stays drawn until **all its children** are ready (meshed, or known empty or buried),
  then they swap in; unused children are evicted only as whole sibling sets, least recently used
  first, while the cache is over `LOD_CACHE_MB` — the view never has holes. **The view itself
  must fit the cache:** eviction can only drop what the view no longer uses, and on a large or
  2× screen the sections it used filled several times the budget (measured in headless Chromium
  at 1440 × 900 at 2×, standing still: 8,700 drawn sections and a 1.7 GB heap after 150 s, still
  growing, and 70 ms of script per frame with drawing off — the playtest's falling frame rate and
  growing memory). So while the bytes of the sections the walk reaches exceed `LOD_CACHE_MB`,
  the pixel error is multiplied by 1.15 (at most once a second, up to ×16), and divided again
  when they use under 60% of it (at most every 10 s); a finer step that overflows (the sections
  needed change in jumps, a band of distance refining at once) is not retried until the camera
  has moved 512 m, so it settles instead of swinging. The same view now holds at ~1,000–1,500
  sections within 256 MB (×2.3), ~4 ms of script per frame. A test runs a view several times the
  budget and checks it settles within it, holds, and grows finer again where the view needs
  less. **Both are settings** (playtest: the coarser view looked worse, and it should be
  tunable): *Distant detail* (the pixel error, 1–16 CSS px) and *Distant memory* (the cache
  budget, 32–1,024 MB; 192 MB on phones) sliders in the settings menu, next to Full detail (`detail.rs`,
  defaults `LOD_PIXEL_ERROR` and `LOD_CACHE_MB`); a change applies at once and restarts the error
  scale from 1, so the view re-fits the new budget within seconds. (A 2× screen's view before the
  cap was ~2 CSS px with no memory bound.) A test walks, turns
  and rises to 2,000 km with jobs finishing in random order, and checks every frame that sampled
  points of the view lie in exactly one drawn, empty, buried or chunk-refined section.
- Buried says only that a section's **LOD cells** are rock: the bounds leave out caves more than
  a few cells below the surface, which the distant view never shows. Up close the streamed
  chunks are the truth, so a buried section the camera would refine descends to its chunks
  (caves included) rather than ending the walk; one whose children are not yet classified stays
  undrawn as a whole. It shows only chunks the view streams anyway and never asks for more: asking
  for all the rock within the full-detail distance loaded hundreds of chunks with nothing to see
  (a playtest regression: frame time and RTT climbing while standing still). *(Playtest fix: caves deep inside the world were streamed and collidable but
  never drawn; only sections at the rim, whose bounds are open below, showed their caves.)*
- Empty and buried sections come from their column's bounds (one `lodBounds` job per column of
  sections), the rest from LOD generation in the generation workers (a queue behind chunk jobs) and
  meshing in the meshing workers, with at most the pools' capacities in flight. Jobs go in order of
  **projected cell size** (largest first): a parent always before its children, coarse before
  fine and near before far — ADR 0012's "coarsest first, then nearest" globally left the camera's
  own ground coarse until the whole horizon was done.
- Requests: a node asks the server when it may be modified — at level ≥ 8 when the index has an
  entry under it, below when its parent's answer was `Explicit` — so a `Generated` answer covers
  a whole subtree, and a view with no modification in it asks nothing. An index change marks the
  entry's section and its ancestors stale; an `Explicit` answer with a new revision marks the
  held children stale (level ≥ 8 children only if indexed): re-asks go top down with the held
  revision, and `Unchanged` sections keep their content. Requests are paced under
  `LOD_REQUESTS_PER_SECOND` (60/s, 32 per message) and re-sent after 5 s unanswered. In full-chunk
  mode every node asks.
- Meshes are greedy-merged with one flat colour per material (the average of its texture tile in
  linear light, per face group, as a mipmapped sRGB texture averages from afar) and face shading,
  written as linear vertex colours like the chunks' decoded texels (sRGB values used directly drew
  distant land paler than the chunks — playtest), in cell units (a group scaled by the cell size).
  A column's top is tinted toward its material's **side colour** by its gradient (its height
  against its four neighbours'), a third of it up to one half, in eighths: full detail shows the
  sides of one-block steps on a hillside — measured over generated terrain, 5 % of the surface at
  a gradient of 0.2, 15 % at 0.5, ~40 % on the steepest cells, alike at 16–64 m cells — which a
  distant cell's smooth top lacks, so hillsides read as pure grass from afar (playtest; ~7 % more
  triangles, as tops of different tints do not merge). Border faces
  the apron hides go to a per-side **skirt**, shown when the neighbour on that side is not
  drawn at the same level (and not buried), closing cracks between levels. The surface and the
  six skirts are one vertex and index buffer on the GPU, each skirt an index range drawn only
  while its side shows (the renderer's `show_lod_sections` takes a bit per side). A generated
  section next to a modified one takes that neighbour's border into its apron before meshing.
- **Depth:** 5 cm to beyond 16 000 km does not fit one depth buffer, so the frame renders a far
  pass and then, after a depth clear, a near pass (0.05 m to `LOD_NEAR_SPLIT_M`). Each pass draws the meshes whose box
  reaches its range of depth along the view axis, which is what its near and far planes clip (a
  test by straight-line distance could miss terrain toward the sides of the view, where it is
  nearer than the split in depth but farther in distance), so a section straddling the split is clipped at it.
  The far pass's near plane is just inside the split (0.95 × `LOD_NEAR_SPLIT_M`) and moves out to
  0.8 × the camera's face-local altitude when that is farther (nothing is nearer up there),
  keeping depth precise from orbit.
- **Height fog:** a light haze in an exponential atmosphere — extinction
  σ(y) = σ₀·e^(−(y − `SEA_LEVEL`)/H) — applied per fragment from the optical depth along the view
  ray (closed form for a straight ray, written from its lower end so it stays finite from orbit),
  mixed toward the sky's colour in the ray's direction (`dwell-look` `sky_color`: the sphere's
  gradient from the zenith's cerulean down to a pale horizon haze, plus a warm glow toward the sun — the same
  function that draws the sky, so far terrain dissolves into exactly the sky behind it) up to a maximum `density`, so nothing is ever fully hidden. The
  ground's horizon hazes, peaks stand clear of valleys, and the haze thins with altitude by itself:
  from the flight ceiling the whole disc shows (with the defaults, an even veil of ~12% haze; with
  a 100 km distance, under 5%). Three settings, adjustable in
  the settings menu (§5, `ui/settingsMenu.ts`, `ui/viewSettings.ts`, kept in the browser's local storage): **distance**
  (at sea level, where the haze reaches half its maximum: σ₀ = ln 2 / distance; 1 km to the
  world's diameter), **density** (the maximum, 0–100%) and **height** (H, 50 m–50 km).
  `dwell-look` `fog.rs` has the model (`haze_amount`, unit-tested) and the renderer's WGSL the
  same formula, once per pixel in the composite pass (§5 `render/`: the shared frame), from the
  point at the pixel's stored view depth relative to the camera, so it is precise at any distance
  (water is fogged with the opaque surface behind it, as it writes no depth). The composite also
  draws the sky, coloured per pixel by the generated `sky` function of the view direction, behind
  whatever the scene covers; the haze is mixed in after tone mapping and the sRGB encoding, in
  output colour, as the sky is drawn.
- Coarse sections: where a cell is taller than the relief, the column's top cell takes the
  material of the column's surface (not the bedrock its bottom voxel samples; a regression test
  covers it); a sea whose water no cell samples (the cell is deeper than the sea) shows water on
  top, as `Downsample` keeps it (`lod: sea`). Every level draws water as the chunks do: a
  see-through surface (opacity 0.55, visible from both sides) 1/8 m below the cell grid, where the
  chunks' water surface sits (the mesher's `waterDrop`, in cells), over the floor at its true
  depth. The column surface carries each wet column's water level (the sea's; until Phase 15 also
  rivers' and lakes' above sea level, off the cells' grid), and the mesher draws the water's top —
  and its side faces up to it — there, with a floor in that cell kept strictly below it — so near
  and distant water join without a step, a seam or a change of look. (Opaque
  water blocks, then a floor tinted as seen through water from level 3, came first; the tint
  showed a seam, a brighter band and a hard edge where it began, in playtests.) Each section's
  water is its own see-through mesh, drawn after the opaque pass far to near like the chunks'.
- **Column surfaces** (true heights at a distance): a cell counts as filled from its bottom voxel,
  so drawing each column's top cell to its top lifted the ground by up to a cell — ~220 m at level
  8, ~2 km at level 12 — and seas to +2,048 m (level 12) and +6,144 m (level 13): the horizon
  stood too tall, with steps where levels met (playtest). LOD generation therefore also returns
  each column's exact surface (`dwell_world::lod::LodSurface`: height, material, wet — a sea floor — and the
  water's level; the biome tints it carried until Phase 15 went with the tints), unbiased
  to a few metres at every level (`lod: column surfaces`); the client's generation worker passes it
  on, and the mesher draws the top of the column's surface cell at that height, in half-cell
  steps (at most 1/4 cell — a pixel or two — off; finer steps cost several times the triangles),
  except on **shores**: a dry column beside a wet one (by the wet flag, or a liquid topmost cell —
  the sea in the apron row of a section whose bottom is sea level) is drawn at its exact height
  since against flat water a quarter cell shows — half-cell steps put a bank a metre above a
  river under its water (121.5 m drawn at 120 m in 16 m cells) and
  beaches flush with the sea, or raised it to its cell's top, sand standing metres out of the
  water (playtest on 0.6.1-dev.18; ~10 % more triangles over a typical landscape),
  with walls to lower neighbours, tops of equal height merged into rectangles and walls into
  strips (1.2–2× the triangles of plain cell tops). A sea floor inside a water cell (a cell
  taller than the sea is deep) is drawn at its depth, with the water's surface above it — never
  level with that surface (at most half a cell up the water cell that holds it) and never as a
  second top over the solid cell below (whose top it covers, so that top is not drawn): both
  z-fought. Each
  wall — those on a section's border included — is drawn by exactly one section (the one whose
  surface cell it borders) into the opaque mesh; only the part the apron hides below it is a
  skirt. (Border steps drawn as skirts alone were hidden between same-level sections: sky-blue
  cracks along section borders, found in a playtest.) The cells themselves — and so the
  server, downsampling, the protocol and the golden hashes — are unchanged; modified sections
  (`Explicit` from the server) carry no surfaces and keep cell tops.
- **Slopes [built, Phase 9d; removed in Phase 15]**: distant terrain was drawn as the slope pieces
  of the generator's slope stage; both went with the placed shapes. Tops are flat at their surface
  height until Phase 18's adaptive blocks shape them again.
- Debug: the F3 overlay shows sections drawn per level, those shown as chunks, nodes, jobs in
  flight (generation, meshing, requests), cache use (and the view's share of it), the pixel
  error's scale and LOD bytes/s, and the frame's draw calls and triangles over both passes;
  `?lodcolors=1` tints sections by level, `?lod=0` turns the LOD off. (Before the port, the
  TypeScript selection measured ~2–4 ms per frame for ~2,800 drawn sections under Node; the Rust
  one has not been measured yet.)
- **Memory (F3, `ui/memory.ts`):** a phone's browser closes a tab that uses too much, without
  warning (mobile Safari reloads it; playtest: periodic crashes), so the overlay shows what the
  game knows it holds: the page's WebAssembly memory (the Rust client), a local world's server
  (its worker reports its module's memory each second) and each job worker's WebAssembly memory,
  the GPU's terrain and LOD geometry and an estimate of the drawing buffers (4× multisampled
  colour and depth, and the resolved image; the renderer's `stats()`), and Chrome's JavaScript
  heap (shown, not added: it may include the main thread's WebAssembly memory);
  `__dwell.memory()` for tests. **Phone budget:** at most 2 generation and 2 meshing workers and
  Distant memory up to 192 MB (`dwell-lod` `detail.rs`, `MOBILE_MEMORY_MAX_MB`). (The phone
  measurements of the Three.js build — ~237 MB counted after 3 minutes standing still in a local
  world — have not been repeated on the Rust client.)
- **No batching:** the renderer draws each chunk and LOD section with its own draw (dynamic
  uniform offsets into one buffer); the Three.js build's experimental `?batch=1` is
  accepted and ignored. `?scale=0.5` (0.25–2) renders at a fraction of the resolution: a frame
  rate that rises with it is bound by pixels rather than draws (`render/display.rs`).

**Seeing it from above: creative flight** **[built, Phase 4]** (§9.1, PLAYER_CONTROLLER.md §6.7).
The player flies — the body itself, simulated by the server and predicted like any movement — up
to `FLIGHT_CEILING` (24,000 km), from where the whole disc fills about two thirds of the view.
Speed grows with height above the sea, so the climb is exponential (capped at 400 m/s within the
terrain band). In the browser test (local mode, SwiftShader) the player flies to the ceiling
and the whole disc is drawn by the time it arrives.

---

## 7. Physics Pipeline

### 7.1 Voxel Awakening & Clustering **[planned]**

Triggered when the server registers an explosion, a block removal, or a structural failure.

1. **Event trigger.** Server resolves the set of voxels directly removed (e.g. sphere of
   radius *R* for an explosion, attenuated by material strength).
2. **De-chunking.** Removed voxels are cleared from the master grid immediately. A reliable
   `VoxelModification` is queued for all interested clients; affected chunk collision meshes
   are rebuilt (server) / re-meshed (client).
3. **Structural integrity.** Starting from solid voxels adjacent to the removed region, the
   server flood-fills (6-connectivity) looking for an **anchor** (the core zone, or any voxel
   flagged as grounded). Components that reach an anchor stay static. Components that do not
   are **detached**.
   **[Phase 13 decided, built in Phase 20]** The bifacial world has no bedrock: the anchor is a
   positional core zone of `CORE_ANCHOR_LAYERS` (8) either side of the midplane, whose voxels can be
   dug — what remains in the zone still holds both faces
   ([`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md) §2, §7).
   - The search is budgeted (max voxels visited per tick). A component that exceeds the
     budget is treated as anchored for this tick and re-queued, so a single event can never
     stall the tick.
4. **Clustering.** Each detached component (from the same flood-fill) is one cluster. Its
   voxels are removed from the grid (part of the same reliable delta).
5. **Tier classification** (§7.2) by block count and materials.
6. **Awakening (Tier 1 only).** One Rapier body per cluster (`dwell-physics`):
   - Shape: a compound collider of per-voxel cuboids (future optimization: merge runs into
     larger boxes).
   - Mass / center of mass / inertia from per-voxel material density.
   - Inherits impulse from the triggering explosion, if any.
   - Assigned a `NetworkEntityID` (`uint32`, never reused within a session) and added to the
     dynamic simulation. The cluster's voxel layout is sent reliably once
     (`EntitySpawn`) so clients can build the render mesh.

### 7.2 Tiered Synchronization **[planned]**

**Tier 1 — Authoritative sync**
- Threshold: `blockCount >= TIER1_MIN_BLOCKS` **or** the cluster contains a material flagged
  `alwaysAuthoritative` (heavy/gameplay materials).
- Server sends position, rotation, linear and angular velocity for each awake Tier 1 body
  via unreliable datagrams at 20 Hz (interest-managed and prioritized, §8.3).
- Client renders with **interpolation** ~`INTERP_DELAY_MS` behind the latest snapshot;
  bounded extrapolation using velocities when snapshots are late.
- Client also inserts Tier 1 bodies into its local worlds as **kinematic** bodies driven by
  interpolated transforms so the local player and debris collide with them.

**Tier 2 — Cosmetic debris**
- Threshold: `blockCount < TIER1_MIN_BLOCKS` (i.e. 1–3 blocks).
- Server removes the voxels (included in the reliable `VoxelModification`) and sends a
  reliable `PhysicsEvent::Explosion(origin, force, radius)` **in the same reliable
  message batch**, so ordering is guaranteed.
- Client derives debris from the voxels that the modification removed, spawns bodies in its
  local debris world, applies the explosive impulse, and simulates locally. The server ignores
  these bodies entirely; they have no gameplay effect and may differ between clients.

### 7.3 Voxel Sleeping & Re-baking **[planned]**

1. **Sleep detection.** For each Tier 1 body, if `|v| < SLEEP_LINEAR_THRESHOLD` and
   `|ω| < SLEEP_ANGULAR_THRESHOLD` continuously for `SLEEP_SECONDS`, it enters the re-bake
   queue. (Rapier's own sleeping may accelerate this but is not relied on as the signal.)
2. **Grid snapping.** Round the body's rotation to the nearest of the 24 axis-aligned
   orientations and its position to the nearest grid cell; map every cluster voxel to a world
   cell.
3. **Conflict resolution.** If a target cell is occupied, try small offsets (±1 cell,
   upward first); voxels that still cannot be placed are dropped and emitted as Tier 2 debris.
   Cells overlapping a player capsule count as occupied, so re-baking never embeds a player
   in terrain; a player standing on the body switches `groundEntityId` to 0 (static) in the
   same tick.
4. **Re-bake.** Remove the Rapier body → write voxels into the master grid → re-sync the
   affected chunks' terrain collision → run a structural-integrity check on the placed voxels (they may be
   unsupported) → broadcast reliable `VoxelModification` with reason `Rebake` plus
   `EntityDespawn(NetworkEntityID)` in the same batch.
5. **Cosmetic cleanup (client).** Tier 2 debris is despawned after `DEBRIS_LIFETIME_SECONDS`
   or once it comes to rest, and is capped at `DEBRIS_MAX_BODIES` (oldest removed first).

### 7.4 Tunable constants

The source spec left several thresholds unspecified. The values below are **initial defaults**
to be tuned. Shared ones live in `shared/protocol/constants.json`, generated into Rust
(`dwell_protocol`, `crates/dwell-protocol/src/constants.gen.rs`: `SIM_HZ`, `world::…`, `lod::…`,
`players::…`) for the server and the client, and into TypeScript
(`client/src/protocol/constants.gen.ts`) for the shell; the controller's tuning is
`dwell-physics` `config.rs` (`PlayerControllerConfig`); rows marked *client* or *code constant*
name the module that holds them. Those of Phases 20–22 (Tier 1, sleep, debris, integrity) are not
in the code yet.

| Constant | Default | Meaning |
|---|---|---|
| `SIM_HZ` | 60 | Physics steps per second (server and client prediction) |
| `SNAPSHOT_HZ` | 20 | Snapshot broadcast rate |
| `TIER1_MIN_BLOCKS` | 4 | Clusters at or above this size are Tier 1 |
| `SLEEP_LINEAR_THRESHOLD` | 0.05 m/s | Sleep velocity threshold |
| `SLEEP_ANGULAR_THRESHOLD` | 0.05 rad/s | Sleep angular threshold |
| `SLEEP_SECONDS` | 2.0 s | Time below threshold before re-bake |
| `DEBRIS_LIFETIME_SECONDS` | 5.0 s | Max lifetime of Tier 2 debris |
| `DEBRIS_MAX_BODIES` | 256 (desktop) / 96 (mobile) | Client debris cap |
| `INTERP_DELAY_MS` | 100 | Tier 1 and remote-player interpolation delay (2 snapshots) |
| `MAX_TIER1_BODIES` | 512 | Server cap; oldest/smallest force-re-baked when exceeded |
| `INTEGRITY_BUDGET_VOXELS` | 32 768 / tick | Flood-fill budget per tick |
| `AUTOSAVE_SECONDS` | 30 s | World autosave interval (per-world `autosave_seconds` setting); local mode saves every 5 s (`LOCAL_AUTOSAVE_SECONDS`, `dwell-client` local.rs), and when the page is hidden or closed |
| `CHUNK_SIZE` | 32 | Voxels per chunk edge |
| **Players (§9)** | | |
| Controller tuning | see `PLAYER_CONTROLLER.md` §7 | Capsule, speeds, jump, crouch, climb, swim, push, damage |
| `RECONCILE_SNAP_DISTANCE` | 1.0 m | Correction above this snaps instead of smoothing |
| `PREDICT_PROXY_RADIUS` | 16 m | Tier 1 bodies within this use present-time proxies |
| `RESPAWN_SECONDS` | 5 s | Death → respawn delay |
| `MAX_HEALTH` | 100 | Player health |
| `MAX_INPUTS_PER_DATAGRAM` | 4 | Input redundancy per `PlayerInput` |
| `REACH_DISTANCE` | 5 m | Block break/place reach from the eye (§6.5; the server allows 1 m more for latency) |
| `BLOCK_EDIT_INTERVAL_MS` | 100 ms | Minimum time between a player's block edits (§6.5; server: token bucket, bursts of 3) |
| `MAX_RESYNC_CHUNKS` | 64 | Chunks per `ChunkResync` or `ChunkRequest` message |
| `MAX_LOD_INDEX_ENTRIES` | 16 384 | Entries per `LodIndex` / `LodIndexUpdate` message (~256 KB; 16 bytes each since Phase 13) |
| `LOD_MAX_REQUEST_SECTIONS` | 32 | Sections per `LodRequest` |
| **Terrain (§6.3)** | | |
| `MIDPLANE_Y` | −2 048 | The halfway plane: face A starts here, face B's mirror image lies below (§6.3) |
| `WORLD_BOTTOM_Y` / `WORLD_MAX_Y` | −10 240 / 6 144 | Vertical world bounds (16,384 m: face B and face A) |
| `FLIP_BAND` | 4 m | Distance from the midplane within which a body moving toward it is accelerated and one moving away feels a fading pull (§6.3) |
| `CORE_ANCHOR_LAYERS` | 8 | Layers either side of the midplane that anchor both faces (Phase 20) |
| `WORLD_RADIUS` | 8 192 000 m | Radius of the world disc; beyond it, the void |
| `POSITION_FIXED_SCALE` | 256 per m | Fixed-point wire positions (`i32`, ±8 388 km at 3.9 mm, §8.3) |
| `POS64_LIMIT` | 33 554 432 m | Range decoders accept for `pos64` positions (§8.3) |
| `FLIGHT_CEILING` | 24 000 000 m | Highest feet position in creative flight (§9.1; PLAYER_CONTROLLER.md §6.7 has the other flight tunables) |
| `FLY_SPEED_MAX_LEVEL` / `FLY_SPEED_SHIFT` | 39 / 4 | Flight speed slider: highest level (2^19.5 × `fly.speed`) and its bit position in `PlayerInput.buttons` (§9.1) |
| `SEA_LEVEL` | 0 | Water fill height |
| `VIEW_RADIUS_CHUNKS` | 3 | Radius of the sphere of chunks streamed around each player |
| `RENDER_RADIUS_CHUNKS` | 12 | Radius within which a client may request chunks to draw at full detail (§6.3, §6.6) |
| `CHUNK_REQUESTS_PER_SECOND` | 256 | Per-client `ChunkRequest` rate limit (chunks) |
| Terrain collider retention | 2 chunks, checked every 64 ticks | Chunks farther than this from every player leave the voxel collider (`dwell-physics` `controller.rs` `RETAIN_CHUNKS`, `RETAIN_EVERY_TICKS`; §6.1; code constants) |
| `UNLOAD_MARGIN_CHUNKS` | 1 | Hysteresis before chunks leaving the view are unloaded |
| `CHUNK_BYTES_PER_SECOND` | 1 MiB/s | Terrain bandwidth budget per client |
| `MAX_CHUNKS_PER_TICK` | 32 | Chunk messages per client per tick |
| **LOD (§6.6), Phase 4** | | |
| `LOD_SECTION_CELLS` | 32 | Cells per LOD section edge (level 0 = a chunk) |
| `LOD_MAX_LEVEL` | 19 | Root level; one section holds the whole disc |
| `LOD_INDEX_LEVEL` | 8 | Level of the modified-section index sent to clients |
| `LOD_PIXEL_ERROR` | 4 px (desktop) / 8 px (mobile), CSS pixels; a setting (1–16) | Refine a node while its cells project larger than this (ADR 0012's 2 px drew ~3× the sections, §6.6), times the error scale below |
| LOD error scale (client, `dwell-lod` `system.rs`) | ×1.15 steps, 1–16 | Coarser (≤ 1/s) while the view's sections exceed `LOD_CACHE_MB`, finer (≤ 1/10 s) under 60% of it; an overflowing finer step is retried after 512 m (§6.6) |
| Full-detail distance (client, `dwell-lod` `detail.rs`) | 256 m desktop / 128 m mobile (96–352 m) | Chunks are drawn out to it (a setting; §6.6) |
| `LOD_NEAR_SPLIT_M` | 1 024 m | Distance splitting the near and far depth passes |
| `OTHER_FACE_NEAR_M` / `RIM_VIEW_M` (client, `dwell-lod` `system.rs`) | 512 m / 16 384 m | Other-face LOD sections nearer than the first, or anywhere within the second of the rim, are kept (§6.6) |
| `LOD_CACHE_MB` | 256 (desktop) / 96 (mobile); a setting (32–1,024) | Client cache of LOD section content and meshes; the view is held within it (error scale) |
| `LOD_BYTES_PER_SECOND` | 256 KiB/s | LOD bandwidth budget per client (`lod` stream) |
| `LOD_REQUESTS_PER_SECOND` | 64 | Per-client `LodRequest` rate limit |
| `LOD_INDEX_UPDATE_MS` | 1 000 ms | Coalescing interval for `LodIndexUpdate` broadcasts |
| `LOD_PROPAGATION_SECTIONS_PER_TICK` | 8 | Server downsampling budget (off the tick) |
| Exposure (client, `dwell-look` `DEFAULT_EXPOSURE`) | 1.0 (0.4–2.5) | Tone-mapping exposure, a slider in the settings menu (Phase 7) |
| Height fog (client, `dwell-look` `fog.rs` `DEFAULT_FOG`) | distance 4 km, density 50%, height 1.5 km | Defaults of the settings menu's sliders (§6.6, from playtesting); players tune them per browser |

---

## 8. Networking

### 8.1 Transport abstraction
```
interface Transport {
  sendDatagram(bytes)                // unreliable, unordered, ≤ MAX_DATAGRAM_BYTES
  sendReliable(channel, bytes)       // reliable, ordered per channel
  onDatagram / onReliable / onClose
}
```
The server's side is `dwell-net`'s event queue (§4.3); the client's is the Rust session's link
(`dwell-client` `web/transport.rs`), which is either a browser transport Rust opens itself or a
JS object with this interface (`client/src/net/Transport.ts`) that the shell hands over with
`connectTransport`. Implementations:
- **WebTransport** — dedicated servers (primary); opened by the Rust client over `web-sys`.
- **WebRTC** — friend worlds (§10.2; the TS shell's `PeerTransport`, handed over) and the
  dedicated-server fallback (ADR 0008; opened by the Rust client over `web-sys`). One unordered,
  `maxRetransmits: 0` data channel carries datagrams; one ordered, reliable data channel per
  reliable channel (`control`, `world`). The peer's DTLS fingerprint comes from signaling, the
  master server, or the invite link, so no CA certificate is involved. For friend worlds
  **[built, Phase 5c]** both sides are browsers: the channels are pre-negotiated by id, and the
  transport binding is the SHA-256 of the host's DTLS certificate (from the answer's
  `a=fingerprint:sha-256`).
- **Loopback** — local single-player (integrated server in a worker; the TS shell's
  `LoopbackTransport`, handed over).

**Wire mapping [built]:**

| | WebTransport | WebRTC | Loopback |
|---|---|---|---|
| `control` | client-opened bidi stream | data channel id 0, reliable, ordered | worker message |
| `world` | server-opened uni stream | data channel id 1, reliable, ordered | worker message |
| datagrams | QUIC datagrams | data channel id 2, unordered, `maxRetransmits: 0` | worker message |
| `lod` **[built, Phase 4b]** | second server-opened uni stream (first byte 2) | data channel id 3, reliable, ordered | worker message |
| framing | first byte = channel id, then `u32 LE length ‖ payload` per message | none (SCTP keeps message boundaries) | none |
| transport binding | SHA-256 of the server certificate | same (DTLS uses that certificate) | 32 zero bytes |

### 8.2 Channels

| Channel | Kind | Content |
|---|---|---|
| Datagrams | Unreliable | Player input (C→S), physics snapshots (S→C) |
| Stream `control` | Reliable, bidi | Status query, handshake + identity challenge, ping/clock sync, chat, block edit requests, chunk resync requests, LOD requests |
| Stream `world` | Reliable, uni S→C | Chunk data, `VoxelModification`, `PhysicsEvent`, `PlayerEvent`, entity spawn/despawn |
| Stream `lod` **[built, Phase 4b]** | Reliable, uni S→C | `LodIndex`, `LodIndexUpdate`, `LodData` (§6.6); requests go on `control` |

All world-affecting reliable messages go on **one** ordered stream so a voxel removal and the
event/entity that depends on it can never be reordered. Bulk chunk streaming may move to
separate uni streams later if head-of-line blocking is measured to matter. LOD traffic (§6.6) is
bulky and needs no ordering against `world` (sections carry their own revisions), so it gets its
own `lod` stream and budget and never delays chunk data or voxel deltas.

`MAX_DATAGRAM_BYTES` = 1200 (safe QUIC payload); `MAX_RELIABLE_MESSAGE_BYTES` = 1 MiB. WebRTC
reliable messages are additionally capped by SCTP `max-message-size` (256 KiB), so large world
messages (Phase 3 chunk data) must stay under it or be split. All multi-byte fields
little-endian; strings are `u16 byte length ‖ UTF-8`, validated and capped per field
(`shared/protocol/constants.json` `limits`).

### 8.3 Message formats

Every message starts with a `u8` type (`constants.json` `messageTypes`). **Built (protocol v12):**
`DatagramPing` 0x02 / `DatagramPong` 0x82, `StatusRequest` 0x40 / `StatusResponse` 0x41,
`ClientHello` 0x42, `Challenge` 0x43, `ClientAuth` 0x44, `Welcome` 0x45, `Reject` 0x46, `Ping`
0x47 / `Pong` 0x48 (Phase 1); `PlayerInput` 0x01, `PhysicsSnapshot` 0x81, `PlayerEvent` 0x30
(Phase 2); `WorldgenCheck` 0x49, `ChunkData` 0x11, `ChunkUnload` 0x12, and the verification chunk
in `Welcome` (Phase 3b; protocol v3); planet-scale positions and the `Air` chunk form (Phase 3c;
protocol v4); `BlockEditRequest` 0x4A, `ChunkResync` 0x4B and `VoxelModification` 0x10 (Phase 3d;
protocol v5); `LodIndex` 0x13, `LodIndexUpdate` 0x14, `LodData` 0x15, `LodRequest` 0x4C and the
`lod` channel (Phase 4b; protocol v6); `Welcome` flags, the `fly` input button, the `Flying`
state and flags, and the wider `pos64` range for creative flight (Phase 4; protocol v7); `ChunkRequest`
0x4D for full detail beyond the view (Phase 4; protocol v8); the flight speed level in `PlayerInput`'s
`buttons` (Phase 4; protocol v9); `HostStatus` 0x4E and the `ServerClosing` reject reason
(Phase 5c; protocol v10); the block registry hash in `Welcome` (Phase 8; protocol v11); the row `j` in `LodIndex` entries and the `faceB` bits of `controllerFlags` and `playerFlags` (Phase 13; protocol v12) — layouts pinned by `shared/protocol/vectors.txt` (made by the independent Python
reference encoder, including half floats; the Rust codec, `dwell-protocol`, round-trips every
vector byte for byte and rejects every malformed one). The Rust port kept protocol v12's layouts. The remaining formats below are drafts, finalized in the
phase that builds them. Enumerations and bit sets (`inputButtons`, `playerStates`, `playerFlags`,
`controllerFlags`, `welcomeFlags`, `groundKinds`, `playerEventKinds`, `damageCauses`, `chunkForms`, `lodForms`,
`blockEditActions`, `voxelModificationReasons`, `hostStates`) are generated from
`constants.json`; decoders reject unknown values.

**Positions at planet scale — protocol v4 [built, Phase 3c]** ([ADR 0011](./adr/0011-planet-scale-world.md)).
f32 steps by 0.5 m near the rim of the 8,192 km disc, so world positions have their own types:
- **`pos64`** (`f64×3`) where client prediction must match the server exactly: the local
  player's position in `PhysicsSnapshot` and the `Respawn` position in `PlayerEvent`. Decoders
  reject values outside ±`POS64_LIMIT` (33,554 km: the world and the sky above it, up to the
  creative-flight ceiling; protocol v7), NaN included.
- **`posfix`** (`i32×3`, 1/`POSITION_FIXED_SCALE` = 1/256 m, ±8 388 km) for remote players, and
  later Tier 1 entities and `PhysicsEvent` origins — the same 12 bytes as f32×3, with a uniform
  3.9 mm step. Encoders round to the nearest step (halves up, as the reference encoder does)
  and clamp to i32 (so a player flying above 8,388 km appears to others at that height).
- Velocities, rotations and body-local offsets keep their types.

**Client → Server: `PlayerInput` (datagram) [built]**
```
u8   type = 0x01
u32  lastReceivedSnapshotTick
u8   count                      // 1..MAX_INPUTS_PER_DATAGRAM (4): the newest inputs, oldest first
repeat count:
  u32  inputSeq
  i8   moveX, moveY             // analog move vector ×127, clamped to the unit circle
  u16  buttons                  // jump 1 | run 2 | crouch 4 | fly 8; bits 4–9: flight speed level
                                // (flySpeed 0x3F0, 0–63 on the wire, used up to 39; §9.1)
  i16  yaw                      // wrapped fraction of a turn (65536 per 360°); 0 = +Z
  i16  pitch                    // ±32767 for ±90°
```

**Server → Client: `PhysicsSnapshot` (datagram, SNAPSHOT_HZ) [built for players]**
```
u8   type = 0x81
u32  serverTick
u32  ackInputSeq                // last input processed for this client
local player:
  f64×3 position                // pos64: capsule centre (feet + half height)
  f32×3 velocity
  u8   flags                    // PlayerFlags: grounded | crouched | climbing | swimming | dead
  u8   health
  u8   state                    // player::State
  u8   inputBuffer              // inputs queued on the server (client tick-rate steering)
  u32  lastKnockbackSeq         // inputSeq of the latest knockback applied (0 = none)
  controller state (47 B + 12 while climbing + 8 after releasing a ladder;
                    exact fields in PLAYER_CONTROLLER.md §8.4)
u8   remotePlayerCount          // nearest first, as many as fit in one datagram
repeat remotePlayerCount:
  u16  playerId
  i32×3 feet position           // posfix
  f16×3 velocity
  i16  yaw, i16 pitch
  u8   state                    // player::State (animation)
  u8   flags                    // PlayerFlags
```
Phase 20 adds, for riding Tier 1 bodies (§9.4), a `u32 groundEntityId` to the local and remote
blocks (positions become body-local when it is non-zero), and the Tier 1 entity list:
```
u8   entityCount
repeat entityCount:             // ~32 bytes each → ~34 entities per datagram
  u32  networkEntityId
  i32×3 position                // posfix
  u32  rotation                 // smallest-three quaternion, 2+10+10+10 bits
  f16×3 linearVelocity
  f16×3 angularVelocity
```
When more awake bodies are relevant than fit, a **priority accumulator** (distance, size,
speed, time since last sent) selects which bodies go in each snapshot; multiple datagrams per
tick are allowed up to a per-client bandwidth budget.

**Status query (reliable, `control`)** — `StatusRequest` / `StatusResponse`: protocol version,
server name, MOTD, player count / max, icon, online/offline mode. Answered without joining; used
by the server browser to ping dedicated servers **[built, Phase 5e]** (reachability is attested by
players' join receipts instead, §10.3).

**`HostStatus` (reliable, `control`, S→C) [built, Phase 5c]** — `u8 state` (paused 1, resumed
2): a friend-world host's page was hidden or shown again (§10.2); sent to every joined player.
The client shows "Host paused" in its status line until resumed.

**Join handshake (reliable, `control`)**
```
C→S  ClientHello   u16 protocolVersion, str clientVersion, u8[32] publicKey (Ed25519),
                   str displayName
S→C  Challenge     u8[32] nonce
C→S  ClientAuth    u8[64] signature over (nonce ‖ transport binding ‖ publicKey)
                   [optional: account attestation — online mode, §10.4]
S→C  Welcome       u16 playerId, u64 worldSeed, u32 generatorVersion, u32 serverTick,
                   i32×3 verificationChunk, u8 flags (WelcomeFlags: 1 = flight — this
                   player may use creative flight), u64 registryHash (the block registry
                   hash, §6.1: runtime state ids on the wire mean the same blocks only if
                   it matches; the client rejects the server otherwise)
     or Reject     u8 reason (ProtocolVersion, Banned, Full, NotAllowListed, AuthFailed,
                   Malformed, Replaced, ServerClosing — the server is shutting down or the
                   friend-world host stopped hosting), str message — followed by closing
                   the session
C→S  WorldgenCheck u64 hash — FNV-1a 64 of the client-generated verification chunk (u16 LE
                   voxels in chunk index order); 0 asks for full-chunk mode. Once, after
                   Welcome; the server streams nothing before it (§6.3)
```

**Client → Server: `BlockEditRequest` (reliable, `control`) [built, Phase 3d]** (§6.5)
```
u8   type = 0x4A
u8   action                     // Break 1 | Place 2
i32×3 cell                      // world voxel coordinate of the targeted cell
u8   face                       // 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z (the targeted face)
Place only: u16 material        // the block placed against the face
```

**Client → Server: `ChunkResync` (reliable, `control`) [built, Phase 3d]** — a revision gap (§6.3)
```
u8   type = 0x4B
u16  count                      // 1..MAX_RESYNC_CHUNKS (64)
repeat count: i32×3 chunkCoord  // answered with ChunkData for those the client is streamed
```

**Client → Server: `ChunkRequest` (reliable, `control`) [built, Phase 4; protocol v8]** — chunks
beyond the view to draw at full detail (§6.3, §6.6)
```
u8   type = 0x4D
u16  count                      // 1..MAX_RESYNC_CHUNKS (64)
repeat count: i32×3 chunkCoord  // streamed as ChunkData within RENDER_RADIUS_CHUNKS, nearest asked first
```
The signature covers `"dwell-auth-v1" ‖ nonce ‖ transport binding ‖ publicKey`; the binding
(§8.1) ties it to the server certificate, so a signed challenge cannot be relayed to a different
server. **[built]** A successful login for a key that already has a joined session replaces it:
the old session receives `Reject(Replaced)` and is closed (so a dropped connection can rejoin
immediately). `Ping`/`Pong` (reliable) and `DatagramPing`/`DatagramPong` carry the client time and
server tick for RTT and clock sync.

**Server → Client: `ChunkData` (reliable, `world`) [built]**
```
u8   type = 0x11
u8   form                       // Generated = 0 (client generates; no payload) | Explicit = 1
                                 // | Air = 2 (all air: nothing to generate or store; no payload)
i32×3 chunkCoord, u32 revision
Explicit only (§6.1): u16 paletteCount (1..32768), u16 × paletteCount materials,
     then runs in layer order until 32768 voxels: LEB128 length (1..32768, minimal),
     palette index (u8, or u16 when paletteCount > 256)
```

**[planned, Phase 30]** `ChunkData` gains the chunk's **tree records** (its tombstones and pruned
node sets, and the planted trees rooted in it), from which the client composes the tree layer as
the server does (§6.3); a tree's stage change while loaded is a `VoxelModification` with reason
`Growth`. Both arrive on a new compatibility line ([ADR 0033](./adr/0033-tree-growth.md)).

**Server → Client: `ChunkUnload` (reliable, `world`) [built]**
```
u8   type = 0x12
u16  count                      // 1..65535
repeat count: i32×3 chunkCoord
```

**Server → Client: `PlayerEvent` (reliable, `world`) [built]** — to every joined client
```
u8   type = 0x30
u8   kind                       // Knockback 1 | Damage 2 | Death 3 | Respawn 4
u16  playerId
u32  serverTick                 // tick the effect was applied
u32  inputSeq                   // that player's input processed on serverTick (predicted replay)
Knockback: f32×3                // velocity change
Respawn: f64×3                  // pos64: respawn feet position
Damage:  u8 amount, u8 cause    // cause: Fall 1 | Crush 2 | Explosion 3
Death:   u8 cause
```

**LOD messages [built, Phase 4b; protocol v6]** (§6.6). Section coordinates are
`u8 level, i32×3 (i, j, k)` counted from the LOD grid's corner.
```
S→C LodIndex 0x13       (lod) u8 flags (1 = last), u32 count (≤ 16384),
                              repeat: i32 i, i32 j, i32 k, u32 lodRevision
                              // modified sections at LOD_INDEX_LEVEL (j: Phase 13, protocol v12);
                              // may span several messages, the last flagged; count 0 allowed
S→C LodIndexUpdate 0x14 (lod) u16 count (1..16384), repeat: i32 i, i32 j, i32 k, u32 lodRevision
                              // coalesced, at most one per LOD_INDEX_UPDATE_MS
C→S LodRequest 0x4C (control) u8 count (1..32), repeat: u8 level (1..19), i32×3 section,
                              u32 knownRevision (0 = none)
S→C LodData 0x15        (lod) u8 form (Generated 0 | Explicit 1 | Unchanged 2),
                              u8 level, i32×3 section, u32 lodRevision,
                              Explicit only: palette + RLE as ChunkData Explicit over the
                              34³ cells of the section and its one-cell apron
```

**Server → Client: `VoxelModification` (reliable, `world`) [built, Phase 3d: reason Edit]**
```
u8   type = 0x10
u8   reason                     // Edit 1 | Explosion 2 | Collapse 3 | Rebake 4 | [planned, Phase 30] Growth 5
u32  serverTick
u16  chunkCount                 // ≥ 1: the changed chunks this client streams
repeat: i32×3 chunkCoord, u32 newRevision (the chunk's previous revision + 1), u16 changeCount (≥ 1),
        repeat: u16 localIndex (x|y<<5|z<<10, < 32768), u16 material
```

**Server → Client: `PhysicsEvent` (reliable, `world`)**
```
u8   type = 0x20
u8   kind                       // Explosion = 1
i32×3 origin (posfix), f32 force, f32 radius
```

**Server → Client: `EntitySpawn` / `EntityDespawn` (reliable, `world`)** — `NetworkEntityID`,
initial transform, and (spawn only) the cluster voxel layout (local offsets + materials).

---

## 9. Players: Physics-Based Characters **[built]** (Tier 1 interactions: Phase 20)

Full specification: **[`PLAYER_CONTROLLER.md`](./PLAYER_CONTROLLER.md)** — a port of the
[Physics Player Controller](https://github.com/zacharysnewman/physics-player-controller)
(its deterministic Quantum 3 version) to Dwell's voxel world, in Rust on Rapier (`dwell-physics`;
first written in C++ on Jolt and re-tuned on Rapier by the port, ADR 0028). This section is the
architectural summary.

### 9.1 Character model
- Each player is a **dynamic Rapier body** (capsule, rotations locked, gravity scale 0,
  frictionless, never sleeps, continuous collision detection so fast flight cannot tunnel)
  driven by **velocity layers**: horizontal, vertical, and
  exclusive climb/swim/fly layers. The aggregate pass writes the summed target velocity to the body
  before each physics step.
- Whatever the solver does to the body (collisions, falling clusters, explosions) shows up next
  tick as a deviation from the target, is **absorbed** as external velocity, and decays. That makes
  players physically reactive without losing tight, predictable movement.
- The controller is plain Rust in `dwell-physics` (`controller.rs`: per-player controller state
  plus ordered passes; transcendental functions from `libm`, so native and WASM agree bit for
  bit). The client runs the same code in its WASM build (§9.3).
- Probes (ground/ceiling rings, wall rays, overlap tests) query the **voxel grid** directly (DDA,
  `query.rs`) for terrain and Rapier only for moving bodies (PLAYER_CONTROLLER.md §5).
- **[built]** `dwell_physics::controller::Players` runs the pipeline for every player of a physics
  world and keeps the terrain collider around them (§6.1); the whole PPC test suite is ported to
  voxel geometry (`crates/dwell-physics/tests`: voxel geometry, crouch, steps and air, step
  smoothness, knockback, flight, platforms, climbing and swimming, a five-player golden trace
  stored as exact floats, and the 64-player < 1 ms/tick gate — ~0.5 ms in Release with
  `enhanced-determinism`), and runs at the origin, ~8,000 km from it and on face B. Player bodies
  are in the `Character` collision group (terrain, Tier 1 and characters are Rapier interaction
  groups, the former Jolt object layers), have f64 positions (ADR 0011), and read their contact
  normals from Rapier's narrow phase after each step (`Physics::contact_normals`). Rapier's
  default 400 m/s speed limit is raised for creative flight.
- **Creative flight** **[built, Phase 4]** (PLAYER_CONTROLLER.md §6.7): an exclusive layer while
  the input's `fly` mode bit is set — no gravity, move along the view's yaw, jump up, crouch down,
  speed growing with height above the sea up to `FLIGHT_CEILING`. A **flight speed slider**
  (top right while flying; the − / = keys; `ui/flightSpeedControl.ts`, kept in local storage) sets
  a level carried in every input (protocol v9) that raises the speed to at least 11 m/s × 2^(L/2),
  up to about the speed near the ceiling — a true minimum: the 400 m/s cap below `WORLD_MAX_Y`
  limits only the height-based speed. The
  client toggles flight (double-tap Space or Jump, or the touch Fly button); the server's flight policy (`--flight`, told to the
  client in `Welcome`) decides who may, clearing the bit for anyone else. While flying the client
  keeps predicting even where the streamed terrain has not arrived yet.

### 9.2 Physical interactions

| Interaction | Behavior | Authority |
|---|---|---|
| Voxel terrain | Collide; step-up ≤ 0.45 m (slabs, debris); full blocks need a jump (optional auto-jump); ladders are climbable materials; water switches to the swim layer | Server; client predicts |
| Player → Tier 1 body | Solver push, capped by contact mass scaling (`maxPushForce`, `pushableMassLimit`) | Server only; client does not predict body motion |
| Tier 1 body → player | Solver push, absorbed as external velocity and decayed | Server; client corrected via reconciliation |
| Standing on a Tier 1 body | Carried by the body's linear + angular velocity at the player's position (PPC platform model); camera turns with it | Server; client predicts in the body's frame (§9.4) |
| Crushing | Opposing contacts with an approaching Tier 1 body sustained for `crushTicks` | Server |
| Other players | Both dynamic: they block and push each other; standing on heads is grounded but not carried by default | Server; remote players are kinematic on the client |
| Tier 2 debris | Bounces off the local player; never affects player movement | Client only |
| Explosions | Radial velocity change with falloff and upward bias (PPC `AddExplosion`), plus damage | Server; client replays at event tick (§9.3) |
| Falling | Fall damage from the controller's `Landed` impact speed | Server |
| Voxel edits | Block removed under feet → normal walk-off; placement into any player capsule is rejected | Server |

### 9.3 Prediction & reconciliation **[built]**
Details and measurements: PLAYER_CONTROLLER.md §8.
- Fixed 60 Hz step on both sides. The client runs the controller pipeline for the local player
  against a small prediction world (terrain, kinematic proxies, one dynamic body), records
  `{inputSeq, input, controller state, body state}` per tick, and sends inputs with the previous 3
  for loss resilience.
- The server applies inputs in sequence order, one per step, through a 2-input jitter buffer; a
  missing input repeats the last known one (and re-primes the buffer); inputs queued beyond 6 for
  a whole 30-tick window are skipped (a burst from a slow client's frame is not). The client
  catches up at most 15 ticks a frame (250 ms), so a device at 4 fps or more keeps the server's
  time (PLAYER_CONTROLLER.md §8). The client nudges its tick
  rate ±2 % to keep the server's queue (reported in snapshots) between 1 and 4. Snapshots carry `ackInputSeq`, the authoritative body state, and the local player's
  controller state (layers, timers, crouch/climb/swim, ground ref).
- On snapshot: if the stored prediction for `ackInputSeq` matches within tolerance, nothing is
  replayed. Otherwise the client restores the server state, replays unacked inputs (pipeline +
  prediction-world step), and smooths the visible correction; errors above
  `RECONCILE_SNAP_DISTANCE` snap.
- **Server-originated impulses** (knockback, explosion) arrive as a reliable `PlayerEvent` with the
  tick and the player's `inputSeq` they were applied at; the client inserts them into history at
  that input and replays. Snapshots carry `lastKnockbackSeq`, so a snapshot that overtakes its event
  is smoothed like a knockback rather than snapped. Built with the debug launch pad (Phase 2; the pad
  was removed in Phase 15, the netcode test now issues the knockback from the server).
- The prediction logic is Rust (`dwell-physics` `predictor.rs`), shared by the browser client
  (on the main thread, in `dwell-client`'s game loop) and the native netcode tests (`dwell-sim`
  `tests/netcode.rs`: 150 ms / 20 ms / 5 % link). Corrections made while the player touched
  another player's proxy are smoothed up to the knockback bound (4 × the snap distance): Rapier's
  player-vs-player bumps mispredict more than Jolt's did.
- Native and WASM physics are kept bit-identical: Rapier's `enhanced-determinism` feature (which
  also turns off SIMD) and `libm` for the controller's transcendental functions. CI checks it with
  the controller's golden trace, held bit-exactly natively and under `wasm32-wasip1` (the
  native↔WASM divergence check); reconciliation absorbs any residual mismatch.

### 9.4 Moving platforms & time frames
The locally predicted player lives in the *present*, while Tier 1 bodies are normally rendered
`INTERP_DELAY_MS` in the *past*. Mixing the two naively makes riding or dodging a falling
structure unplayable. Therefore:
- **Ground-relative state.** When a player stands on a Tier 1 body, snapshots report
  `groundEntityId` and the player position/velocity **in that body's local frame**. The client
  predicts in the same frame, so riding a moving body is stable regardless of latency.
- **Present-time proxies.** Client collision proxies for Tier 1 bodies within
  `PREDICT_PROXY_RADIUS` of the local player are **extrapolated to predicted time** from the
  latest snapshot (position + velocities), not interpolated. The body the player stands on is
  also *rendered* at present time so the player's feet stay planted. Bodies farther away stay
  interpolated.
- Mispredicted contacts with these proxies are corrected by normal reconciliation.

### 9.5 Remote players **[built]**
Remote players are interpolated like Tier 1 bodies (in the ground body's frame when
`groundEntityId` ≠ 0), exist in the client's physics worlds as kinematic capsules, and are
animated from `State` and flags in snapshots.

### 9.6 Health, death & respawn **[built: fall damage, death, respawn]**
- Health (`MAX_HEALTH` 100) is server-authoritative; damage sources are fall (built: from the
  `Landed` impact speed above 12 m/s, 8 points per m/s, and falling out of the world), crush
  (Phase 20), explosion (Phase 21). `Damage`, `Death`, and `Respawn` are `PlayerEvent`s to every
  client; a dead player's body leaves the physics world, and snapshots mark it `dead` where it
  died. The server respawns the player at the spawn point after `RESPAWN_SECONDS`.
- On death clients currently draw the body lying down (a cosmetic pose). The **cosmetic ragdoll**
  (in the client's local Rapier world, Tier 2 rules — local, unsynchronized, despawned on respawn)
  arrives with the client debris world in Phase 21.

### 9.7 Anti-cheat implications
Clients only send inputs, never positions, so speed/teleport/fly hacks are structurally
impossible; the server's simulation is the only source of player state. Creative flight is an
input the server honours only for players its flight policy allows.

---

## 10. Multiplayer Hosting, Discovery & Identity **[in progress]**

Decisions: [ADR 0003](./adr/0003-multiplayer-hosting-model.md) (hosting model),
[ADR 0004](./adr/0004-player-identity.md) (identity), [ADR 0005](./adr/0005-domain-and-origins.md)
(domain). Two hosting tiers — always-on dedicated servers and client-hosted friend worlds —
follow from browser constraints: pages cannot accept inbound connections, and hosts rarely have
trusted certificates.

### 10.1 Dedicated servers
- Distributed as native binaries (Windows/macOS/Linux) and a Docker image, built by CI per
  release. The Electron app can launch the same binary as a background process ("Host world").
- Transports: WebTransport, WebRTC fallback (ADR 0008). Certificates per §2.3. Invite links
  **[built]**: `?join=host:port&cert=<sha256 hex>[&rtc=<port>&ice=<ufrag>:<pwd>]`. The WebRTC part
  requires an IP-literal host (WebRTC host candidates can't carry DNS names). `dwell-server
  --advertise <ip>` sets the address printed in the link and used as the WebRTC candidate.
  **[built, Phase 6]** The link also carries `&v=<the server's app version>`, which the launcher
  reads to open a client build on the server's compatibility line (§2.1); the server prints its
  version at startup and refuses a world it may not open (§6.4). **[planned, Phase 23]** The
  licence notices beside the binary (the C++ build copied them there; `cargo build` does not).
- Operator settings, stored in the world database's `settings` table (§6.4; **[built]** name, MOTD,
  max players, edit policy and autosave interval, set by launch options): name, MOTD,
  icon, max players, visibility (public / unlisted / none), password or allow-list,
  online/offline mode (§10.4), physics and view-distance caps (`MAX_TIER1_BODIES`, view radius),
  autosave and backup schedule. Edited by admin commands or the server CLI.
- Admin commands (ops, kick, ban by public key), UPnP/NAT-PMP port mapping with port-forwarding
  guidance when it fails.
- **[built, Phase 5d]** Master registration: a persistent Ed25519 server key (its seed in the
  world's `settings` as `server_key`; a new one each run for `--world ""`) signs a heartbeat to
  `POST /v1/servers` at start and every `--heartbeat` seconds (default 30), sent by a background
  thread in `dwell-net` (`master.rs`: `Master::start`, `heartbeat`, `status`, `stop`; `ureq` over
  rustls, signatures with `ring`, the format of `shared/master/vectors.json`, tested against it).
  The heartbeat (its JSON built by `dwell-server`) carries the port, RTC port and ICE
  credentials, the current certificate SHA-256, the LAN addresses (`if-addrs`: IPv4 and
  non-link-local IPv6, no loopback), name, MOTD, players and limit, protocol version,
  visibility, tags (`--tags a,b`, Phase 5e: for the lobby list's search) and heartbeat interval; `--advertise` is sent only when given. `--visibility
  public|unlisted|none` (default unlisted; public also puts it in the lobby list, Phase 5e; `none` never contacts the master; CI and the Electron
  smoke test use it), `--master <url>` (default the deployed master). The server prints its join
  code and a `?code=` link once the master answers, and on shutdown (after its players'
  `Reject(ServerClosing)`) tells the master it is leaving, so it disappears at once. Players then
  join by **code** or by typing the **address**, resolved to address + cert hash through the
  master (§10.3); WebTransport's certificate-hash pinning works for any address the server is
  reached at, and the ICE-lite WebRTC fallback answers on every interface, so LAN addresses need
  no `--advertise`.
- **[built, Phase 5c]** Shutdown (SIGINT/SIGTERM) sends `Reject(ServerClosing)` ("The server is
  shutting down.") to every session before the network stops.

### 10.2 Friend worlds
- Any client hosts its **integrated server** — the sim core already used for local
  single-player — and "opens" it through the master server. Browser, Capacitor, and Electron
  all qualify.
- Guests connect over **WebRTC** (§8.1). The master relays the offer/answer and ICE candidates
  (signaling); STUN enables direct connections; a **TURN relay** carries traffic when NAT
  traversal fails (common on mobile data).
- Host profiles cap the load: e.g. mobile 4 players, desktop browser 8, with reduced physics
  caps. When the host backgrounds the app/tab, the world pauses and guests are notified; when
  the host quits, the session ends (no host migration — [ADR 0009](./adr/0009-friend-world-lifetime.md)).
- Saves live on the host (browser: OPFS; apps: OPFS or app storage) as the same redb world
  file as dedicated servers (§6.4), so a friend world can be exported to a dedicated server.
- **[built, Phase 5c] Browser hosting.** "Host…" in the game menu of a local world sets the
  guest limit (host profiles: up to 8 guests in a desktop browser, 4 on a touch device) and who
  may build and who may fly (everyone, only the host, nobody — the server's edit and flight
  policies; the host is an op), then opens a `Room` on the master (§10.3) and shows the join
  code, an invite link (`?code=`) and its QR code, and the guests playing. The local server
  (`LocalWorld.host`, `dwell-sim` `Server::set_hosting`) raises its player cap and applies the policies for this session only
  (nothing is saved to the world). Peer connections are not available in workers, so the host
  page (`net/hosting.ts`) holds one `RTCPeerConnection` per guest on the main thread and relays
  each guest's data channels (the §8.1 mapping, channels 0–3) to the local-mode worker as its own
  session (`TransportKind` WebRTC; session ids never reuse the host's). All of a hosting session's
  peer connections share one `RTCCertificate` (ECDSA P-256), whose SHA-256 is the transport
  binding (§8.3); guests take it from the answer's DTLS fingerprint. The host keeps playing over
  `LoopbackTransport`. Guests join by typing the code in the Join box or opening the link
  (`connectToRoom`): the master's room introduces them, they exchange the offer, answer and
  trickled candidates, and the guest keeps its room socket open while playing (so the room
  counts it against the limit and can say the host left). While hosting, the page holds a Screen
  Wake Lock (re-taken when shown again); when it is hidden the worker stops stepping the world
  and guests receive `HostStatus(paused)`, then `resumed`. Stop hosting (or leaving the page)
  sends every guest `Reject(ServerClosing)` ("The host stopped hosting.") and closes their
  connections after 0.5 s, then the room; a guest whose host vanished sees "the host left" (the
  room's `host-left`, or the connection dropping). Visibility **[built, Phase 5d]**: code only,
  or code + same network (the default: the world is listed by its name under "On your network" to
  players with the host's public IP); public **[built, Phase 5e]**: also in the lobby list (§10.3),
with the host's protocol version, and its players (the host and connected guests) counted by the
room. **[built, Phase 6]** The room also holds the host's app version, which a guest's code lookup
returns and the share link carries as `&v=` (§10.5).

### 10.3 Master server **[built]** (5b: Worker, Durable Object classes, signing, rate limits, CI and deploy; 5c: rooms, signaling, TURN credentials; 5d: dedicated servers, codes, addresses, on your network; 5e: the lobby list and join receipts. Accounts: later)
A small HTTPS JSON service (`services/master`) on **Cloudflare Workers** with **Durable Objects**
(SQLite storage, Workers Free plan; [ADR 0013](./adr/0013-master-server-on-cloudflare.md)); no
game traffic passes through it. Hostname: the account's `workers.dev` subdomain
(`dwell-master.<subdomain>.workers.dev`), configured in the client build (`VITE_MASTER_URL`,
`?master=` overrides) and in `dwell-server` (`--master`, Phase 5d), so a custom domain can replace
it later.

**Built (Phase 5b).** `src/index.ts` routes `/v1/` and adds CORS headers for the origins in
`ALLOWED_ORIGINS` (the Pages site and local dev servers; requests without an Origin, such as a
dedicated server's, need none). `GET /v1/health` answers `{ ok, service, api }`; `POST /v1/whoami`
(signed) answers with the signer's key, to check a client's signing and clock end to end. Bodies
are capped at 16 KiB; errors are JSON `{ error, message }`. The `Directory` runs its SQLite schema
migrations (so far only its `meta` table with the schema version) and holds the rate limits: token
buckets per key (burst 30, 1/s) and per IP (burst 60, 2/s) in memory, bounded to 10,000 buckets;
over the limit is `429` with `Retry-After`. The client side is
`client/src/net/master.ts` (`MasterClient`, `signatureHeaders`, `masterUrl`).

**Built (Phase 5c): rooms and TURN.** A friend world's join code is six characters from
`ABCDEFGHJKMNPQRSTUVWXYZ23456789` (no look-alikes; shown `KQ7-XM4`; typed in any case, with or
without spaces and dashes) and is the **name of its `Room` Durable Object**, so there is no code
table: `POST /v1/rooms` (signed, `{ maxGuests }`, at most 16) picks a free code and answers
`201 { code, display, hostToken }`; the host must open the room socket within 60 s (an alarm frees
the code otherwise). `POST /v1/rooms/<code>/join` (signed; per-IP guessing limit, burst 20 and one
per 2 s) answers `{ token, peer }` — a one-use token valid for 60 s — or `404` (no such open room)
or `409 full` (connected guests and outstanding tokens reach the limit). `GET
/v1/rooms/<code>/ws?token=` upgrades to the room's hibernating WebSocket (tags `host` and
`guest:<peer>`). Messages are JSON: the host sends `{t:'signal', to, data}` and a guest `{t:'signal',
data}`; the room forwards them as `{t:'signal', from, data}` to the host and `{t:'signal', data}`
to the guest, and tells the host `{t:'guest', peer}` and `{t:'guest-left', peer}` and the guests
`{t:'host-left'}` (after which the room deletes its state). Messages are capped at 16 KiB and 200
per 10 s per socket. `POST /v1/turn` (signed) answers `{ iceServers }`: credentials minted for six
hours from Cloudflare's TURN service (`TURN_KEY_ID`, `TURN_KEY_API_TOKEN` secrets), or
`stun:stun.cloudflare.com:3478` alone when there is no key or the service fails.

**Built (Phase 5d): dedicated servers.** The Directory's schema gains `server_codes` (a join code
per server key, kept for good: a server's code is stable), `servers` (the latest report of each
live server, its public IP and advertised host, and when it expires) and `nearby_rooms`.
`POST /v1/servers` (signed by the server key) validates the report (`src/servers.ts`: ports,
64-hex certificate, ICE credentials, IP-literal LAN addresses, at most 8; text trimmed; the
heartbeat interval clamped to 2–60 s), stores it with the request's `CF-Connecting-IP` as the
public address and an expiry of **two heartbeat intervals**, and answers `{ code, display,
heartbeatS }`; the first registration picks a code no server or open room uses, and rooms avoid
server codes in turn. Queries ignore expired records; a Directory alarm deletes them.
`POST /v1/servers/leave` removes the record at once. `POST /v1/resolve` (signed by the player's
key, limited per IP like room joins) takes `{ code }` — a live server's connection details, else
an open room (`{ kind: 'room' }`), else 404 — or `{ address }` (`host`, `host:port`, `[v6]:port`),
matched against servers' public IPs and advertised hosts from anywhere, and against their LAN
addresses only when the requester has the server's public IP; without a port, 4433 is preferred.
A server's entry gives the address to use: its first LAN address for a player on its network,
else its advertised host or public IP. `POST /v1/nearby` lists the live servers and the "code +
same network" friend worlds (`POST /v1/rooms` with `visibility: 'network'` and the world's name)
with the requester's public IP; a listed world whose room has closed is dropped. Unlisted servers
are found by code, by address and on their own network; public ones are also in the lobby list.

**Built (Phase 5e): the lobby list.** `GET /v1/servers` (unsigned; per-IP limit, burst 30 and one
per second) lists the live **public** servers and the open public friend worlds, most players first,
at most 100: `?q=` (words that must all appear in the name, MOTD or tags), `tag=`, `protocol=`
(compatible only), `notFull=1`, `hasPlayers=1`, `limit=`. A server's entry is the same as
`/v1/resolve`'s plus its tags and `verified`; a world's is its code, name, players and limit (from
its `Room`, which counts the host and connected guests; a room whose host hasn't connected yet is
skipped, a closed one is dropped) and protocol. Servers report up to 8 tags (`--tags`: `a-z`,
`0-9`, `-`, up to 24 characters). **Reachability is player-attested** (ADR 0013): resolving a
server's code or address records the player's key against it for 10 minutes, and `POST
/v1/receipts {code}` (signed, once per resolve) turns that into a receipt — one per player and
server, refreshed by later joins. A public server with receipts from **2 distinct players within 7
days** is verified; the default list shows verified servers and the friend worlds, and `new=1`
lists the unverified servers instead ("New servers"). Friend worlds need no receipts: their host
is connected to the room, and TURN reaches them. Receipts come from player keys, which anyone can
make, so verification keeps honest-but-unreachable servers out of the default list rather than
stopping a determined liar; accounts (§10.4) can later weight receipts.

| Durable Object | Holds |
|---|---|
| `Directory` (one) | Registered dedicated servers (expired after two missed heartbeats, deleted by alarm) and their stable join codes, friend worlds listed to their network **[built, 5d]** or publicly, join receipts and the resolutions that entitle them **[built, 5e]**, rate-limit state |
| `Room` (one per hosted friend world, named by its join code) **[built, 5c]** | The host's and guests' signaling WebSockets (Hibernation API), guest tokens and the guest limit; closes when the host leaves |

| Function | Detail |
|---|---|
| Registration & heartbeat **[built, 5d]** | Dedicated servers register with their server key and heartbeat every ~30 s: port, RTC port and ICE credentials, current cert SHA-256, LAN addresses, name, MOTD, players, protocol version, app version **[built, Phase 6]**, visibility, tags **[built, 5e]**. The public address is the request's (`CF-Connecting-IP`) unless advertised. Two missed heartbeats delist. |
| Join codes | Short codes (e.g. `KQ7-XM4`, unambiguous alphabet) resolve to the current address + cert hash (dedicated, stable per server key; built, 5d) or name a `Room` (friend world; built, 5c); either answer carries the host's `appVersion` **[built, Phase 6]**, which the master validates (SemVer, build metadata dropped) and passes along without judging compatibility — the client does (§10.5). Guessing is rate-limited per IP. |
| Address resolution **[built, 5d]** | `host[:port]` → address + cert hash (+ WebRTC parameters). LAN addresses resolve only among servers sharing the requester's public IP. |
| On your network **[built, 5d]** | Servers and friend worlds whose public IP matches the requester's — LAN discovery for browsers. |
| Server browser **[built, 5e]** | Public servers and public friend worlds, with search/filter; clients ping dedicated servers themselves (`StatusRequest`). Reachability is **player-attested**: after joining through the master, clients post signed receipts, and a server is verified once distinct players have joined it recently (Workers cannot send UDP probes). Unverified servers appear only under a "new" filter. Entries carry the host's app version **[built, Phase 6]**; the browser labels a host on another version line and joins it through the launcher. |
| Signaling | WebRTC offer/answer and trickled ICE relayed through the friend world's `Room`. |
| TURN credentials | Short-lived credentials for Cloudflare's managed TURN, for signed requests, rate-limited per player key; STUN only when no TURN key is configured (development, CI). |
| Accounts (later) | Sign-in and account attestations (§10.4). |

Every mutating request is signed with Ed25519 — a player's device key (§10.4) or a server's key —
**[built, 5b]**: the signature covers the UTF-8 of `dwell-master-v1`, the method, the path with its
query and the Unix time in milliseconds, each followed by `\n`, then the SHA-256 of the body; it is
sent as `X-Dwell-Key` (64 hex), `X-Dwell-Time` and `X-Dwell-Signature` (128 hex) and accepted
within ±60 s of the master's clock. `shared/master/vectors.json` (made by `make_vectors.mjs` with
Node's crypto from a fixed key) pins the format; the client must produce its signatures and the
master must accept them.
**[built, 5b]** Development runs the master locally with `wrangler dev` (`npm run dev`, port 8787;
clients use `?master=http://localhost:8787`); tests use the Workers runtime
(`@cloudflare/vitest-pool-workers`); `.github/workflows/master.yml` deploys on pushes to `main`
that touch `services/master` once the repository secrets exist (skipped with a notice before),
then checks `/v1/health`; the Pages build takes the master URL from the repository variable
`VITE_MASTER_URL`.

Direct invite links (`?join=host:port&cert=<sha256>`) work without the master server.

### 10.4 Identity
- **Device keys (now) [built on web]:** each install generates an Ed25519 key pair; the public
  key is the player ID. Web: non-extractable WebCrypto key in the `dwell` IndexedDB database
  (`identity` store); apps: OS keychain/keystore (Phase 23). Because the key is non-extractable it
  cannot be exported; moving an identity between devices waits for accounts, which link several
  device keys (ADR 0004 amendment). Proven on every join by signing the server's challenge
  (§8.3), verified server-side with `ed25519-dalek` (`dwell-protocol` `auth.rs`). Servers key bans, allow-lists, ops, and player
  data by public key.
- **Accounts (later, additive):** sign-in through the master server; the master issues short-lived
  signed **attestations** binding device keys to an account, which servers verify offline with
  the master's published keys. Servers choose **offline mode** (any key) or **online mode**
  (attested keys only). Enables friends lists, cross-device identity, recovery.

### 10.5 Versioning **[built, Phase 6]**
- The handshake rejects incompatible `protocolVersion`s with a clear reason **[built, Phase 6]**
  naming the server's app version ("Server runs Dwell 0.1.0 (protocol 10), client runs protocol 9");
  no wire change, the `Reject` message text carries it.
- **App version** ([ADR 0014](./adr/0014-versioned-releases.md), [`RELEASES.md`](./RELEASES.md)):
  Semantic Versioning 2.0.0, one for the whole app, kept in `client/package.json` as the version the
  next release can have at least (a floor, §2.1), embedded in every build (TS shell: `__APP_VERSION__`; Rust:
  `dwell_sim::app_version::APP_VERSION`, set by `crates/dwell-sim/build.rs` from `DWELL_VERSION` or
  `package.json`) and recorded in `build.json`. The declared public API is the saved
  world format, the terrain a seed generates, the network protocols and the cross-version storage
  contract. Releases are stable (`0.1.0`); pushes to `main` are dev pre-releases
  (`0.2.0-dev.42`, with the commit as build metadata).
- **Compatibility line:** versions sharing MAJOR (MAJOR.MINOR before `1.0.0`) have the same world
  format, terrain and protocols; a pre-release's line is itself. Builds on one line can play
  together, and a world opens in the newest published build of its line, never an older one (§6.4).
- **Choosing a build** (§2.1): the launcher at `/dwell/` picks by world, by host version (`v=` in
  invite and code links) or the latest. Hosts report their app version to the master (§10.3), so a
  code or the server browser leads to a build of the host's line: the browser labels such a host and
  joins it through the launcher, and `connectToCode` redirects to the launcher when the host is on
  another line than the page's build. A host that reports none (before versioned releases) is tried
  as is; the handshake decides.

### 10.6 Platform reachability

| Joining from → | Dedicated server | Friend world |
|---|---|---|
| Browsers with WebTransport (Chromium, Firefox), Electron, Android app | ✅ WebTransport + cert hash | ✅ WebRTC |
| Browsers / WebViews without WebTransport (e.g. Safari, iOS) | ✅ WebRTC + DTLS fingerprint | ✅ WebRTC |
| Server behind strict NAT | ✅ WebRTC via TURN relay | ✅ via TURN |

No platform needs a trusted certificate to join any server (ADR 0008).
Every client platform also needs WebGPU to play (ADR 0028): browsers and WebViews without it
(Firefox on some platforms, iOS before 26, Android WebView before Chrome 121, some Linux GPU
drivers) get the "unsupported" message (§5).

---

## 11. Security & Validation

- Server is the only authority on voxels, Tier 1 bodies, and player positions.
- Inputs are rate-limited and range-checked **[built]**: at most 2 × `SIM_HZ` `PlayerInput`
  datagrams per second (excess dropped); an input is rejected if its move vector is longer than 1
  (beyond quantization slack), its pitch is out of range, or its `inputSeq` is more than 256 ahead;
  unknown button bits fail decoding. The per-session queue is capped at 16. Rejections are counted
  per session (`SessionStats`).
- **[built, Phase 3d]** Block edits are checked for reach, line of sight, rate (token bucket),
  and permissions (`ServerConfig::edits`); indestructible states cannot be broken or replaced, only palette materials can be
  placed, and placements into blocks, outside the world, or overlapping any player capsule are
  rejected (§6.5). At most 8 requests per session queue per tick; `ChunkResync` answers only chunks
  the client is streamed, at most `MAX_RESYNC_CHUNKS` per request.
- **[built, Phase 4]** `ChunkRequest`s are bounded to `RENDER_RADIUS_CHUNKS` of the player,
  rate-limited (`CHUNK_REQUESTS_PER_SECOND`, a token bucket; at most 1,024 queued) and share the
  chunk byte budget with the view; the server never generates chunks for them.
- Message decoders bounds-check every length field; malformed messages drop the connection.
- **[built, Phase 4b]** `LodRequest`s are rate-limited (`LOD_REQUESTS_PER_SECOND`, a token
  bucket; at most 256 queued, the rest dropped and counted in `LodStats`) and bounded (levels
  1–19 in the decoder; sections outside the world are answered `Generated` without work); LOD
  replies share the `LOD_BYTES_PER_SECOND` budget.
  By design, LOD data shows every player's builds from anywhere, including the coarse layout of
  enclosed rooms (2 m cells at level 1) — an accepted trade-off (ADR 0012); sealing enclosed voids
  server-side is a possible later mitigation.
- **Servers are player-run and untrusted by clients:** the client validates every server message
  as strictly as the server validates client messages (bounds, sizes, rates), and never executes
  server-provided content.
- **Clients are identified by device key** (§10.4); impersonation requires the private key.
  **[built, Phase 3e]** Banned keys are refused after the identity check, and a world with an
  allow-list admits only its keys (§6.4).
- **World files** are only opened by their host (never received from peers); saved chunk blobs
  are bounds-checked when read (the LZ4 blob's declared size against the largest valid palette +
  RLE before decompressing, then the canonical palette + RLE); player records must be well formed
  and finite.
- **Content-Security-Policy** (`client/index.html`): `default-src 'self'`; scripts and workers
  only from the site plus `'wasm-unsafe-eval'` for the client's WASM module; `connect-src 'self' https: ws: wss:
  http://localhost:* http://127.0.0.1:*` for WebTransport to player servers and the master server
  (and the Vite dev server and a local `wrangler dev` master); no objects, no inline scripts.
- The Electron shell uses context isolation, the renderer sandbox, and no Node integration.
- WebRTC endpoint: half-open clients time out (10 s) and are capped (512) so STUN floods can't
  allocate unbounded state.
- Master server: Ed25519-signed requests with a timestamp window; per-key and per-IP rate limits
  (join-code guessing included); verified listing requires player-attested reachability; server
  keys can be revoked. It relays friend-world hosts' DTLS fingerprints, so it is trusted for
  friend-world signaling (ADR 0013).

---

## 12. Open Decisions

Record each resolution as an ADR in `docs/adr/` and update the relevant section above. Ideas
deliberately out of scope for the current implementation live in [`FUTURE.md`](./FUTURE.md).

| # | Decision | Current leaning |
|---|---|---|
| 1 | ~~Server WebTransport/QUIC library~~ | **Resolved:** Rust `wtransport` — [ADR 0001](./adr/0001-webtransport-server-library.md) (behind a C ABI for the C++ server until [ADR 0028](./adr/0028-rust-port.md); now called directly in `dwell-net`) |
| 2 | ~~Client renderer~~ | **Resolved:** Three.js on WebGL2 behind a thin render interface — [ADR 0002](./adr/0002-client-renderer.md); the backend moves to WebGPU (`WebGPURenderer`, TSL materials, an automatic WebGL2 fallback shown in the settings menu, a look registry) — [ADR 0027](./adr/0027-webgpu-renderer.md), **superseded** by [ADR 0028](./adr/0028-rust-port.md): one Rust codebase with a wgpu client, WebGPU only, Rapier instead of Jolt **[built, Phase 24]** |
| 3 | ~~Server hosting~~ | **Resolved:** player-hosted dedicated servers + friend worlds + master server — [ADR 0003](./adr/0003-multiplayer-hosting-model.md); identity [ADR 0004](./adr/0004-player-identity.md); domain [ADR 0005](./adr/0005-domain-and-origins.md) |
| 4 | ~~Browser multithreading / cross-origin isolation~~ | **Resolved:** single-threaded web build + worker pools of independent WASM instances; threads natively — [ADR 0007](./adr/0007-threading-model.md) (Electron loads the same single-threaded build) |
| 5 | ~~World persistence format~~ | **Resolved:** one database per world holding all data — [ADR 0006](./adr/0006-world-persistence-sqlite.md); the engine is `redb` with LZ4 blobs instead of SQLite and zstd — [ADR 0028](./adr/0028-rust-port.md) §8 |
| 6 | Final values for §7.4 tunables | Tune in Phases 20–22 |
| 7 | ~~Worlds larger than ±65 km (Jolt `JPH_DOUBLE_PRECISION`)~~ | **Resolved:** an 8,192 km disc, 8,192 m tall, with double-precision physics (Jolt then, Rapier's f64 build since [ADR 0028](./adr/0028-rust-port.md)) and f64 / fixed-point wire positions — [ADR 0011](./adr/0011-planet-scale-world.md); whole-world view via a 3D LOD octree — [ADR 0012](./adr/0012-lod-octree.md). Re-sized by the baseline to a 50 km bifacial disc with a spherical build volume, the precision decisions kept — [ADR 0025](./adr/0025-baseline-world.md) **[planned, Phase 16]** |
| 8 | ~~Worldgen noise numerics~~ | **Resolved:** strict IEEE float with integer-hash gradients, enforced by a native-vs-WASM golden test — [ADR 0010](./adr/0010-worldgen-noise-numerics.md) |
| 9 | Movement feel on voxels: PPC recommended feel (walk 5 / run 8 m/s) vs. slower voxel-genre speeds | Start with PPC feel; playtest in Phase 2 |
| 10 | ~~Master server platform, database, and hostname~~ | **Resolved:** Cloudflare Workers + SQLite-backed Durable Objects on the free plan, `workers.dev` hostname first — [ADR 0013](./adr/0013-master-server-on-cloudflare.md) |
| 11 | ~~TURN relay: managed vs. self-hosted `coturn`~~ | **Resolved:** Cloudflare's managed TURN, credentials minted by the master — [ADR 0013](./adr/0013-master-server-on-cloudflare.md) |
| 12 | ~~Trusted hostnames for player servers~~ | **Deferred:** out of scope — see [`FUTURE.md`](./FUTURE.md) |
| 13 | ~~Dedicated-server fallback transport~~ | **Resolved:** WebRTC (`str0m`), no WebSocket — [ADR 0008](./adr/0008-dedicated-server-transports.md) |
| 14 | ~~Own subdomain for the client~~ | **Deferred:** out of scope — see [`FUTURE.md`](./FUTURE.md) (ADR 0005) |
| 15 | ~~Friend-world host migration~~ | **Resolved:** no migration; sessions end with the host — [ADR 0009](./adr/0009-friend-world-lifetime.md). Migration and paid cloud worlds in [`FUTURE.md`](./FUTURE.md) |
| 16 | ~~Dedicated servers accepting WebRTC~~ | **Resolved** with #13 — [ADR 0008](./adr/0008-dedicated-server-transports.md) |
| 17 | ~~Water above sea level: terraced static water in river channels and lakes vs. other approaches~~ | **Resolved (Phase 11a):** rivers as noise contours in valley floors, static water at a terraced surface below the valley floor with waterfall steps, lakes with flat surfaces — [ADR 0018](./adr/0018-drainage-consistent-terrain.md), [`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §3.3 |
| 18 | ~~Sky islands: archipelago layout and presence over altitude, the dome's surface, island anchors for integrity~~ | **Withdrawn (2026-10-12):** Phase 12 cancelled by the baseline rework (no special terrain features) — [`BASELINE_EVALUATION.md`](./BASELINE_EVALUATION.md) |
| 19 | Block identity and voxel shapes: the material table vs. namespaced block states; slopes under water | Namespaced block states with string palettes on disk (owner, 2026-10-05): **resolved for identity by [ADR 0015](./adr/0015-block-registry.md) (Phase 8)**; `flooded` for slopes under water and the shapes are decided by Phase 9's ADR — [`BLOCK_REGISTRY.md`](./BLOCK_REGISTRY.md), [`SLOPE_BLOCKS.md`](./SLOPE_BLOCKS.md). Baseline (2026-10-12): six blocks, natural blocks shaped by their neighbours from a rule table held as data — [ADR 0024](./adr/0024-adaptive-natural-blocks.md), placed slope and slab families removed — [ADR 0025](./adr/0025-baseline-world.md) **[planned, Phases 15 and 18]** |
| 20 | ~~Release pipeline: versions loaded from tags, worlds locked to their compatibility line~~ | **Resolved:** one public repository (a private-source split rejected to keep free CI), builds as tagged GitHub Releases served same-origin behind a launcher at `/dwell/`, SemVer 2.0.0 from `0.1.0`, worlds opening only in builds of their version's compatibility line at or after the version that last saved them, all rights reserved — [ADR 0014](./adr/0014-versioned-releases.md), [`RELEASES.md`](./RELEASES.md) |
| 21 | The bifacial world: crossing between faces, light on face B, face B's character, crust thickness, reaching the rim | Decided 2026-10-05: no crossing routes (dig through the diggable core, which is anchored by position — no bedrock — or go around the rim); a static sun for face A and a counter-angled static moon for face B; spawn on face A; a ~4 km crust; the rim ocean kept; face B reuses face A's generator, biomes and islands. Recorded by ADR in Phase 13 — [`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md) §9. Amended by the baseline (2026-10-12): the midplane at y = 0, a 10-layer stone core per face instead of a ~4 km crust, face B's terrain by terrain definition with its own seed — [ADR 0025](./adr/0025-baseline-world.md) **[planned, Phases 16–17]** |
| 22 | ~~The baseline (owner, 2026-10-12): the build limit's shape and radius, flight at it, the stone layers, face B's terrain, launch pads, the movement playground, terrain scale, the midplane, how far terrain is data, the WebGL fallback~~ | **Resolved (Phase 14):** decisions D1–D10 of [`BASELINE_EVALUATION.md`](./BASELINE_EVALUATION.md) §7 — a 50 km disc around a midplane at 0, a 10-layer core, a 50 km build sphere that bounds building but not movement, the same terrain definition on both faces with their own seeds, launch pads removed, the playground a structure file, one island of ~20–30 km — [ADR 0025](./adr/0025-baseline-world.md); terrain as data over a fixed vocabulary of node kinds — [ADR 0026](./adr/0026-terrain-definitions.md); WebGPU with an automatic WebGL2 fallback — [ADR 0027](./adr/0027-webgpu-renderer.md) (D4 superseded by [ADR 0028](./adr/0028-rust-port.md): WebGPU only, no fallback) |
| 23 | Terrain renderer: the mesher or a ray-marched renderer (per device class), and moving bodies' rendering | Measure all, the owner decides (2026-10-14): both built complete and compared side by side in local builds — [ADR 0029](./adr/0029-ray-marched-renderer.md), [`RAY_MARCHED_RENDERER.md`](./RAY_MARCHED_RENDERER.md); decided at Phase 25's A7, before Phase 20 **[planned, Phase 25]** |
| 24 | Structures in worldgen: whether and how terrain definitions place structures (partly revisiting B10) | Deferred until structures exist (owner, 2026-10-14); placement rules in the terrain definition, a new compatibility line — [ADR 0031](./adr/0031-data-driven-structures.md) **[planned, Phase 27's S4]**; procedural trees (Phase 30, [ADR 0033](./adr/0033-tree-growth.md)) wait for it |
