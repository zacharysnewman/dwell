# Agent quickstart

One page for a typical task, read **instead of** all of `CLAUDE.md`. Each rule is one line with a
pointer; open the full `CLAUDE.md` section (or the doc it names) only when your change is in that
rule's territory — the table at the end says when.

## Build rules

- **One warm target dir.** On the owner's Mac every agent and worktree shares
  `CARGO_TARGET_DIR=/Users/zacknewman/Projects/dwell/target` (the disk is tight; a cold `target/`
  per worktree costs minutes). Cloud sessions use their own checkout's `target/`.
- **A build script panics** (stale version or generated input): `touch crates/*/build.rs` and build
  again.
- **A worktree has no `client/node_modules`:** symlink the main checkout's
  (`ln -s /Users/zacknewman/Projects/dwell/client/node_modules client/node_modules`) rather than
  `npm ci` a second copy.
- **Multi-line scripts and edits** go through `python3 - <<'EOF' … EOF` (a quoted heredoc: no shell
  quoting or expansion to get wrong), not long `sed`/`echo` chains.
- **One heavy job at a time** and nothing quiet for more than 5 minutes without a reason
  (CLAUDE.md "keep work fast"); reproduce natively first, the browser once at the end.

## Check what you touched

`npm run check:<area>` (in `client/`) runs one area's generators — failing if their checked-in
output was stale — its node tests over the data, `cargo fmt`/`clippy -D warnings`/tests for its
crates, and its Vitest files. CI still runs everything; locally run only the areas you touched.

| Area       | Covers                                                                                         |
| ---------- | ---------------------------------------------------------------------------------------------- |
| `protocol` | wire constants and golden vectors; `dwell-protocol`, `dwell-net`; `src/net`                    |
| `data`     | blocks, items, structures, explosives, fluids, ground, brushes; `dwell-blocks`; `src/world`    |
| `worldgen` | terrain, trees, the world map; `dwell-worldgen`, `dwell-world`                                 |
| `mesh`     | meshing, the LOD, the texture atlas; `dwell-mesh`, `dwell-lod`; `src/chisel`                   |
| `render`   | `shared/look/*`, models, characters; `dwell-look`, `dwell-client` (+ wasm32 clippy), …        |
| `sim`      | server, physics, controller (+ the face-B mirror suite), creatures, storage                    |
| `ui`       | the HUD, menus, settings, input; `shared/ui`, `play`, `input`, …; `dwell-game`; `src/ui`       |
| `audio`    | sounds and music; `dwell-game`; `src/audio`                                                     |
| `tools`    | release, site, launcher and bench scripts (`client/scripts`, `client/launcher`)                |
| `all`      | every area, workspace-wide                                                                     |

`--dry-run` prints the commands; `node --experimental-strip-types scripts/check.ts --list` the
areas. The table is `client/scripts/checks.ts`; `checks.test.ts` fails when a generator, crate or
test file belongs to no area — add it there. TS changes also need `npm run typecheck`, `npm run
lint` and `npx prettier --check <files>`.

## Screenshots and browser checks

Local machine only (cloud sessions have no GPU, [CLOUD_SESSIONS.md](CLOUD_SESSIONS.md)). In `client/`:

- **Don't build the client per check:** `npm run bench -- --warm …` builds the WASM and client only when
  their sources changed (cached by the sources' git tree hash in the main checkout's
  `target/bench-client/`; a docs- or bench-only change reuses it) and serves it on a long-lived
  preview at `:4174`. **Stop it when done:** `npm run bench:warm -- --stop`.
- **A place:** `npm run bench -- --warm --spot starter:broadleaf` (saved spots, `client/bench/spots.json`;
  add yours there). In a scenario, `{ "do": "shot", "name": "x", "above": [x, z, 6], "yaw": 90,
  "pitch": -10 }` puts the eye exactly 6 m over the ground at x, z (or `at`: an exact eye), waits for the
  view to settle and shoots. An `at` y on the wrong face's side of y 0 fails the parse.
- `look` turns a placed camera too; a load not playing within 60 s is retried once (`stalls` in the
  report). Details: `docs/DEV_TOOLS.md` "Screenshots of a place".

## Must-follow rules

- **Extendable systems:** a new feature is data + one generator + names, not constants; ask the
  owner before designing (CLAUDE.md "every new feature is an extendable system").
- **Content is prototype:** don't treat or extend placeholder blocks/terrain as design (CLAUDE.md
  "Content is prototype").
- **Both faces:** face-local code through `dwell_world::FaceFrame`, tested with `both_faces!`;
  controller changes pass `DWELL_FACE=b` (CLAUDE.md "both faces", `check:sim` runs it).
- **Regression tests red → green:** a bug fix's test fails on the unfixed code first; say so in the
  commit (CLAUDE.md "every bug fix has a regression test").
- **Versions and compatibility:** don't break saved worlds, seeds' terrain, protocols or browser
  stores without a new MINOR the owner raises; adding blocks or properties is fine (CLAUDE.md
  "versions and releases", `docs/RELEASES.md` §3).
- **Owner's requests:** record each in `docs/WORK_QUEUE.md` the same turn, quoted, with an **"Owner should
  see:"** line (the result in game; the brief's acceptance check); done only when a screenshot or the owner
  confirms it; a later request that shows an earlier one fell short says so and links it (CLAUDE.md "record
  the owner's requests").
- **Release note:** every PR ships as a release with a player-facing note. Follow the brief: cloud
  and brief-driven PRs put it in the PR body under "Release note" and leave `package.json` and
  `shared/releases/changes.json` to the maintainer ([CLOUD_SESSIONS.md](CLOUD_SESSIONS.md)).
- **Docs to update (lighter paperwork):** small changes (bug fixes, tuning, tests, docs) update only
  `docs/WORK_QUEUE.md` (and `docs/BUGS.md` for a reported bug) and the release note;
  `docs/ARCHITECTURE.md` and ADRs only when the architecture actually changes;
  `docs/IMPLEMENTATION_PLAN.md` only for phase work (CLAUDE.md "lighter paperwork").
- **New dependency in a build:** `python3 shared/licenses/gen.py` (CLAUDE.md "Licence notices").

## Commits and PRs

Commit messages end with the trailers the session's brief gives, e.g.

```
Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_…
```

PR bodies end with `🤖 Generated with [Claude Code](https://claude.com/claude-code)`, a blank line and
the session link. No model identifiers anywhere else in commits, PRs or code. Work on the branch the
brief names; open a PR only when asked.

## When to read the full CLAUDE.md

| You are…                                                    | Read                                                  |
| ----------------------------------------------------------- | ----------------------------------------------------- |
| designing a new feature                                     | "every new feature is an extendable system"           |
| touching terrain, meshing, collision, controller, camera, … | "both faces", `docs/BIFACIAL_WORLD.md` §11            |
| changing a save format, protocol, worldgen or a store       | "versions and releases", `docs/RELEASES.md`           |
| changing workflows or release scripts                       | "versions and releases"                               |
| working on a phase of the plan                              | "track implementation progress", "progress table"     |
| chasing an e2e failure                                      | "Known CI failures", "Running the browser in a sandbox" |
| testing in the browser or benchmarking                      | `docs/DEV_TOOLS.md` (the `bench` skill)               |
| looking for a design doc                                    | "Key documents"                                       |
