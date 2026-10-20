# CLAUDE.md

## Project

Dwell is a server-authoritative, multiplayer-first voxel sandbox with large-scale dynamic
physics. One Rust codebase (ADR 0028): a Cargo workspace (`crates/`) for the dedicated server, the
simulation core (Rapier physics) and the browser client (WASM, wgpu on WebGPU), with a TypeScript
shell for the DOM UI, the master server and the release scripts; WebTransport and WebRTC
networking. The client is deployed to GitHub Pages (`https://dropkickarcade.com/dwell/`) first,
then wrapped in Electron and Capacitor. Game servers are hosted by players (dedicated servers
and friend worlds), with a small master server for discovery; there are no official game servers.

## Key documents

- `docs/ARCHITECTURE.md` — the authoritative description of the game architecture.
- `docs/PLAYER_CONTROLLER.md` — detailed spec of the physics player controller (architecture sub-spec).
- `docs/IMPLEMENTATION_PLAN.md` — the phased build plan and each phase's exit criteria.
- `docs/WORLD_GENERATION.md` — design for Phases 7 and 10–12: art direction and palette, continents,
  rivers/mountains/biomes, sky islands; largely superseded by the baseline rework (Phase 12 cancelled).
- `docs/BLOCK_REGISTRY.md` — design for Phase 8: namespaced block states, the registry, string
  palettes in world files.
- `docs/SLOPE_BLOCKS.md` — Phase 9's shape mechanism (shapes, collision, meshing), trimmed in Phase 15
  (the placed slope and slab families, terrain slope shaping and LOD slopes were cut).
- `docs/ADAPTIVE_BLOCKS.md` — design for Phase 18 (formerly 9e): natural blocks shaped from their neighbours by
  five rules (replaces placed slopes and terrain slope shaping for natural materials).
- `docs/BASELINE_EVALUATION.md` — the owner's simplified baseline (2026-10-12) compared with what
  is built: what to keep, add and cut, graded on whether each part is an extendable, data-driven
  system; built by the baseline rework, Phases 14–19 of the implementation plan.
- `docs/prototypes/` — saved design prototypes (Adaptive Block Lab, Shape Playground) with their
  published links; republish to the same link after editing.
- `docs/SPYGLASS.md` — design for Phase 38 (ADR 0039): the spyglass zoom, its data file, and the
  LOD's detail inside the zoomed cone.
- `docs/RELEASES.md` — Phase 6: builds as tagged releases loaded by a version launcher,
  version-locked worlds, the license, and the release procedure.
- `docs/BIFACIAL_WORLD.md` — design for Phase 13: the disc's second face below the midplane, gravity
  toward the midplane, crossing between the faces.
- `docs/reference/` — external references kept for implementation (e.g. the Aether floating-island
  spec used by the cancelled Phase 12; removed in Phase 15).
- `docs/DEV_TOOLS.md` — the dev telemetry and the bench: run the real game in Chrome on the GPU,
  drive it with scripted scenarios (`client/bench/scenarios.json`, `npm run bench`) and read frame
  timings, job latencies, controller ticks, traces and screenshots. Use and extend it to test and
  benchmark in the browser.
- `docs/RAY_MARCHED_RENDERER.md` — a note on Phase 25's ray-marched renderer, dropped at A7 (ADR 0044):
  what was measured and where the code lives (the `archive/ray-marcher` tag and branch).
- `docs/WATER_RENDERING.md` — design for Phase 26 (ADR 0030): fluid looks as registry data, the water
  shader, refraction, underwater and reflections in quality tiers.
- `docs/STRUCTURES.md` — design for Phase 27 (ADR 0031): the structure format, the in-game sandbox world,
  the structures store; worldgen placement deferred.
- `docs/TREE_GROWTH.md` — design for Phase 30 (ADR 0033): hand-built trees grown by a computed skeleton
  reveal, skeleton-jitter variants, and trees as a layer composed at chunk load (tombstones, `growers`).
- `docs/ART_DIRECTION.md` — the storybook art direction: the palette (`shared/textures/palette.json`),
  the brush vocabulary, how materials read, the lighting assumptions; shared by the world and characters.
- `docs/PAINTERLY_TEXTURES.md` — design for Phase 39 (ADR 0040): painterly textures as data, one stroke
  generator baked at build time; the default set (`?textures=classic` keeps the old one).
