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
  a file a tool could edit — not constants and branches in C++ or TypeScript.
- **One generator or loader** that turns the data into what the server, WASM core and client use, so
  they agree, with CI failing on stale output (as `shared/blocks/gen.mjs` and
  `shared/protocol/gen.mjs` do).
- **References by name, not by code constant** (`"dwell:sand"`, not `Materials::kSand`).
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
  version if that is higher. Never bump it after a release (nothing needs it).
- **A push to `main` is a dev build** (`v<next>-dev.<run>`), **unless it leaves `package.json` above
  the newest release, which releases that version as stable.** So raising `package.json` is a
  release: do it only when that should ship, and **only a human decides it** — do not raise it on your
  own initiative. Patch releases are manual (Actions → Release → Run workflow, or a `v<version>` tag).
- **A breaking change to the public API — the saved world format, the terrain a seed generates, the
  network protocols, the cross-version browser storage — needs a new compatibility line** (before
  `1.0.0`, the next MINOR). Say so in the PR and let the owner raise `package.json`; do not leave
  such a change shipping under the old line, whose players' worlds it would break.
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
  0.5 m bar on CI. On the C++ build it read exactly 0.4158 m on every run. On the Rust build it
  read 0.455 m on one PR branch (A1, run 38694905876), and it passed on main's runs up to 9a4dc44.
  Locally on SwiftShader it read 0.11 m before the slow-device fix (`MAX_TICKS_PER_FRAME`,
  2026-10-15) and passed after. If it recurs, check the page's tick rate (`stats.ticks` against
  wall time) before widening the wait.
- CI's software renderer draws the e2e pages at ~5 fps. That is why `play.spec.ts`'s two-client
  test snapped (fixed 2026-10-15: a client that dropped time predicted fewer ticks than the
  server's 60 Hz). A page that predicts far fewer than 60 ticks a second is the first thing to
  check when prediction misbehaves only on CI.
- Locally on a Mac, the suite's SwiftShader flags run the pages at ~3 fps, slower than CI. Then
  the native-server and friend tests (`edit` ×2, `friend`, `join:30`, `lobby`, `play` two-client)
  time out waiting for terrain or a join, and they pass on CI. To check those locally, run with
  the real GPU: a copy of `playwright.config.ts` without the four SwiftShader flags.
- The hotbar slot count in `e2e/edit.spec.ts` is the palette's length (5); update it when the palette
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
