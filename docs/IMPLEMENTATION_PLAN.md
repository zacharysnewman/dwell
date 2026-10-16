# Dwell — Phased Implementation Plan

This plan turns the voxel multiplayer physics spec into buildable phases. The architecture
lives in [`ARCHITECTURE.md`](./ARCHITECTURE.md); section references (§) point there.

**Deployment order:** GitHub Pages (static web client) → Electron → Capacitor.
The authoritative server always runs outside GitHub Pages, except for **local mode**, where
the server core runs in the browser as WASM (§2.1).

Each phase lists its **goal**, **deliverables**, and **exit criteria**. A phase is done only
when its exit criteria pass and `ARCHITECTURE.md` reflects what was built.

## Progress

Deliverables and exit criteria below are checkboxes, ticked in the same commit that completes
them (see `CLAUDE.md`). This table summarizes the state of each phase on this branch.

| Phase | Status |
|---|---|
| 0 — Repository, tooling & Pages | ✅ Complete |
| 1 — Server core, protocol, transports, local mode | ✅ Complete |
| 2 — Physics player controller | ✅ Complete (playtested; follow-up fixes made) |
| 3 — Terrain generation & streaming | 🚧 In progress — every sub-phase built (3a–3e: 3d block edits and meshing workers, 3e persistence and debug tooling). Outstanding: playtests for the long walk (3b) and walking/jumping/swimming the terrain |
| 4 — World LOD & whole-world view | 🚧 In progress — 4a, 4b and 4c built, the dev camera replaced by creative flight; playtest follow-ups built: fog off, super tall mountains (generator version 4), chunks shown first on slow devices, no popping when turning and matching distant colours, flight/HUD/transport fixes and the distant-water comparison, distant terrain at its true height and tinted distant water, seamless see-through distant water without cracks at section borders, no z-fighting on distant water, height fog with a settings menu and playtested defaults, full-detail chunks beyond the view on request (protocol v8) (its velocity lookahead removed 2026-10-14: it looked bad), a flight speed slider (protocol v9) that is a true minimum near the ground, caves deep underground drawn without requesting buried chunks, an FPS counter, the LOD view held within its cache budget with its pixel error in CSS pixels, one draw call per LOD section, static transforms and GPU-only vertex data, the LOD's pixel error and memory as settings, batched terrain behind `?batch=1` and a `?scale=` resolution switch, a memory readout in F3 and a phone memory budget, clipped ancestor stand-ins for the holes on the forced path to the chunks (chopped mountains, `FIX_SPEC_LOD_SKY_FLIP.md` item 1), flight toggled by a dedicated F key instead of a double-tapped Space or Jump (owner, 2026-10-15; the touch Fly button unchanged); outstanding: the frame-rate check on a desktop and a mobile device; `package.json` raised to 0.11.13, a patch release of the 0.11 line |
| 5 — Multiplayer ready (menus, web hosting, master on Cloudflare, lobby list) | 🚧 In progress — every sub-phase built: 5a (main menu, world management, game menu), 5b (master Worker skeleton, signing, CI, deploy workflow; the master runs at `dwell-master.dropkick.workers.dev`), 5c (friend worlds: host from the browser, join by code), 5d (dedicated servers on the master, join by address, On your network), 5e (lobby list, receipts, server browser). Outstanding: the manual phone checks (5a, 5c, 5d) and the TURN key |
| 6 — Versioned releases: builds by tag, version launcher, version-locked worlds, license | 🚧 In progress — 6a (release pipeline, launcher, version-locked worlds, license) complete; 6b (a user-facing version selector: `/dwell/?versions`, a Versions link in the menu) built, unit and site e2e tests passing; outstanding: the phone-width check |
| 7 — Fantasy look: a first pass at colour (rendering only) | ✅ Complete — shared look module, face tints, sky gradient with matched haze, tone mapping and an exposure slider, retuned palette, turquoise water, screenshot script; `package.json` 0.1.1; follow-up built: one sky sphere that turns as a unit; the owner approved the before/after and the frame time (2026-10-06) |
| 8 — Block registry: namespaced block states and palettes | ✅ Complete — namespaced block states, the registry and string palettes in world files (`package.json` 0.2.0, the new compatibility line); paletted in-memory chunks were measured and deferred |
| 9 — Slope blocks (shapes, collision, building, terrain, LOD) | 🚧 In progress — 9a–9d built and tested natively and in Vitest; `package.json` raised to 0.3.0, the new compatibility line (generator v5, registry hash); playtest follow-up built: generated slopes left gaps under stacked pieces and steep ground unsloped, fixed in place on the 0.3 and 0.4 lines (patches 0.3.1 and 0.4.1, same generator versions, the owner's decision); ladders targetable again; a slab's and a slope's side faces show the grass fringe along their top edge (playtest); outstanding: WASM suites, e2e (incl. new shape specs), frame time, owner review; **re-scoped by the baseline rework**: the placed slope and slab families and the slope stage are to be cut (Phase 15), which drops their outstanding checks; 9e (adaptive natural blocks) moves to Phase 18 |
| 10 — Continents from Voronoi plates | 🚧 In progress — built and tested natively (12–13 continents, separation, shape statistics, coast, goldens regenerated for generator v6, the whole-disc inspect image mode, the F4 zoom); `package.json` raised to 0.4.0, the new compatibility line (generator v6); outstanding: the WASM suites and client-module goldens (CI), the F4 zoom in a browser, LOD generation within +10 % at coasts, the owner's review of whole-disc images; **re-scoped by the baseline rework**: to be cut to one island (Phase 15) and replaced by the terrain definition's layout (Phase 17), which drops these outstanding items |
| 11 — Natural terrain: rivers, mountains, climate & biomes | 🚧 In progress — 11a built (generator v7: valley floor, three river tiers, lakes, terraced water above sea level, `ADR 0018`; `package.json` raised to 0.5.0), the climate moved to continental scale (v8, ADR 0019; 0.6.0) and the level of detail's rivers, water levels and plate edges made to match full detail (ADR 0020), the shores beside distant water drawn at their true height and distant slopes tinted by their step sides; 11b built (generator v9: the derivative-damped ridged cascade, ADR 0021); 11c built (generator v10: continent climate biases and rain shadows, the 19-biome table, wetland ponds, grass and leaves tinted by biome, ADR 0022); all tested natively and in the client's Vitest suites; 11d (stretch) waits for the owner's approval of 11a–c; outstanding: the WASM suites and client-module goldens (CI), generation cost over the whole phase (LOD +30–57 % against version 8, see 11b/11c), the manual river walk and a flight over a range, the owner's review of the map images, a browser look at distant rivers and at the new foliage; **re-scoped by the baseline rework**: rivers, climate, biomes, tints and features are to be cut (Phase 15), which drops these outstanding items |
| 12 — Sky islands in a dome | ✖ Cancelled by the baseline rework (owner, 2026-10-12: no special terrain features); never started |
| 13 — Bifacial world: a second face below, gravity toward the midplane | 🚧 In progress — built: ADR 0023, bounds without bedrock, face B's terrain (generator v11) and LOD, protocol v12 (index rows, `faceB` flags), the controller's face frame, flip band (accelerating toward the midplane), `--dwell-face=b` controller suites (native and WASM), face-B chunk/LOD meshing, per-face light, night sky and camera turn-over; `package.json` raised to 0.8.0, the new compatibility line (protocol v12, generator v11); outstanding: a look at it in a browser, client worldgen-module goldens (CI), the owner's manual review; re-scoped by the baseline rework: the 50 km disc, midplane at 0 and 10-layer core (Phase 16), whose exit criteria include the browser look; follow-up built: the LOD bounds cache keyed per column again (near chunks above the first column's height were hidden); the LOD culls the other face and meshes straddling sections for the viewer's face; the camera crossing is a half forward somersault, one sky sphere; `package.json` raised to 0.8.1, a patch release of the 0.8 line (no compatibility change) |
| 14 — Baseline decisions & ADRs | ✅ Complete — the owner answered D1–D10 (2026-10-12; D3 changed: the build sphere bounds building, not movement; D4 adds the active renderer shown in the settings menu); ADRs 0025–0027 written and 0024 amended, all Accepted |
| 15 — Cut to the baseline | 🚧 In progress — built and verified locally (native, WASM, Vitest; goldens regenerated; generator version 12, one island; `package.json` 0.9.0, the new compatibility line); e2e passes in CI; outstanding: the owner's look in a browser |
| 24 — Rust port (one Cargo workspace, Rapier, wgpu client; runs after 15, before 16) | 🚧 In progress — 24f (the cutover): 24a–24e built; the C++/TS tree deleted; every Rust suite, the e2e suite and Electron's smoke test pass in CI on the Rust client and `dwell-server`; `package.json` 0.10.0, the new compatibility line (world files are redb now); playtest follow-ups built (2026-10-14): the grey see-through haze on distant ground (atlas mipmaps whose filtering reached past a tile's gutter; checked in a browser), each depth pass draws what reaches its depth range (by distance it could miss terrain toward the sides of the view; suspected, not confirmed, as the cause of gaps seen from high up), replaced meshes never freeing their GPU buffers on the web (the frame rate falling over a session), the LOD's velocity lookahead removed (detail loads by location alone), and named views (`client/views/views.json`: a placed camera, window and settings per view, opened by e2e specs and shot by `scripts/shots.ts`, no flying); the gaps seen from high up not reproduced and not pursued (owner); the dev telemetry and bench (`docs/DEV_TOOLS.md`), and with it the jump onto a block from against it fixed (Rapier's contact clustering hid the body's contacts; also crouching against a wall moving the player back), and the frame time cut about 3.5× idle and flying low (no dropped frames at 60 Hz when flying fast and low; fast hasher, arena-paged meshes with one draw call each, instance-indexed draw data, leaner LOD traversal and chunk padding); the menus redesigned (owner, 2026-10-14: a themed main menu, a full-screen game menu with Settings, Controls and Debug pages, key hints, phones upright and sideways, touch controls no longer taking the menus' taps); `package.json` raised to 0.10.3, a patch release of the 0.10 line (no compatibility change), with the launcher owning the main menu; outstanding: the owner's look at the new menus; the frame-time measurement and the owner's manual check, including a long session's frame rate; `package.json` raised to 0.11.15, a patch release of the 0.11 line |
| 16 — World shape & build volume (50 km disc, 10-layer core, spherical build volume) | ⏳ Not started — built in Rust after Phase 24 |
| 17 — Terrain definitions (terrain as data) | ⏳ Not started — built in Rust after Phase 24 |
| 18 — Adaptive natural blocks (formerly 9e) | ⏳ Not started — built in Rust after Phase 24 |
| 19 — WebGPU, looks & textures | ⏳ Not started — re-scoped by ADR 0028: WebGPU is done by Phase 24 (wgpu, no fallback); the look registry and textures as data are built on wgpu after it |
| 20 — Voxel awakening | ⏸ Waits for Phases 6–19 (2026-10-05; the baseline rework, 14–19, added 2026-10-12) and for Phase 25's renderer decision (A7, 2026-10-14: cluster rendering is built once, for the chosen renderer) |
| 21 — Tiered physics | ⏸ Waits for Phases 6–19 (2026-10-05; the baseline rework, 14–19, added 2026-10-12) |
| 22 — Sleep / re-bake | ⏸ Waits for Phases 6–19 (2026-10-05; the baseline rework, 14–19, added 2026-10-12) |
| 23 — Dedicated servers & packaging | ⏳ Not started |
| 25 — Ray-marched renderer comparison | 🚧 In progress — A0 (ADR 0029), A1 (the shared frame and GPU timing; the owner approved its image differences, 2026-10-14, and the e2e suite passes on CI), A2 (the renderer seam, compare mode, bench `--compare`; its exit check done) and A3a (the CPU core: `crates/dwell-march`) built; A3b (the GPU march behind the seam, checked against the reference per pixel; its CI run outstanding) built; next: A3c (overlays over marched terrain, LOD column heights in the bricks, raster ids), then A4 (speed: 30 ms at 720p on an M1); outstanding: A2's edit-to-visible latency and the bench's id/depth agreement (moved to A3); local builds only until the owner's decision (A7) |
| 26 — Water and fluid looks | 🚧 In progress — W1a (fluid data, the eye-under-fluid uniform) done; W2 built (it is Phase 25's A1, built once there; the owner approved its image differences, 2026-10-14); W1b (the water shader, the underwater fog stub, the water quality setting) built and checked in Chrome on an M1, outstanding: the owner's look; next: W3; W6 only if marching is kept at A7; `package.json` raised to 0.11.9, a patch release of the 0.11 line |
| 27 — Structure sandbox | 🚧 In progress — S0 (ADR 0031), S1 (the structure format, `gen.mjs`, the Rust loader, the playground converted), S2a (floor block, sandbox world type), S2b (the structures store: save, load through the region paste, export and import) and S3a (region edits, the region tools) done, S2a's e2e check outstanding; `package.json` raised to 0.11.0, the new compatibility line (S2a's registry change; the owner's decision, 2026-10-14); next: S3b (anchor, touch) and the S3 sandbox e2e outstanding; S4 (worldgen placement) deferred; `package.json` raised to 0.11.11, a patch release of the 0.11 line |
| 28 — Sound effects | 🚧 In progress — F0–F6 built and tested (procedural Web Audio, no library; sound data and generator, the registry's `sound` field, Rust sound events, synth, variation, mixer, spatial playback, menu and game-action sounds, volume sliders, offline WAV renders); outstanding: the owner listens to the WAV renders and approves them, and answers `AUDIO.md` §9; `package.json` raised to 0.11.12, a patch release of the 0.11 line |
| 29 — Motion effects (speed FOV and wind) | 🚧 In progress — built and tested (the FOV widening with the camera's speed for the renderer and the LOD alike, the wind streaks, vignette and edge blur in the shared frame's composite, tunables as data, the settings, bench scenarios and screenshots); outstanding: the owner's look in a browser and approval, CI; `package.json` raised to 0.11.7, a patch release of the 0.11 line |
| 30 — Tree growth | 🚧 In progress — G-ADR (ADR 0033) and the G0 prototype page built and verified in Chromium; outstanding for G0: publishing the page and the owner's review; next: G1; G2 needs Phase 27 (S2a/S3); G3 is a new compatibility line (the owner decides the bump) and needs Phase 20 for felling and the day/night cycle's day length; `package.json` raised to 0.11.14, a patch release of the 0.11 line |
| 31 — Sky clouds | 🚧 In progress — C0–C4 built and tested: a 2D cloud deck in the composite's sky (ADR 0034, `CLOUDS.md`), tunables as data (`shared/look/clouds.json`, generator and CI stale check), the Clouds setting (Off / Low / High), the drift on the server's tick, bench scenarios and screenshots, GPU cost measured (about +0.9 ms composite on High at 720p, Apple GPU); outstanding: the owner's look and answers to `CLOUDS.md` §6, CI, a phone's frame time; `package.json` raised to 0.11.10, a patch release of the 0.11 line |

Phase numbering: Phase 4 was inserted on 2026-09-29 for the planet-scale world (ADRs 0011, 0012);
the former Phases 4–7 are now 5–8, and Phase 3's former 3c and 3d are now 3d and 3e.
Phase 5 (multiplayer ready) was inserted on 2026-09-30 (ADR 0013); the former Phases 5–8 became
6–9.
On 2026-10-05 the owner added eight phases and placed them before the physics phases, in this
order: **6 — versioned releases** ([`RELEASES.md`](./RELEASES.md)); **7 — a first pass at colour**
and **10–12 — continents, natural terrain, sky islands** ([`WORLD_GENERATION.md`](./WORLD_GENERATION.md));
**8 — the block registry** ([`BLOCK_REGISTRY.md`](./BLOCK_REGISTRY.md)); **9 — slope blocks**
([`SLOPE_BLOCKS.md`](./SLOPE_BLOCKS.md)); **13 — the bifacial world** ([`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md)).
The former Phases 6–9 (voxel awakening, tiered physics, sleep/re-bake, dedicated servers and
packaging) are now **14–17**. References in this plan,
`ARCHITECTURE.md` and code comments use the new numbers; ADRs keep the numbers of their day
(`adr/README.md`). Why this order: versioned releases first, so every later change to world files
or terrain leaves earlier worlds playable in their own version; colour alongside it (rendering
only); the registry before slopes, which are registry block families; slopes before the terrain
phases, so terrain is tuned with slopes in place; continents before natural terrain, which builds
on coast distance and plate edges; sky islands after natural terrain (they use its biomes); the
bifacial world after both, since face B repeats them, and before physics, so falling bodies handle
both gravity directions from the start.
On 2026-10-12 the owner set a simpler baseline ([`BASELINE_EVALUATION.md`](./BASELINE_EVALUATION.md))
and inserted the **baseline rework** as **Phases 14–19**, before the physics phases: it cuts back
Phases 9–11, cancels Phase 12, re-sizes Phase 13's world and takes over 9e (as Phase 18). The former
Phases 14–17 (voxel awakening, tiered physics, sleep/re-bake, dedicated servers and packaging) are
now **20–23**; references in this plan, `ARCHITECTURE.md`, the sub-specs and code comments use the
new numbers, ADRs keep the numbers of their day.
On 2026-10-13 the owner decided to port Dwell to Rust ([ADR 0028](./adr/0028-rust-port.md)) and
added it as **Phase 24**, numbered last so no other phase is renumbered but **run after Phase 15
and before Phase 16**; Phases 16–23 are then built in Rust.
On 2026-10-14 the owner adopted three proposals as **Phases 25–27**, numbered after the existing
phases: **25 — the ray-marched renderer comparison** ([ADR 0029](./adr/0029-ray-marched-renderer.md)),
**26 — water and fluid looks** ([ADR 0030](./adr/0030-fluid-looks-and-water-rendering.md)) and
**27 — the structure sandbox** ([ADR 0031](./adr/0031-data-driven-structures.md)). Each needs Phase 24
only; they can run alongside Phases 16–19. Phase 20 waits for Phase 25's decision (A7). Deliverables
are named by their design docs' step ids (A0–A7, W1a–W6, S0–S4).
Also on 2026-10-14 the owner adopted the tree-growth proposal as **Phase 30 — tree growth**
([ADR 0033](./adr/0033-tree-growth.md), [`TREE_GROWTH.md`](./TREE_GROWTH.md)), numbered after the
existing phases; it builds on Phase 27's structures, and its deliverables are G-ADR and G0–G4.
On 2026-10-15 the owner asked for procedural clouds in the sky ("in the skybox, not 3D"), added as
**Phase 31 — sky clouds** ([ADR 0034](./adr/0034-sky-clouds.md), [`CLOUDS.md`](./CLOUDS.md)). It
builds on Phase 25's A1 (the shared frame's composite pass draws the sky) and changes no world file,
protocol or terrain. Phases 28 and 29 are taken by work on other branches (sound effects, motion
effects).
On 2026-10-14 the owner asked for sound effects (procedural, data-driven, every game and menu
action): **Phase 28** ([ADR 0032](./adr/0032-audio.md), [`AUDIO.md`](./AUDIO.md)); it needs Phase 24
only.
Phase 6's pipeline (6a) is complete; its version selector (6b) can land alongside later phases.
Phase 12's dome bounds (12a) can go earlier.

**Code names in earlier phases.** Phases 0–15 were built on the C++ server and core (CMake,
Emscripten, Jolt, SQLite, doctest), a TypeScript sim and the Three.js client; their deliverables,
deviations and measurements keep the file, test and tool names of that code (`core/*.h`,
`dwell_tests`, `dwell_worldgen_inspect`, `mesher.ts`, …) as the record of what was built. Phase 24
ported it all to one Rust workspace (ADR 0028); `ARCHITECTURE.md` §3 maps today's crates. Phases
16 onward name the Rust crates.

---

## Phase 0 — Repository, Tooling & GitHub Pages Pipeline

**Status:** complete — CI green and the Pages deployment live.

**Goal:** A working monorepo skeleton that deploys a blank client to GitHub Pages on every
push to the default branch.

Deliverables
- [x] Monorepo layout per §3: `client/`, `server/`, `shared/protocol/`, `platforms/`, `docs/`.
- [x] `client/`: Vite + TypeScript (strict), ESLint, Prettier, Vitest. `base: '/dwell/'`.
  Three.js (pinned) behind the `client/render` interface (ADR 0002); renders an empty scene
  and a build-info overlay (commit SHA).
- [x] `server/`: CMake project, C++20, Jolt pulled via `FetchContent`, a unit-test target
  (e.g. Catch2/doctest), clang-format config. Rust toolchain pinned with `rust-toolchain.toml`;
  empty `server/net/wt` crate linked through Corrosion (ADR 0001) so the mixed build works from
  day one.
- [x] GitHub Actions:
  - [x] `ci.yml` — lint, typecheck, test, build for client; configure/build/test for server
    (C++ + cargo, with cargo caching, `cargo clippy`, and a stale-`cbindgen`-header check).
  - [x] `pages.yml` — build client, upload artifact, `actions/deploy-pages`.
- [x] `docs/adr/` with an ADR template.

Exit criteria
- [x] `https://dropkickarcade.com/dwell/` serves the blank client (default project path under the
  user site's custom domain; ADR 0005).
- [x] CI is green on both client and server jobs.

---

## Phase 1 — Server Core, Jolt, Headless Voxel Grid & Transport

**Status:** complete, including the manual Safari check.

| Exit criterion | Result |
|---|---|
| Chromium and Electron connect to a native server on localhost via WebTransport, exchange ping/pong over datagrams and a reliable stream, and show RTT | ✅ Automated: Playwright e2e (Chromium) and the Electron `--smoke` run in CI |
| Forcing the WebRTC fallback produces the same behavior, including in Safari | ✅ Chromium (e2e) and Electron (manual, against a routable IP). Safari: ✅ manual check |
| The Pages deployment runs in local mode with no external server | ✅ e2e test against the production build; Pages workflow builds the WASM core |
| Protocol golden tests pass in both languages | ✅ C++ (doctest) and TS (Vitest) against vectors from an independent Python encoder |

Deviations from the deliverables below:
- WebRTC uses its own UDP port (WebTransport port + 1) instead of sharing one (ADR 0008 notes).
- Device keys are non-extractable, so key export/import was dropped (ADR 0004 amendment).
- Added beyond plan: session replacement on re-login (reject reason `Replaced`), a strict CSP, and
  an e2e CI job.
- Electron loads the single-threaded WASM build; the threaded build comes later (ADR 0007).

**Goal:** Spec Phase 1. A C++ server with Jolt and a headless voxel grid, reachable from the
browser client over WebTransport, plus local mode so the Pages build works without a server.

Deliverables
- [x] **Server core (`server/core`)**
  - [x] Fixed-timestep loop: 60 Hz step (§4.2). The 20 Hz snapshot tick moved to Phase 2, where
    snapshots first exist.
  - [x] Jolt `PhysicsSystem` initialized with object/broad-phase layers
    (static terrain, Tier 1 dynamic, characters).
  - [x] Headless voxel grid: chunk storage (32³, §6.1), material table, flat test world generator.
  - [x] Core exposes a message-in / message-out interface with no OS dependencies (§4.3).
- [x] **Protocol (`shared/protocol`)**
  - [x] v0 message definitions (§8.3) and shared constants (§7.4).
  - [x] Encoders/decoders in TS and C++, with golden-byte test vectors both sides must pass.
- [x] **Server network front-end (`server/net`)**
  - [x] **Spike first:** `wtransport` crate behind the C ABI (ADR 0001) accepts a session from
    Chrome (dev cert via `serverCertificateHashes`) and Electron; one reliable stream message
    and one datagram each way. Fallback if it fails: Google QUICHE behind the same C ABI.
  - [x] WebTransport server on that crate: handshake, `control` + `world` streams, datagram
    send/receive, ping / clock sync; transport events drained into the main loop once per tick.
  - [x] Status query and join handshake with **device-key identity** (ARCHITECTURE §8.3, §10.4,
    ADR 0004): Ed25519 challenge signed by the client, bound to the transport; protocol-version
    rejection with reasons.
  - [x] Dev TLS: the crate generates a short-lived ECDSA cert at startup and prints its SHA-256 for
    `serverCertificateHashes`.
  - [x] WebRTC fallback endpoint (ADR 0008): ICE-lite `str0m` in the same crate, same channel
    mapping and framing. Spike includes the invite-link path (client synthesizes the remote
    description from address + fingerprint + ICE credentials); if it fails, invite-link
    fallback joins require the master server (Phase 23).
- [x] **Client networking (`client/net`)**
  - [x] `Transport` interface with `WebTransportTransport`, `WebRtcTransport`,
    `LoopbackTransport` (§8.1). Auto-select: WebTransport → WebRTC.
  - [x] Connect via invite link `?join=host:port&cert=<sha256>[&rtc=<port>&ice=<ufrag>:<pwd>]`;
    `?local=1` forces local mode.
  - [x] Device key: generated on first run (non-extractable WebCrypto Ed25519 in IndexedDB; no
    export — ADR 0004 amendment).
  - [x] Connection status + RTT overlay.
- [x] **Local mode**
  - [x] Emscripten build target for `server/core` (Jolt linked in, single-threaded per ADR 0007;
    Jolt `JobSystemSingleThreaded` vs. `JobSystemThreadPool` selected at build time); loaded in a
    Web Worker by the client and wired through `LoopbackTransport`. Added to the Pages workflow.
    The same build later hosts client prediction (Phase 2) and worldgen (Phase 3).
- [x] **Electron shell (`platforms/electron`)** loading the same build; connects to a local server.

Exit criteria
- [x] Browser (Chromium) and Electron connect to a native server on localhost via WebTransport,
  exchange ping/pong over datagrams and a reliable stream, and show RTT.
- [x] Forcing the WebRTC fallback produces the same behavior (Chromium, Electron).
- [x] … including in Safari (manual check).
- [x] The Pages deployment runs in local mode with no external server.
- [x] Protocol golden tests pass in both languages.

---

## Phase 2 — Physics Player Controller (Prediction + Reconciliation)

**Status:** complete — every deliverable and exit criterion is verified by automated tests (C++
natively and under WASM, TypeScript unit tests, Playwright e2e). Moved out: the cosmetic death
ragdoll and animation from `State` (Phase 21, see below). Playtested by a human (Open Decision
#9): two findings, fixed: forward/back looked faster than
strafing, which was the camera's wide horizontal field of view rather than the sim (now capped
at 100°, `client/src/render/fov.ts`), and the touch Crouch button now holds instead of toggling.
Later finding: jumping onto a block while holding forward gave a
burst of speed as the player came down on the edge — the step-up's forward nudge was added on top
of the tick's movement (also on every slab step, and on slopes every tick); it is now taken out of
that tick's velocity (PLAYER_CONTROLLER.md §4). The fix is in.
Later finding (fixed): jumping over a lone block while pressed against it
launched the player forward, and one jump floated the player up a diagonal staircase of full
blocks — the block's top edge deflected the forward drive upwards and the airborne vertical layer
absorbed that as an external force (a second jump). That lift is no longer absorbed
(PLAYER_CONTROLLER.md §4, "Edge lift is not absorbed").

| Exit criterion | Result |
|---|---|
| Ported PPC scenarios pass natively and in WASM; golden trace stable | ✅ `dwell_tests` (Debug and Release) and `dwell_player_tests.js` under Node, in CI |
| Doorways, crawlspaces, blocks need a jump, slabs step up | ✅ `player: voxel geometry`, `crouch`, `steps and air`, `step smoothness` |
| Two clients see each other; 150 ms / 20 ms / 5 % | ✅ `netcode: prediction` (~1 % of snapshots replay, p95 error at ack < 1 mm, no snaps; input applies on the next tick) and e2e `two clients on a native server see each other move` |
| Knockback smooth at 150 ms; bumps without jitter | ✅ launch pad: no snaps, largest rendered step 0.6 m/tick; head-on bump: no snaps, ≤ 0.23 m/tick |
| Out-of-range inputs rejected; fall damage and respawn on all clients | ✅ `netcode: server` |
| 64 players < 1 ms/tick | ✅ ~0.36 ms (native Release), ~0.42 ms (WASM) |

Deviations from the deliverables below:
- `maxStepHeight` is 0.55 m (PPC 0.45) so half-block slabs step up, as the exit criteria require.
- Terrain collision is one static body with a `MutableCompoundShape` of per-chunk meshes, and chunk
  meshes use unit quads rather than greedy-merged faces: separate chunk bodies and T-junctions both
  caused ghost contacts at seams (PLAYER_CONTROLLER.md §5). Phase 3's greedy mesher is therefore for
  render meshes only.
- Controller fixes beyond the PPC (PLAYER_CONTROLLER.md §4): blocked motion isn't absorbed as
  external velocity (needed to jump onto a block while holding forward), step-up nudge and grace,
  crouched step probe height; ladders gained an over-the-top region.
- `PlayerControllerConfig` lives in the C++ core rather than `shared/protocol`: the client runs the
  same C++ in WASM, so no TypeScript copy is needed.
- Added beyond plan: a playground world generator (generator version 1) with every test feature
  near the spawn; `inputBuffer`-driven client tick-rate nudging and a 2-input server jitter buffer;
  `lastKnockbackSeq` in snapshots; the F3 debug overlay's in-world probe rays.
- Remote-player proxies in the prediction world sit at the latest snapshot position,
  dead-reckoned, rather than being interpolated or extrapolated: measured to correct bumps far more
  gently (PLAYER_CONTROLLER.md §8.1).
- Dwell uses right-handed axes: the controller's camera-right vector is the mirror of the PPC's
  (Unity, left-handed) so that strafing matches the screen.
- The snapshot's `groundEntityId` fields are deferred to Phase 20, when Tier 1 bodies can be stood
  on (ARCHITECTURE §8.3).
- Deferred to Phase 21: the cosmetic death ragdoll (Jolt `Ragdoll`) — dead players are drawn lying
  down until the client debris world exists — and animating players from `State` (players are
  capsules; there are no character models yet).

**Goal:** Spec Phase 2. Port the Physics Player Controller to C++/Jolt on voxel terrain, then
network it with client prediction and server reconciliation (§9, `PLAYER_CONTROLLER.md`).

The port follows the PPC Quantum port's own phase order, so each step can be checked against the
behaviour and tests of the original.

Deliverables
- [x] **2a — Skeleton.** `PlayerController` state, `PlayerControllerConfig` (recommended-feel defaults
  with voxel dimensions, `PLAYER_CONTROLLER.md` §7), spawn, dynamic Jolt body (rotation locked,
  gravity 0, frictionless, no sleeping, enhanced internal edge removal), aggregate pass, and the
  ordered pipeline running before `PhysicsSystem::Update`.
- [x] **2b — Voxel collision & queries.** Per-chunk static collision `MeshShape` built from the grid
  (as sub-shapes of one terrain body) and rebuilt in the same tick as an edit; `VoxelQuery` (DDA ray cast, capsule-vs-cell overlap)
  merged with Jolt queries for dynamic layers (`PLAYER_CONTROLLER.md` §5); `PlayerTestWorld`
  builder (floors, block and slab steps, walls, 1×2 doorways, crawlspaces, ladder columns, water).
- [x] **2c — Probes & horizontal layer.** Ground/ceiling rings, wall rays; walk/run, accel/decel/reverse,
  air control, step-up, external absorption and decay.
- [x] **2d — Vertical layer & jump.** Gravity, ground following + snap, walk-off, ceiling, launches;
  buffer/coyote in ticks; `Jumped`/`Landed` events.
- [x] **2e — Crouch.** Shape swap with feet planted / head kept; overlap-tested stand-up; crawlspaces.
- [x] **2f — Climb & swim.** Climbable voxel materials with facing variants; exclusive climb layer;
  water and the exclusive swim layer (Dwell addition); optional auto-jump and edge guard.
- [x] **2g — Test port.** The PPC headless suites ported to C++ on voxel geometry
  (`PLAYER_CONTROLLER.md` §10), including the multi-player golden trace.
- [x] **2h — Networking.**
  - [x] 20 Hz snapshot tick in the server loop (moved from Phase 1).
  - [x] Server: input queue ordered by `inputSeq`, validation and rate limiting (§11); snapshots with
    `ackInputSeq`, body state, and local controller state (47 B + conditional ladder fields).
  - [x] Client: sim-core WASM build hosts the prediction world (terrain + kinematic proxies + local
    dynamic body); input ring buffer; redundant input datagrams (last 4); analog move vector.
  - [x] Reconciliation: compare prediction at `ackInputSeq`; replay only on mismatch; smooth small
    corrections, snap large ones.
  - [x] Knockback: `PlayerEvent(Knockback, tick)` inserted into prediction history and replayed.
    Tested with a debug launch-pad block.
  - [x] Player-vs-player collision (block/push; standing on heads not carried); remote players as
    interpolated kinematic capsules. (Animation from `State` moved to Phase 21.)
  - [x] Health, fall damage from `Landed` impact speed, death → respawn. (The cosmetic ragdoll moved
    to Phase 21; dead players are drawn lying down.)
- [x] **2i — Divergence measurement.** Jolt built with `JPH_CROSS_PLATFORM_DETERMINISTIC`, no FMA
  contraction; CI runs the scenario suite natively and under WASM (Node) and reports max per-tick
  divergence, checked against `externalAbsorbThreshold`. (Measured: positions bit-identical,
  velocities within 1.2 × 10⁻⁷ m/s.)
- [x] **Presentation & tools.** First-person camera with crouch eye smoothing and step smoothing;
  debug overlay (state, probe rays, layer velocities, prediction error); network condition
  simulator (latency, jitter, loss).

Exit criteria
- [x] All ported PPC scenarios pass on voxel geometry (natively and in WASM); the golden trace is
  stable across repeated native runs.
- [x] The player fits through 1×2 doorways and, crouched, through 1-tall crawlspaces; full blocks need
  a jump; slabs are stepped up without leaving the ground.
  Automated: `player: voxel geometry`, `player: crouch`, `player: steps and air`, `player: step
  smoothness`.
- [x] Two clients see each other move smoothly. With 150 ms RTT, 20 ms jitter and 5 % loss simulated,
  local movement feels immediate, steady-state correction error stays under ~5 cm, and most
  snapshots need no replay.
- [x] Debug knockback plays smoothly (no snap) at 150 ms RTT; players bump into each other without
  jitter.
- [x] Server rejects out-of-range inputs; fall damage and respawn work on all clients.
- [x] 64 players' controller passes cost < 1 ms/tick on the reference server (excluding the Jolt step).
  ~0.36 ms/tick in the Release build (`player: performance`, run strict in CI's Release step).

---

## Phase 3 — Static Terrain Streaming

**Status:** in progress — all sub-phases built; two exit criteria await playtests (the long walk,
and walking/jumping/swimming the terrain). Sub-phases: **3a — generator** (done); **3b — streaming** (done:
chunk encoding, `Generated`/`Explicit`, the verification chunk, interest management,
server and client worldgen pools; the walking-without-hitches exit criterion awaits a playtest);
**3c — scale foundations** (done: the planet-scale world of ADR 0011 — bounds and
the rim, double-precision physics with region-anchored terrain collision, protocol v4 positions,
generator version 3, air chunks, spherical streaming; every 3c exit criterion verified);
**3d — block edits** (done: protocol v5 edit loop, block interaction and infinite
inventory on desktop and touch, revision gaps and resync, the greedy meshing worker pool; every 3d
exit criterion verified);
**3e — persistence and debug tooling** (done: SQLite + zstd world files natively and in
OPFS, autosave off the tick, migrations, crash-safe saves, settings and permissions from launch
options; the in-game terrain map and regenerate-and-diff checks; every 3e exit criterion verified).
3c comes before edits and persistence so the world's
bounds, generator version and wire formats change before saved worlds depend on them.

**Goal:** Spec Phase 3. Generate a real procedural world, stream it reliably, and keep client
and server collision identical (§6) — at planet scale: an 8,192 km disc, 8,192 m tall (ADR 0011).

Deliverables
- [x] **Terrain generator** in `server/core` (`dwell/worldgen`, §6.3), built in stages:
  1. [x] Deterministic noise library (integer-hash gradients; fixed-point vs. strict-float
     prototype → [ADR 0010](./adr/0010-worldgen-noise-numerics.md): strict float).
  2. [x] Climate/biome fields, biome-blended base height, 3D overhang density.
  3. [x] Caves (spaghetti + cheese), surface/strata materials, water to `SEA_LEVEL`, bedrock.
  4. [x] Ores and features (trees, boulders) using order-independent hashed placement.
  5. [x] Stability pass removing small floating components.
- [x] Server worldgen thread pool with per-tick budget; spawn region pre-generated. Client worldgen
  worker pool with transferable buffers (ADR 0007). *(3b)*
- **Scale foundations** *(3c;* [ADR 0011](./adr/0011-planet-scale-world.md)*, §6.3)*:
  - [x] World constants in `shared/protocol/constants.json`: `WORLD_RADIUS` (8 192 000 m),
    `WORLD_MIN_Y` / `WORLD_MAX_Y` (−2 048 / 6 144), `SEA_LEVEL` (0), `POSITION_FIXED_SCALE` (256);
    `WORLD_HALF_EXTENT` and `VIEW_HEIGHT_CHUNKS` removed.
  - [x] Jolt built with `JPH_DOUBLE_PRECISION` natively and in every WASM build; `RVec3` world
    positions through the server, player controller, probes, terrain collision (regions with
    anchors, PLAYER_CONTROLLER.md §5) and predictor; the client sim's state block in doubles.
  - [x] Protocol v4: `pos64` for the local player's snapshot state and `Respawn`, `posfix`
    (1/256 m `i32`) for remote players (entities and `PhysicsEvent` get it when built); C++ and TS
    codecs; golden vectors regenerated from the Python reference encoder (positions near the rim,
    a NaN and an out-of-range position among the malformed ones).
  - [x] Generator version 3: split-coordinate noise (integer lattice cell + float offset, no whole
    world coordinate converted to float); large-scale variation across the disc (a placeholder
    continent/ocean layer with ranges and basins — terrain style stays prototype, §6.1); relief
    rescaled to the new vertical bounds with sea level at 0; nothing generated outside the disc
    (in every generator); spawn search unchanged apart from the new sea level; golden hashes
    regenerated, adding chunks near the rim, ~8,000 km out, and at the top and bottom of the world;
    `dwell_worldgen_inspect` accepts far coordinates.
  - [x] Air test (`AirTestFor`, `TerrainGenerator::IsAirChunk`): the generator's own sky shortcut,
    cached per chunk column; unmodified all-air chunks are neither generated nor stored, and travel
    as payload-free `Air` messages (see deviations).
  - [x] Spherical interest management (`x² + y² + z² ≤ r² + r`, unloading outside r + margin),
    clipped to the world's rows; the server's generation region and startup pre-generation skip
    air chunks.
- [x] Client meshing worker pool. *(3d: `mesh/pool.ts`, `cores − 2` workers, 1–4)*
- **Cross-platform determinism:**
  - [x] The generator compiled to WASM for local mode and the client sim; CI golden test comparing
    chunk hashes between native and WASM builds (`dwell_tests`, `dwell_worldgen_tests.js`).
  - [x] The generator in the client worldgen worker (`dwell_worldgen.wasm`; the client test
    reproduces the golden hashes). *(3b)*
- [x] Handshake carries `worldSeed` + `generatorVersion` + the verification chunk; the client's
  `WorldgenCheck` hash selects generated vs. full-chunk mode. *(3b)*
- [x] Chunk encoding: palette + RLE, with `revision` per chunk; `ChunkData` `Generated` /
  `Explicit` forms and `ChunkUnload` (§8.3). The server keeps modified chunks and evicts unmodified
  ones far from players. *(3b; zstd compresses chunks in the world file, 3e; the wire stays
  uncompressed — see the 3e deviations)*
- [x] **World persistence** (§6.4, ADR 0006): SQLite + zstd in `core/storage`; schema v1 (`meta`,
  `settings`, `chunks`, `players`, `permissions`); native VFS (WAL) and OPFS VFS in the worker;
  transactional autosave of dirty data off the tick; load on start; migrations framework.
  Local-mode worlds persist across page reloads. *(3e)*
- Debug tooling:
  - [x] Seed and generator selection (`?seed=`, `?world=`; `dwell_server --seed --generator`).
  - [x] Biome/heightmap overview: `dwell_worldgen_inspect` (ASCII map, biome shares, timings,
    spawn, vertical sections); in game, the F4 terrain map (biome and hill-shaded height around the
    player, from a worldgen worker). *(overlay: 3e)*
  - [x] "Regenerate chunk and diff" check: `dwell_world FILE diff [cx cy cz]` over a world file,
    and in game the F3 overlay's line for the player's chunk. *(3e)*
- [x] Interest management: per-client view radius; stream nearest-first; unload far chunks;
  bandwidth budget per client. *(3b)*
- [x] Greedy mesher shared in spirit by both sides:
  - [x] Server: per-chunk Jolt `MeshShape`s, rebuilt on change (built in Phase 2b, as sub-shapes
    of one terrain body with unit-quad faces; greedy merging is not used for collision because
    its T-junctions cause ghost contacts — PLAYER_CONTROLLER.md §5).
  - [x] Client: greedy mesher in a Web Worker producing render meshes; the client prediction world
    uses the same collision as the server (the sim core's C++ `TerrainCollision`, not triangles from
    the worker — see deviations). *(3d)*
- [x] Block edit loop: client `BlockEditRequest` on `control` → server validation → reliable
  `VoxelModification` broadcast → clients apply in order and re-mesh. *(3d; protocol v5)*
- [x] Revision gap detection → client requests chunk resync (`ChunkResync`). *(3d)*
- [x] **Block interaction** (§6.5) *(3d)*:
  - [x] Targeting: voxel ray cast from the eye within `REACH_DISTANCE`; outline on the targeted
    cell.
  - [x] Break (left click) and place against the targeted face (right click); touch: tap the view,
    with a Break/Place toggle button.
  - [x] Infinite creative inventory: every placeable material of the prototype set (§6.1); hotbar
    HUD; number keys, scroll wheel, or tapping a slot selects.
  - [x] Server validation (§11): reach, line of sight, cooldown, permissions, no placement into a
    player capsule, bedrock unbreakable.

Deviations and additions (3a):
- The generator lives in `server/core/{include/dwell,src}/worldgen` (the core's layout) rather than
  a `server/core/worldgen` directory.
- Procedural terrain (generator version 2) replaced the playground as the default world for
  dedicated servers and local mode; the playground stays available as version 1.
- Players spawn at a generator-chosen point (level, open land near the origin) instead of a fixed
  configured one; `ServerConfig.spawn` still overrides it.
- The stability pass works within the chunk (components touching a chunk face are kept), so it
  stays a pure function of the chunk coordinate; small pieces crossing a chunk border survive.
- Until the worker pools (3b), the client generated and meshed chunks on the main thread within a
  4 ms per-frame budget (it was two chunks per frame; procedural chunks cost ~1.5–4 ms each).
  3b moved generation to the worldgen workers; meshing stays budgeted on the main thread until 3d.
- Found along the way: WebTransport datagram writes queued behind a slow main thread, so on slow
  frames the server received inputs seconds late; datagrams now coalesce (newest per type).
- The e2e two-client test walks 2 s instead of 1 s: two pages rendering terrain on CI's software
  renderer can run below 60 ticks/s, and the test checks visibility, not speed.

Deviations and additions (3b):
- The default view is `VIEW_RADIUS_CHUNKS` = 3 and `VIEW_HEIGHT_CHUNKS` = 1 (about 110 chunks;
  3a drew 5 × 5 × 3). A radius of 5 and ±2 rows (~485 chunks) streamed fine, but CI's software
  renderer drew the one-quad-per-face meshes at 2–6 fps, starving prediction. Raise the view once
  render meshes are greedy-merged and meshed in a worker (3d).
- The wire carries palette + RLE without general-purpose compression: generated mode sends 18-byte
  `Generated` messages for untouched chunks. zstd comes with storage (3e).
- `server/core` now owns threads: the worldgen pool (`ServerConfig::worldgen_threads`; 0 in the
  browser, where local mode generates on its tick within a 4 ms budget) rather than exposing jobs
  for the host to schedule.
- The client worldgen workers run a separate, generator-only WASM build (`dwell_worldgen.wasm`,
  ~30 KB) instead of the full sim core, so each worker stays small.
- The client sim's world is streamed: missing chunks read as air, and prediction waits until the
  chunks around the player have arrived ("Loading terrain…").
- `WorldgenCheck` with hash 0 requests full-chunk mode; `?chunks=full` does that for testing.
- The e2e walks count predicted ticks instead of wall time, and wait for the terrain to finish
  loading: two pages on CI's few cores run well below 60 ticks/s otherwise.

Deviations and additions (3c):
- Terrain collision is split into region bodies (2 048 m, each at its region's centre) so sub-shape
  offsets stay exact. Dividing chunks between bodies by position bumped the capsule 5.6 cm at
  region borders, so bodies hold the chunks around the players anchored to them and a player
  collides only with its anchor's body (Jolt group filter, anchor hysteresis 8 chunks); Jolt is
  now built with RTTI to allow the filter (ADR 0011 implementation notes).
- Air chunks are sent as a payload-free `Air` form of `ChunkData` instead of not at all: the client
  waits for the chunks around the player before predicting, so it must know they exist. Neither
  side generates, stores or meshes them.
- Tests can move their world: `--dwell-origin-x=far` places every player/netcode test 7 999 488 m
  east; `ServerConfig::generator_override` lets the network simulation shift its playground.
- Found along the way: debug lines were written to the vertex buffer in absolute float32
  coordinates (0.5 m steps far out); they are now relative to their first point
  (`render/debugLines.ts`, with a test).
- The golden player trace was regenerated: double precision resolves a borderline "fits standing"
  check (a crouched player pressed exactly against a ledge) the other way from tick 320; the far
  run matches the origin within 1 µm.
- The flat and playground generators respect the rim too, so test worlds near the rim behave like
  the terrain.

Deviations and additions (3d):
- Render meshing moved entirely to TypeScript (`mesh/mesher.ts`, in workers): the sim core no
  longer builds render faces (`BuildRenderFaces` and `dwell_client_chunk_faces` removed) and instead
  hands out each chunk's voxels with a one-voxel apron (`dwell_client_chunk_padded`). The workers
  produce render meshes only: collision stays in the client sim's C++ `TerrainCollision`, the
  server's own code, which is how the prediction world collides with exactly the server's geometry.
- Greedy merging applies to full cubes and water; slabs and ladders stay one quad per face. The
  chunk shader repeats textures per block across merged quads (fract + `textureGrad` into the atlas)
  instead of switching to a texture array.
- Bedrock is not placeable (placed bedrock could never be removed); the palette is every material
  but air, liquids, bedrock and the launch pad, with the four ladders as one slot whose facing
  follows the placement.
- The edit cooldown is a token bucket (one per `BLOCK_EDIT_INTERVAL_MS` = 100 ms, bursts of 3) so
  edits bunched by the network are not rejected, and the server allows 1 m of reach beyond
  `REACH_DISTANCE` for its lagging view of the player. Permissions are an edit policy (everyone /
  ops / nobody) with ops by device key; the `permissions` table (3e) will supply them.
- The line-of-sight check samples five points on the targeted face and follows each ray 1 m past
  it: at grazing angles a ray needs a while to cross the last centimetre (found by the netcode test).
- Found along the way: creating a chunk where the air test had read open sky did not advance the
  world's epoch, so probes kept a cached pointer to the shared air chunk and did not see a block
  placed there (`block edit: probes see a block placed into a chunk they read as open sky`, red
  before the fix).
- Touch taps allow 500 ms (the software-rendered e2e page delivers the lift ~435 ms later).
- The view radius stays 3: raising it now that meshes are greedy and built in workers is left to a
  measured change (CI still renders with SwiftShader).

Deviations and additions (3e):
- The browser VFS is Dwell's own (`core/src/storage/opfs_vfs.cpp`, SQLite built with
  `SQLITE_OS_OTHER`), calling OPFS sync access handles the worker opens up front (`dwellFiles`),
  rather than SQLite's JS/WASM distribution: the storage code is the core's C++ in both builds.
  Having no shared memory, it uses exclusive locking and a rollback journal (`TRUNCATE`) instead of
  WAL; each side converts the other's file on open.
- Saved chunks are read on demand (≤ 16 per tick around players, or when streamed) from an index
  loaded at startup, and modified chunks are evicted once saved, so memory stays bounded as
  edited worlds grow.
- Local mode saves every 5 s (not `AUTOSAVE_SECONDS`) and when the page is hidden or closed: a tab
  can go away at any time. One world file per generator and seed; a second tab on the same world
  runs without persistence (sync access handles are exclusive).
- Settings and permissions come from launch options saved into the world (`--name`, `--motd`,
  `--max-players`, `--edits`, `--op`, `--ban`); in-game admin commands stay in Phase 23. Bans and an
  allow-list (`allow_list` setting) are enforced at join. `permissions` records who granted an
  entry (`granted_by`).
- The wire stays without zstd: Explicit chunks are rare in generated mode, and QUIC/SCTP framing
  already bounds the cost; storage uses it.
- `dwell_server` now saves to `world.dwellworld` in its working directory by default (`--world ""`
  for an in-memory world); CI's smoke run and the e2e server use their own.
- SQLite comes from sqlite.org's amalgamation (3.53.4). This sandbox could not reach sqlite.org, so
  local builds here used the same version's amalgamation as bundled by the better-sqlite3 npm package
  (via `FETCHCONTENT_SOURCE_DIR_SQLITE3`);
  CI downloads the official archive.
- Found along the way: the e2e test `local mode: the predicted player walks forward` could walk off
  a 1 m terrain ledge (its walk overshoots the 60 counted ticks by however long releasing the key
  takes); it now walks on the flat world.
- Backups, export/import UI, `bodies` and `lod_sections` remain for later phases (§6.4).

Exit criteria
- [x] *(3c)* With double-precision Jolt, the controller scenarios, golden trace and netcode tests
  pass natively and in WASM both at the origin and ~8,000 km from it, and 64 players still take
  < 1 ms per tick (native Release and WASM). *`dwell_tests` and `dwell_player_tests.js` with and
  without `--dwell-origin-x=far` (CI runs both); golden trace within 1 µm at 8,000 km; 0.42 ms per
  tick at the origin, 0.48 ms far out (native Release); `divergence.mjs` native↔WASM 1.2e-7 m/s,
  no state mismatches.*
- [x] *(3c)* Terrain near the rim has the same detail as near the origin: an automated check finds
  no float quantization in noise sampled at 1 m steps there. *`noise is as detailed ~8,000 km from
  the origin as at it` (also shows the old float-coordinate approach failing there).*
- [x] *(3c)* Walking off the rim of the disc falls into the void and kills the player. *`world
  rim: walking off the edge of the disc falls into the void and kills` (fails with the rim
  disabled).*
- [x] *(3c)* Open sky costs nothing: all-air chunks are neither generated nor stored, travel
  without payload, and the per-client chunk set stays bounded with 256 rows. *`streaming: open sky
  costs nothing; the view is a sphere across the world's rows`, `chunks the air test reports as
  air generate as all air`, and the existing unload/memory test.*
- [ ] Walking across the world streams chunks without hitches; memory stays bounded when moving.
  *Automated for the server (`streaming_test.cpp`: chunks are generated ahead of a moving player,
  never on the tick; world, collision and per-client chunk sets stay bounded) and the client
  (`chunkStream.test.ts`: unloads drop chunks and meshes; e2e: the view streams in and the player
  walks). Outstanding: a playtest walking a long distance.*
- [x] *(3d)* A player can break and place every placeable block type (desktop and touch), picking
  it from the hotbar; invalid edits (out of reach, into a player, bedrock) are rejected. *e2e
  `edit.spec.ts` (all 14 slots, by number key and hotbar click) and `touch.spec.ts` (hotbar tap,
  Break/Place toggle, taps on the view); `block_edit_test.cpp` (reach, line of sight, bedrock,
  occupied cells, players, the world's bounds, rate, edit policy).*
- [x] *(3d)* A block placed/removed by one client appears for all clients, and the player collides
  with it immediately after the update on both server and client. *e2e `an edit by one client
  appears for another on a native server`; `netcode: block edits` (a wall placed by one player
  stops another on the server and in its prediction, at 100 ms RTT, at the origin and ~8,000 km
  out); `collides with a block right after the edit arrives` (WASM client sim).*
- [x] Chunk serialization round-trips byte-for-byte between C++ and TS (golden tests:
  `chunk_data_*` in `shared/protocol/vectors.txt`, from the Python reference encoder).
- [x] *(3e)* A world edited on a native server and one edited in local mode both survive
  restarts/reloads; a crash mid-save leaves the previous save intact; a world file saved natively
  opens in the browser build and vice versa. *`persistence: an edited world survives a server
  restart` (native and WASM); e2e `local mode: an edited world is saved in the browser and survives
  a reload` (OPFS in Chromium); `storage: a crash at any point of a save leaves the previous save or
  the new one` (a VFS dropping every write after the Nth, each N: 231 crash points natively over
  WAL, 84 in WASM over the rollback journal); `storage: world files written natively and in WASM
  open in both builds` (golden files in `server/tests/storage/golden`).*
- [x] The same seed produces bit-identical chunks natively, in local mode, and in the client
  worker (CI golden test: `dwell_tests`, `dwell_worldgen_tests.js`, and
  `worldgen/generator.test.ts`); untouched chunks cost only a `Generated` message on the wire
  (`streaming_test.cpp`). Re-verified for generator version 3 (3c), including chunks near the rim
  and ~8,000 km out.
- [ ] Generated terrain shows varied relief (prototype style, §6.1), caves, and overhangs, and the
  player can walk, jump, and swim through it with no collision mismatches.

---

## Phase 4 — World LOD & Whole-World View

**Status:** in progress — every deliverable built (4a grid and generation, 4b propagation and
streaming, protocol v6, 4c the client's LOD system, rendering and creative flight — protocol v7),
with the playtest follow-ups listed in the Progress table (full-detail chunks beyond the view,
`ChunkRequest`, protocol v8; a flight speed slider, protocol v9; the LOD view held within
`LOD_CACHE_MB`; batched terrain and a memory budget for phones; stand-ins fill the holes of the forced path to the chunks with the nearest ready ancestor clipped to the hole, replacing empty sky; manual phone check outstanding). Also
outstanding: z-fighting reported high up, not reproduced here (see deviations).
Outstanding: the frame-rate part of 4c's second exit criterion, which needs a desktop GPU and a
phone (this sandbox renders with SwiftShader). Added 2026-09-29 with [ADR 0012](./adr/0012-lod-octree.md) (concepts from
the Distant Horizons mod, adapted to 3D). Sub-phases: **4a — LOD data and generation**; **4b —
propagation and streaming**; **4c — rendering and the dev camera** (built as creative flight, see
deviations). Depends on Phase 3c (planet
scale) and 3d (meshing worker pool); 4b's propagation cache lands in the database from 3e.

**Goal:** Everything that should be visible from the camera is visible, at a detail that drops
with distance — from the ground, a mountain top, or a dev camera high enough to see the whole
8,192 km disc — including every player's builds (§6.6).

Deliverables
- **LOD data and generation** *(4a)*:
  - [x] LOD grid and coordinates (`(L, i, j, k)` from the corner (−2²³, `WORLD_MIN_Y`, −2²³);
    levels 0–`LOD_MAX_LEVEL`), shared C++/TS; section content as 32³ materials plus a one-cell
    apron, encoded with the chunk palette + RLE codec.
  - [x] `GenerateLod(seed, generatorVersion, L, i, j, k)` in `server/core/worldgen`: generator
    evaluated at cell centres, octaves and features smaller than a cell dropped; all-air and
    buried sections skipped from column height bounds. Built into `dwell_worldgen.wasm`.
  - [x] `Downsample` of 8 children (≥ 4 of 8 solid → solid; else ≥ 4 liquid → liquid; most common
    material, ties to the upper cells), with unit tests on crafted layouts (one-voxel walls and
    floors survive a level; pillars thinner than a cell do not).
  - [x] Golden hashes of `GenerateLod` sections at several levels (surface, mountains, ocean, the
    rim, the root), checked natively and under WASM in CI.
- **Propagation and streaming** *(4b)*:
  - [x] Server propagation: chunk changes (every `VoxelModification` source) mark level-1 sections
    dirty; a budgeted off-tick job (`LOD_PROPAGATION_SECTIONS_PER_TICK`) downsamples dirty
    sections nearest to players first up to the root, assigning `lodRevision`s; the
    `lod_sections` cache table and its migration (§6.4); rebuilt on a generator version change.
  - [x] The LOD index at `LOD_INDEX_LEVEL`: `LodIndex` after `WorldgenCheck`, coalesced
    `LodIndexUpdate` broadcasts.
  - [x] `LodRequest` / `LodData` (`Generated` | `Explicit` | `Unchanged`); the `lod` stream on
    WebTransport, WebRTC (data channel 3) and loopback; `LOD_BYTES_PER_SECOND` budget and
    `LOD_REQUESTS_PER_SECOND` limit; full-chunk mode answers `Explicit`. Golden vectors for every
    new message in C++, TS and the Python reference encoder.
- **Rendering and the dev camera** *(4c)*:
  - [x] `lod/`: octree walk around the camera by screen-space error (`LOD_PIXEL_ERROR`, lower
    quality on mobile), parent-until-all-children-ready swaps with level-0 nodes backed by the
    streamed chunks, coarsest-then-nearest job scheduling to the worldgen and meshing pools,
    bounded cache (`LOD_CACHE_MB`).
  - [x] LOD section meshing in the meshing worker pool: greedy-merged, flat colour per material,
    border faces culled only against same-level neighbours.
  - [x] Two-pass depth split at `LOD_NEAR_SPLIT_M` (far LOD pass, depth clear, near pass).
  - [x] ~~`devcam/`: free-fly dev camera~~ → creative flight for the player's body (see
    deviations): toggled by the F key (double-tapping Space / Jump until 2026-10-15) or the touch
    Fly button, speed scaled with
    altitude, able to rise until the whole disc is in view; server-authoritative and predicted,
    with a `--flight everyone|ops|nobody` policy (protocol v7).
  - [x] Debug: F3 overlay shows LOD node counts per level, pending jobs, cache use and LOD bytes/s;
    optional per-level colouring of LOD sections.

Deviations and additions (4a):
- **Where a cell samples.** A cell samples the pipeline at its centre column and *bottom voxel*,
  not its centre: under the ≥ 4-of-8 rule a floor keeps a cell solid exactly when the cell's
  bottom voxel is solid, so this is what makes generated and downsampled sections agree (and a
  centre sample would drop terrain that fills less than half of the cells above level ~12, where
  cells are taller than the world's relief).
- **Downsample material.** The most common material among the top filled cell of each of the
  block's four columns (ties to the upper cells), not among all qualifying cells: with the
  latter, rock beneath the surface outvotes it on uneven ground and coarse levels turn grey.
- **What `GenerateLod` drops:** fractal octaves finer than a cell (fBm still normalised by the
  full amplitude, ridged sums by the kept one so ranges do not sink), tunnels outside 2 m cells,
  caves deeper than three cells below the surface, trees above 4 m cells and boulders above 2 m,
  ores and the stability pass. The apron below the world reads as bedrock (the floor is never
  drawn).
- **Bounds.** `LodBoundsAt(L, i, k)` classifies a whole column of sections from its 2D fields
  (`Empty` above, `Buried` below), so the client needs one cheap job per column rather than one
  per section to skip sky and rock; `GenerateLod` uses the same bounds.
- **Content layout.** Section content is stored in layer order (the mesher's padded layout), so
  the palette + RLE codec takes cells as they are (`EncodeLodCells`, 34³ cells, palettes up to
  65 535); the chunk codec now shares the implementation.
- The flat and playground generators share a flat `GenerateLod` (the playground's features are
  below a cell), which equals the downsample of their chunks exactly.
- Section content round-trips the codec in C++ (`lod: encoding`) and TypeScript
  (`chunkVoxels.test.ts`); golden *wire* vectors come with the `LodData` message in 4b.

Deviations and additions (4b):
- **Modified sections keep generated octants.** A modified section is `GenerateLod` of itself with
  only the octants of *modified* children replaced by their downsample, rather than the downsample
  of all 8 children with unmodified ones generated at the level below: one generation instead of
  up to eight per write, and a build changes only the octants above it (less popping when a
  section turns from generated to modified). The two agree within 4a's tolerance.
- **Apron.** Stored sections keep their generated apron; the server fills it from modified
  same-level neighbours' borders when sending (`CellsForClient`).
- **`Generated` means "nothing modified yet".** A section dirty but not yet written answers
  `Generated`; the index update after its level-8 ancestor is written (propagation goes bottom-up)
  makes the client ask again. So a client may treat a `Generated` answer as covering the whole
  subtree.
- **Wire details:** `LodIndex` has a flags byte (1 = last message) and may be empty; index messages
  carry at most 16 384 entries (`limits.maxLodIndexEntries`, under SCTP's 256 KiB); `LodRequest`
  levels are 1–19. Type ids: `LodIndex` 0x13, `LodIndexUpdate` 0x14, `LodData` 0x15, `LodRequest`
  0x4C; the `lod` channel is id 2 (its WebTransport stream's first byte; WebRTC data channel 3).
- **Threads.** Propagation (and full-chunk-mode generation) runs on one thread when the server has
  worldgen threads, else on the tick within `lod_budget_us` = 2 ms (the browser's local mode).
- **Storage format 2** adds `lod_sections` with a generator version column; empty blobs bind as
  empty, not NULL (found by the persistence test).
- Requests beyond the bucket or a 256-deep queue are dropped (counted in `LodStats`); the client
  retries what goes unanswered (4c).

Deviations and additions (4c):
- **Frame rate and memory (playtest, after #37):** the frame rate fell for minutes on desktops and
  phones (11 fps in desktop Safari) and Chrome's memory kept growing. Measured in headless
  Chromium at a realistic 1440 × 900 at 2×: the LOD's sections grew without bound (8,700 drawn,
  75,000 nodes, a 1.7 GB heap after 150 s; 70 ms of script per frame with drawing off), because
  the pixel error was counted in device pixels (a 2× screen: 4× the sections) and the cache can
  only evict what the view no longer uses. Now the pixel error is in CSS pixels and scaled up
  while the view's sections exceed `LOD_CACHE_MB` (§6.6): ~1,000–1,500 sections within 256 MB,
  ~4 ms of script. Also: a section's six skirts share its mesh (they were two thirds of the LOD's
  draw calls: 942 → 535 per frame in a small window), world matrices are computed once rather
  than for every object in both passes each frame (the largest script cost), and static vertex
  data leaves the JS heap once uploaded. The frame rate on real GPUs is still to be checked.
- **The LOD's pixel error and memory as settings (playtest after #41):** the view held within
  256 MB looked coarser than before; "Distant detail" (pixel error, 1–16 CSS px) and "Distant
  memory" (cache budget, 32–1,024 MB) sliders let players trade detail for frame rate and memory.
- **Batched terrain as a switch (after #42):** to tell whether draw calls or pixels limit the frame
  rate on real devices, `?batch=1` draws the chunks and LOD sections in three batches (7 draw calls
  instead of ~790 in a headless test, same triangles, ~150 MB more heap) and `?scale=` renders
  at a fraction of the resolution; F3 shows draw calls and triangles. Off by default until compared.
- **Memory on phones (playtest after #43):** batching and `?scale=` raised the frame rate, but
  mobile Safari crashed periodically (a tab over its memory limit is reloaded). F3 now shows
  what the game holds (WebAssembly memories, GPU geometry and drawing buffers, Chrome's heap),
  and phones get a budget: 2 workers per pool, Distant memory up to 192 MB, the WASM cores
  starting at 16 MB, batches that start small and grow by half. Phone emulation: 296 → 237 MB
  counted (batched: 356 → 266 MB). Whether that stops the crashes needs the phone.
- **`LOD_PIXEL_ERROR` 4 px desktop / 8 px mobile** (was 2 / 4): at 2 px a view from the ground
  draws ~8,200 sections (one or more draw calls each), at 4 px ~2,800; the level table in §6.6
  moves accordingly (level L from ~176 × 2^L m).
- **Job order by projected cell size** instead of "coarsest first, then nearest" globally: with
  the latter the camera's own ground stayed at 1 km cells (and the chunks under the player hidden)
  until every coarse section to the horizon was done — seen in the browser. Parents still come
  before children and coarse before fine.
- **Near sections refine off-frustum:** a coarse section beside or behind the camera has its
  surface rounded up to its cells and showed as a wall at the edge of the view; nodes closer than
  their own size refine regardless of the frustum.
- **Chunks as level 0:** a level-1 section refines into its chunks when all 8 are drawable; other
  chunks are hidden while LOD is active (all show until the root is ready). The refine test
  includes the chunk regions in its coverage check.
- **Requests:** `Generated` covers a subtree, re-asks go top down after an index change, level ≥ 8
  children are re-asked only when indexed; pacing 60/s with a 5 s retry.
- **Skirts** are separate meshes per side (only non-empty ones are created), shown per frame.
- **Renderer:** the scene lost its background colour (three.js clears with it in every `render()`,
  wiping the far pass — found in the browser); the far pass's near plane follows altitude; fog
  scales with altitude. Then, from playtest feedback, fog is off for now (after briefly reaching
  512 km): the whole world is drawn without haze (`fog.test.ts` failed while fog was on); later
  replaced by height fog (below).
- **Coarse surfaces:** a column's top cell takes its surface's material, not the bedrock a cell
  taller than the relief samples at its bottom (found in the browser: the disc was grey from
  orbit; `lod: surface` failed before the fix, and the LOD golden hashes were regenerated).
- **Liquids count as filled** in `Downsample` (≥ 4 of 8 non-air, material from the columns' top
  filled cells) and a coarse `GenerateLod` cell over the sea is water: with solids and liquids
  counted apart, oceans showed their floor from afar (playtest feedback; `lod: sea` failed before
  the change). From level 3 liquids meshed opaque (since replaced, below). LOD
  golden hashes regenerated.
- **Creative flight replaces the dev camera** (playtest feedback: the "dev camera" was meant as a
  creative flying mode for the player). A new exclusive controller layer (PLAYER_CONTROLLER.md
  §6.7) driven by a held `fly` input bit: no gravity, move along the view's yaw, jump up / crouch
  down, speed `11 m/s × (run ? 2.5) × (1 + height above sea / 32 m)`, capped at 400 m/s below
  `WORLD_MAX_Y`, feet stopping at `FLIGHT_CEILING` (24,000 km). Server policy `--flight`
  (default everyone) clears the bit for others and is told to the client in `Welcome` (u8 flags);
  `pos64` decoders accept ±`POS64_LIMIT` (33,554 km) — protocol v7. The player body's Jolt velocity
  limit is raised for it. The client keeps predicting while flying even where streamed terrain has
  not arrived (it deadlocked otherwise: found in the browser, `gate.test.ts` failed before the
  fix). `devcam/`, F8 and `?devcam=1` are gone; the LOD camera is the eye.
- **Flight has its own key** (owner, 2026-10-15: "remove double jump to fly, make it a dedicated
  button for desktop as well"). F (`dwell-game` `input::FLY_KEY`, not with Ctrl/⌘, which is the
  browser's find) toggles flight where the world allows it; a double-tapped Space or touch Jump is
  now two jumps, and the 300 ms double-tap window is gone. The touch Fly button is unchanged.
  Flight is still the input frame's `fly` bit, so the protocol is unchanged.
- **Super tall mountains** (playtest request): generator version 4 adds massifs in the cores of
  the largest ranges, crests rising a further 3,600 m (peaks ~5.3–5.6 km, ~1 % of land above
  3 km), so the whole-world view has something tall to see. Prototype relief (§6.1); version 3 is
  retired like version 2. `worldgen: terrain` "super tall peaks" failed on version 3 (highest
  ~1,900 m); the golden chunk and LOD sets gained a massif and were regenerated.
- **Chunks first on slow devices** (phone playtest: nothing drawn, though collision and block
  outlines worked): chunks around the player showed only once every LOD level above them was
  generated. Drawable chunks now wait at most `FORCE_CHUNKS_AFTER_MS` (1 s); after that the
  walk descends to them regardless, leaving unready coarse siblings empty until ready.
  `lodSystem.test.ts` "shows the streamed chunks … not ready" failed before the fix.
- **No popping when turning** (phone playtest): out-of-view sections were kept coarse, so detail
  popped in wherever the view swept. Refinement now depends on distance alone; the view only
  orders the work (out of view ranks 8× lower). Settled, ~65% more sections are loaded (all
  around rather than in view); desktop's cache reached ~85 MB after 75 s in the browser (256 MB
  budget; mobile's 96 MB is to be checked on a phone). "turning around shows the detail already
  loaded" failed before the change.
- **LOD colours match the chunks** (phone playtest: distant land paler): LOD vertex colours were
  sRGB bytes used as linear, while the chunks' sRGB texture is decoded before lighting. They are
  now linear, and a tile's average is taken in linear light. `lodMesher.test.ts` "writes linear
  vertex colours" failed before the fix.
- **Distant water as a tinted floor** (playtest): compared side by side with opaque water blocks
  (via a temporary `?lodwater=tint` switch), the tinted floor was chosen: coarse liquids (level 3
  up) are left out and the floor under them is recoloured as seen through the near water. The
  opaque mode and the switch are gone. `lodSystem.test.ts` "draws coarse water as the floor under
  it, tinted" failed while opaque was the default. *Since replaced (next item).*
- **Distant water see-through at every level** (playtest: a seam and a height step where near
  water met LOD water, a brighter band at the transition, darker tinted seas, a hard edge where
  the surface stopped at level 3): every level now draws the see-through surface over the floor
  at its true depth, 1/8 m below the cell grid as the chunks draw it, at the chunks' opacity and
  from both sides; the tint mode is gone. Compared with an improved tint (lit like the water's
  surface) for cost: per-section water meshes added ~25% draw calls in a coastal view (triangles
  +3–8%, meshing time equal), so all LOD water is one `BatchedMesh` — draw calls within a few
  percent of the tint's. `lodMesher.test.ts` "draws a sea floor inside a water cell under a water
  surface at the chunks' water height" and `lodSystem.test.ts` "draws water at every level as
  see-through, at the chunks' water height" failed before the change.
- **Cracks at section borders** (playtest: sky-blue gaps along lines in the terrain, since
  column surfaces): a step between surfaces across a section border was drawn only as a skirt,
  which is hidden when the neighbour section is at the same level. Border steps are now opaque
  walls of the section that owns them (skirts keep only the part the apron hides). In the browser,
  four downward views from 400 m (seed 5) showed 234–368 sky-coloured pixels before, 0 after;
  `lodMesher.test.ts` "closes steps between surfaces across the section border without skirts"
  failed before the fix.
- **Z-fighting on distant water** (playtest, since column surfaces): a sea floor inside a water
  cell, rounded to half cells, could land on the cell's top — level with the water surface, 1/8 m
  from it — or on its bottom, in the plane of the solid cell's own top below (drawn too, in
  another colour). The floor now stays at most half a cell up the top water cell, and the covered
  top below it is not drawn. `lodMesher.test.ts` "never draws a sea floor in the water surface or
  over the cell below" failed before the fix.
- **Snapshots dropped while flying (and swimming)** (phone playtest: terrain never finished
  loading after fast flight): `PlayerFlagsOf` resolved `kClimbing`/`kSwimming`/`kFlying` to the
  *ControllerFlags* constants of the same names, so a flying player's snapshot carried an
  unknown player-flag bit and every decoder dropped it; the client predicted on alone and the
  server streamed around its own, lagging copy. Swimming had the same defect since Phase 2 (and
  climbing showed as swimming to others). "player flags use the PlayerFlags bits" and "fast
  creative flight keeps snapshots coming" failed before the fix.
- **Loading status in a corner, and phone overlays** (playtest): "Loading terrain…" moved from the
  centre of the view to a bottom-left status (`hudText.test.ts` failed before); on touch screens
  the connection status and F3 overlay stack below the hotbar instead of under it
  (`touch.spec.ts` layout test failed before); an ⓘ button top right toggles the debug overlay
  on touch screens, which have no F3 (its e2e test failed before).
- **Replaced sessions lost their Reject** (CI flake, seen twice): a client write racing the
  server's close failed first and dropped the pending `Reject(Replaced)`; the WebTransport client
  now reads the control stream to its end before reporting the close (`webTransport.test.ts`
  failed before; the e2e passed 15/15 after).
- **Distant terrain at its true height** (playtest: the horizon, oceans included, looked too
  tall, with a solid edge): cells fill from their bottom voxel, so cell tops lifted land by up to
  a cell (+220 m at level 8, +2 km at 12) and seas to +2,048 / +6,144 m at levels 12 / 13.
  `GenerateLod` now also returns each column's exact surface, and the client draws column tops at
  it in half-cell steps (1.2–2× the triangles; exact per-column tops measured 10–70×). `lod:
  column surfaces` (unbiased within a few metres at every level) and the `lodMesher.test.ts`
  surface tests failed before. Cells, server, protocol and golden hashes are unchanged.
- **Height fog and a settings menu** (playtest request: a light atmospheric haze that fades out
  higher up, so the whole map shows from the flight ceiling): fog was linear and scaled with
  height, then off. It is now an exponential atmosphere's optical depth along each view ray,
  capped at a maximum density (§6.6), with distance, density and height sliders (distance up to
  the world's diameter), Reset and Copy JSON in a new ☰ settings menu, top left; the connection
  status and the F4 map moved clear of the button. `fog.test.ts` (horizon hazed, clearer with
  altitude, the whole disc clear from the ceiling) failed against the fog-off code;
  `settingsMenu.test.ts` covers the log-scaled sliders and the JSON, `settings.spec.ts` the menu
  and the clipboard. Checked in Chromium (SwiftShader): no shader errors, haze on the horizon,
  a clear disc from the ceiling, sliders applied live and kept after a reload. Defaults from
  playtesting: distance 4 km, density 50%, height 1.5 km (`fog.test.ts` pins them; it failed on
  the first guess of 100 km / 60%).
- **Full detail beyond the view, loading ahead** (playtest: coarse sections close by, and
  modified chunks must show at full detail, not pop in close): the pixel error wanted chunks out to
  ~120–350 m, but only the 96 m view was streamed, and drawable chunks waited 1 s. The LOD system
  now asks the server for the chunks a refined level-1 section needs (`ChunkRequest`, protocol
  v8, within `RENDER_RADIUS_CHUNKS` = 12), nearest first; the server streams them like the view's
  (Generated markers, explicit when modified, edits included) and keeps them within that radius;
  sections switch to their chunks as soon as all are drawable; and refinement and load order look
  1.5 s ahead along the camera's velocity. `chunk_request_test.cpp` (sent after the view, range
  and rate limits, explicit modified chunks with their edits, kept and unloaded by radius) and
  `lodSystem.test.ts` "asks for the chunks beyond the view…" and "looks ahead along the velocity…"
  failed before (no requests); the kept-view-chunk case failed before its fix. (The lookahead was
  removed on 2026-10-14, playtest: it looked bad; detail loads by the camera's position alone.)
  The full-detail
  distance is then a setting (playtest request): a "Full detail" slider in the settings menu,
  96–352 m (default 256 m desktop, 128 m phone), which decides where chunks are drawn instead of
  the pixel error; "draws full detail out to the chosen distance…" failed on the pixel-error rule.
- **Z-fighting high up** (playtest, "at various heights" while flying high): not reproduced in
  this sandbox. A detector rendering each view twice with different depth ranges (a depth tie
  changes pixels; nothing else does) found no ties at 30, 300 or 3,000 km, and the far pass's near
  plane clips nothing; screenshots or positions from the report are needed to go further.
- **Flight speed slider** (playtest request, to explore faster in dev): while flying, a "Flight
  speed" slider (top right) and the − / = keys set a level 0–39 carried in every input frame
  (`InputButtons.flySpeed`, bits 4–9 of `buttons`; protocol v9). Level 0 is the height-based speed
  as before; level L flies at least 11 m/s × 2^(L/2), up to ≈ the speed near the flight ceiling
  (2^19.5 ≈ 741,000×, ≈ 8,200 km/s). The server applies it like every input, so prediction stays
  exact. Kept in this browser. *Playtest fix (#32): the level is a true minimum — the 400 m/s cap
  below `WORLD_MAX_Y` now limits only the height-based speed, so the slider works near the
  ground (`flight_test.cpp` "the speed slider's level is a true minimum speed", red → green).* Tests:
  `flight_test.cpp` "the speed slider's level sets a floor…" and "…travels in the input's buttons",
  `flight.test.ts` and `flightSpeed.test.ts`, and the PlayerInput golden vector (now carrying
  level 39). Not a bug fix, so no red → green; level 0 keeps the golden trace unchanged.
- Debug hooks: `window.__dwell.fly(on)`, `window.__dwell.flySpeed(level)`; `?lod=0` disables LOD,
  `?lodcolors=1` tints sections by level.

Exit criteria
- [x] *(4a)* `GenerateLod` is bit-identical natively and in WASM (CI golden test), and a section
  generated at level L agrees with the downsample of generated level-0 chunks within a stated
  tolerance on crafted and sampled terrain. *`lod: golden` (`lod-hashes.txt`, 11 sections from
  level 1 to the root, natively and in `dwell_worldgen_tests.js`) and `worldgen module (WASM)`
  in the client; crafted: the flat world equals its downsample exactly at levels 1–3; sampled:
  at the spawn and a site of each biome (levels 1–2, 3 in the mountains) ≥ 95% of cells agree in
  class and ≥ 95% of column surfaces are within one cell, mean difference under half a cell
  (measured: ≥ 96.2%, ≥ 97.7%, 0.2 cells).*
- [x] *(4b)* A block placed by one client changes the LOD sections above it on the server within
  a bounded time, and another client far away receives the change (index update → request →
  `Explicit`) without re-downloading unchanged sections; a client with no modifications in view
  receives no `LodData` beyond the index. *Server side verified: `lod: streaming` — an edit reaches
  the root within 19 ticks and a client 50 km away gets the index update 8 ticks after it; its
  re-requests come back `Explicit` on the changed path, `Generated` beside it, and `Unchanged` for
  a held section a second edit did not touch; a client 60 km the other way receives only the index
  and its updates. Client side: `lodSystem.test.ts` — no requests while the index has no
  modification in view; under an entry, requests go top down and only to the modified path and
  its children (`Generated` covers the rest); an index change re-asks with the held revision and
  nothing more.*
- [x] *(4c)* From the ground, the view reaches the horizon with no holes: automated check that the
  selected node set covers the view frustum at every frame while moving, and that swaps never
  leave a region without a drawn node. *`lodSystem.test.ts` "covers the view with no holes or
  overlaps": walking, turning and rising to 2,000 km with jobs finishing in random order, every
  frame's sampled view points lie in exactly one drawn, empty, buried or chunk-refined section,
  and every drawn section has a mesh.*
- [ ] *(4c)* Flying up (was: with the dev camera), the whole disc becomes visible within 30 s of reaching altitude
  on a desktop build, and the frame rate stays above 60 fps (desktop) / 30 fps (mobile) with
  memory within `LOD_CACHE_MB`. *Verified: e2e `lod.spec.ts` (local mode, Chromium with
  SwiftShader) — the player flies to the 24,000 km ceiling (~65 s under SwiftShader, where the
  sim runs slower than real time; ~27 s by the formula) and the disc is covered by drawn sections
  on arrival (well within the 30 s), with the cache under
  `LOD_CACHE_MB`. Outstanding: the frame rates, on a desktop GPU and a phone.*
- [x] *(4c)* A structure built by another player is visible in LOD from 50 km away and from
  altitude. *`lod: builds from afar`: a 512 m × 512 m wall (1 m thick) is solid in the sections
  drawn from 50 km (level 8) and 100 km up (level 9) — a wall or floor survives every level once
  it spans a cell in two dimensions, so what a build needs to be seen from distance d is ~d / 90 m
  across (4 px at 1080p); `lod: streaming` carries it to another client.*

---

## Phase 5 — Multiplayer Ready (Web-First Hosting, Joining & Discovery)

**Goal:** Anyone on the GitHub Pages site (desktop or mobile) can start, host and join a
multiplayer world without typing certificates or forwarding ports: a main menu with world
management, **one-click Host** from the browser (a join code), **join by code or address**, and
a **lobby list**. The master server is built as a Cloudflare Worker with Durable Objects, and
TURN is Cloudflare's managed relay ([ADR 0013](./adr/0013-master-server-on-cloudflare.md),
§10). This phase takes the web parts of the former hosting phase (now Phase 23) ahead of the
physics phases (6–8); see *Deviations* below.

**Status:** In progress — every sub-phase is built. 5a's exit criteria are covered by
`e2e/menu.spec.ts` and a manual phone check (outstanding). 5b: the master runs at
`https://dwell-master.dropkick.workers.dev`. 5c (friend worlds) has its e2e test; its phone check is
outstanding. 5d: dedicated servers register with the master (join codes, join by address, "On your
network"); outstanding is its manual phone check. 5e: the lobby list, join receipts and the server
browser. What remains of the phase is the manual phone checks and the TURN key (manual setup;
without it the master hands out STUN only).

### 5a — Main menu & world management (client only)

Deliverables
- [x] **Main menu** when the page opens with no invite: Play (world list), Join, Settings (the
  existing settings panel). Deep links keep working: `?join=…` goes straight into the game,
  `?local=1` / `?world=` / `?seed=` straight into a local world (so existing e2e tests and shared
  links are unaffected).
- [x] **World list** (local worlds in OPFS): name, world type, seed, last played. Worlds get
  id-based file names (`dwell/worlds/<id>.dwellworld`) and a metadata index in local storage
  (`dwell.worlds`, `local/worldIndex.ts`); the world file stays the source of truth for seed and
  generator. Existing `local-g<generator>-s<seed>` files are adopted into the list when the menu
  opens.
- [x] **Create world:** name, seed (blank = random, shown afterwards), type (terrain /
  playground / flat). **Delete** (with confirmation). **Regenerate:** recreate from the same seed
  with the current generator version, discarding edits (with confirmation).
- [x] **Game menu** (the ☰ panel, which opens when the pointer is released with Esc; the ☰
  button on touch screens): Resume, Quit to main menu, and the settings. Quitting saves the world
  (waiting for the worker's `saved` reply) and returns to the menu page, which ends the worker and
  releases its OPFS handles. Host… is added in 5c.
- [x] **Join screen:** paste an invite link; a list of recently joined servers. (Join codes: 5c;
  addresses: 5d.)

Exit criteria
- [x] E2E (Chromium): open the site → create a world with a given seed → play → quit to the menu
  → the world is listed with that seed → reopen it and see an earlier edit → regenerate it and
  see the edit gone → delete it. *`e2e/menu.spec.ts`, passing in CI (run 36676901490).*
- [x] An existing local world from before 5a still opens, with its edits. *`menu.spec.ts`: a world
  saved per seed, its index entry removed, is adopted into the list and keeps its edit.*
- [ ] Manual: the menus are usable on a phone in landscape (touch, iOS Safari and Android Chrome).

### 5b — Master server on Cloudflare (skeleton, dev loop, CI, deploy)

Deliverables
- [x] ADR 0013 accepted; Open Decisions #10 and #11 resolved (§12).
- [x] `services/master`: a TypeScript Cloudflare Worker (Wrangler), routes under `/v1/`, with two
  Durable Object classes on SQLite storage (the Workers Free plan): `Directory` (one instance,
  with schema migrations and the rate-limit state) and `Room` (one per hosted friend world; a stub
  answering 501 until 5c). `GET /v1/health`, and `POST /v1/whoami` (signed: answers with the
  signer's key, to check a client's signing and clock). The Directory's tables for join codes,
  servers and receipts arrive with 5c–5e as migrations.
- [x] Request signing: Ed25519 (WebCrypto in the Worker) over
  `"dwell-master-v1" ‖ method ‖ path ‖ timestamp ‖ SHA-256(body)`, ±60 s clock skew, with the
  player's device key (§10.4) or the dedicated server's key; per-key and per-IP rate limits.
  Client side in `client/src/net/master.ts`; `shared/master/vectors.json` (made with Node's crypto,
  checked in CI) pins the format for both.
- [x] Local development: `wrangler dev` runs the master (and its Durable Objects) locally; the
  client's master URL comes from the build (`VITE_MASTER_URL`) and `?master=<url>` overrides it
  (the CSP allows `http://localhost:*` for this). `dwell_server --master <url>` moved to 5d, where
  the server first talks to the master.
- [x] Tests: Vitest with `@cloudflare/vitest-pool-workers` (routes, CORS, signatures, rate limits,
  Durable Object state, and that the main module exports only what workerd accepts); CI job
  `master` (format, lint, typecheck, tests, and a `wrangler deploy --dry-run` bundle).
- [x] Deploy workflow (`.github/workflows/master.yml`): `wrangler deploy` on pushes to `main`
  that touch `services/master`, using the repository secrets from the manual setup below
  (skipped with a notice until they exist), then a health check.
- [x] The Pages build embeds the master URL (repository variable `VITE_MASTER_URL`); the client's
  CSP already allows `https:`/`wss:`.

Exit criteria
- [x] The deployed `GET /v1/health` answers; a push to `main` redeploys it. *`https://dwell-master.dropkick.workers.dev/v1/health`
  answered `{"ok":true,…}` (checked in a browser, 2026-09-30); the push merging #29 redeployed it
  ("Deploy master server" run 36729254518, health check included).*
- [x] CI runs the master's tests against the local Workers runtime. *CI job `master` (19 tests in
  workerd), green on PR #27 (run 36685737412).*

### 5c — Friend worlds: one-click Host from the browser

Deliverables
- [x] **Join codes and signaling** (the `Room` Durable Object's protocol, over the WebSocket
  Hibernation API; the code names the `Room`, see *Deviations*): Host opens a `Room` over WebSocket and gets a short code
  (e.g. `KQ7-XM4`, unambiguous alphabet); guests join the room by code; the room relays SDP
  offers/answers and trickled ICE candidates between the host and each guest, and closes when
  the host leaves. Code guessing is rate-limited per IP.
- [x] **TURN credentials:** `POST /v1/turn` (signed by the player's key, rate-limited) returns
  short-lived ICE servers from Cloudflare's TURN service; STUN-only when TURN secrets are absent
  (local dev, CI).
- [x] **Client `PeerTransport`** (full WebRTC with signaling, unlike the ICE-lite
  `WebRtcTransport`): the same data channels 0–3 and mapping as §8.1; the transport binding is
  the SHA-256 of the host's DTLS certificate taken from the answer's fingerprint.
- [x] **Hosting:** the host page creates one `RTCCertificate` per hosting session and a peer
  connection per guest on the main thread (peer connections are not available in workers),
  relaying each guest's channels to the local-mode worker as a separate session
  (`TransportKind` WebRTC, binding = the host certificate's SHA-256). The host keeps playing
  through `LoopbackTransport`.
- [x] **Host dialog** (Host… in the game menu): who may edit and fly (`--edits`/`--flight`
  policies), the host is op; shows the code, a copyable invite link and a QR code, and the guests
  playing; Stop hosting. Visibility moved to 5d (code + same network) and 5e (public): in 5c every
  friend world is code only.
- [x] **Host profiles:** max players by platform (touch devices 4 guests, desktop browsers 8; the
  host picks up to that), enforced by the local server and the room. (Physics caps: see
  *Deviations*.)
- [x] **Backgrounding and leaving (ADR 0009):** Screen Wake Lock while hosting; when the host
  page is hidden the world pauses and guests see "host paused" (new reliable message
  `HostStatus` paused/resumed); when the host stops or closes the page, guests get
  `Reject(ServerClosing)` (new reject reason, also sent by dedicated servers on shutdown) or, on a
  lost connection, "host left". Golden-byte tests in C++ and TS for the new message and reason.

Exit criteria
- [x] E2E (Chromium, CI): two browser contexts against a local master — one hosts, the other joins
  by code; each sees the other move and a block edit. *`e2e/friend.spec.ts` (#34): passing
  locally three runs in a row, and in CI on #34 and #33.*
- [ ] Manual: a desktop browser hosts; a phone on mobile data joins by code (through TURN).
- [ ] Manual: iOS Safari hosts, an Android Chrome guest joins; locking the host phone pauses the
  world for the guest and unlocking resumes it; closing the host page shows "host left".

### 5d — Dedicated servers on the master; join by address

Deliverables
- [x] `dwell_server --master <url>` (default: the deployed master) and a persistent Ed25519
  **server key** (world `settings`); the server registers
  and heartbeats (~30 s, signed) with the master through an HTTPS client in the Rust `net/wt`
  crate: port, RTC port and ICE credentials, current cert SHA-256, name, MOTD, players,
  protocol version, visibility (`--visibility public|unlisted|none`, default unlisted; `none`
  never contacts the master). The master takes the public address from the request
  (`CF-Connecting-IP`) unless `--advertise` is given; servers also report their LAN addresses.
  Missed heartbeats expire the record (a `Directory` alarm).
- [x] **Join by address:** the Join box resolves `host[:port]` through the master to address +
  cert hash (+ WebRTC parameters). LAN addresses resolve only among servers whose public IP
  matches the requester's, so typing `192.168.1.50` works on the same network.
- [x] **Join codes for dedicated servers** (stable per server key) and invite links that carry a
  code instead of a cert hash, so they survive certificate rotation. (The server prints its code
  and a `?code=` link; `?code=` resolves to a server or a friend world.)
- [x] **"On your network":** the main menu's Join section lists friend worlds and servers whose
  public IP matches the player's (visibility "code + same network" or public) — LAN discovery for
  the web. The host dialog gains the visibility choice (code only / code + same network, the
  default).

Exit criteria
- [x] E2E: a native server registers with a local master; a browser joins it by typing its
  address and by its code; after the server stops, it disappears within two heartbeat periods.
  *`e2e/servers.spec.ts` (#36): passing locally (the server killed without a goodbye is gone
  within 2 × 2 s heartbeats plus a reload); CI on #36.*
- [ ] Manual: a phone on the same Wi-Fi finds a dedicated server under "On your network" and joins
  it (Safari over WebRTC, Chrome over WebTransport).

### 5e — Lobby list

Deliverables
- [x] `GET /v1/servers`: public servers and public friend worlds, with search and filter (name,
  MOTD, tags, player count, compatible version). *(Servers report tags with `--tags`.)*
- [x] **Reachability (player-attested, ADR 0013):** after a successful join through the master,
  the client posts a signed receipt; a server is shown as verified once receipts from distinct
  player keys arrive within a window. Unverified public servers appear only under a "new"
  filter.
- [x] **Server browser** in the client: the list, client-side ping via `StatusRequest` for
  dedicated servers (friend worlds show players only), incompatible servers marked with the
  handshake's reason.

Exit criteria
- [x] E2E: a public native server and a public hosted friend world both appear in the list on a
  third client, and joining either from the list works. *`e2e/lobby.spec.ts` (#39): passing locally,
  with the other master and hosting specs (servers, friend, menu), and in CI on #39.*
- [x] Master tests: a server without receipts is not listed as verified; one with enough is.
  *`services/master/test/lobby.test.ts`: also that a receipt needs a prior resolve, one player
  counts once, unlisted servers are never listed, and friend worlds are listed only while their
  host is connected.*

### Manual setup (Cloudflare and GitHub — done by the project owner)

These steps need an account owner's dashboard access and cannot be done from code. Do them before
5b's deploy exit criterion; nothing here requires a paid plan.

- [x] Create (or pick) a Cloudflare account on the **Workers Free** plan; note the **Account ID**
  (dashboard → Workers & Pages → right sidebar). Don't add a payment method, so the free limits
  can't turn into charges; over-limit requests fail instead.
- [x] Choose the account's `workers.dev` subdomain (`dropkick`) (Workers & Pages → Overview). The master's
  default URL is `https://dwell-master.<subdomain>.workers.dev`; record it in
  `services/master/README.md` and as the client's `VITE_MASTER_URL`.
- [x] Create an **API token** ("Edit Cloudflare Workers" template, scoped to this account only;
  an Account API token — Manage Account → Account API Tokens — was used). Add GitHub repository secrets `CLOUDFLARE_API_TOKEN` and
  `CLOUDFLARE_ACCOUNT_ID` (Settings → Secrets and variables → Actions). Optionally put them in a
  `master` environment that only `main` may deploy from.
- [x] Add a GitHub repository variable `VITE_MASTER_URL` with the Worker URL (read by the Pages
  workflow).
- [ ] Create a **TURN key** (dashboard → Realtime → TURN Server → Create). Store its key ID and
  API token as Worker secrets: `npx wrangler secret put TURN_KEY_ID` and
  `npx wrangler secret put TURN_KEY_API_TOKEN` from `services/master` (or Worker → Settings →
  Variables and Secrets). Check the current free TURN allowance on the pricing page and note it in
  `services/master/README.md`.
- [x] After the first deploy, confirm the Worker and its Durable Object classes (`Directory`,
  `Room`) exist, and that `GET /v1/health` answers. *The deploy output lists both Durable Object
  bindings; the health check answers.*
- [ ] Optional: enable Workers Logs (Worker → Settings → Observability) for debugging.
- [ ] Optional, later: serve the master at `master.dropkickarcade.com` (Worker → Settings →
  Domains & Routes → Custom domain). This needs `dropkickarcade.com`'s DNS on Cloudflare; moving
  the zone means recreating the GitHub Pages DNS records there first. Then update
  `VITE_MASTER_URL` and the server default.

### Deviations

- **New phase, inserted 2026-09-30.** Multiplayer hosting was the last phase; playtesting multiplayer
  needed an easy way to start, host and join worlds, so the web-first parts moved ahead of the
  physics phases. The former Phases 5–8 are now 6–9. The master server, friend worlds and server
  browser moved here from Phase 23; the dedicated-server distribution (binaries, Docker, admin
  commands, UPnP, backups, certificate rotation), world export/import, versioned client builds,
  and the Electron and Capacitor apps stay in Phase 23.
- Physics caps in host profiles cover what exists (players, view distance); Tier 1/Tier 2 caps
  are added with Phases 20–22.
- LAN discovery on the web is "same public IP" through the master (5d); Electron's local-network
  discovery stays in Phase 23.
- 5a: the world index is kept in local storage rather than an IndexedDB store — it is a few
  hundred bytes, read synchronously at startup, and cleared together with the worlds' OPFS files.
- 5a: the menu opens worlds and servers by navigating (`?play=<id>`, or the invite), and Quit
  returns to the menu page, instead of tearing the game down in place: a page load releases the
  worker, its OPFS handles and every render resource, a reload continues the same world, and Back
  returns to the menu. Links (`?join=`, `?local=1`, `?world=`, `?seed=`) open directly as before.
- 5a: the game menu does not pause the world — the simulation is the (local or remote) server's.
- 5a: Phases 20–22 were put on hold until Phase 5 is complete (2026-09-30).
- 5b: the `Room` class is a stub (501) and the Directory holds only its schema version and the
  rate limits; their real contents are 5c–5e deliverables, added as migrations. `POST /v1/whoami`
  was added to check signing end to end. `dwell_server --master` moved to 5d.
- 5c: join codes are the names of `Room` Durable Objects rather than rows in a Directory table:
  `idFromName(code)` finds the room, which knows whether it is open, so no second store has to
  be kept in step and an abandoned code frees itself (the room's alarm). Dedicated servers' codes
  (5d) will need the Directory table.
- 5c: host visibility moved to 5d/5e (see the Host dialog deliverable); the host's world is
  paused by the worker not stepping the simulation while the page is hidden (the browser would
  throttle it anyway).
- 5c: guests keep the room socket open while playing, so the room counts them against the guest
  limit and can tell them the host left even when the peer connection lingers.
- 5d: "unlisted" servers (the default) are found by code and address and are shown on their own
  network; only the lobby list (5e) is reserved for "public". A server that crashes disappears
  after two missed heartbeats (the interval it reports; `--heartbeat` shortens it for tests); one
  that shuts down tells the master and disappears at once.
- 5d: a server's key is kept as its Ed25519 seed in the world's `settings` (`server_key`); an
  in-memory world (`--world ""`) gets a new key, so a new code, each run.
- 5d: no certificate or `--advertise` changes are needed for LAN addresses: browsers accept a
  hash-pinned WebTransport certificate at any address, and the ICE-lite WebRTC fallback answers
  on every interface (checked in Chromium against the machine's LAN address). The e2e browser
  runs with `--no-proxy-server`: a proxy from the environment cannot carry WebTransport.
- 5d: friend worlds listed to their network are checked against their room when listed (a
  closed room's entry is dropped then), rather than removed by the room itself.
- 5e: friend worlds need no receipts to be listed: the room knows its host is connected, and TURN
  reaches it. A receipt counts only after the player resolved that server through the master
  (within 10 minutes; one receipt per resolve), and a server is verified by 2 distinct players
  within 7 days. Player keys are free to make, so this keeps unreachable servers out of the
  default list rather than stopping a determined liar; accounts can weight receipts later.
- 5e: tags are a launch option (`--tags`), not yet a saved world setting like the name and MOTD.
- 5e: the browser pings a server by opening a transport for one `StatusRequest` (WebTransport, or
  WebRTC where that is all there is) and marks other protocol versions with the handshake's own
  reason text, from the version the server reports, without connecting.
- 5b: rate limits are in the Directory's memory, not SQLite: an evicted object starts with full
  buckets, which errs on allowing requests (acceptable for abuse limits; revisit if abused).
- 5b: the master pins Vitest 4 (what `@cloudflare/vitest-pool-workers` supports) while the client
  uses Vitest 5, and uses `legacy-peer-deps` (npm's resolver crashed on optional peers).

---

## Phase 6 — Versioned Releases: Builds by Tag, a Version Launcher, Version-Locked Worlds

**Goal:** Every build stays playable (owner, 2026-10-05): each build is a git tag with a GitHub
Release holding the built files; the Pages site is assembled from the releases, serving each at
`/dwell/v/<version>/`; a small launcher at `https://dropkickarcade.com/dwell/` loads versions
dynamically from them; and **worlds record the app version that saved them and are locked to its
compatibility line** (SemVer) for now. The repository stays public under a restrictive license
chosen by the owner (a private-source split was considered and dropped to keep the free CI
pipeline). Design and setup steps: [`RELEASES.md`](./RELEASES.md).

**Status:** In progress — 6a complete (release pipeline, launcher, version-locked worlds, license);
6b (version selector, added 2026-10-06) built and tested (unit, `npm run e2e:site`); outstanding: the
phone-width check, and a stable release (owner's call) for the menu link to reach stable players.
Absorbs Phase 23's "versioned client builds"
for the web.

**Deviations:** the launcher's source is `client/launcher/`, not a top-level `launcher/` (it shares
the client's toolchain, tests and version library); the site and release scripts are TypeScript in
`client/scripts/` run with Node's type stripping, not shell. The "handshake's rejection names the
server's version" needed no wire change: the `Reject` message text carries it.

### 6a — Release pipeline, launcher, version-locked worlds, license

Deliverables
- [x] ADR: versioned releases — the app version and channels, builds as tagged GitHub Releases,
  the site assembled from releases, the launcher, version-locked worlds, the cross-version storage
  contract, the license; supersedes the deployment parts of §2.1 and ADR 0005's "older builds" note.
- [x] License: **all rights reserved** (owner, 2026-10-05) in `LICENSE`, the package manifests
  (`"license": "UNLICENSED"`, lockfiles in step; Cargo `license-file`) and the README; a root
  `THIRD_PARTY_NOTICES` generated by `shared/licenses/gen.py` (third_party.json + texts for the
  CMake, npm and Emscripten components; `cargo metadata` for the 196 crates linked into the server).
- [x] `THIRD_PARTY_NOTICES` shipped in every build (and beside the native server), linked from the
  menu's About screen; CI runs `shared/licenses/gen.py --check`.
- [x] App version: one semantic version embedded in every build (HUD and menu instead of the commit
  SHA), recorded with `protocolVersion` and generator versions in `build.json`; stable and dev
  channels. Semantic Versioning 2.0.0 with a declared public API (world format, generated terrain,
  protocols, the storage contract); the baseline release is `0.1.0`; before `1.0.0` breaking
  changes bump MINOR, compatible ones PATCH; dev builds `0.2.0-dev.<run>+<sha>` (`RELEASES.md` §3).
- [x] Release workflow: build each version for `base: '/dwell/v/<version>/'`, publish it as a
  GitHub Release (`v<version>`, or a `v<version>-dev.<run>` pre-release) with the build archive and
  `build.json`; prune old dev pre-releases.
- [x] Pages workflow: assemble the site from the releases (every stable, the newest dev), generate
  `versions.json`, deploy; size reported per deploy against the 1 GB limit.
- [x] Launcher: pick the version (world, invite/code, server, latest stable or dev), redirect with
  the query kept; Back to the menu returns to `/dwell/`; clear errors for missing versions.
- [x] Worlds: `app_version_created` / `app_version_last` in the world file and `appVersion` in the
  world index; saves from before the baseline ignored (deletable from the menu), none migrated; a
  build opens a world only on the world's compatibility line and at or after `app_version_last`
  (dev builds' worlds: that exact build), in the browser and `dwell_server` (which names the
  version to run); the launcher picks the newest qualifying build; the menu lists all worlds with a
  version badge.
- [x] Cross-version storage contract: the world index, settings and other shared stores are
  append-only and preserve unknown fields on rewrite — in the baseline release before anything
  else depends on it.
- [x] Master server: rooms and listings carry the host's app version; join-by-code and the server
  browser open the matching build; the handshake's rejection names the server's version.
- [x] `ARCHITECTURE.md` §2.1, §3.1, §6.4, §10.3 and §10.5 updated.

Exit criteria
- [x] A release produces a tag and a GitHub Release holding the build and `build.json`, listed in
  `versions.json` and served at `/dwell/v/<version>/`. Verified by the Release run on #47's merge:
  tag and Release `v0.1.0` exist, and its site deploy succeeded.
- [x] e2e against a locally assembled two-version site: the launcher opens the latest stable;
  `?play=` opens a world in its own version; an invite opens the host's version; Back returns to
  `/dwell/`; other versions' worlds are listed with badges.
- [x] Compatibility rules hold in the browser and `dwell_server`: a `0.1.0` world opens in `0.1.1`,
  a `0.1.1` world not in `0.1.0`, neither in `0.2.0`, a dev build's world only in that build.
- [x] An older build rewriting the world index keeps fields it does not know (test).
- [x] Saves from before the baseline are ignored without errors and can be deleted from the menu.
- [x] `LICENSE` in place and referenced from the manifests and README; `THIRD_PARTY_NOTICES`
  generated, its picks audited (each crate's chosen license matches the text it ships), and
  `gen.py --check` reproduces it exactly.
- [x] Every build carries `THIRD_PARTY_NOTICES`, reachable from the menu; CI fails when it is stale.

### 6b — Version selector

**Goal:** Players choose which published build to play (owner, 2026-10-06), not only developers
through `?version=`: a list of the site's versions, from which any one can be opened. Today the
launcher always picks for them (the latest stable, or the latest dev if they ticked "Open dev
builds").

**Design:** the selector lives in the **launcher**, not in the game's menu, and its entrance is
the launcher's too (owner, 2026-10-06: a link in the menu only reaches players on a build that has
it). The launcher is the only unversioned page, so it is always current; published builds are
immutable and could never gain or fix a selector. A plain visit to `/dwell/` lands on a short
launcher screen — **Play Dwell <latest stable>** and **Choose version**, waiting for a click
(owner, 2026-10-06: no auto-continue; the default is the latest stable) — so the picker is one step from the site root whatever build was
played last. Links that name a world, game or build go straight through. `/dwell/?versions` opens
the picker directly, and newer builds' menus also link to it. A choice
opens that build for the visit only and is not remembered: Back to the menu still lands on the
latest, so a player is never stranded in an old build whose menu has no way back. The choice obeys
the same rules as the launcher's other routes: version-locked worlds (`RELEASES.md` §6) and
`minLauncher`.

Deliverables
- [x] 6b: the launcher's version page at `/dwell/?versions`: every build in `versions.json`, newest
  first, with its version, channel, date and compatibility line; the latest stable marked as
  recommended; dev builds listed only when the player opts in (the existing `dwell.channel` key —
  no new meaning for older builds that read it). Selection logic in `select.ts`, with tests.
- [x] 6b: for each build, which of the player's worlds (from the shared world index) it can open,
  and a note that worlds created in it are locked to its compatibility line (for a dev build, to
  that exact build).
- [x] 6b: opening a build navigates to `/dwell/v/<version>/` through the same `build.json` check as
  the other routes (the page checks `build.json` before navigating); a build needing a newer launcher, or one that fails to load, shows the existing
  clear errors.
- [x] 6b: a plain visit to `/dwell/` lands on the launcher's screen with **Choose version**
  (`isPlainVisit`, tested).
- [x] 6b: the game's main menu gets a **Versions** entry linking to `/dwell/?versions`; the "Open
  dev builds" tick moves from About to the version page (same key).
- [x] 6b: `RELEASES.md` §5 and `ARCHITECTURE.md` §2.1 describe the version page as built.

Exit criteria
- [x] Unit tests: the list's order, the dev opt-in, and per-build world compatibility, including
  dev builds' worlds and an index with unknown fields.
- [x] e2e against a locally assembled two-version site (`npm run e2e:site`): the menu's Versions
  entry opens the version page; a plain visit's landing screen waits for a click and offers Play (latest stable) and
  Choose version; picking the older build opens it at `/dwell/v/<older>/`; its Back
  to the menu returns to `/dwell/` and the latest; `/dwell/?versions` works for a build without
  the menu entry.
- [ ] Manual check on a phone: the version page is usable at phone width.

---

## Phase 7 — Fantasy Look: A First Pass at Colour

**Goal:** A first stab at better colours, so the world reads as warm, colourful, happy high
fantasy: warm light and cool shadows, saturated greens, turquoise water, warm rock, a gradient sky
with distance fading into a pale horizon haze. **Rendering and palette only — no terrain
generation change:** no new materials, no generator version bump, the same landforms and
vegetation. Design, measured target palette and a full description of the reference image:
[`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §1. (Biome-tinted vegetation is in Phase 11c;
clouds are in Phase 12.)

**Status:** ✅ Complete. The owner approved the before/after screenshots and the
frame time on 2026-10-06. Follow-up (`FIX_SPEC_LOD_SKY_FLIP.md` item 4, built): the sky is one sphere
graded from day to night whose frame turns as a unit (client-only; `?skyrot`, `?skyspin`, a settings
slider), with the lights weighted smoothly by height; manual check in a browser outstanding. Not done: the full-disc view is in the screenshot script but was never shot
(a 24,000 km climb at a few frames a second); shoot it with the next change that touches the look.

**Deviations:** the screenshot script is `client/scripts/shots.ts` (run with Node's type stripping
against a served build), not a Playwright spec: it drives one page through the views and writes
PNGs, with no assertions. Shadows from the sun were suggested during the phase and are not part of
it (a new render pass, against this phase's no-extra-pass rule).

Deliverables
- [x] Material colours and procedural tiles of the existing materials retuned toward the measured
  palette (§1.3–1.4); LOD colours follow via tile averages.
- [x] One shared per-face RGB tint table (warm top, cooler sides, blue bottom) used by both the
  chunk mesher and the LOD mesher, replacing the scalar face shades.
- [x] Warm sun, re-coloured hemisphere light, tone mapping with an exposure setting (in the
  material shaders, no extra pass).
- [x] Sky gradient (zenith → horizon, sun glow) replacing the flat clear colour; height fog fades
  to the sky's colour in the view direction; turquoise water (chunk and LOD).
- [x] An e2e screenshot script for fixed views (spawn, forest edge, coast, distant mountains, the
  disc from the flight ceiling), run before and after.
- [x] `ARCHITECTURE.md` §5 (rendering) and §6.6 (LOD colours, fog) updated; `WORLD_GENERATION.md`
  §1 trimmed to what was built plus rationale.

Exit criteria
- [x] The owner approves before/after screenshots of the fixed views as a step toward the
  reference's mood (manual).
- [x] Unit tests: chunk and LOD faces of the same material and direction get identical colours;
  the tint table is warm on top and bluest underneath; the horizon fog colour equals the sky
  gradient's horizon colour (`render/look.test.ts`).
- [x] Worldgen golden hashes (chunks and LOD) unchanged — generation untouched.
- [x] Frame time within ±5 % of before on desktop and a phone (F3 readout).

---

## Phase 8 — Block Registry: Namespaced Block States and Palettes

**Goal:** Replace the hand-numbered global material table with Minecraft's pattern (owner's
decision, 2026-10-05): namespaced blocks (`dwell:dirt`) with typed properties, canonical state
strings (`dwell:ladder[facing=east,flooded=false]`), dense runtime state ids generated from data
files, and world files that store chunk palettes as strings, so content can change without breaking
saved worlds. The foundation for slopes (Phase 9), flooded blocks and new content. Design:
[`BLOCK_REGISTRY.md`](./BLOCK_REGISTRY.md).

**Status:** ✅ Complete (marked complete by the owner, 2026-10-06). No visible change in the game:
same terrain, same blocks. Breaking: the saved world format and the network protocol (v11) change, so
it starts a new compatibility line: `package.json` is raised to 0.2.0 by the owner's decision
(2026-10-06) ([`RELEASES.md`](./RELEASES.md)).

Deviations from the design ([ADR 0015](./adr/0015-block-registry.md)): state ids follow the
declaration order of the data files, not a sort by name (appending a block keeps earlier ids); a
state's `placeable` flag comes from an optional `palette` list in the data; `wasm.dwellworld` was
written natively with the browser's journal settings (`DWELL_WRITE_GOLDEN=wasm-style`) because the
WASM toolchain was not available when it was regenerated — CI's WASM job opens it; paletted
in-memory chunks were measured and deferred.

Deliverables
- [x] ADR: the block registry — identity and canonical strings, data files and generated
  registries, runtime ids and the registry hash, string palettes in the world file (no migration
  of earlier worlds: they are version-locked, Phase 6); supersedes the material-table parts of
  §6.1.
- [x] Block data files (`shared/blocks/`) for today's 21 materials (ladders as
  `dwell:ladder[facing,flooded]`) and a generator emitting the C++ and TypeScript registries,
  replacing `voxel.h`'s table and `materials.ts`'s hand mirror; canonical string parse/write.
- [x] Every user of material ids (generator, meshers, collision, controller, edits, palette, LOD)
  on runtime state ids resolved by name; `Placeable` and render styles from the registry.
- [x] World file: the `block_states` table and chunk palettes of world state ids; the golden world
  file regenerated in the new format; the LOD cache dropped on a registry change. (Aliases, upgrade
  rules and an unknown-state placeholder are deferred to world upgrades, `FUTURE.md`.)
- [x] Protocol: the registry hash in `Welcome` (version bump, golden vectors in both languages).
- [ ] Paletted in-memory chunks (bit-packed indices): measure memory and meshing/collision time;
  adopt only if the trade is good, otherwise record the numbers and defer.
- [x] `ARCHITECTURE.md` §6.1, §6.4, §6.6 and §8.3 updated.

Exit criteria
- [x] Every state's canonical string round-trips; C++ and TypeScript registries give the same order
  and hash. (`block registry:` tests in C++ and `blocks.test.ts`, against `shared/blocks/vectors.txt`.)
- [x] A world saved and reopened in the same build is identical (terrain, edits, every voxel's
  string): `storage: a world saved and reopened is identical…` and the server restart test.
- [ ] The regenerated golden world file reads natively and in the browser. Native: ✅ (`storage:
  world files written natively and in WASM open in both builds`). Browser (WASM): pending CI.
- [x] Determinism goldens pass natively (regenerated once, the registry hash recorded in each).
- [ ] Determinism goldens pass under WASM and in the client module; the e2e palette test passes
  (needs the WASM build and a browser: CI).

---

## Phase 9 — Slope Blocks

**Goal:** Standard (45°) and gentle (26.57°) slope blocks with hip and valley corners, upright and
inverted, made from the shapeable materials: placed by players, generated on the terrain surface,
walked on smoothly by the physics player controller with identical collision on server and
client, and used by the LOD to draw distant terrain as faceted slopes instead of terraces. Design,
the shape table and the angle check: [`SLOPE_BLOCKS.md`](./SLOPE_BLOCKS.md).

**Status:** Built (9a–9d); verification outstanding; **9e (adaptive natural blocks) planned, not started** (below). **Re-scoped by the baseline rework**: Phase 15 cuts the placed slope and slab families and the slope stage; 9e is built as Phase 18, with the rule table as data. Sub-phases: **9a — shapes, slope families, meshing, collision
and the controller**; **9b — building** (palette, orientation, validation); **9c — terrain shaping**
(generator version 5); **9d — LOD slopes**. Runs after Phase 8 and before Phase 20, whose cluster
shapes and integrity rules must know about slopes. Outstanding: everything that needs CI or a
person — the WASM suites (controller scenarios, determinism goldens, native↔WASM divergence), the
browser e2e tests (including the new shape specs), the frame-time and generation/meshing timings on
a desktop and a phone, and the owner's manual review. **Breaking change:** generator version 5
changes the terrain a seed generates and the registry hash changes with the new states, so this
is a new compatibility line, so `client/package.json` is raised to 0.3.0 by the owner's decision
(2026-10-06). **Playtest follow-up (patches 0.3.1 and 0.4.1):**
in hills and mountains the generated slopes had gaps under them and many one-block steps stayed
bare cubes — where a cell's corners spanned a block from a half-height, the slope rule stacked two
pieces with the upper one's flat bottom over the lower one's slope (14 % of shaped cells sampled),
and ground steeper than a block per cell stayed cubes (39 % of one-block steps unsloped). The fix
gives each column one surface cell on solid cells, clamping such corners into the cell holding the
column's own surface (regression tests for both). It changes the terrain a seed generates, but by
the owner's decision ships as patches with the same generator versions — 0.3.1 on the 0.3 line
(`release/0.3`, the first backport) and 0.4.1 on the 0.4 line: worlds take it in chunks not yet
edited. **Playtest follow-up (rendering):** a slab's and a slope's side faces took the side tile by world height, so the grass fringe stayed at the cell's top (slab sides all dirt, slope sides with a horizontal band); the side tile now runs down from each side face's top edge (regression test in the mesher).

**Deviations:** `stone_slab` keeps its slot but the other materials' shaped blocks have no palette
slot of their own (one slot per material plus a shape key); families use `palette: all`;
partly covered faces are kept, so some back-to-back interior faces remain (the adjacency test proves
no holes); the "smooth" brush is not built. Details: [ADR 0016](./adr/0016-slope-blocks.md).

Deliverables
- [x] ADR: slope shapes as registry block families (`<m>_slope[facing,flooded,half,shape]`,
  `<m>_slab[flooded,half]`) and flooded blocks.
- [x] 9a: the 9 shapes × 4 facings × upright/inverted (§1.2) as generated slope and slab families
  for the shapeable materials, with coverage, triangles, volume, convexity and `placeable` per
  state; `SurfaceHeightAt` for point queries; flooded states drawn and swum in like water.
- [x] 9a: chunk mesher — coverage-based culling, sloped faces with true normals, top-tile
  projection, normal-interpolated face tint shared with the LOD mesher.
- [x] 9a: terrain collision meshes with sloped triangles; `maxSlopeAngle` raised above 45°
  (a tuning change with a test that fails on 45°); a slope playground and controller scenarios
  (ramps, corners, gentle 2 × 2 corners, slope into a wall, crouching under a sloped ceiling).
- [x] 9b: creative palette shape selector (key and touch), orientation from facing and the hit
  face, a placement preview, `CheckBlockEdit` against the shape's true volume; breaking gives the
  base material.
- [x] 9c: the corner-height surface rule (§5) in the generator, point queries and
  `GenerateLod`; flooded slopes below water levels; generator version bump, goldens regenerated.
- [x] 9d: the LOD mesher derives slopes from surface heights at every level (no LOD format
  change).
- [x] `ARCHITECTURE.md` §6.1, §6.3, §6.5, §6.6 and §8.3 and `PLAYER_CONTROLLER.md` §6.2 updated.

Exit criteria
- [x] Shape table verified from geometry (corner heights, volumes, coverage, convexity); the slope
  families' canonical strings round-trip.
- [x] Exhaustive adjacency test: no holes or overlapping faces for any pair of shapes on any side.
- [ ] Controller slope scenarios pass natively (done, at the origin and ~8,000 km out) and in WASM
  (CI); the divergence check passes (CI); no slope launches, hops or sliding at rest.
- [ ] Generated slopes within half a block of the continuous surface, shared corners agree, flooded
  below water; LOD slopes within half a cell (all tested natively: 99.8 % within half a block,
  worst 0.72 m since 0.3.1 / 0.4.1; every shaped cell on a full one); determinism goldens pass everywhere (native done; WASM and client module in CI).
- [ ] e2e: every shape in every orientation placed and broken, seen identically by a second client.
- [ ] Frame time within budget on desktop and a phone; chunk generation and meshing times reported.
- [ ] Manual: walking a sloped landscape and building a sloped roof, reviewed by the owner.

### 9e — Adaptive natural blocks (slope improvement)

> Moved to the baseline rework as **Phase 18**, which tracks it;
> the text below is the original plan.

**Goal:** natural blocks (dirt, grass, stone, sand, snow — block data, `"adaptive": true`) take
their shape from the 3 × 3 columns around them, so touching natural blocks always join into one
clean surface and nobody places slopes for them: exposed tops rest at half height, edges slope
gently to the ground, 1-block steps become one even slope across both blocks, one-block-wide
shapes become half-height ridges, and a lone block against a higher one is a 45° ramp. Crafted
blocks, slabs and crafted materials' placed slopes keep their shapes. Design, the five rules and
the shape set: [`ADAPTIVE_BLOCKS.md`](./ADAPTIVE_BLOCKS.md); decision:
[ADR 0024](./adr/0024-adaptive-natural-blocks.md) (proposed); prototypes the rules were fitted to:
[`prototypes/`](./prototypes/README.md).

**Status:** Not started. Designed with the owner in the prototypes (2026-10-07/08); ADR 0024 awaits
the owner's acceptance; open details in `ADAPTIVE_BLOCKS.md` §8. **Breaking change:** removing the
generator's slope stage changes the terrain a seed generates (a generator version bump) and
removing adaptive materials' slope families changes the registry hash, so this is a new
compatibility line: the owner raises `client/package.json` when it ships.

Deliverables
- [ ] ADR 0024 accepted by the owner; `ADAPTIVE_BLOCKS.md` §8 decided.
- [ ] Shapes and table: the 12 corner shapes and 6 ridge shapes in `shared/blocks/shapes.mjs`
  (3 × 3 surface form for the ridge shapes), the 6,561-entry rule table baked by `gen.mjs` from the
  five rules, `adaptive` in `dwell.json`, C++ and TS readers and shared test vectors.
- [ ] Chunk mesher, terrain collision, `RayEnterShape` and `SurfaceHeightAt` use derived shapes for
  adaptive top blocks (apron with diagonals and the layer above); an edit re-meshes and rebuilds
  collision for the neighbours it re-shapes.
- [ ] Registry: adaptive materials keep only their cube state (their slope and slab families
  removed); the palette's shape key only for crafted materials.
- [ ] Generator: step 5b removed; point queries, spawn, water, features and `SurfaceAt` use the
  shaped surface; generator version bump; goldens regenerated.
- [ ] LOD: the rule table on LOD cells in place of slopes from surface heights.
- [ ] Controller scenarios on every new shape (ridges, saddles, half-and-half, single-block 45°
  ramps).
- [ ] `ARCHITECTURE.md` §6.1, §6.3, §6.5, §6.6 and `PLAYER_CONTROLLER.md` updated;
  `ADAPTIVE_BLOCKS.md` and `SLOPE_BLOCKS.md` status notes updated.

Exit criteria
- [ ] The rule table equals the five rules for all 6,561 neighbourhoods in C++ and TS, and
  reproduces every block of the owner's Shape Playground export.
- [ ] No holes: derived shapes of same-height neighbours meet without a gap at every shared edge
  (exhaustive test).
- [ ] Controller scenarios pass natively and in WASM; the divergence check passes.
- [ ] e2e: placing and breaking adaptive blocks re-shapes neighbours identically on two clients.
- [ ] Generation, meshing and frame time on a desktop and a phone within budget, against Phase 9.
- [ ] Manual: the owner walks generated terrain and builds with natural blocks, compared with the
  Adaptive Block Lab.

---

## Phase 10 — Continents from Voronoi Plates

**Goal:** A two-level jittered Voronoi layout (continent cells ~2,560 km, plates ~256 km) splits
the disc into **6–14 distinct continents with at least `OCEAN_GAP` of open ocean between any
two**, natural fractal coastlines, island chains, an ocean ring at the rim, and per-continent
character; continentalness becomes a signed distance to the coast. Design:
[`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §2.

**Status:** Built, verified natively and in Vitest; outstanding: the WASM suites and the client
module's goldens (CI: no Emscripten here), the F4 zoom in a browser, LOD generation within +10 % at
coasts and channels (≈ 2× there; interiors and open sea are within), and the owner's review of
whole-disc images. **Breaking change:** generator version 6 changes the terrain a seed generates, so
this is a new compatibility line and `client/package.json` is raised to 0.4.0 by the owner's decision
(2026-10-06). Design and what differs from it: [`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §2 and
§2.6; decisions: [ADR 0017](./adr/0017-continents-from-voronoi-plates.md).

**Deviations:** 12–13 land cells chosen by a seed-hashed count and rank (not a 0.35 chance per cell:
the design's 6–14 continents at 25–35 % land do not fit that chance); the grids are centred on the
origin with jitter 0.2–0.8 (not 0.15–0.85); the coast distance is `(D_sea − D_land) / 2` (continuous
where the bisector form jumps) and the separation clamp is relative to the point's continent; island
clearance is 750 km (not 150 km) so islands pass the separation test; `PLATE_INSET` 90 km,
`RIM_OCEAN` 512 km, shelf 80–160 km; the separation test runs 500 points per seed in CI (the
10,000 of the exit criterion take 30 s natively and run on demand: `DWELL_SEPARATION_POINTS`);
LOD sections of wide cells interpolate the layout between anchors in interiors and the deep sea.

Deliverables
- [x] ADR: continents from Voronoi plates (layout, separation clamp, macro lattice), amending
  ADR 0010 to allow correctly rounded `sqrt` (no other `<cmath>`), guarded by the goldens
  ([ADR 0017](./adr/0017-continents-from-voronoi-plates.md)).
- [x] Continent and plate layers: bounded-jitter sites, land/ocean hashing, forced land at the
  origin, ocean beyond `WORLD_RADIUS − RIM_OCEAN`, bays, island plates with their blob layer.
- [x] Domain warp shared by both lookups; signed coast distance from plate bisectors plus
  scale-dependent coast fBm; the separation clamp (continents, islands, rim).
- [x] Shelf / slope / abyss and inland rise driven by the coast distance; per-continent record
  (elevation, mountainousness, climate bias, wind, shelf width); internal plate-edge distance and
  convergence exported for Phase 11.
- [x] Macro lattice (256 m) shared by chunks, point queries and `GenerateLod`; per-thread caching
  of lattice corners and plates; chunk generation within +10 % of today (+5 % on average over two
  seeds; the layout is four cached corners a chunk) and LOD generation within +10 % in interiors
  and open sea (`dwell_worldgen_inspect <seed> bench` against `main`: levels 1–12 within noise).
- [ ] LOD generation within +10 % at coasts, shelves and channels, where every column's layout is
  evaluated exactly: ≈ 1.2 ms more a section at levels 8–12 (about twice), because the coast detail
  is rougher than a block of columns. Follow-up: share a section's layout between its bounds query
  and its generation, or evaluate the plates once per section.
- [x] Generator version bump (6); chunk and LOD goldens regenerated with coast, ocean-gap, island,
  interior and abyss entries.
- [x] `dwell_worldgen_inspect` whole-disc image mode (continent ids, plate edges, height; also
  `stats` over many seeds and `bench` timings); the F4 map zooms out to the whole disc (`-` and `=`;
  unit-tested view math, not yet run in a browser).
- [x] `ARCHITECTURE.md` §6.3 (climate, base height, world bounds' scale-of-terrain paragraph),
  §6.6, the client rows and `WORLD_GENERATION.md` §2.6 updated.

Exit criteria
- [x] Separation test: for 8 seeds and 10,000 land points each, every sample within
  0.99 × `OCEAN_GAP` (64 directions × 4 radii: 20.5 million samples) is sea or the same continent
  (0 violations; CI runs 500 points per seed).
- [x] For 8 seeds: 6–14 continents (12–13); land fraction 0.25–0.35 (28.4–33.1 %; 25.9–33.4 % over
  64 seeds); the origin on land (32 seeds); no land within `RIM_OCEAN` of the rim; coastline length
  grows ≥ 1.5× from a 16 km to a 1 km ruler (1.9–2.3×).
- [ ] Determinism goldens pass natively (done), under WASM and in the client module (CI); timings
  reported (above).
- [ ] Whole-disc images for 3 seeds reviewed by the owner (manual): generated with
  `dwell_worldgen_inspect <seed> disc out.ppm 16`, review pending.

---

## Phase 11 — Natural Terrain: Rivers, Mountains, Climate & Biomes

**Goal:** Realistic, drainage-consistent terrain after the Epic Terrain mod's techniques
(re-implemented, described in [`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §3.1): rivers in
three tiers as noise contours that always run along valley floors and reach the sea; mountains
that rise away from rivers within plate-driven uplift belts, with derivative-damped ridged detail;
lakes, terraced river water above sea level with waterfall steps; climate from altitude, coast
distance and rain shadows; biomes from a data table driven by the terrain, with biome-tinted
vegetation. Design: §3.

**Status:** In progress. Planned sub-phases (each a generator version bump): **11a — height model,
rivers, lakes, water above sea level** (built: generator version 7, tested natively; the climate was then moved to continental scale as
version 8 after a playtest found snow scattered everywhere — ADR 0019, the first part of 11c's
temperature; a playtest follow-up then made the level of detail's rivers, water and coarse
columns match full detail — ADR 0020; outstanding: the
WASM suites and client goldens in CI, a look at distant rivers in the browser, the manual river walk, the owner's review of the map images and
of the new compatibility line); **11b — mountain detail cascade** (built: generator version 9, ADR 0021, tested natively; outstanding: the WASM suites and client goldens in CI, the owner's review and the new compatibility line); **11c — climate, the
biome table and colourful vegetation** (built: generator version 10, [ADR 0022](./adr/0022-climate-biome-table-vegetation.md), tested natively and in Vitest; outstanding: the same, the cost over the phase, and a look at the foliage in a browser); **11d — fantasy landforms** (karst spires,
mesas; stretch, after the owner approves 11a–c). What 11a built, and where it differs from the
design: `WORLD_GENERATION.md` §3.10; 11b: §3.11; 11c: §3.12.

Deliverables
- [x] ADR: drainage-consistent terrain — rivers as noise contours, water above sea level as
  terraced static water, and their effect on air chunks and LOD bounds
  ([ADR 0018](./adr/0018-drainage-consistent-terrain.md)).
- [x] 11a: valley floor `V`, uplift `U` (the plate-convergence belts and the old ranges; the
  per-continent mountainousness waits for 11b/11c), distance-from-rivers factor; three river tiers
  with channel profiles that fade inland and with altitude; terraced river surfaces with waterfall
  steps; lakes; caves suppressed under water; overhangs reduced. *Wetland ponds move to 11c (they
  depend on the biome table).*
- [x] 11a: `IsAirChunk`/`SkyFloor` and `LodBoundsAt` include water above sea level; spawn prefers
  land near water.
- [x] 11b: analytic-derivative gradient noise and the derivative-damped ridged cascade (§3.4),
  amplitude scaled by distance from rivers and uplift; LOD octave dropping
  ([ADR 0021](./adr/0021-mountain-detail-cascade.md), `WORLD_GENERATION.md` §3.11).
- [x] ADR: climate with continent biases and rain shadows, the biome table, wetland ponds and
  biome-tinted vegetation ([ADR 0022](./adr/0022-climate-biome-table-vegetation.md)).
- [x] 11c: temperature with a lapse rate (v8) and the continents' biases, humidity with coast
  distance and rain shadow; the biome table (zones + altitude bands + terrain overrides + roughened
  borders); surface rules and trees from it; wetland ponds in wet, low, flat ground (moved from
  11a). *Deviations:* the borders are roughened by two small noises, not a hash per column; the
  rain shadow reads a smooth ground height, not `V + U × A` (`WORLD_GENERATION.md` §3.12).
- [x] 11c: colourful vegetation (§3.7), **as a tint by biome** (the owner's decision, 2026-10-07; the
  seven variant blocks first built were removed): grass and foliage colours in each biome row;
  `TerrainGenerator::TintGrid` and the WASM export; the worldgen worker job, the `ChunkStreamer`'s
  cache and the meshing job's tint; the mesher's `tints` vertex attribute, the atlas alpha masks and
  the chunk shader; the level of detail's per-column tint in its surface data and its mesher; the
  blossom tree shape; distant forests keep their colour (`GenerateLod` gives forested columns a
  `leaves` surface above the tree-cell limit). The in-game F4 map and the inspect tool know the 19
  biomes. *Not built:* per-tree accent groves (colour is by biome; the owner will design the trees
  later); tint on sections the player has modified, seen from afar.
- [x] 11c: an `ice` block (placeable, a cracked pale-blue tile): a frozen ocean's top water voxel is
  ice (chunks, and level-of-detail cells up to 4 m), over water and a gravel floor
  (`climate_test.cpp`: every sampled frozen column). *Not built:* freezing lakes and rivers; ice
  being slippery (no per-material friction in the player controller yet).
- [ ] 11d (stretch): karst spire and mesa provinces.
- [x] Inspect tool (11a): rivers and lakes on a local map image with a hillshade mode
  (`dwell_worldgen_inspect <seed> map`, with `biome` and `valley` modes), and on the in-game F4 map.
  *The biome table's biomes joined the map with 11c.*
- [x] 11a: goldens regenerated (generator version 7) with a lake, a stream, a river, a great river and
  a waterfall per seed, chunks and LOD; the LOD agreement test gains a lake, a river and a great
  river site.
- [x] Goldens regenerated per later sub-phase, with alpine and coast entries (11b: generator
  version 9; 11c: version 10, with alpine meadow, bare rock, snowfield, wetland, blossom grove,
  autumn woods and conifer sites, chunks and LOD) — natively; the WASM runs are CI's.
- [x] 11a: `ARCHITECTURE.md` §6.3 (pipeline stages, water rule) and §6.6 (LOD water) updated.
- [x] 11a follow-up (playtest: distant terrain changed shape and colour as levels switched, a wide
  river from afar was a dry gully with pools up close, distant water stood over its banks): LOD
  rivers keep their valleys while the cell resolves them, channels at their own width (no longer
  widened to a cell), each wet column's water level in the surface data and the water drawn there,
  and coarse columns given the plate edges (their uplift belts were missing from 256 m cells: the
  55–80 m bias once recorded as a known limit) — [ADR 0020](./adr/0020-lod-rivers-at-their-width.md),
  `lod: river valleys and their water`, `lod: column surfaces` at every level again, and the
  mesher's and LOD system's water-level tests. *Deviation:* ADR 0018's widening of channels to a
  cell is dropped (it flooded valleys from afar); streams are not drawn from 32 m cells.
- [x] 11a follow-up (playtest on 0.6.1-dev.18: river and sea shores swallowed by the water beyond
  full detail, sand sticking up out of it): with water at its level, the half-cell steps of
  distant surfaces put banks under it, beaches flush with the sea, or raised them to their cell's
  top; the mesher draws shore columns at their exact height (`lodMesher.test.ts`, shores).
- [x] 11a follow-up (same playtest: everything turned green grass farther away): distant
  materials agreed with full detail's (96–100 % of columns at every level, measured), but
  distant slopes lacked the dirt and stone sides of one-block steps that full detail shows;
  distant tops are now tinted toward their side colour by gradient, as measured in full detail
  (`lodMesher.test.ts`, step sides). (Distant forests keep their colour by 11c's canopy.)
- [x] `ARCHITECTURE.md` §6.3 (surface rules, climate, biomes) updated for 11b–11c.

Exit criteria
- [x] Rivers lie in valleys: sampled channel beds are no higher than the terrain 50–500 m to either
  side (800 m–2.5 km for great rivers, whose banks reach ~800 m): `rivers_test.cpp`.
- [x] Great rivers' water is at sea level within 5 km of the coast (12 mouths over 4 seeds).
- [x] No floating water: every water voxel has water or solid below; horizontal water/air contacts
  occur only at terrace steps (143,000 water voxels around a lake, streams, rivers and a waterfall:
  none with air below, 564 contacts all at steps, 0 elsewhere).
- [x] No cave air within the suppression depth below water.
- [x] Snow only above the altitude its temperature implies; lee sides of ranges drier than
  windward sides (`climate_test.cpp`: every snowfield column is cold and every cold column snow;
  across 1,209 ranges the lee side is 0.41 drier).
- [x] Vegetation: the biomes' tints agree between neighbouring chunks (shared edges identical), blend
  smoothly (no step over a third of the biomes' range), reach the table's colours inside a biome
  and agree with the level of detail's; the client's mesher, LOD mesher, atlas masks, streamer and
  pool tests cover the tint; a forested site's level-4 LOD surface is leaves
  (`vegetation_test.cpp`, `mesher.test.ts`, `lodMesher.test.ts`, `textures.test.ts`). *Deviation:*
  clumped accent trees are not built (colour is by biome).
- [x] Biome shares within bands for 8 seeds (every climate biome 1–35 %, over 8 seeds: 1.2–21 %).
- [x] LOD agreement thresholds still met (native).
- [ ] Chunk and LOD generation ≤ +25 % of today: measured per version (`bench`, near the spawn):
  11b chunk ±0 %, LOD sections +14–26 % over version 8; 11c chunk +23 %, LOD +11–25 % over 11b —
  about +30–57 % at LOD levels over version 8. Needs a decision or an optimisation.
- [ ] Determinism goldens pass everywhere: regenerated and passing natively; the WASM suites and
  the client module's goldens run in CI.
- [ ] Manual: a walk along a river from its spring to the sea and a flight over a range, with
  screenshots for the owner.

---

## Phase 12 — Sky Islands in a Dome

**Goal:** The world extends upward into a **full hemispherical dome over the disc** (radius
8,192 km, the disc's radius; decided by the owner, 2026-10-05), and the sky inside it is sparsely
populated with separate floating islands, after the Aether mod's sky-island terrain (landforms
only — no dungeons or creatures; the owner's spec of it was kept in
`docs/reference/aether-floating-islands.md`, removed in Phase 15), using existing
blocks only. Design: [`WORLD_GENERATION.md`](./WORLD_GENERATION.md) §4.

**Status:** Cancelled by the baseline rework (owner, 2026-10-12: no special terrain features;
Phase 15); never started. Kept below for the record until Phase 15 removes the
sky-island design. Sub-phases were: **12a — dome world bounds** (decided; does not depend on the
islands, so it may be built earlier than the rest of the phase); **12b — islands** (the
Aether density field in sparse archipelagos, surface layering, decorations); **12c — clouds**
(render-only, separable). Open details in `WORLD_GENERATION.md` §4.8.

Deliverables
- [ ] 12a: ADR superseding ADR 0011's vertical bounds: `TERRAIN_MAX_Y` (6,144 m, the ground band)
  and the dome (`InsideWorldDome`, radius `WORLD_RADIUS`); what lies at the dome's surface.
- [ ] 12a: every use of `WORLD_MAX_Y` classified as ground band or world bound and switched; edit
  validation, chunk rows, streaming (view sphere clipped to the world) and air chunks follow the
  dome; nothing is generated outside it.
- [ ] 12a: a 3D LOD above level 8 (rows up to the dome's top); `LodIndex`/`LodIndexUpdate` carry the
  row `j` — protocol version bump with golden vectors in both languages.
- [ ] 12a: player, netcode and LOD tests near the dome's top; rendering checked from inside the dome
  at altitude.
- [ ] 12b: ADR: sky islands — the Aether density field, archipelagos (layout, scale, presence over
  altitude), decoration as deterministic feature functions, island anchors (decided here, before
  Phase 20's integrity work).
- [ ] 12b: the island field (§4.3): fields A, B and selector S, height gain, vertical ramps, the
  8 × 4 × 8 lattice shared by chunks, `SolidAt` and LOD; field statistics tests against the
  reference's targets.
- [ ] 12b: archipelagos in 3D jittered cells with scale variants and a footprint fade, from
  `ISLAND_MIN_Y` (above the ground band) to inside the dome, never touching.
- [ ] 12b: island surface layering on every floor; region climate (Meadow / Grove / Forest /
  Woodland) setting tree attempts and leaf colour.
- [ ] 12b: decorations as feature functions using point queries, no neighbour reads (§4.4): edge
  shelves, lakes with the leak check, ores and pockets, springs as static waterfalls, trees with
  the layered ground finder, tiny islets (exempt from the stability pass).
- [ ] 12b: air chunks and LOD classification with archipelago bounds (cost bounded per section at
  every level); goldens with island chunks and sections.
- [ ] 12c: render-only clouds from the reference's blob walks and cloud banks (§4.9), a cloud mask
  beside each chunk from the worldgen worker, plus banks in the ground sky; ≤ ~1 ms per frame on a
  phone.
- [ ] `ARCHITECTURE.md` §6.1, §6.3 (world bounds), §6.6 (LOD), §8.3 (`LodIndex`), §7.1 (island
  anchors) and §5 (clouds) updated.

Exit criteria
- [ ] 12a: nothing generated outside the dome; edits accepted up to the dome and refused outside it;
  streaming and LOD reach the dome's top; player and netcode suites pass near it.
- [ ] 12b: field statistics within a few points of the reference's targets (solid share by band
  height, ~1/3 of columns with land, typical thickness 15–20 m, ~1 in 5 land columns with a second
  layer).
- [ ] 12b: archipelagos never touch, stay above `ISLAND_MIN_Y` and inside the dome, presence matches
  the profile, and no island is cut by a footprint edge.
- [ ] 12b: lakes never leak; springs, trees and ores follow their rules; features crossing chunk
  borders are identical from both sides.
- [ ] 12b: open sky still skipped as air chunks (a flight test counts generated chunks); LOD
  sections above the ground band classified correctly; determinism goldens pass everywhere.
- [ ] 12c: cloud cost measured on a phone.
- [ ] Manual: a flight through an island field and views of the dome's islands from the ground and
  from high up, reviewed by the owner.

---

## Phase 13 — The Bifacial World

**Goal:** A second inhabited face on the disc's underside with gravity inverted past the halfway
point (owner, 2026-10-05): the midplane (y = −2,048) becomes the world's core — diggable rock
anchored for integrity by position, no bedrock; face B mirrors face A's structure — its own terrain
(different seed streams) and its own dome of sky islands — so the world is a two-sided, sphere-like
object. Down is always toward the midplane. Design: [`BIFACIAL_WORLD.md`](./BIFACIAL_WORLD.md).

**Status:** In progress — everything below is built and tested except what is unticked. Follow-up (`FIX_SPEC_LOD_SKY_FLIP.md` item 2, built): the LOD no longer loads or draws the other face beyond 512 m (except near the rim), and level ≥ 7 sections are meshed for the viewer's face alone; manual check in the underworld outstanding. Item 3 (built): crossing the midplane turns the heading by 180° and the camera over in a half forward somersault about its right axis (no view snap); manual check outstanding. Fix (built): the client's LOD bounds cache was keyed by row 0, outside the world at levels ≤ 17 since Phase 13's row numbering, so every column shared the first column's bounds and ground above that column's height read as empty, hiding the streamed chunks there (mountains, then all land above ~62 m on 0.9.0's terrain); now keyed by the column's first row. Depends on Phases 10–12 (face B repeats their terrain and dome). Decided
(2026-10-05): no crossing routes (dig through or go around the rim), a static sun for face A and a
counter-angled static moon for face B, spawn on face A, a ~4 km crust, the rim ocean kept, and
face B reusing face A's generator, biomes, palette and islands (`BIFACIAL_WORLD.md` §9). No open
questions.

Deliverables
- [x] ADR: the bifacial world — the midplane and mirror mapping, face B's bounds and dome, the core
  anchor zone (no bedrock), gravity toward the midplane and the flip band, crossing by digging or
  around the rim, sun and moon; supersedes ADR 0011's vertical bounds
  and Phase 12's single dome.
- [x] Bounds: no bedrock and no void below — a diggable core with a positional anchor zone
  (`CORE_ANCHOR_LAYERS`); face B's ground band and dome; the LOD origin at −2²³
  in y; streaming, air chunks, edits and the flight limits on both sides.
- [x] Generation in face-local coordinates with per-face seed streams; face-B chunks as flipped
  face-local chunks; point queries by face; LOD cells by their side of the midplane; per-face
  water.
- [x] Gravity: per-face direction, the flip band with drag, Jolt gravity factors by side.
- [x] Player controller: a face sign through every vertical quantity and probe; mirrored voxel
  queries; the mirror-equivalence suite; crossing the flip band with a smooth camera turn
  (protocol bump if the snapshot needs a flag).
- [x] Crossing without dedicated routes: a shaft dug through the core and going over the rim both
  work through the flip band (no generated wells, ledges or portals).
- [x] Lighting and rendering: a static sun (face A) and a counter-angled static moon (face B), each
  fragment lit only by its own face's light and ambient; camera up by face; face-B tops shaded as
  tops; face B's moonlit night sky and haze in the viewer's face frame.
- [x] `ARCHITECTURE.md` §6.3, §6.6, §7, §9 and `PLAYER_CONTROLLER.md` updated.

Exit criteria
- [x] A face-B chunk equals the flip of its face-local chunk; face B's terrain differs from face A's;
  goldens with face-B chunks and midplane-straddling LOD sections pass natively, in WASM and in the
  client module.
- [ ] Mirror-equivalence: every controller scenario mirrored onto face B gives the mirrored trace,
  natively and in WASM, at both origins.
- [x] A player walks, jumps, swims and climbs on face B, and crosses between the faces by digging
  through the core and by going over the rim, the face sign and camera turning over once.
- [ ] Face A is lit only by the sun and face B only by the moon (ceilings included).
- [x] A body falling off the rim is accelerated through the flip band; bodies on face B fall toward the
  midplane.
- [x] Nothing generated outside the two bands and domes; streaming and LOD reach both domes.
- [ ] Manual: face B walked, a crossing made, both domes seen, reviewed by the owner.

---

Deviations (reasons in `BIFACIAL_WORLD.md` §10): sections up to level 6, not 7, lie wholly on one
face; the flip band accelerates a body moving toward the midplane (owner's decision) instead of settling it; the controller
runs in a face-local mirror frame instead of a sign threaded through every probe, and the
mirror-equivalence suite (`--dwell-face=b`) holds in every decision and to contact-solver precision
in positions, not bit for bit. Browser check (headless Chromium, local world, a player placed on
face B): face B draws upright for its player under a moonlit night sky with the moon's glow, trees and
terrain meshed right way up, face A unchanged under the sun; mouse-up looks up on both faces. Exit criteria ticked as
each passed in CI or in the browser: chunk flip and goldens (native suite, WASM worldgen suite, client
module test), controller on face B and crossings (native `bifacial` suites, the `--dwell-face=b` run, the
flip-roll test), the rim and Tier-1 gravity, and the two bands and domes. Still open: the mirror-equivalence
criterion until CI has run the native face-B steps at both origins (the step was missing its working
directory in an earlier run; the same commands pass locally), ceilings' lighting and the crossing's camera
turn in a browser, and the owner's review. The F4 map and debug overlays show face A only; straddling LOD
levels (7+) draw face B at cell resolution, shaded as undersides.

## The Baseline Rework (Phases 14–19)

On 2026-10-12 the owner set a simpler baseline, evaluated against what is built in
[`BASELINE_EVALUATION.md`](./BASELINE_EVALUATION.md). Phases 14–19 build it before the physics
phases (20–22): they cut back Phases 9–11, cancel Phase 12, re-size Phase 13's world and replace 9e.

### Goal

Replace the over-built, hardcoded world with a small baseline built on **extendable systems**:

- A **bifacial disc of 50 km radius**, a **10-layer stone core per face**, a **spherical build
  volume** around the origin.
- **Simple noise terrain** with **one Voronoi island** that keeps clear of the rim — described by a
  **terrain definition** (data), not by code.
- **Natural slopes** from the Shape Playground's rules (adaptive blocks), with the rule table as data.
- **WebGPU** rendering, with block looks and textures as data.
- **Six blocks**: grass, dirt, stone, sand, water, ladder. **No biomes, no features.**

**The system rule** (owner, 2026-10-12): a part counts as done only when it exists as an extendable
system — *if you can't reasonably build a tool to add something to it, it's not an extendable
system.* Every phase below that adds content does it through data with a generator, names
instead of code constants, and validation (`BASELINE_EVALUATION.md` §1.1). The baseline's content
is still prototype (`ARCHITECTURE.md` §6.1).

### Rules for every phase of the rework

- **Docs in the same change.** Each phase updates `ARCHITECTURE.md` (and its sub-specs) as it
  lands, removes descriptions of what it removes, and updates status tags (`CLAUDE.md`).
- **Determinism stays.** ADR 0010's rules (integer-hashed noise, integer wavelengths, `+ − × /`,
  strict IEEE) hold for everything the terrain definitions can express; goldens run natively and
  in WASM (the client's workers run the same `dwell-worldgen` crate, ADR 0028).
- **Tests with the change**; bug fixes found on the way get a red → green regression test.
- **One compatibility line.** Phases 15–18 change the terrain a seed generates, the registry and protocol
  constants: together they are a new compatibility line (`0.9.0` under today's numbering). Pushes to
  `main` meanwhile are dev builds; **only the owner raises `client/package.json`**, once Phases 15–18 are
  in (or earlier if they choose to ship a step on its own line). Phase 19 changes no public API.
- **Licence notices** if a dependency changes (`python3 shared/licenses/gen.py`).

### Order

```
14 decisions & ADRs ─► 15 cut ─► 16 world shape & build volume ─► 17 terrain definitions ─► 18 adaptive blocks
                         └─────────────────────────────────────────────────────────────► 19 WebGPU, looks & textures
```

Phase 19 depends only on Phase 15 (fewer blocks, no tints to port) and may run alongside Phases 16–18.

### What the rework does not change

Server core and threading, the player controller (minus launch pads), networking, persistence,
streaming, menus, hosting, the master server, identity, versioned releases and the launcher, and
Phases 20–23 (which wait for this rework, as they waited for Phases 6–13).

---

## Phase 14 — Baseline Decisions and ADRs

**Goal:** Settle the open decisions and record the baseline's architecture before code changes.

**Status:** Complete (2026-10-12). The owner answered D1–D10; ADRs 0024–0027 are Accepted.

The owner's answers (2026-10-12; `BASELINE_EVALUATION.md` §7 records them too):

| # | Decision | Answer |
|---|---|---|
| D1 | "10 layers of stone" | A fixed core of 10 stone layers per face from the midplane; higher ground is stone under a soil skin |
| D2 | Build sphere radius | 50 km (= the world radius) |
| D3 | Flight at the sphere | **No movement limit** (changed from the proposal, held at the boundary): the sphere bounds building and storage only; players may fly anywhere |
| D4 | WebGL fallback | WebGPU first; Three's WebGL2 backend only as an automatic fallback, all shaders in TSL; **the settings menu shows which backend the device is using** (added by the owner) — **superseded** by ADR 0028 (2026-10-13): WebGPU only, on wgpu in Rust, no fallback and no backend readout |
| D5 | Face B's terrain | Same definition as face A, own seed (own island); selectable per face |
| D6 | Launch pads | Remove the block, `launch_speed` and its tests |
| D7 | Movement playground world | Becomes a structure file the controller tests load |
| D8 | Terrain scale | Island 20–30 km radius, ≥ 8 km sea to the rim, sea level `h` = 32, land to `h` ≈ 96–160 |
| D9 | Midplane | y = 0 |
| D10 | Terrain data scope | Fixed node kinds in code; parameters and composition in data; no scripting |

Deliverables
- [x] The owner confirms or changes D1–D10; this table and `BASELINE_EVALUATION.md` §7 record it.
- [x] **ADR 0025 — the baseline world**: 50 km bifacial disc, midplane at 0, 10-layer core, spherical
  build volume, six blocks, no biomes or features. Supersedes ADR 0011's scale (keeps its precision
  decisions) and ADRs 0017–0022; amends ADR 0023 (thickness, midplane) and ADR 0016 (placed slope
  families removed).
- [x] **ADR 0026 — terrain definitions**: terrain as data (layout, height nodes, layers, spawn rule),
  generated into C++/TS, its hash identifying the generator.
- [x] **ADR 0024** amended (rule table as data, with overrides) and accepted.
- [x] **ADR 0027 — WebGPU renderer**: supersedes ADR 0002's backend (Three.js and the render
  interface stay); the look registry.
- [x] `adr/README.md` index; `ARCHITECTURE.md` §12 (Open Decisions) and the plan updated: Phase 12
  cancelled, Phases 9e/10/11's outstanding items replaced by this rework.

Exit criteria
- [x] ADRs 0024–0027 are Accepted and indexed; the decisions table has the owner's answers.

---

## Phase 15 — Cut to the Baseline

**Goal:** Remove everything outside the baseline, so later steps build on a small codebase. The
terrain stays the current generator, cut down (an interim, hardcoded step that Phase 17 replaces).

**Status:** Built (2026-10-13): every deliverable and exit criterion below is done and verified
locally — native suites (both faces, ~8,000 km out), WASM suites (player, worldgen, storage) and the
client's Vitest suites pass, with goldens regenerated; `package.json` raised to **0.9.0**, the new
compatibility line (registry, terrain and the LOD surface layout changed), by the owner's decision.
The e2e suite passes in CI. Outstanding: the owner's look at the island in a browser.

Deliverables
- [x] **Blocks** (`shared/blocks/dwell.json`): keep air, grass, dirt, stone, sand, water, ladder;
  remove bedrock, launch_pad (and `launch_speed`, its controller code and scenarios, D6),
  stone_slab, every slope/slab family (and shape picking in the hotbar), sandstone, gravel, snow,
  ice, log, leaves, coal/iron/gold ore. Regenerate the tables; the registry hash changes. Keep the
  `shapeFamilies` mechanism in `gen.mjs`, unused.
- [x] **Textures**: remove the removed blocks' tiles from `render/textures.ts`.
- [x] **Worldgen cuts** (`server/core/worldgen`): rivers, lakes, ponds, waterfalls, `Column::water`,
  the valley floor `V` and `D` (`rivers.*`); climate, the biome table, rain shadows, snow lines
  (`biomes.*`, `SampleClimate`); the ridged cascade, belts, ranges, massifs; the 3D density and
  overhangs; caves; the stability pass; ores; boulders and trees; slope stage 5b (`slopes.*`);
  multi-continent layout, plates, island plates and the shelf/abyss profile (one island at the
  origin remains in the interim).
- [x] **Tint pipeline**: `TintGrid`, WASM `dwell_worldgen_tint`, the worker's tint job,
  `TintField`, the `tints` vertex attribute, `TINT_MASKS` and atlas alpha, `LodSurface` tints,
  `tint` in block data.
- [x] **Test worlds**: the flat world gets the stone core instead of bedrock (`BEDROCK_LAYERS`
  removed); the movement playground becomes a structure file loaded by the controller tests (D7),
  `kGeneratorPlayground` removed.
- [x] **Tools**: `dwell_worldgen_inspect`'s `biomes` and `valley` modes and the biome map in F4 go
  (height map only).
- [x] **Tests**: remove `rivers_test`, `climate_test`, `vegetation_test`, `cascade_test`,
  `slopes_test`, `slope_terrain_test`, `water_search.h`, `biome_search.h` and the controller's
  launch-pad scenario; regenerate chunk and LOD goldens; hotbar count in `e2e/edit.spec.ts` = 5.
- [x] **Docs**: `ARCHITECTURE.md` §6.1, §6.3, §6.6, §7.4; `WORLD_GENERATION.md` cut to what remains
  (and its sky-island part removed with `docs/reference/aether-floating-islands.md`);
  `SLOPE_BLOCKS.md` trimmed to the shape mechanism; `BLOCK_REGISTRY.md` examples;
  `PLAYER_CONTROLLER.md` launch-pad text; `CLAUDE.md`'s hotbar note.

Exit criteria
- [x] The registry holds exactly air, grass, dirt, stone, sand, water and ladder (a test lists them).
- [x] No code references a removed block, generator stage or tint (build and grep check).
- [x] Native, WASM and Vitest suites pass with regenerated goldens.
- [x] Chunk and LOD generation time measured against generator version 11 (`… bench`).
- [x] e2e passes in CI (known failures in `CLAUDE.md` aside): green once the specs that named block
  ids by number name them by state (`stateId`), and the settings spec counts the sky rotation slider.

**Measured** (`dwell_worldgen_inspect <seed> bench`, Release, seeds 0 and 20260925): a chunk
1.03–1.07 ms (version 11) → 0.07 ms (version 12); LOD sections 0.9–2.1 ms → 0.54–0.84 ms per
section across levels 1–12. Over seeds the island reaches 28–32 km and the sea starts 19–22 km from
the origin.

**Deviations and notes**
- **Interim terrain** (generator version 12): one island (`worldgen/island.h`: a noisy, warped
  25 km circle), a 2D height field (inland rise, hills, bumps; sea floor −20 m) and a fill by depth
  (grass over dirt, sand at the shore and under water, stone). Sea level stays at face-local 0 and
  the world at 8,192 km until Phase 16. Noise fields read hashed per-seed offset coordinates, since
  gradient noise is zero on its lattice (round coordinates in every world read 0).
- **The movement playground** is a text structure file (`playground.structure`, parsed by
  `core/structure.h`: `fill` boxes of named block states) stamped over the flat world by the netcode
  tests; the client's `playground` world type went too (old index records stay readable: the store is
  append-only; resetting one starts it as a flat world).
- **Shaped-block tests removed with the families:** `slope_test.cpp`, the slope cases of the shape,
  mesher, LOD mesher, edit and interaction tests, and the client's shape-picking tests; the
  controller's half-height steps are static boxes now. The shape table and adjacency tests remain,
  over the cube. Phase 18 restores shaped coverage with adaptive blocks.
- **LOD slopes** (the mesher's slope pieces, `slopePieces.ts`, `PIECE_NEAREST`) and the shape
  placement preview were cut with the slope stage and shape picking.
- `indestructible` stays a block property with no block using it (the structural anchor of Phase 20
  may); the `kUnbreakable` check is untested meanwhile. *(Phase 27 S2a: the sandbox floor uses
  it, and the check is tested.)*
- The LOD surface's per-column data shrank from 6 floats to 4 (tints removed) and the F4 map column
  from 4 bytes to 3 (height and flags).

---

## Phase 24 — Rust Port: One Cargo Workspace, Rapier, a wgpu Client

**Goal:** Replace the C++ server and core, the Emscripten build, Jolt, and the TypeScript sim and
Three.js renderer with one Rust codebase that reproduces the Phase 15 baseline on the same
platforms (GitHub Pages, Electron, Capacitor, native dedicated servers), with a faster build and
test pipeline ([ADR 0028](./adr/0028-rust-port.md)). Runs after Phase 15 and before Phase 16.

**Status:** In progress (24f, the cutover) — every sub-phase's code is built: the workspace and
generators (24a), the world, worldgen, redb storage and protocol (24b), Rapier physics and the
player controller (24c), the server core, `dwell-net` and `dwell-server` (24d), and the wgpu
client running in the browser with workers, local mode on OPFS and the TS shell on top (24e). The
C++ tree, CMake, Emscripten, Jolt and Three.js are deleted (24f); every Rust suite passes, natively
and under WASM, and the e2e suite passes 32 of 32 locally against the Rust client and
`dwell-server`, and every CI job passes on the Rust build (Electron's smoke test included).
`package.json` raised to 0.10.0, the new compatibility line (world files moved from SQLite to
redb). CI takes ~4 min on a warm cache. Playtest follow-ups (2026-10-14, built with regression
tests): the atlas's mipmaps stop where a tile's gutter is one texel wide (filtering smaller levels
at a tile's edge reached the neighbouring tiles and the atlas's empty space: a grey, see-through
grid on distant ground at grazing angles; gone in before/after browser screenshots); each depth pass
draws what reaches its depth range along the view axis (by distance, terrain toward the sides of
the view nearer than the split in depth could be in neither pass; suspected as the cause of the gaps
seen from high up, but the gaps did not reproduce in a browser on the old code, at 0.3–2.5 km, still
or flying fast, with the owner's settings, so the cause stays open); meshes,
targets and the draw uniforms destroy their GPU buffers when replaced (wgpu's WebGPU backend does
not on drop, so GPU memory grew until the garbage collector ran and the frame rate fell over a
session); the LOD no longer loads ahead along the camera's velocity (it looked bad; distances are
from the camera's position alone). Named views (built, the owner's request): a tool places the
camera (`Game::set_view`, `__dwell.placeCamera`; the player stays at the spawn), views are entries
in `client/views/views.json` (checked by `scripts/views.ts`), `e2e/views.ts` opens one and waits
for the LOD to settle, `scripts/shots.ts` shoots them; `game_local.rs` checks that the LOD refines
under a camera placed 6 km away (failing without the override), `e2e/views.spec.ts` that a view
settles in a browser. Under software rendering the LOD at the owner's settings 1 km up takes
minutes to settle (jobs start once a frame, and SwiftShader draws a few frames a second; natively
the same view settles in 74 s of game time), so the high views wait up to 600 s; a views file with
`scale` 0.25 settles faster (1 km: 475 s, 2.5 km: 385 s; neither showed gaps). Fix found with it:
`?debug=1` never showed the F3 text (the switch arrived before the game existed and was dropped;
`e2e/views.spec.ts` failed on it, now kept by the client and applied when the game starts). The
gaps seen from high up are not pursued further (owner, 2026-10-14: no need to reproduce them). The dev
telemetry and bench (built, the owner's request, `docs/DEV_TOOLS.md`): per-frame phase timings, job
latencies, uploads and a per-tick controller trace in the client (`telemetry.rs`), read through
`__dwell`; `scripts/bench.ts` runs scripted scenarios (`client/bench/scenarios.json`) in the
installed Chrome on the GPU. Fix found with it (playtest, 2026-10-14: "usually the player can't
make a 1 block high jump"; crouch-jumping against a wall moved the player backwards): Rapier's
contact clustering leaves `solver_contacts` empty on a pair's manifolds when the pair has several (a
player on a terrain collider and against its wall), so the controller saw no contacts and absorbed
the wall-stopped drive as an external push that never decays in the air; contacts are now read
from the manifolds' points (`a_jump_always_clears_a_one_block_step`,
`crouching_against_a_wall_never_moves_the_player_backwards`, both failing before; the golden trace
regenerated, its players no longer drifting off walls while jumping; checked in a browser with the
`jump-onto-block` scenario). Performance (the owner's request, 2026-10-14: the hitch flying low and
fast; measured with the bench, unthrottled in Chrome on an Apple GPU, against the build before):
the hot maps' hasher (FxHash for integer keys, `dwell-world::hash`), a mesh job's padded voxels
copied by rows, the LOD traversal's node lookups (once per node; children copied, not cloned),
interleaved vertex attributes, per-draw data in an instance-indexed storage array, meshes in arena
pages (one `drawIndexed` per draw), pipelines and pages bound once per run, opaque draws near to
far, touching index ranges joined, both depth passes collected in one sweep. Idle 13.6 → 4.3 ms
per frame; flying low at 176 m/s p50 18.9 → 5.5 ms, p99 23.7 → 7.7 ms; at 60 Hz the flight no
longer drops frames (interval p95 32.7 → 17.6 ms); the idle frame is pixel-identical.
Outstanding: the frame-time comparison and the owner's manual play and look comparison,
including whether the frame rate now holds over a long session. ADR 0028 accepted (2026-10-13). Storage, compression and crypto decided by
Claude at the owner's request (2026-10-13, ADR 0028 §8): `redb` (files natively, OPFS in the
browser), LZ4 (`lz4_flex`), `ed25519-dalek`.

### 24a — Workspace, generators and pipeline
- [x] Cargo workspace at the repo root (`crates/`). Crates are added as their sub-phase starts
  (deviation: an empty crate per name up front would only be noise); 24a adds `dwell-protocol`,
  `dwell-blocks` and `dwell-client`. Planned: `dwell-protocol`, `dwell-blocks`, `dwell-world`
  (voxels, chunks, storage), `dwell-worldgen`, `dwell-physics` (Rapier wrapper and player
  controller), `dwell-sim` (tick, netcode), `dwell-net` (moved from `server/net/wt`; the C ABI and
  cbindgen header removed; it stays at `server/net/wt`, excluded from the workspace, while the C++
  build links it), `dwell-server` (binary), `dwell-client` (WASM, wgpu), `dwell-tools`
  (inspect, scenario trace, divergence).
- [x] `shared/blocks/gen.mjs` and `shared/protocol/gen.mjs` emit Rust (and TS only where TS still
  reads them); CI fails on stale output.
- [ ] CI: `cargo fmt`, `clippy -D warnings` (native and wasm32), `cargo nextest`, the WASM build once
  per run through `wasm-bindgen`, `rust-cache`; runs alongside the existing jobs until 24f. Built
  (the `rust` job); not yet run on CI.
- [ ] **Headless WebGPU in CI proven first**: Playwright's Chromium draws a wgpu triangle on the
  runners (software adapter); the result decides how e2e covers rendering. Built: `dwell-client`'s
  probe draws offscreen (pixels read back and checked) and to a canvas; passes locally with
  SwiftShader's Vulkan (`--use-webgpu-adapter=swiftshader`) and fails with WebGPU disabled; not yet
  run on CI.

### 24b — Core: blocks, chunks, worldgen, storage
- [x] Block registry and states from `shared/blocks` data, by name (`dwell-blocks`: tables, state
  strings and properties, shapes).
- [x] Chunks and the voxel grid, LOD sections, structure files and block edits (`dwell-world`); the
  Phase 15 generator (one island, the stone core) ported (`dwell-worldgen`). Its chunks and LOD
  sections reproduce the C++ generator's golden hashes exactly (server/tests/worldgen/golden), so
  no new goldens were needed; the goldens pass natively and in WASM (wasm32-wasip1 under Node's
  WASI, `tools/wasi-run.mjs`). The C++ worldgen, LOD, bifacial, block-edit and structure tests are
  ported alongside.
- [x] World storage on `redb` natively (`dwell-storage`: one table per former SQLite table, world
  state ids mapped by string, the save thread; the C++ storage tests that need no server are
  ported) with LZ4 chunk and LOD blobs; Ed25519 checks on `ed25519-dalek` (`dwell-protocol::auth`),
  passing the master's WebCrypto vectors.
- [x] The OPFS storage backend for the browser: built with the client (24e), which runs it
  (`dwell-client`'s `LocalWorld`, one redb file on a sync access handle; the e2e local-world
  reload and version-lock specs pass on it).

Deviation (24b): the protocol codec (`dwell-protocol`: every message, palette + RLE, posfix, the
auth transcript) was ported here rather than in 24d, because storage stores chunks in its encoding;
it passes the golden vectors (round-trips byte for byte, rejects every malformed one).

### 24c — Physics and the player controller on Rapier
- [x] `dwell-physics` wraps Rapier (`rapier3d-f64`, `enhanced-determinism`); no Rapier types
  outside it (`BodyHandle` is opaque; `Physics::rapier` and `BodyHandle::rapier` are an escape
  hatch for tests that inspect the engine's bodies).
- [x] The controller per `PLAYER_CONTROLLER.md`, re-tuned; every scenario suite ported (voxel
  geometry, crouch, steps and air, step smoothness, knockback), at the origin, ~8,000 km out and on
  face B; golden traces re-recorded.
- [x] Native↔WASM divergence check in Rust.

Deviations (24c):
- The divergence check is the golden-trace test itself: the trace is stored as exact floats and
  held bit-exactly in the origin frame, and CI runs the suite under `wasm32-wasip1` as well
  (`server/tools/divergence.mjs` and `scenario_trace` have no Rust counterpart).
- The predictor (`predictor.rs`) and wire conversions (`net.rs`) were ported here rather than in
  24d, with unit tests against an in-process server; the latency, loss and multi-client netcode
  suites need the server and stay in 24d.
- Not ported: the scenario suite's terrain-region crossing case and its two Jolt collision-mesh
  cases; one voxel collider has no regions or meshes (PLAYER_CONTROLLER.md, Rust port note).
- Re-tuning on Rapier: the world's speed limit is raised for flight; the grounded launch check
  discounts lift from driving into a step's top edge; the terrain collider keeps no mask for empty
  chunks and the test worlds read untouched chunks as air (fast flight used ~860 MB natively and
  crashed Node's WASI).

### 24d — Server, netcode and networking
- [x] Tick loop, snapshots, prediction and reconciliation, input validation, fall damage and
  respawn; the netcode suites ported (150 ms / 20 ms / 5 % link).
- [x] `dwell-server`: WebTransport and WebRTC through `dwell-net` directly, hosting, the master
  client (registration; receipts are checked by the client, which keeps its master code in TS).
- [x] Browser networking in `dwell-client` through `web-sys` (WebTransport, WebRTC data channels):
  the e2e join specs (WebTransport, the WebRTC fallback, codes, the lobby) pass on it.

Deviations (24d, so far):
- `dwell-sim` holds the server core (`server.rs`, LOD propagation, the worldgen pool, the app
  version, entropy); its suites port the C++ server, streaming, chunk-request, LOD, edit,
  hosting, persistence and netcode suites (the SQLite/zstd/WASM-VFS specifics adapted to redb and
  LZ4; the WASM branches go with the OPFS backend in 24e). Sessions are kept in id order, so every
  pass over them is deterministic (the C++ iterated a hash map).
- `server/net/wt` is copied into the workspace as `crates/dwell-net` with a safe Rust API rather than
  moved: the C++ server still links the old crate through its C ABI until the cutover deletes it
  (24f).
- The predictor smooths corrections made while the player touched another player's proxy up to the
  knockback bound (4 × the snap distance): Rapier's player-vs-player bumps mispredict by up to
  ~1.3 m where Jolt's stayed under ~0.7 m (measured on the same scenario), which snapped the spawn
  bump of the 150 ms / 5 % suite.
- The Tier 1 flip-band suite (`bifacial_physics_test.cpp`, a 24c item) is ported here as
  `crates/dwell-physics/tests/tier1_bifacial.rs`.
- The version lock reads a world's meta read-only (`WorldDb::peek_meta`, redb's `ReadOnlyDatabase`)
  before opening it: opening a redb file for writing always rewrites its header, so a refused world
  would otherwise be modified. `version_lock_test.py` (SQLite edits on a running binary) became
  `crates/dwell-server/tests/host.rs`, with a smoke run of the binary.

### 24e — Client on wgpu
- [x] wgpu renderer, WebGPU only, WGSL shaders; an "unsupported" screen without WebGPU (checked in
  Chromium without an adapter; `ui/startupError.ts`).
- [x] Chunk meshing and LOD in Rust, in Web Workers with their own WASM instances (no
  `SharedArrayBuffer`); the near/far passes and the whole-world view (the e2e LOD and terrain
  specs).
- [x] The Phase 7 look ported: palette, face tints, sky, height fog, tone mapping, exposure; block
  textures; the camera and field of view; the block outline and debug lines (the owner's
  before/after comparison is the exit criterion below).
- [x] Input (keyboard, mouse, touch), prediction and interpolation, block editing, local mode (the
  sim in a worker), browser world storage (the e2e play, edit, touch and flight specs).
- [x] The DOM UI (menus, settings, server browser, HUD) kept in TS over a small `wasm-bindgen` API;
  Three.js and the TS sim, protocol codec and render code removed from the client (the generated
  `protocol/constants.gen.ts` and block tables stay: the DOM UI reads them).

Built so far (24e; nothing above is ticked until the client runs end to end on it):
- Client logic crates, ported with their TS tests: `dwell-mesh` (the chunk and LOD meshers, the
  texture atlas; buffers cross-checked identical to the TS output), `dwell-look` (palette, face
  tints, sky gradient with its generated WGSL, sun and moon, fog, exposure, the sky frame),
  `dwell-game` (input, touch, flight, the eye and face flip, remotes, block interaction, the chunk
  streamer, the HUD text) and `dwell-lod` (the LOD selection, grid, frustum and detail settings).
- Networking and local mode in `dwell-client`: the session state machine, framing, datagram
  batching limits, the WebRTC answer, simulated net conditions; WebTransport and WebRTC
  transports over `web-sys`; `LocalCore` (the `dwell-sim` server stepped in the client) with an
  OPFS storage backend (`LocalWorld`, a `FileSystemSyncAccessHandle` for `redb`).
- The wgpu renderer (`dwell-client::render`): terrain chunks, LOD sections with skirts and
  clipped stand-ins, players, the block outline, debug lines, the sky, height fog, ACES tone
  mapping and exposure; camera-relative drawing in two depth passes split at
  `LOD_NEAR_SPLIT_M`; the mipmapped atlas. A smoke scene renders in headless Chromium
  (`client/e2e-webgpu/renderer.spec.ts`: sky, lit terrain and a player read back from the GPU).
- The game loop (`dwell-client::game`, game.ts ported): prediction ticks, input datagrams,
  reconciliation, remote players, the eye and face flip, terrain streaming, the whole-world view,
  block targeting and edits, the HUD text and debug overlay, behind a `Scene` trait the renderer
  implements. Background jobs (`jobs`, `pool`): generation, LOD bounds and sections, the debug
  map and meshing, with a byte codec for workers, run inline natively. `tests/game_local.rs`
  joins a local world through the real session and handshake: the terrain streams and is meshed,
  the player walks, breaks a block and sees it gone, and LOD sections are generated and meshed.
- The browser app (`dwell-client::web::app`, a wasm-bindgen `Client`): the WebGPU canvas, the
  session over Rust's WebTransport and WebRTC (or a transport the shell hands over), the game loop,
  the input and touch state machines, settings, the HUD's text and the `window.__dwell` hooks;
  jobs in Web Workers running the same module (`web::jobs`, `engine/jobWorker.ts`); the local
  world's worker on `LocalWorld` (one redb file in OPFS); the lobby ping and hotbar swatches in
  Rust. The TS shell (menus, HUD, settings, master, rooms, hosting) drives it; Three.js, the
  TS sim, meshers, worldgen, LOD, game logic, protocol codec, session and transports are
  removed. A local world plays in Chromium (terrain, LOD to the horizon, editing, the HUD).
- The e2e suite runs against the Rust client and `dwell-server`: 32 of 32 locally (Chromium headed
  under Xvfb on SwiftShader's WebGPU). The CI client, e2e and release jobs build the Rust client
  (`npm run build:wasm`: cargo and wasm-bindgen) instead of the Emscripten core; Electron turns
  WebGPU on (Linux, and SwiftShader for its smoke test).
- Those CI jobs pass. Outstanding: the frame-time comparison.

Deviations (24e, so far):
- `?batch=1` (batched terrain draws) is dropped: the wgpu renderer draws each chunk and section
  (`e2e/batching.spec.ts` became `terrainDraw.spec.ts`). The version-lock e2e rewrites the world
  file's version through the client's own world-file code (`__dwell.setWorldFileVersion`): redb
  checksums its pages, so the SQLite-era byte patch cannot work.
- Found while documenting the cutover and fixed, each with a test seen red then green: players
  were drawn upright, unturned and without their visor (the Three.js renderer's pose, yaw and
  visor, now `render/geometry.rs` `player_mesh` and the draw's turn), and the capsule was wound
  inside out; local worlds autosaved every 30 s instead of the C++ build's 5 s
  (`LOCAL_AUTOSAVE_SECONDS`); without a WebGPU adapter the page showed wgpu's error rather than
  the "needs WebGPU" screen (`ui/startupError.ts`); and the main menu waited for the game client
  (and WebGPU) although it is DOM only (the launcher e2e caught it). World-file tools for tests
  moved to `window.__dwellTools`, available on the menu.
- Room peer transports (friend worlds' WebRTC through the master's signalling) and the local
  loopback stay in TS and are handed to the Rust session as a generic JS transport object; Rust
  owns WebTransport and the dedicated-server WebRTC path.
- `dwell-lod` keys column bounds by the column's first in-world row: the TS keyed them by row 0,
  which is below the world at levels 1–17, so every column there shared one bounds entry (hidden
  by uniform test worlds; `keeps_each_columns_bounds_apart`). The TS client got the same
  fix on `main` before the cutover deleted it.
- Headless Chromium's SwiftShader adapter loses the device ("A valid external Instance reference
  no longer exists") as soon as a WebGPU canvas presents, even from plain JS: the probe page
  renders its offscreen scene before the canvas probe, and the browser e2e suite runs Chromium
  headed under Xvfb.
- On a CPU (software) adapter the renderer draws without MSAA (`render::samples_for`): CI's
  SwiftShader at 4× starved the page's main thread, so terrain meshed too slowly and remote
  players' updates arrived late (two e2e cases failed on CI). A GPU keeps 4×.
- `std::time` panics on wasm32-unknown-unknown: the server core counts work items instead of
  measuring time there (24d), and world files take their save time from JS.

### 24f — Shells and cutover
- [x] Electron runs the Rust client; the launcher and release scripts build it. The release
  workflow builds it; Electron turns WebGPU on (Linux flags, SwiftShader for its smoke test, which
  passes in CI). Capacitor is not built yet: moved to Phase 23, which will wrap this build.
- [x] The e2e suite passes against the Rust client and server: 32 of 32 locally (Chromium headed
  under Xvfb) and in CI.
- [x] The C++ tree, CMake, Emscripten, Jolt and Three.js deleted; `shared/licenses` and
  `THIRD_PARTY_NOTICES` regenerated (`gen.py` walks `cargo metadata` for `dwell-server` and
  `dwell-client`; the generators no longer write C++). The worldgen goldens and the playground
  structure moved to `shared/`.
- [x] `ARCHITECTURE.md` and `PLAYER_CONTROLLER.md` rewritten for the Rust build; `CLAUDE.md`
  updated (project description; the SQLite build note replaced by running WebGPU in a sandbox).
- [x] The owner raises `package.json` to the next MINOR (a new compatibility line): 0.10.0.

Exit criteria
- [x] Every native suite, WASM suite and e2e test of the Phase 15 baseline has a Rust counterpart
  that passes in CI (WebGPU covered as 24a decides).
- [x] Worldgen goldens agree natively and in WASM; the divergence check passes (the worldgen
  goldens and the controller's golden trace under `wasm32-wasip1`, 24b and 24c).
- [x] 64 players < 1 ms/tick (native Release, `enhanced-determinism` on): 0.48 ms locally
  (`scenario` test `sixty_four_players…`, also a step of the `rust` CI job).
- [ ] Frame time at least matches the Three.js build on a desktop and a phone (measured).
- [x] CI wall time under 6 minutes on a warm cache; a one-line change in the sim rebuilds and
  tests locally in under a minute (measured): CI ~4 min on a warm cache (~17.5 min cold, the
  `rust` job's first build); `cargo test -p dwell-sim` after a one-line change 37 s.
- [x] No CMake or Emscripten in the repository (one third-party crate, `aws-lc-sys` under the
  server's TLS stack, runs CMake in its own build script).
- [ ] Manual: the owner plays the baseline in a browser, in Electron and on a phone, and compares
  before/after screenshots of the look.

---

## Phase 16 — World Shape and Build Volume

**Goal:** The baseline's geometry, with every bound read from one data-driven predicate.

**Status:** Not started.

Deliverables
- [ ] **Constants** (`shared/protocol/constants.json`): `worldRadius` 50,000; `midplaneY` 0;
  `seaLevel` and the core's thickness move to the terrain definition in Phase 17 (interim values here:
  sea level `h` = 32, core 10); `worldBottomY`/`worldMaxY` replaced by `bounds`
  `{ "shape": "sphere", "center": [0, 0, 0], "radius": 50000 }`; `lod.maxLevel` 12;
  `flightCeiling` removed (D3: no movement limit); fly-speed levels re-checked for a 50 km world.
  Protocol version bumped.
- [ ] **`InsideBuildVolume`** in `core/voxel.h` and `client/src/world/`: shapes a closed set in code
  (sphere; slab kept for tests), parameters from data. Used by: edits (server rejection, client
  pre-check), generation and storage (chunks wholly outside are air, never stored; `IsAirChunk`,
  `LodBoundsAt`), streaming and LOD requests. Not movement: players may fly outside it (D3).
- [ ] **Bifacial re-tune**: the mirror constant, `kMinChunkY`/`kMaxChunkY` from the bounds, the
  flip band and `CORE_ANCHOR_LAYERS` (8) checked against the 10-layer core.
- [ ] **Thin-crust LOD**: sections straddling the midplane at every level draw the viewer's face
  only, without holes or the other face showing through.
- [ ] Far-origin test runs moved to ~49 km; F4 zoom range narrowed.
- [ ] Docs: `ARCHITECTURE.md` world bounds, bifacial, §6.6, §7.4; `BIFACIAL_WORLD.md` geometry.

Exit criteria
- [ ] Edits outside the sphere are rejected by the server and not sent by the client (unit tests;
  one e2e edit at the boundary).
- [ ] No chunk or LOD section wholly outside the sphere is generated, stored or requested (tests).
- [ ] Flight is not bounded by the sphere (D3): a controller scenario flies out through it, native
  and WASM, and the client requests no chunk or LOD section outside it while there (test).
- [ ] LOD meshing of straddling sections at levels 1–8 shows no holes and none of the other face
  (mesher tests over a 20-layer crust).
- [ ] Walking over the rim still carries a player across the midplane to face B (controller
  scenario).
- [ ] Manual: a look in a browser from the ground, from altitude and from face B.

---

## Phase 17 — Terrain Definitions

**Goal:** Terrain as data. The interim generator from Phase 15 is replaced by an evaluator of terrain
definitions; the baseline world is `dwell:baseline`.

**Status:** Not started.

Deliverables
- [ ] **Format** (`shared/worldgen/*.json`, schema documented in `ARCHITECTURE.md` §6.3):
  - `layout` — `voronoiIsland`: site `cell`, `warp`, `land.radius` and `radiusNoise`,
    `rimClearance`, `coastDetail`; an extensible `kind`.
  - `height` — land and sea lists of nodes: `fbm` (wavelength, octaves, gain, amplitude), the coast
    distance profile, constants; `mask` by name (`inland`, `coast`, …); sum, scale, clamp.
  - `core`, `surface` (conditioned layer lists: `underwater`, coast distance, height above sea
    level, slope), `fill`, `seaLevel`, `liquid` — all blocks by name.
  - `spawn` — a rule (e.g. nearest dry, level land to the origin).
- [ ] **Generator** `shared/worldgen/gen.mjs`: validates (block names against the registry, node
  kinds, parameter ranges, integer wavelengths), emits Rust tables for `dwell-worldgen` (and TS
  only where a TS tool reads them); CI fails on stale output. A **definition hash** (FNV-1a 64, like the registry hash) identifies the generator:
  worlds record it beside the generator version; `Welcome` carries it.
- [ ] **Evaluator** in `dwell-worldgen` (replacing `terrain.rs`'s `TerrainGenerator`): layout → height
  field (4-column lattice, bilinear) → fill by layers; `generate_lod` and point queries
  (`column_at`, `ground_y`, `spawn_point`) from the same fields. Reuses the Voronoi coast distance
  from `island.rs` (`IslandLayout::coast_at`), parameterised.
- [ ] **Generators are definitions**: the version branches of `generator_for`, `air_test_for`,
  `spawn_point_for`, `lod_generator_for` and `lod_bounds_for` (`dwell-worldgen` `lib.rs`) replaced
  by a definition lookup; the flat world (and the sandbox's floor) is a definition with no noise.
- [ ] **Per-face definitions** (D5): each face names a definition and a seed stream.
- [ ] **`dwell:baseline`**: one island (D8), the 10-layer core, stone under 3–4 layers of dirt and
  grass, sand at the coast and under water, sea level `h` = 32. Tuned with the inspect tool.
- [ ] **Tools**: a worldgen inspect tool in Rust (the C++ `dwell_worldgen_inspect` went with the C++
  tree and has no Rust counterpart yet; a `dwell-tools` binary, ARCHITECTURE.md §6.3) that takes a
  definition file (`--def path`) for height maps, disc images, first-person views and `bench`.
  Stretch: a definition editor page with a live preview in `docs/prototypes/`.
- [ ] Goldens for `dwell:baseline` (surface, coast, sea, core rows on both faces, the rim, near the
  sphere's edge; `shared/worldgen/golden`, `dwell-worldgen` `tests/golden.rs`); the old generator
  code deleted.
- [ ] Docs: `ARCHITECTURE.md` §6.3 rewritten (pipeline, determinism, the format);
  `WORLD_GENERATION.md` becomes the definition guide (or is folded into §6.3).

Exit criteria
- [ ] Adding a noise layer or a surface layer is a data change only: a test loads a modified
  definition and sees the terrain change, with a different hash.
- [ ] Invalid definitions (unknown block, unknown node kind, non-integer wavelength) fail the
  generator with a message (tests).
- [ ] Land never lies within `rimClearance` of the rim (sampled over seeds); the origin is land.
- [ ] Every column has its core layers on both faces and no water voxel has air below it.
- [ ] Goldens agree natively and in WASM (`wasm32-wasip1`; the client's workers run the same crate).
- [ ] Generation time per chunk and per LOD section reported against Phase 15's.
- [ ] Manual: the owner reviews whole-disc and first-person images of both faces.

---

## Phase 18 — Adaptive Natural Blocks

**Goal:** The Shape Playground's slopes on all natural terrain — Phase 9e
([`ADAPTIVE_BLOCKS.md`](./ADAPTIVE_BLOCKS.md)), simplified by the baseline (every solid block is
natural) and with the rule table as data.

**Status:** Not started. Replaces Phase 9e (its original plan stays under Phase 9 for the record).

Note (Phase 15): the shaped-block tests (controller slope scenarios, shape-table rows, mesher and
LOD slope cases) were removed with the placed families; this phase restores shaped coverage for the
adaptive blocks, and LOD sloped tops (the LOD mesher's slope pieces were cut too).

Deliverables
- [ ] **Shapes**: the 12 corner and 6 ridge shapes added to `shapes.mjs` as corner-height data;
  geometry tests (closure, winding, volume, coverage).
- [ ] **The table as data** (`shared/blocks/adaptive.json`): generated from the five rules, plus
  per-neighbourhood **overrides**; the generator checks shape names and runs the exhaustive no-holes
  adjacency test; it emits the table into `dwell-blocks` (`blocks.gen.rs`), checked against shared
  vectors (`shared/blocks/vectors.txt`, `dwell-blocks` `tests/vectors.rs`).
- [ ] **The Shape Playground** exports overrides in that format (republished to the same link,
  `docs/prototypes/README.md`).
- [ ] `"adaptive": true` on grass, dirt, stone and sand (block data).
- [ ] **Meshing, collision, targeting**: derived shapes in the chunk mesher (`dwell-mesh`
  `mesher.rs`), terrain collision (`dwell-physics` `terrain.rs`, today a collider of whole voxels),
  `ray_enter_shape` and `surface_height_at` (`dwell-blocks` `shape.rs`); edits re-shape and re-mesh
  neighbours (`dwell-world` `edit.rs`, `dwell-game` `chunk_stream.rs`).
- [ ] **Terrain and LOD**: point queries, spawn and water read the shaped surface; the rule applied
  on LOD cells (`dwell-mesh` `lod_mesher.rs`); flooded shaped blocks at shores.
- [ ] Docs: `ARCHITECTURE.md` §6.1, §6.5, §6.6; `ADAPTIVE_BLOCKS.md` reduced to rationale and tables;
  ADR 0024 as built.

Exit criteria
- [ ] The table equals the five rules plus overrides for all 6,561 neighbourhoods, in the generator
  and in `dwell-blocks` (native and WASM), and reproduces every block of the owner's playground
  export.
- [ ] No holes between any two neighbouring derived shapes at equal height (exhaustive test).
- [ ] Controller scenarios on every new shape (ridges, saddles, half-and-half corners, 45° ramps; no
  launches, hops or sliding at rest; `dwell-physics` tests), native and WASM; the divergence check
  (the golden trace under `wasm32-wasip1`) passes.
- [ ] e2e: placing and breaking natural blocks re-shapes neighbours identically on two clients.
- [ ] LOD seams against chunks checked; generation and meshing times against Phase 17's.
- [ ] Manual: the owner walks the terrain and builds with natural blocks, comparing with the
  Adaptive Block Lab.

---

## Phase 19 — WebGPU, Looks and Textures

**Goal:** Render with WebGPU, and make how blocks look a system: looks registered by name and
textures described by data.

**Status:** Not started (the look registry and textures as data). Its WebGPU half is done by
Phase 24. Needs Phase 24.

**Re-scoped by ADR 0028 (2026-10-13), a deviation:** the plan was Three's `WebGPURenderer` with TSL
materials, an automatic WebGL2 fallback and the active backend shown in the settings menu (D4, ADR
0027). Phase 24 replaced that renderer with the wgpu client (`dwell-client` `render/`, WebGPU only,
WGSL shaders), so those items are done by Phase 24 or removed, as marked below. What remains is the
look registry and textures as data, built on the wgpu renderer and WGSL. It overlaps with the
transparent-blocks work (render modes as block data: opaque, cutout, translucent; the `cross` look),
built on its own branch (`claude/transparent-blocks`; its phase and ADR numbers are given when it
merges — Phase 31 and ADR 0034 went to sky clouds): that work's modes and looks are the registry's
first entries, and whichever lands second builds on the other.

Deliverables
- [x] **CI first**: Playwright's Chromium runs WebGPU (SwiftShader's Vulkan, headed under Xvfb) for
  e2e and `client/scripts/shots.ts`; a smoke test proves a WebGPU frame renders in CI. *Done by
  Phase 24* (24a's probe, `client/e2e-webgpu/probe.spec.ts` and `renderer.spec.ts`; the e2e suite
  on CI).
- ~~**`WebGPURenderer`** behind `client/render` (async init); the WebGL2 backend as automatic
  fallback (D4); the active backend shown in the settings menu (owner, D4) and in F3.~~ *Removed by
  ADR 0028*: the wgpu renderer is WebGPU only, with an "unsupported" screen instead of a fallback
  (`ui/startupError.ts`) and no backend readout.
- [x] ~~**TSL ports**~~ **WGSL**: height fog and per-face light, the sky, chunk and LOD materials,
  tone mapping and exposure, with no GLSL. *Done by Phase 24* (`dwell-client` `render/shaders.rs`,
  the sky's generated WGSL in `dwell-look`).
- [ ] **Look registry**: each look (`cube`, `shaped`, `water`, `ladder`, …) is registered by name
  with its WGSL pipeline and its meshing style; the `match` on look names in `dwell-mesh`
  `style.rs` and the meshers' branches on `Look` replaced by the registry; `shared/blocks/gen.mjs`
  validates block looks against it (today against a fixed `LOOKS` list).
- [ ] **Texture data**: tile recipes (parametric noise: palette, scales, features) or image files in
  `shared/blocks/` (or a sibling), named by block data; the atlas built from them and sized to fit;
  the hardcoded tile names (`tile_rect`) and painter functions (`painter`) in `dwell-mesh`
  `textures.rs` removed. Stretch: a recipe editor page in `docs/prototypes/`.
- [x] Re-verify the near/far depth passes and the LOD's draw path on WebGPU. *Done by Phase 24*
  (the wgpu renderer draws them, `e2e/lod.spec.ts`, `e2e/terrainDraw.spec.ts`); batched terrain
  (`?batch=1`) went with the Three.js build.
- [ ] Docs: `ARCHITECTURE.md` §5 (`render/`, `dwell-mesh`), §6.1; `BLOCK_REGISTRY.md`. ADR 0027 is
  superseded by ADR 0028, so the registry is recorded in `ARCHITECTURE.md` (and in an ADR if it
  settles an open decision).

Exit criteria
- [x] e2e suites pass on WebGPU in CI. *Done by Phase 24.* ~~And on the WebGL2 fallback; the
  settings menu names the backend in use on each (e2e).~~ *Removed by ADR 0028.*
- [ ] A new texture or look is added by data plus (for a look) one module — a test adds a fixture
  look and texture without touching the mesher (`dwell-mesh` tests).
- [ ] Screenshots match the look before the registry (`client/scripts/shots.ts` on the named
  views; owner review of before/after).
- [ ] Frame time on a desktop and a phone, against the build before the registry (the bench,
  `npm run bench -- --compare`).
- [ ] Manual: Safari (macOS, iOS) and an Android device render with WebGPU.

---

## Phase 20 — Voxel Awakening (Integrity + Flood-Fill → CompoundShapes)

**Goal:** Spec Phase 4. Detached structures become single rigid bodies (Rapier, behind `dwell-physics`;
§7.1).

**Status:** Not started. Runs after Phases 6–13 (owner's decision, 2026-10-05); previously on hold
until Phase 5 (multiplayer ready) was complete (2026-09-30). **Waits for Phase 25's renderer decision
(A7; ADR 0029, owner 2026-10-14)**, so cluster rendering is built once, for the chosen renderer: greedy
meshes with a rotation per draw if the mesher stays, brick volumes per body if marching is chosen
(`RAY_MARCHED_RENDERER.md` §6).

Deliverables
- [ ] Bifacial world (Phase 13): the core anchor zone around the midplane (positional, diggable;
  there is no bedrock) anchors both faces; bodies fall toward the midplane by their side and are accelerated through the band
  in the flip band.
- [ ] Slopes (Phase 9): any two solid voxels sharing a face are connected; cluster bodies use one
  convex shape per voxel from the state table (inner corners as two wedges), mass from volume
  (`SLOPE_BLOCKS.md` §7).
- [ ] Anchor definition (the core anchor zone of Phase 13 + `grounded` flag) and budgeted 6-connected flood-fill
  structural-integrity pass triggered by voxel removal (`INTEGRITY_BUDGET_VOXELS`). The anchor
  rule must also cover generated sky islands (Phase 12; `WORLD_GENERATION.md` §4.5 item 7),
  whichever phase lands first.
- [ ] Clustering of detached components; removal from grid in the same `VoxelModification`
  (reason `Collapse`).
- [ ] Cluster → a Rapier compound collider of cuboids (behind `dwell-physics`'s `Physics`, so game
  code sees no Rapier types); mass/COM/inertia from material density.
- [ ] `NetworkEntityID` allocation; reliable `EntitySpawn` (voxel layout) / `EntityDespawn`.
- [ ] Tier 1 snapshot replication for all clusters (tiers are introduced in Phase 21); client
  interpolation and rendering of cluster meshes; kinematic proxies in client physics worlds.
- [ ] **Player ↔ Tier 1 interaction** (§9.2, §9.4):
  - [ ] Players push light clusters (contact mass scaling: `maxPushForce`, `pushableMassLimit`);
    moving clusters push players (absorbed as external velocity).
  - [ ] Standing on / riding moving clusters: `groundEntityId` + body-local player state in
    snapshots; client predicts in the body's frame.
  - [ ] Present-time proxies for Tier 1 bodies within `PREDICT_PROXY_RADIUS`; the ground body is
    rendered at present time.
  - [ ] Crush damage / crush death (contact listener, `PLAYER_CONTROLLER.md` §6.6).
  - [ ] Port of the PPC platform suite onto Tier 1 bodies (translating, rotating, falling).
- [ ] Admin/debug command: cut a pillar / delete a region to trigger collapses on demand.

Exit criteria
- [ ] Removing the supports of a tower causes it to fall as one body, on all clients, in sync.
- [ ] A 10 000-voxel detached structure is processed without the server tick exceeding its budget
  (work is spread across ticks).
- [ ] Unit tests for integrity/clustering on crafted voxel layouts (bridges, overhangs, rings).
- [ ] A player can ride a falling slab to the ground at 150 ms RTT without sliding off or
  jittering; a player under a falling tower is crushed on every client consistently.
- [ ] Collapsing generated terrain (a cave ceiling, an overhang) works like built structures.

---

## Phase 21 — Tiered Physics (Authoritative Tier 1, Cosmetic Tier 2)

**Goal:** Spec Phase 5. Keep CPU and bandwidth bounded during large explosions (§7.2).

**Status:** Not started. Runs after Phases 6–13 (owner's decision, 2026-10-05); previously on hold
until Phase 5 (multiplayer ready) was complete (2026-09-30).

Deliverables
- [ ] Explosion system on the server: radius/force, material strength attenuation, impulse to
  existing Tier 1 bodies, newly awakened clusters, and **players** (knockback + damage via
  `PlayerEvent`, replayed into prediction).
- [ ] Tier 2 debris collides one-way with the local player (debris bounces off; player movement
  unaffected).
- [ ] Tier classification: `TIER1_MIN_BLOCKS`, `alwaysAuthoritative` materials.
- [ ] Tier 2 path: voxels removed in `VoxelModification` (reason `Explosion`) + `PhysicsEvent`
  `Explosion(origin, force, radius)` in the same reliable batch; client derives debris from the
  removed voxels and simulates in its local debris world.
- [ ] Snapshot packing: quantization (§8.3), priority accumulator, multiple datagrams per tick,
  per-client bandwidth budget.
- [ ] Client debris lifecycle: lifetime, at-rest removal, `DEBRIS_MAX_BODIES` cap (platform-based).
- [ ] **Threading checkpoint (ADR 0007):** profile a browser-hosted friend world at its host
  profile's caps; if simulation-bound, plan a hosting-only threaded web build behind
  `coi-serviceworker`.
- [ ] Cosmetic death ragdoll (Rapier bodies and joints behind `dwell-physics`, Tier 2 rules) in the
  client debris world; player
  models animated from `State` (both moved from Phase 2).
- [ ] Performance instrumentation: server tick time, active body count, bytes/sec per client;
  client frame time and debris count, shown in a debug overlay.

Exit criteria
- [ ] A large explosion (hundreds of fragments) runs at stable server tick rate; only Tier 1 bodies
  appear in snapshots; per-client bandwidth stays within budget.
- [ ] Debris looks plausible on each client and never affects gameplay state or player movement.
- [ ] Players caught in a blast are knocked back smoothly and consistently across clients.
- [ ] Late-joining clients see correct terrain (debris is not replayed, by design).

---

## Phase 22 — Sleep / Re-bake Cycle

**Goal:** Spec Phase 6. Long-running servers keep a bounded number of dynamic bodies (§7.3).

**Status:** Not started. Runs after Phases 6–13 (owner's decision, 2026-10-05); previously on hold
until Phase 5 (multiplayer ready) was complete (2026-09-30).

Deliverables
- [ ] Sleep monitor with `SLEEP_LINEAR_THRESHOLD`, `SLEEP_ANGULAR_THRESHOLD`, `SLEEP_SECONDS`.
- [ ] Grid snapping to 24 axis-aligned orientations; occupied-cell conflict resolution with
  fallback to Tier 2 debris.
- [ ] Player-safe re-bake: cells overlapping players are treated as occupied; riders transition
  from body-relative to static ground without a visible pop.
- [ ] Re-bake: destroy body → write grid → rebuild chunk collision → integrity check on placed
  voxels → reliable `VoxelModification(Rebake)` + `EntityDespawn` in one batch.
- [ ] `MAX_TIER1_BODIES` enforcement (force re-bake of oldest/smallest).
- [ ] Persist in-flight Tier 1 bodies in a `bodies` table of the world's redb file (ADR 0006, ADR
  0028 §8) so a world saved mid-collapse
  resumes it on load.
- [ ] Soak test harness: headless bots + scripted explosions for hours; tracks body count, memory,
  tick time.

Exit criteria
- [ ] After a collapse settles, every cluster is back in the static grid within
  `SLEEP_SECONDS` + a small margin, and clients show identical static terrain.
- [ ] A multi-hour soak test shows flat memory and body count and no tick overruns.

---

## Phase 23 — Dedicated Server Distribution & Platform Packaging

**Goal:** Complete player-hosted multiplayer with no official game servers (ADR 0003):
distributable dedicated servers and packaged desktop/mobile apps (ARCHITECTURE §10), on top of the
web hosting, master server and lobby list built in Phase 5.

Deliverables
- [ ] **Dedicated server distribution:** CI builds native binaries (Windows/macOS/Linux) and a
  Docker image per release; settings and permissions in the world database edited via admin
  commands and a server CLI (ADR 0006); ops, kick, ban by key; allow-list/password;
  UPnP/NAT-PMP with port-forward guidance; rotating backups of the world's redb file
  (`dwell-storage`); host-configurable
  physics and view caps. Certificate rotation, publishing each new hash to the master (5d).
- [ ] **World export/import** (`.dwellworld`) across dedicated servers, browsers, and apps; the
  browser's world storage (redb over an OPFS sync access handle) verified per Capacitor platform.
- [ ] Versioned builds for the Electron and Capacitor shells: whether they load versions from the
  site (store policies on downloaded code, iOS in particular) or bundle one version (the web
  launcher and versioned builds themselves are Phase 6).
- [ ] **Electron:** packaging for Windows/macOS/Linux (electron-builder), custom protocol with
  COOP/COEP and a multithreaded WASM build of the Rust client and sim (wasm32 atomics, ADR 0007),
  "Host world" launching the native server; local-network discovery.
- [ ] **Capacitor:** Android and iOS projects; check `SharedArrayBuffer` availability on the app
  scheme (threaded build if available, ADR 0007); verify WebTransport per WebView (WebRTC
  where unavailable); touch
  controls (auto-jump preset); mobile caps; friend-world hosting (5c) with the apps' backgrounding.
- [ ] Dedicated-server WebRTC joins through master signaling and TURN (servers behind strict NAT).

Exit criteria
- [ ] A player hosts a dedicated server at home from the downloadable binary; players on the GitHub
  Pages site, Electron, and a phone find it in the server browser and see the same collapse.
- [ ] The Capacitor app hosts a friend world; a browser player and an Electron player join by code,
  including one on mobile data through the TURN relay.
- [ ] Certificate rotation on a dedicated server is invisible to players joining through the master.
- [ ] An iOS Safari player joins a self-signed dedicated server over WebRTC, including one behind
  strict NAT via TURN.
- [ ] An outdated app (Electron, Capacitor) is rejected with a clear message and offered the matching
  versioned build.

---

## Phase 25 — Ray-Marched Renderer Comparison

**Goal:** Build a second, complete terrain renderer — GPU ray marching over a brickmap, full-detail
chunks and every LOD level — beside the mesher, compare the two side by side in local builds, and let
the owner decide which draws Dwell's terrain ([ADR 0029](./adr/0029-ray-marched-renderer.md),
[`RAY_MARCHED_RENDERER.md`](./RAY_MARCHED_RENDERER.md)).

**Status:** In progress — A0 (the ADR) and A3a (the CPU core in `crates/dwell-march`: layout,
builders, pool, reference marcher) done; A2 (the renderer seam, compare mode, bench `--compare`)
built, its exit check done (compare mode with raster on both sides reports zero difference); A1
(the shared frame and GPU timing) built, its exit check done: the bench before/after (design doc §7,
"A1 as built") and the picture differs slightly — antialiased edges, the sea's colour (water now
blends in linear light), the outline no longer multisampled — which the owner approved (2026-10-14);
the e2e suite passes on CI. A3b (the GPU march, uploads, brick jobs, telemetry) built, its CI run of
the per-pixel test outstanding; next: A3c (overlays, LOD column heights, raster ids). Needs Phase 24 only. The marcher and compare mode are in local builds only (dev server, local preview, bench)
until A7; deployed builds keep the mesher throughout and stay shippable after every step. Phase 20
waits for A7; Phase 26 shares A1 (its W2). No public API change, so no new compatibility line.

Deliverables
- [x] **A0** — ADR 0029 (a second renderer behind a runtime switch, amending ADR 0028's renderer);
  the owner's answers recorded (`RAY_MARCHED_RENDERER.md` §9).
- [x] **A1** — The shared frame and GPU timing (also Phase 26's W2): an offscreen `rgba16float`
  target, stored sampleable depth (handling the far/near split and MSAA), fog and tone mapping moved
  to a composite pass; timestamp queries in the telemetry (`gpu.<pass>`); `bench.ts` passes
  `--enable-webgpu-developer-features`. 4× MSAA kept (owner).
- [x] **A2 bench --compare** — `npm run bench -- --compare <scenario…>` (`scripts/compare.ts`,
  `configs.ts`, `compareReport.ts`): each scenario once per configuration of
  `client/bench/configs.json` (renderer, scale, upscaler; checked, a test adds one by data) in fresh
  pages; `OUT/compare.json`, a printed table, per shot a side-by-side image and a ΔE heatmap
  (% over the threshold, mean and max), and `OUT/compare.html` (metrics tables, the screenshots side
  by side, a blink toggle per pair, the heatmaps); configurations the build lacks (from `__dwell.renderers()` /
  `upscalers()`; today every `march` one) skipped with a note; metrics: frame interval percentiles,
  dropped frames at 60 Hz, CPU phases, `gpu.*` when present, memory, uploads, streaming, a near/far
  split. Scenarios' `renderer` (`?renderer=`) and `split` fields, a `repeat` step, and the scenarios
  `look-horizon`, `look-down`, `edit-burst`, `fly-high`, `thermal-10min` (each run once on a Mac).
- [x] **A2** — The seam and compare mode: payload-generic terrain calls in `Scene` (`render/seam.rs`:
  a `TerrainRenderer` per renderer's payloads, routed by the selection; the meshing jobs make the
  payloads it needs; a runtime switch rebuilds every chunk and LOD section from the client's data),
  the `?renderer=raster|march|compare` switch (and `?compare=raster`: raster on both sides) behind
  the build-time `__RENDERER_SWITCH__` (off in the release workflow's builds) and the active
  renderer in the settings menu (with a picker) and F3; `__dwell.renderers()` / `upscalers()` for
  the bench; compare mode's split (scissored, a draggable divider), blink (F8) and difference (F7
  cycles; coverage, view depth and colour ΔE, counted on the GPU) views and the per-side overlay
  (CPU, GPU, memory, settings), with a placeholder "march" side (`render/march.rs`, magenta).
- [ ] **A2 follow-ups, moved to A3** (they need the marcher's output): edit-to-visible latency (an
  edit's sequence carried through the remesh to the frame that draws it; `DEV_TOOLS.md` TODO) and
  the bench's image agreement by cell id, face and depth (the renderers' id buffers; the bench
  compares colour only, compare mode's difference view compares coverage and depth in game).
- [x] **A3a** — The CPU core (`crates/dwell-march`, pure Rust, native and `wasm32`): the GPU data
  layout as `u32` words (clipmap levels and slots, 64-tree nodes, raw and 1/2/4-bit palette bricks,
  column min-max grids, the state table from the registry's data); the builders (chunk → raw
  bricks, LOD section → palette bricks, both with column grids and per-empty-brick distances); the
  pool's size-class allocator and the clipmap levels (CPU side); the CPU reference marcher (integer
  + fraction ray state, cone LOD, masks, column and brick-distance skips, step cap) with a
  brute-force oracle; steps-per-ray statistics (`examples/march_steps.rs`).
- [x] **A3b** — Marched terrain at every distance on the GPU (`crates/dwell-client/src/render/march/`):
  the state table, pool and clipmap uploaded incrementally (`Scene::take_dirty`, one `writeBuffer`
  per changed range); bricks made in the workers for chunks and LOD sections (the masked meshing
  jobs' `BRICKS` payload); the primary march at full resolution, a WGSL port of the reference
  marcher with its constants generated from the Rust layout, with the hierarchy's masks, column
  grids, brick distances and cone LOD; behind the seam (`MarchRenderer`, `BUILT`), copied into the
  shared frame; steps-per-ray telemetry (`march.steps.*`, an atomic histogram) and `gpu.march`; the
  GPU-against-reference per-pixel test (native and the WebGPU probe); `?marchtest=1`. `march` mode
  draws all terrain with no meshes. Deviation: a buffer that fills is re-created at double size and
  re-filled from the CPU copy (which holds every word), not grown by `copy_buffer_to_buffer`.
- [ ] **A3c** — The rest of A3: overlays over marched terrain through the `frag_depth` pass
  (players, the outline, water); LOD column heights and side tints in the bricks; a raster id target
  so compare mode and the bench compare cell ids and faces; edit-to-visible latency (from A2).
- [x] **A4a** — Empty-space skipping and the per-step cost on the GPU (design doc "A4 as built"):
  column min-max heights, per-brick empty distances, the fog distance and the step cap (on the GPU
  since A3b); each level's occupied rows (a ray above or below everything its level holds leaves
  the level's box in one skip, clipped to its cone-LOD band); the march's inner loop vectorised and
  branch-light, the column search walking from a guess, shading in a pass of its own; the cone-LOD
  switch waits while the coarser cell is occupied (the owner's concentric rings, a test that failed
  first); `perf.rs`, a native profiling harness. Reference and WGSL together, the per-pixel check
  exact.
- [ ] **A4b** — The rest of A4's list: the beam prepass (a conservative start distance per 8×8
  tile, with a test that it never passes the true hit) and the temporal start distance (invalidated
  on edits and disocclusion). Moved out of A4a: after the level-rows skip the sky takes 15–20
  steps and the ground 5–10, and the profile puts the cost in the per-step latency, not the count
  (design doc "A4 as built").
- [x] **A5a** — Render scales 0.5 and 0.75 for the marcher with FSR 1 (EASU + RCAS in WGSL) or a
  depth- and normal-aware bilateral upscale (`render/march/upscale.rs`), `?scale=` with
  `?upscaler=fsr1|bilateral`, `__dwell.upscalers()`, the bench's `march-half-fsr`, `march-075-fsr`
  and `march-half-bilateral` configurations.
- [ ] **A5** — The rest of A5: TAA-U (reprojection), and shaped cells as analytic planes.
- [ ] **A6** — Lighting under both: half-res sun-shadow and AO rays from the G-buffer (march) or
  raster's depth (the hybrid), bilateral upsample, temporal accumulation; a quality setting.
- [ ] **A7** — The decision: the owner decides from the full comparison ("measure all"; design doc
  §5); ADR 0029's status, `ARCHITECTURE.md` §5 and this plan updated for the chosen path (and
  Phase 20 unblocked).

Exit criteria
- [x] A1: bench before/after; raster screenshots match today's, or the owner approves the
  difference. (The owner approved the differences — edges, the sea's colour, the outline — 2026-10-14.)
- [x] A2: compare mode runs with raster on both sides and reports zero difference (2026-10-15, Mac,
  Chrome on the GPU: `?local=1&seed=7&renderer=compare&compare=raster`, difference view at the
  settled spawn: 0 of 921 600 px in every class over five reads; raster against the placeholder
  marcher differs as expected; `bench --compare idle-spawn` ran, march configurations skipped).
- [x] A3a: the reference marcher's unit tests (first hit cell, face, distance, rebasing, both faces)
  pass — `crates/dwell-march/tests/march.rs`: against a brute-force DDA on random scenes (with and
  without skipping, exact ties, chunk and LOD levels), chunk and brick borders, 4,000 km from the
  origin (bit-identical), face B as face A mirrored, a state added by data alone.
- [ ] A3b: the GPU-against-reference per-pixel test passes in CI (SwiftShader). Passes locally on
  SwiftShader (the same probe CI runs: every record bit-identical) and on an M1's Metal (ids exact,
  `t` within 2 ULP, depth 2.6e-7); CI's run outstanding.
- [x] A3b: a first comparison report on a desktop (2026-10-15, M1 Mac, `bench --compare idle-spawn
  look-horizon look-down fly-high`: design doc "A3b as built").
- [ ] A3c: the named views, near and distant, agree with the mesher (over 99.5 % cell-id match where
  both hit) — needs raster ids and LOD column heights in the bricks.
- [ ] A4: steps-per-ray p99 down on `look-horizon` and `fly-high`; a comparison report on a desktop
  and a phone.
- [ ] A5: the full comparison report on every device in the design doc (Mac, Windows integrated and
  discrete, an iPhone from the last three years, a recent Android flagship; SwiftShader for
  correctness only).
- [ ] A6: a comparison with lighting on and off; the owner's look.
- [ ] A7: the owner's decision recorded in ADR 0029.


Deviations
- A3 is split into A3a (the CPU core) and A3b (the GPU side), so the finished part can be ticked.
- Column min-max grids are built for chunks too, not only LOD sections (design doc §4.3 item 3):
  on generated terrain they cut horizon rays' p99 from 455 to 83 steps for 2.7 KiB a node (the
  measurements are in the design doc). The CPU reference also has A4's column and brick-distance
  skips; A4 still needs them, and the rest of its list, on the GPU.
- Water is its own `Transparent` march class: never occupied for primary rays (it stays a raster
  pass, W6 later), its ids kept in mixed bricks. Level-0 bricks with shaped cells carry the optional
  shape plane as a second fixed size class; the shape hook hits the cell's cube until A5.
---

## Phase 26 — Water and Fluid Looks

**Goal:** Fluid looks as registry data, and a water shader with refraction, absorption, foam,
underwater rendering and reflections in quality tiers
([ADR 0030](./adr/0030-fluid-looks-and-water-rendering.md),
[`WATER_RENDERING.md`](./WATER_RENDERING.md)).

**Status:** In progress — W1a done (fluid looks in the registry, the surface drop and opacity read
from them, the eye-under-fluid test in the frame uniform); W2 built (Phase 25's A1); W1b built (the
water shader, `water_fs`, with the fluid id per vertex, the fluid buffer, the underwater fog stub and
the water quality setting; WATER_RENDERING.md §8), outstanding: the owner's look. Next: W3; W3–W5
build on the shared frame (A1); W6 needs A3 and the decision at A7 to keep marching. Fluids are per build, looks only, edited in the
data file; the water surface stays still. No public API change.

Deliverables
- [x] **W1a** — Fluid data: a `fluids` table in `shared/blocks/` (absorption, scatter, roughness,
  IOR, normal and wave scales, flow speed, foam, caustics, surface drop), blocks referencing a fluid
  by name (`"look": "fluid", "fluid": "dwell:water"`), validated by `gen.mjs` and emitted as a fluid
  table; the mesher taking the surface drop from the data
  (`WATER_SURFACE`, `CHUNK_WATER_DROP_M` gone); `WATER_OPACITY` from the data; the eye-submerged test
  with the 0.875 rule (the cell at `floor(eye)` liquid and, under a non-fluid cell, `frac(eye.y)`
  below the surface; the face's up) into a frame uniform, with its regression test.
- [x] **W1b** — The water shader (after A1): the mesher writing the fluid id per vertex (moved from
  W1a), a dedicated `water_fs` with scrolling normal maps, Schlick Fresnel, a GGX glint by the
  fluid's roughness and the sky reflection.
- [x] **W2** — The frame restructure: the same work as Phase 25's **A1**, built once there; ticked
  with it.
- [ ] **W3** — Refraction with the depth check, Beer–Lambert absorption (replacing the fixed alpha),
  shoreline foam.
- [ ] **W4** — Underwater: absorption fog, the water line, Snell's window and total internal
  reflection, caustics, distortion (god rays with W5's High tier).
- [ ] **W5** — Screen-space reflections and the quality tiers (Low, Medium, High, Ultra) in the
  settings menu; Low the phone default, tentatively (owner).
- [ ] **W6** — Under ray marching (after A3, and only if marching is kept at A7): reflection and
  refraction rays replace SSR and the copy on the high tiers.

Exit criteria
- [x] W1a: a fixture test adds a second fluid (`test:oil`) by data alone and sees it meshed with its
  own parameters; a bad fluid entry fails `gen.mjs` naming it; the regression test for the
  cell-against-surface mismatch (the eye 0.9 into a water cell under air is not underwater), seen
  failing first.
- [x] W1b: the fixture fluid drawn with its own parameters: `test:oil`'s vertices carry its own id
  (`dwell-mesh` `a_fluid_added_by_data_alone_…`) and its row of the fluid buffer holds its own
  parameters (`render::water` `packs_each_fluid_at_its_id_…`); the shader validates (naga) and draws
  in Chrome on the GPU (`npm run bench -- water-views`).
- [ ] W1b: the owner's look (`water-views` screenshots in the PR).

Deviations (W1b):
- Beyond the plan, from the brief: the underwater fog stub (the composite reads the `water` uniform:
  the fluid's absorption and in-scatter replace the height fog while submerged; W4 builds the rest)
  and the water quality setting (Low, Medium, High; only Low differs in W1b — one ripple layer —
  W5 adds the tiers' real differences).
- The normal "maps" are procedural (gradient noise with derivatives), not textures: no assets, and
  each layer fades by its own wavelength against the pixel's footprint, which mipmaps would do less
  well for a scrolling, wrapped pattern.
- `dwell:water`'s data tuned from the screenshots (prototype content): scatter `#2b9cc0` (was
  `#1d6f8a`), opacity 0.65 (was 0.55). Then, at the owner's request for bluer water: block colour
  `#2f86e0` (was `#3fb2d6`), scatter `#1c7ed8`, absorption [0.7, 0.28, 0.06] (was [0.45, 0.09,
  0.06]), opacity 0.72. Then tropical (owner): scatter `#1a8ee0`, absorption [0.45, 0.12, 0.04]
  (clear water: red goes first, so shallows over sand read turquoise once W3 grades by depth).

Deviations (W1a):
- The per-vertex fluid id moved to W1b: it changes the terrain vertex format, which Phase 25's A1
  is restructuring, and nothing reads it before the water shader. Until then a chunk's fluids share
  one transparent draw at `dwell:water`'s opacity.
- A fluid's `opacity` is a field of the fluid (the fixed alpha until W3's absorption replaces it);
  fluid blocks take it from there instead of declaring their own.
- The fixture fluid is a test registry generated alongside the build's
  (`shared/blocks/fixtures/` → `crates/dwell-blocks/src/fixture.gen.rs`, feature `fixture`); the
  chunk mesher takes a registry's kinds so the test meshes `test:oil` by data alone.
- Checked by: `client/src/world/fluids.test.ts` (bad entries fail naming the fluid and field),
  `dwell-mesh` `a_fluid_added_by_data_alone_is_meshed_with_its_own_surface_drop_and_opacity`, and
  `dwell-world` `fluid::tests` (the regression: an eye 0.9 m up a water cell under air is not
  underwater; failed against the whole-cell rule, as physics counts it, then passed).
- [ ] W3: screenshots of shore views; bench.
- [ ] W4: named views underwater and half in; the owner's look.
- [ ] W5: bench on a desktop and a phone per tier; the owner confirms the phone default after seeing
  it on the two target phones.
- [ ] W6: parity against W5's views.

---

## Phase 27 — Structure Sandbox

**Goal:** A data-driven structure format and an in-game sandbox world to build structures in, saved
locally and exported into `shared/structures/`
([ADR 0031](./adr/0031-data-driven-structures.md), [`STRUCTURES.md`](./STRUCTURES.md)).

**Status:** In progress — S0 (the ADR), S1 (the structure format and its generator), S2a (the floor
and the sandbox world type), S2b (the structures store) and S3a (region edits and the region tools)
done; outstanding for S2a: its e2e exit check; outstanding for S3: S3b (anchor, touch) and the
sandbox e2e.
Needs Phase 24 only. S2b needs S1; S3 needs S2a. Only the owner adds structures (per build). S4,
placement in worldgen, is **deferred** (owner: decided once structures exist); it would revisit B10
in part and ship on a new compatibility line. S1–S3 change no terrain.

Deliverables
- [x] **S0** — ADR 0031 (the format with a `kind` and open vocabularies, the sandbox world type, the
  unbreakable floor (S2a: the existing `indestructible` property), the structures store, owner-only authoring, placement deferred); the owner's
  answers recorded (`STRUCTURES.md` §7).
- [x] **S1** — The structure format: JSON in `shared/structures/` (version, `kind: "voxels"`, size,
  anchor, palette by block name, layers, named variant operations; capped at 32³);
  `shared/structures/gen.mjs` validation, canonical output and hash, stale check in CI; a Rust loader
  replacing `Structure::parse`; the playground converted.
- [x] **S2a** — The floor and the world type: the registry's existing `indestructible` property
  (reused instead of a new `breakable` flag) checked by the server's edit validation on Break and,
  newly, on Place into the cell; an indestructible white floor block (`dwell:sandbox_floor`, full,
  untextured, not placeable, not adaptive) appended to the registry; a `sandbox` world type
  (generator 100: the flat generator with a layers parameter, in the floor block; flight on, the
  default policy) in the main menu and the launcher's new-world route (`?new=1&type=sandbox`).
- [x] **S2b** — The structures store (after S1): OPFS `dwell/structures/<name>.json`, append-only
  (unknown fields kept, unreadable files left alone); save a selection as a structure; export and
  import JSON (download, clipboard). *(Built: `client/src/local/structureStore.ts`, `structures.ts`,
  the game menu's Structures page `ui/structuresPanel.ts`; the format's rules shared with `gen.mjs`
  as `shared/structures/format.mjs`; save from the B/N selection, everything built near the player
  or two corners; load through the region tools' paste (`RegionTools::load_clipboard`).)*
- [x] **S3a** — Editing tools: box select with an outline and its size, fill (also hollow, replace,
  remove), copy and paste with a ghost preview, rotate and mirror, undo/redo (`dwell-game`
  `region_tools.rs`); the `RegionEdit` message (≤ 32³ per request, negotiated within the 0.10 line
  by a `regionEdits` Welcome flag, ARCHITECTURE.md §10.5) with server validation
  (`dwell-world` `region.rs`) and golden tests; the DOM UI (a status line above the hotbar, the
  controls table).
- [ ] **S3b** — Set anchor (moved here from S3: it belongs with saving a selection as a structure,
  S2b); touch controls for the region tools.
- [ ] **S4** — *Deferred (owner: decided once structures exist).* Placement in worldgen, after Phase
  17: placement rules in the terrain definition, hashed jittered-grid placement, conditions, LOD
  handling, goldens; a new ADR revisiting B10, and a new compatibility line.

Exit criteria
- [x] S1: a fixture structure stamps by data alone; bad entries (unknown block, bad palette
  character, wrong layer size, unknown kind or variant operation, over 32³) fail naming the entry;
  the playground's tests pass on the converted file. *(Verified: `dwell-world` `tests/structures.rs`
  — `shared/structures/test/ladder_post.json` stamps by data alone, variants are deterministic, a
  ladder turns and mirrors with its wall, the converted playground equals the old boxes cell for
  cell (red on a one-cell change); `shared/structures/gen.test.mjs` covers every bad entry listed and
  the 32³ cap; the netcode suite passes on `playground.json`.)*
- [x] S2a: breaking or replacing an indestructible state (the sandbox floor) is rejected by the
  server (`dwell-world` test `the_sandbox_floor_cannot_be_broken_or_replaced_but_can_be_built_on`).
- [ ] S2a: a sandbox world opens on the white floor with flight on (e2e). Not yet an e2e spec. Manual
  check (2026-10-14, Chrome on the GPU, `?new=1&type=sandbox`): the index records a `sandbox` world
  (generator 100); the ground is the floor block to depth; a Break on it leaves it; a stone placed on
  it lands; flight turns on. Finding: lit and tone-mapped, the white floor read light grey, close
  to the sky's fog colour, so the horizon disappeared and only placed blocks gave depth; the owner
  approved a grid, now built (a faint line at every block edge from the `grid` tile, a stronger one
  every 8 blocks from the `grid` look option), checked near, far and from above: the lines stay
  readable under tone mapping, fade without shimmer at a distance, and leave the block outline
  clear.
- [x] S2b: a structure saved in the sandbox survives a reload and exports to JSON that `gen.mjs`
  accepts; the store's append-only test. *(Verified: `structureStore.test.ts` (append-only: unknown
  fields kept through rename and save-over, broken, newer-format and invalid files never rewritten or
  deleted); `structures.test.ts` validates an export with `gen.mjs`'s `loadDir`; browser check
  (2026-10-15, Chrome on the GPU, `?world=sandbox`): stones built, saved as `dwell:s2b_check`
  (2 × 2 × 2), downloaded and passed `gen.mjs`, listed after a reload, deleted, imported from the
  file, loaded and pasted — the stones doubled.)*
- [ ] S3: build, copy, rotate, mirror, paste and undo a structure in the sandbox (e2e; needs S2a's
  world type — covered natively meanwhile by `dwell-client` `game_local` and `dwell-game`
  `region_tools` tests).
- [x] S3: region edits over the cap or outside the build volume are rejected (`dwell-sim`
  `tests/region_edits.rs`, `dwell-world` `region.rs` tests).

Deviations (S3a): region edits leave **indestructible** cells as they are — the registry's existing
`indestructible` flag and the same check single breaks make, not a new `breakable` flag
(coordinator, 2026-10-14). `RegionEdit` ships without a `protocolVersion` bump: an older server
rejects unknown messages, so it is gated by the `regionEdits` Welcome flag instead (ARCHITECTURE.md
§10.5); a new compatibility line was not needed.

Deviations (S2b):
- **Loading uses S3's region paste**, not edits stamped at the anchor: the structure goes on the
  region tools' clipboard in paste mode, so it is placed like a copy (ghost box, rotate, mirror, a
  right click, undo) and stands on its bottom centre; the anchor is not used until S3b's anchor work.
  A structure's kept (`null`) cells take what the world holds when the paste is sent.
- **Found and fixed:** the region tools refused boxes reaching into all-air chunks ("Not loaded
  yet"), which the client never stores; the streamer now answers what is loaded (regression test
  `a_box_over_an_air_chunk_reads_as_loaded_air`).
- The store needs OPFS `createWritable` on the page (no fallback where it is missing).

Deviations (S1):
- Choices are **per voxel** (each cell draws independently, hashed from the seed and the unturned
  cell); a structure-wide pick would be a new variant operation. Mirroring (`x` or `z`) is applied
  before rotation, and one of "none" and the listed axes is picked.
- The playground fits the 32³ cap (22 × 6 × 14), so no box or RLE encoding was needed; its box was
  extended north to z = 0 (kept cells) so its anchor is the world origin. RLE is not built.
- Test fixtures are generated into a separate table (`tests/fixtures/structures.gen.rs`), so they
  are not in the build.

## Phase 29 — Motion Effects (Speed FOV and Wind)

**Goal:** A "fast moving" feel (owner, 2026-10-14: "sprint wind effects and dynamic FOV
adjustment … using shaders for VFX"): the field of view widens with the camera's speed and wind
streaks blow past the screen's edges, from data-driven tunables
([`MOTION_EFFECTS.md`](./MOTION_EFFECTS.md)).

**Status:** In progress — built and tested; outstanding: the owner's look in a browser and approval
(and the open questions in `MOTION_EFFECTS.md`), CI. Built on Phase 25's shared frame (A1): the
wind is part of its composite pass.

**Deviation:** the owner was not asked the "ask before designing" questions first; the defaults
chosen are recorded in `MOTION_EFFECTS.md` with the open questions.

**Deliverables:**

- [x] Tunables as data: `shared/look/motion.json`, validated and generated into dwell-look's
  `motion.gen.rs` by `shared/look/gen.mjs` (`node --test shared/look/gen.test.mjs`; CI fails on
  stale output).
- [x] dwell-look's `motion.rs`: speed → FOV boost and wind targets (eased walk→run, log-scaled to
  flight), per-state gains, a critically damped spring, the FOV caps, the vanishing point, resets
  on teleport, respawn, a tool camera and the face-B turn-over, held while the menu is open.
- [x] The frame's FOV computed once by the game from the predicted velocity and state and used by
  the renderer (projection, sky rays, pass culling) and the LOD selection.
- [x] The wind in the composite: procedural streaks from the vanishing point, a flight layer, an
  edge vignette and an edge radial blur of the scene (WGSL, naga-validated).
- [x] Settings: Speed effects (off under `prefers-reduced-motion`) and FOV boost 0–100 %, stored
  append-only under `dwell.motion`.
- [x] Bench scenarios `sprint-straight` and `fly-fast-turn`; the cost measured on `fly-low-fast`.
- [x] The owner's answers (2026-10-15): a Field of view setting (60–100°, default 75°, append-only
  `dwell.view`) as the base for the projection, the LOD and culling, with the wide-screen rule
  derived from it (`dwell_lod::base_fov`); the boost as a zoom-out percentage of the base (+10 %
  sprinting, +20 % at top flight speed, clamped to 120°; the boosted caps removed); the open
  questions decided (`MOTION_EFFECTS.md`).

**Exit criteria:**

- [x] Unit tests: speed → target FOV, the spring, the caps, resets, the menu hold, a tunable changed
  by data changing the effect; the settings store's append-only test.
- [x] A local world natively: sprinting widens the view for the renderer and the LOD alike, and
  standing settles it back; the Field of view setting is the base for both (`tests/game_local.rs`).
- [x] Setting → base FOV at 60/75/100° (wide screens included) and the boost → effective FOV at
  0/10/20 % (unit tests).
- [x] Cost: `fly-low-fast --unthrottled` on this Mac's GPU, effects on against off: the composite
  +0.2 ms GPU (p50 1.13 → 1.37 ms), no change in frame interval.
- [ ] The owner looks at it in a browser and approves (subtle at sprint, stronger in fast flight,
  the crosshair area clear).

## Phase 30 — Tree Growth

**Goal:** Hand-built trees (structures whose model is the mature tree) that grow over time by a
computed reveal of their skeleton, vary by skeleton jitter, and persist as a layer composed at chunk
load, for both procedural and planted trees
([ADR 0033](./adr/0033-tree-growth.md), [`TREE_GROWTH.md`](./TREE_GROWTH.md)).

**Status:** In progress — G-ADR (the ADR) done; G0's page built and verified in Playwright's
Chromium (the three samples' mature stages equal their models; findings in `TREE_GROWTH.md` §8);
outstanding: publishing it and the owner's review of its stages and variants; next: G1. Depends on Phase 27
(the structure format, S1, for G1; the sandbox, S2a/S3, for G2), on Phase 20 (a cut tree's crown
becoming a body that settles and re-bakes) for G3's cuts, and on the day/night cycle, which is not
designed yet (rates are in in-game days; a 30-minute day is assumed until it is). G0–G2 change no
world file, protocol or terrain. G3 is **a new compatibility line** (world-file tables, protocol);
the owner decides the bump. Procedural trees also wait for ADR 0031's deferred worldgen placement
(Phase 27's S4), which is its own new compatibility line.

Deliverables
- [x] **G-ADR** — ADR 0033 (the model is the mature tree; a computed skeleton reveal; the trunk 1 m per
  30 in-game days, branches at `branch_rate` of it, default 1/3; a per-world speed multiplier; stop
  and prune at obstacles; cuts by Phase 20; skeleton-jitter variation, no grafting, no space
  colonization; the owner's persistence model; procedural and planted trees; compatibility); the
  owner's answers recorded (`TREE_GROWTH.md` §7).
- [x] **G0** — The prototype page `docs/prototypes/tree-growth.html`: load an exported structure,
  show its extracted skeleton, scrub the age, roll variant seeds, place obstacles, export the
  `growth` and `variants` sections; three sample trees whose mature stages equal their models
  (checked in the page); verified in Playwright's Chromium (samples, ages, variants, an obstacle,
  JSON load and errors, a phone viewport; no console errors). No protocol or release impact.
- [ ] **G0 review** — The page published (its link in `docs/prototypes/README.md`) and the owner's
  review of whether the inferred stages and variants look right; the owner's choices exported for
  G1.
- [ ] **G1** — Roles (`wood`, `leaf`, optional `trunk`, `root`, `core`) and the `growth` block in the
  structure format; skeleton extraction in `shared/structures/gen.mjs` (emitted with the canonical
  output, stale-checked in CI; bad roles or growth parameters fail naming the entry); Rust
  `stage_voxels(structure, variant seed, age)`; tests: the final stage equals the model, stages only
  add connected voxels, and a fixture tree added by data alone grows.
- [ ] **G2** — A growth preview in the sandbox (after Phase 27's S2a and S3): a "preview growth"
  command or age slider over a placed tree.
- [ ] **G3** — Growth in the world (**a new compatibility line**; the owner decides the bump): the
  tree layer composed at chunk load on the server and the client from the same records (sent with
  the chunk); per-chunk tombstones of chopped procedural trees and per-tree pruned node sets; the
  `growers` table for planted trees; scheduled catch-up from world time (on load and on a slow
  schedule while loaded); the `VoxelModification` `Growth` reason; pruning on obstacles and on cuts
  (the cut-off part a Phase 20 body); the per-world growth-speed setting; planting a tree.
- [ ] **G4** — Jitter variants: branch positions, lengths and droop (optionally dropped branches),
  leaves following their nodes, `core` rigid; seeded per tree.

Exit criteria
- [ ] G1: the final stage of every tree structure equals its model; a fixture tree added by data
  alone grows through its stages; a bad role or growth parameter fails naming the entry; the
  generator's output is stale-checked in CI.
- [ ] G2: in the sandbox, a tree previewed from planting to maturity reaches exactly its model.
- [ ] G3: a planted tree grows by world time while its chunks are loaded and catches up the same
  after they were unloaded (test); a world that isn't running doesn't grow; the server and client
  compose the same chunk (test, natively and under WASM); growth never overwrites a player's block
  (a blocked node prunes its subtree); chopping prunes or tombstones and survives a reload; the
  speed multiplier scales growth; protocol golden tests for the tree records and the `Growth` reason.
- [ ] G4: two variant seeds give different trees from one model, each reproducible from its seed,
  and a variant's final stage equals its jittered model.

---

## Phase 31 — Sky Clouds

**Goal:** Procedural clouds drawn as part of the sky, not 3D (owner, 2026-10-15). A thin cloud deck
at a fixed altitude has true perspective and parallax, a domain-warped noise shape, wind drift, and
light from the sky frame's sun and moon. It is an extendable system: tunables as data, coverage one
run-time parameter for a future weather system, and a quality setting
([ADR 0034](./adr/0034-sky-clouds.md), [`CLOUDS.md`](./CLOUDS.md)).

**Status:** In progress. C0–C4 are built and tested, natively, in Node and in a browser on the
GPU. Outstanding: the owner's look and answers to `CLOUDS.md` §6 (the defaults were taken without
asking: clouds are new), CI, and a phone's frame time. Built on Phase 25's A1 (branch
`claude/a1-shared-frame`).

Deliverables
- [x] **C0** — ADR 0034 and `CLOUDS.md` (the defaults for CLAUDE.md's questions, the model, the open
  questions); ARCHITECTURE.md §5, §6.6 (Clouds) and §7.4.
- [x] **C1** — The cloud deck in the composite (`dwell-look` `clouds.rs`, `cloud_wgsl`): the ray's
  hit on a sphere-curved deck (hidden below the horizon, faded with distance and near the deck);
  periodic value-noise fBm with a domain warp and a lattice-preserving octave transform, offsets
  wrapped in f64 (f32-safe anywhere); octaves faded by the pixel's footprint; light from the
  frame's sky frame and lights (self-shadow on High, silver lining, low-sun warmth, night by
  direction); the terrain's fog toward the clear sky; face B over its own horizon.
- [x] **C2** — Tunables as data: `shared/look/clouds.json` → `clouds.gen.rs` by
  `shared/look/clouds.mjs` (validated, with `clouds.test.mjs`; CI regenerates and fails on a diff);
  coverage a frame uniform (`Renderer::set_cloud_coverage`, `Client.setCloudCoverage`).
- [x] **C3** — The Clouds setting (Off / Low / High: fewer octaves and no self-shadow on Low), High on
  desktops and Low on touch devices, kept under `dwell.clouds` (append-only store); the drift's
  clock is the server's tick (`world_clock.rs`).
- [x] **C4** — Bench scenarios `clouds-high`, `-low`, `-off` (GPU cost), `-night`, `-dusk`, `-fly`
  and `-altitude`; named views and scenarios take `settings.clouds` and `settings.skyRotation`.

Exit criteria
- [x] The deck intersection, the wrap, coverage, the quality setting, the clock and a tunable changed
  by data are pinned by tests; the WGSL validates with Naga (alone and in the renderer's module);
  the generator's output matches the data (`node --test shared/look/clouds.test.mjs`).
- [x] In a browser on the GPU: clouds looking up, at the horizon, toward the sun, at night, with a
  low sun, while flying fast (parallax and drift), fading near the deck, and over face B by day and
  night, from screenshots.
- [x] GPU cost measured against Off (`gpu.composite`, `--unthrottled`, `CLOUDS.md` §4).
- [ ] The owner's look at the clouds, and answers to `CLOUDS.md` §6.
- [ ] CI green, and the frame time on a phone with the default (Low).

---

## Phase 28 — Sound Effects

**Goal:** Procedural, data-driven sound effects for every game and menu action: block sound sets
(footsteps, landing, break, place, a refused hit), the player's movement and water, flight, the
hotbar and the menus, with per-play variation for game sounds and none for UI sounds, spatialised
for other players ([ADR 0032](./adr/0032-audio.md), [`AUDIO.md`](./AUDIO.md)).

**Status:** In progress — F0–F6 built; every automated exit criterion verified (Vitest, Rust tests,
`npm run sfx:render`) and the sounds checked playing in the built game and the main menu in Chromium
(a local world: footsteps, jump, landing, place, break, refusal, hotbar, game menu and its tabs; the
menu's hover, click and error). Outstanding: the owner's listening review of the WAV renders and the
open questions (`AUDIO.md` §9). Needs Phase 24 only. Defaults chosen without the owner's
answers (the owner gave the brief directly); open questions in `AUDIO.md` §9.

Deliverables
- [x] **F0** — `AUDIO.md` and ADR 0032 (procedural, plain Web Audio, no library; data format;
  events from the Rust game; UI sounds without variation; volumes).
- [x] **F1** — The sound data (`shared/sounds/dwell.json`) and `shared/sounds/gen.mjs`: validation
  with messages, generated TS and Rust, the registry's `sound` field checked by
  `shared/blocks/gen.mjs`, CI stale check.
- [x] **F2** — Rust sound events (`dwell-game` `sound.rs`): footsteps by gait stride, climbing,
  swimming, landing, jump, water enter/exit, crouch, sprint, slide, flight rise/dive, local and
  remote edits, refused edits; `takeSounds` and `listener` on the JS API.
- [x] **F3** — The synth, variation and mixer (`client/src/audio/`): the vocabulary, seeded
  variation, buses, room reverb, limiter, spatial playback, autoplay unlock, mute when hidden.
- [x] **F4** — Game actions and menus: event playback in the game page; action sounds (hotbar,
  flight, game menu, F3/F4/F6, touch mode); delegated UI sounds on the main menu (launcher too) and
  the game menu; the controls coverage test.
- [x] **F5** — Master, Effects and Interface volume sliders (append-only `dwell.audio` store).
- [x] **F6** — Offline renders (`npm run sfx:render`): every sound rendered in Chromium, checked
  (non-silent, peak < 1.0, sensible length) and written to WAV, with footstep sequences and
  break/place variations.

Exit criteria
- [x] A new sound set and block mapping work by data alone (Vitest); bad entries fail naming the
  entry.
- [x] Rust tests: footstep cadence from distance, landing intensity from impact speed, edit events
  (echo suppressed, remote spatial, refused hit).
- [x] Every controls-table action has a sound mapping (Vitest); a new button gets the UI click
  without wiring (Vitest).
- [x] Every render is non-silent, peaks below 1.0 and has a sensible length.
- [ ] The owner listens to the WAV renders and approves them.

Deviations
- The levels of the sounds were set by measuring the renders (A-weighted loudness per category:
  footsteps, landings, breaks, places, hits, player and UI sounds) rather than by ear; the owner's
  listening review may retune them in the data.
- Chat and screenshots do not exist yet, so they have no sounds; the coverage test asks for them
  when they join the controls table.
- The limiter is a waveshaper soft clipper, not a `DynamicsCompressorNode`: Chrome's compressor adds
  its own make-up gain, which pushed sequences past full scale in the renders.

---

## Cross-Cutting Work (every phase)

- Materials and terrain style are prototype placeholders (ARCHITECTURE §6.1): phases build and
  test mechanisms with them; none of them commits to a block set or world look.

- Keep `docs/ARCHITECTURE.md` current (required by `CLAUDE.md`).
- Record significant choices as ADRs in `docs/adr/`.
- Every new message type gets golden-byte tests in `dwell-protocol` (`tests/vectors.rs`) against
  `shared/protocol/vectors.txt` from the independent Python encoder (`make_vectors.py`).
- CI must stay green; the Pages deployment must stay playable in local mode.