- `docs/CHARACTERS.md` — design for Phase 40 (ADR 0041): characters and creatures as rigid chunky parts
  on a data rig (`shared/characters/characters.json`), the first-person arms view, clips; a concept and
  the Character Lab prototype, not in the game yet.
- `docs/proposals/` — investigated but not yet adopted plans, each with open questions for the owner;
  not architecture until an ADR adopts it (adopted ones move to a design doc; its README lists them).
- `docs/adr/` — architecture decision records.
- `docs/FUTURE.md` — out-of-scope future plans; items move into the architecture only via an ADR.

## Content is prototype

All current materials (blocks, ores) and the terrain style are **prototype placeholders** for
exercising systems, not Dwell's content design (`docs/ARCHITECTURE.md` §6.1). Don't treat them as
design decisions, don't extend them with content borrowed from other games (e.g. Minecraft's
block set), and keep new work data-driven so the real block set and world style can replace them.

## Requirement: every new feature is an extendable system

Any new feature must be built as an **extendable system**, not a one-off hardcoded result. The test
(owner, 2026-10-12): **if you can't reasonably build a tool to add something to it, it's not an
extendable system.** The baseline rework (Phases 14–19, `docs/BASELINE_EVALUATION.md`) exists
because earlier features were hardcoded; don't add more work of that kind.

An extendable system has:

- **A data source of truth** for the things it holds (blocks, shapes, terrain layers, looks, …), in
  a file a tool could edit — not constants and branches in Rust or TypeScript.
- **One generator or loader** that turns the data into what the Rust crates (server, sim core and
  client) and the TypeScript shell use, so they agree, with CI failing on stale output (as
  `shared/blocks/gen.mjs` and `shared/protocol/gen.mjs` do, emitting Rust for `dwell-blocks` and
  `dwell-protocol` and TS for the shell).
- **References by name, not by code constant** (`"dwell:sand"`, looked up with `dwell_blocks::find_block` or `parse_state`, not a code constant
  such as `const SAND: StateId = 13`).
- **Validation and tests over the data**: a bad entry fails with a message, and a test adds a
  fixture entry by data alone and sees it work.

The engine behind the data (the node kinds, conditions or shapes it understands) is code, and should
be a small, fixed vocabulary. Extendable means the *system* can take more; it is not licence to add
content or features nobody asked for. Build the content that was requested, on a system that can
grow.

**Ask before designing.** What "extendable" means depends on the feature: what gets added to it
later, by whom, and with what tool. Before designing a new feature, ask the owner clarifying
questions (with the question tool when available), for example:

- What will be added to this later: more entries of the same kind, new kinds, or new rules?
- Who adds them, and with what: a data file, an editor page like `docs/prototypes/`, a CLI, players?
- What must stay fixed in code, and what must be data?
- Is it per build, per world, or per player?

Record the answers in the feature's design doc or ADR. If the owner prefers a hardcoded shortcut for
a prototype, record that as a deviation with the reason. Bug fixes and tuning of an existing system
don't need this step; a fix in hardcoded code that the baseline rework will replace doesn't need to
make it extendable first.

## Requirement: keep ARCHITECTURE.md up to date

Whenever a change adds, removes, or modifies any part of the architecture, update
`docs/ARCHITECTURE.md` — and any architecture sub-spec it links to, such as
`docs/PLAYER_CONTROLLER.md` — **in the same change**. This includes, but is not limited to:

- components, modules, or process boundaries (server, client, workers, shells);
- network transports, channels, message types, or wire formats;
- voxel/chunk data formats and storage;
- physics tiers, the awakening / clustering / sleep / re-bake pipeline;
- tunable constants and thresholds (§7.4);
- deployment topology, hosting, and build/CI pipelines;
- third-party engines or libraries that shape the architecture.

When doing so:
- Update the status tag (**[planned]**, **[in progress]**, **[built]**) of affected sections.
- Remove descriptions of anything that no longer exists — the document must not describe
  architecture that has been removed.
- If a decision in "Open Decisions" is resolved, add an ADR and update the table.
- If a change makes the implementation plan inaccurate, update `docs/IMPLEMENTATION_PLAN.md` too.

A change that alters architecture without updating `docs/ARCHITECTURE.md` is incomplete.

