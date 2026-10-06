# CLAUDE.md

## Project

Dwell is a server-authoritative, multiplayer-first voxel sandbox with large-scale dynamic
physics. C++ server with Jolt Physics; TypeScript client running the shared C++ sim core (with Jolt) as WASM; WebTransport
networking. The client is deployed to GitHub Pages (`https://dropkickarcade.com/dwell/`) first,
then wrapped in Electron and Capacitor. Game servers are hosted by players (dedicated servers
and friend worlds), with a small master server for discovery; there are no official game servers.

## Key documents

- `docs/ARCHITECTURE.md` — the authoritative description of the game architecture.
- `docs/PLAYER_CONTROLLER.md` — detailed spec of the physics player controller (architecture sub-spec).
- `docs/IMPLEMENTATION_PLAN.md` — the phased build plan and each phase's exit criteria.
- `docs/WORLD_GENERATION.md` — design for Phases 7 and 10–12: art direction and palette, continents,
  rivers/mountains/biomes, sky islands (becomes an architecture sub-spec as the phases land).
- `docs/BLOCK_REGISTRY.md` — design for Phase 8: namespaced block states, the registry, string
  palettes in world files.
- `docs/SLOPE_BLOCKS.md` — design for Phase 9: slope block shapes, collision, building, terrain
  shaping and LOD (becomes an architecture sub-spec as the phase lands).
- `docs/RELEASES.md` — Phase 6: builds as tagged releases loaded by a version launcher,
  version-locked worlds, the license, and the release procedure.
- `docs/BIFACIAL_WORLD.md` — design for Phase 13: the disc's second face below the midplane, gravity
  toward the midplane, crossing between the faces.
- `docs/reference/` — external references kept for implementation (e.g. the Aether floating-island
  spec used by Phase 12).
- `docs/adr/` — architecture decision records.
- `docs/FUTURE.md` — out-of-scope future plans; items move into the architecture only via an ADR.

## Content is prototype

All current materials (blocks, ores) and the terrain style are **prototype placeholders** for
exercising systems, not Dwell's content design (`docs/ARCHITECTURE.md` §6.1). Don't treat them as
design decisions, don't extend them with content borrowed from other games (e.g. Minecraft's
block set), and keep new work data-driven so the real block set and world style can replace them.

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
- Keep the plan's **Progress** table (phase status and PR) current, and the phase's `**Status:**`
  line accurate, including anything outstanding.
- Record deviations from the plan under the phase, with the reason.

## Requirement: keep the progress table current

> Temporary: remove this section once every phase in `docs/IMPLEMENTATION_PLAN.md` is complete.

Every change that advances, completes, reopens, or re-scopes implementation work updates the
**Progress** table at the top of `docs/IMPLEMENTATION_PLAN.md` **in the same change**:

- The phase's status (⏳ Not started, 🚧 In progress — naming the current sub-phase, 🔍 In review,
  ✅ Complete) and anything still outstanding.
- The PR number(s) once a PR exists for the phase.
- Follow-up work on a finished phase (playtest findings, tuning, fixes) is noted in its row and in
  the phase's `**Status:**` line until it merges.

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

## Building without sqlite.org

The server and WASM builds fetch the SQLite amalgamation from `www.sqlite.org`
(`server/cmake/Dependencies.cmake`). Some sandboxes (e.g. Claude Code on the web) block that host
(HTTP 403). Take the amalgamation from npm instead, which the sandbox allows: `better-sqlite3`
bundles `sqlite3.c` / `sqlite3.h`, and version 13.0.3 carries SQLite 3.53.4, the version the build
pins. Point CMake at it with `FETCHCONTENT_SOURCE_DIR_SQLITE3`:

```sh
cd "$(mktemp -d)" && npm pack better-sqlite3@13.0.3 && tar xzf better-sqlite3-*.tgz
grep -m1 'define SQLITE_VERSION ' package/deps/sqlite3/sqlite3.h   # must match Dependencies.cmake
SQ="$PWD/package/deps/sqlite3"
cd /path/to/dwell/server
cmake --preset dev  -DFETCHCONTENT_SOURCE_DIR_SQLITE3="$SQ"   # native server (e2e)
cmake --preset wasm -DFETCHCONTENT_SOURCE_DIR_SQLITE3="$SQ"   # WASM core (needs EMSDK, see CI)
```

If the pinned SQLite version changes, pick the `better-sqlite3` release that bundles it. This is
for local builds only; CI downloads from sqlite.org as usual.