## Requirement: track implementation progress

While implementing a phase of `docs/IMPLEMENTATION_PLAN.md`:

- Tick each deliverable's checkbox (`- [x]`) **in the same commit** that completes it; tick exit
  criteria only once they are actually verified (automated test or a described manual check).
- Never tick partially done work: split the item, or move the unfinished part to a later phase
  with a note.
- Keep the plan's **Progress** table (phase status) current, and the phase's `**Status:**`
  line accurate, including anything outstanding.
- Record deviations from the plan under the phase, with the reason.

## Requirement: keep the progress table current

> Temporary: remove this section once every phase in `docs/IMPLEMENTATION_PLAN.md` is complete.

Every change that advances, completes, reopens, or re-scopes implementation work updates the
**Progress** table at the top of `docs/IMPLEMENTATION_PLAN.md` **in the same change**. The table
describes the state of the **work as it is on the branch**, not the history of branches, PRs or
merges: each session works on its own branch, so a session that finds a change in `main` knows it
is in, and nothing needs to record that it merged.

- The phase's status (⏳ Not started, 🚧 In progress — naming the current sub-phase, ✅ Complete) and
  anything still outstanding. A phase is ✅ only when its deliverables and exit criteria are ticked,
  which needs them actually done and verified, not merely merged. Work that is built but still
  needs CI, a device or the owner's review stays 🚧, with that named as outstanding.
- Follow-up work on a finished phase (playtest findings, tuning, fixes) is noted in its row and in
  the phase's `**Status:**` line.
- **Do not record PR numbers, merges, deploys or "in review".** Don't add them, and don't go to
  GitHub to update the table after a merge. When asked about project status, read the table and the
  checkboxes on the branch you are on; if a checkbox looks wrong, check it against the code and
  tests, not against GitHub.

## Requirement: every bug fix has a regression test

When fixing a bug (reported, or found along the way):

- First write an automated test that reproduces it, and **run it against the unfixed code to see
  it fail** for the reported reason. A test that has never failed does not prove the fix.
- Then fix the bug and see the same test pass, along with the rest of the suite.
- Commit the test with the fix. Mention the red → green check in the commit message or PR.
- If the bug lives in code that can't be tested as written (e.g. per-frame rendering logic inside
  the game loop), extract the logic into a testable unit as part of the fix.
- If a bug genuinely cannot be covered by an automated test, say so in the PR and describe the
  manual check that was done instead.

Behaviour changes that are tuning rather than bugs (e.g. a longer coyote time) follow the same
pattern: a test that pins the new behaviour and fails on the old.

## Requirement: versions and releases

Builds are tagged GitHub Releases behind a launcher at `/dwell/`; worlds are locked to their version's
compatibility line ([`docs/RELEASES.md`](docs/RELEASES.md), ADR 0014). When you work on anything
that touches the public API, a release or the workflows:

- **The app version is Semantic Versioning.** `client/package.json`'s `version` is a **floor**, not a
  counter: the next version is the next patch after the newest published release, or `package.json`'s
  version if that is higher. Never bump it on its own after a release; the bump rides in the next PR.
- **Every PR merged to `main` raises `package.json` and ships as a stable tagged release** (owner,
  2026-10-19): features, bug fixes, CI, test and workflow fixes alike. Nothing merges as a dev-only
  build: the owner sees a change only once a tag is released. Raise it to the next patch above the
  newest release, or the next MINOR for a compatibility break (below), in the PR just before it
  merges, one release per PR. The owner has given standing approval for these bumps. A push that
  leaves `package.json` at or below the newest release (e.g. a direct push) is a dev build
  (`v<next>-dev.<run>`).
- **A breaking change to the public API — the saved world format, the terrain a seed generates, the
  network protocols, the cross-version browser storage — needs a new compatibility line** (before
  `1.0.0`, the next MINOR). Say so in the PR and let the owner raise `package.json`; do not leave
  such a change shipping under the old line, whose players' worlds it would break.
- **Blocks and registry sync (ADR 0038):** the server sends its block registry at join and clients
  map ids by name, so **adding** blocks or states, or changing looks, is not breaking. Still
  breaking: removing or renaming a state (or adding/removing a property), changing an existing
  state's shape or rules, changing the shapes or the adaptive shape rules, and changing worldgen
  (`docs/RELEASES.md` §3).
- **The browser's shared stores are append-only** (the world index, settings, device key): new
  fields optional, unknown fields and unreadable records preserved on rewrite (a test pins it).
- **Release logic lives in tested scripts** (`client/scripts/release.ts`, `site.ts`), not in workflow
  shell: change the script and its tests, then the workflow. The workflows cannot be run locally; the
  launcher and the assembled site can (`npm run e2e:site`).
- **Licence notices:** adding or upgrading a dependency that ships in a build updates
  `shared/licenses` and `THIRD_PARTY_NOTICES` (`python3 shared/licenses/gen.py`; CI checks it).

## Known CI failures (e2e, not caused by feature work)

Check these before chasing an e2e failure:

- `e2e/touch.spec.ts` "touch controls: stick, look, and jump": the jump peak has read short of the
  0.5 m bar on CI, three causes so far. (1) Before 0.15.3 the client caught up at most 250 ms a
  frame; at CI's 440–810 ms frames it dropped time and the starved server flew and landed the held
  jump (0.1123 m, one tick; fixed by `MAX_TICKS_PER_FRAME` 60, `game_local.rs`
  `a_device_at_under_two_frames_a_second_…`). (2) The test read the drawn position, which at
  300–800 ms frames can miss the whole ~0.7 s arc (0, 0.11 or 0.22 m in half of six CI runs); it
  now reads the controller's tick trace (`__dwell.ticks()`). (3) On the generated terrain CI drew
  the page at ~1 fps by the jump (0.8–1.7 s frames), so a frame past the 1 s catch-up still lost
  it; the test now uses `?world=flat`. The failure message prints ticks/s and the frame p50/max: a
  max over 1 s during the jump means the page is dropping time again, not a controller bug.
- CI's software renderer draws the e2e pages at ~5 fps. That is why `play.spec.ts`'s two-client
  test snapped (fixed 2026-10-15: a client that dropped time predicted fewer ticks than the
  server's 60 Hz). A page that predicts far fewer than 60 ticks a second is the first thing to
  check when prediction misbehaves only on CI.
- Locally on a Mac, the suite's SwiftShader flags run the pages at ~3 fps, slower than CI. Then
  the native-server and friend tests (`edit` ×2, `friend`, `join:30`, `lobby`, `play` two-client)
  time out waiting for terrain or a join, and they pass on CI. To check those locally, run with
  the real GPU: a copy of `playwright.config.ts` without the four SwiftShader flags.
- Main's e2e shard 1 (`edit.spec` two-client tests, `friend.spec`) timed out from 0.11.10 to
  0.11.19, waiting for terrain or for the walker to move. The cause was not one change: the
  suite is frame-bound on SwiftShader, every test there ran about 2x slower, and the same commit
  passed on one runner and failed on another (bisect PRs #105–#107). Fixed 2026-10-16: a CPU
  adapter now draws the cheapest tier of everything (`render/mod.rs`: `samples_for`,
  `clouds_for`, `water_for`, `stars_for`, `motion_blur_for`, and `pixel_ratio_for`, which halves
  the drawing buffer), and shard 1 went from 8m43s failing to 3m58s passing. A new per-pixel
  visual needs its CPU-adapter tier too. If shard 1 slows again, compare its per-test times with
  that run before widening a timeout.
- The hotbar slot count in `e2e/edit.spec.ts` is the palette's length (7); update it when the palette
  changes.

## Running the browser in a sandbox

The client renders with WebGPU. Without a GPU, Chromium uses SwiftShader's Vulkan
(`--enable-unsafe-webgpu --enable-features=Vulkan --use-vulkan=swiftshader
--use-webgpu-adapter=swiftshader`), but **headless** Chromium loses the WebGPU device as soon as a
canvas presents ("A valid external Instance reference no longer exists"): run it headed under Xvfb
(`xvfb-run -a npx playwright test`; `playwright.config.ts` sets `headless: false`). Offscreen
rendering (the WebGPU probe, `npm run e2e:webgpu`) works headless. A preinstalled Chromium can be
used with `DWELL_CHROMIUM=/path/to/chrome`. Panics in the WASM module are logged to the console
with their message (a panic hook in `crates/dwell-client/src/web/mod.rs`).
