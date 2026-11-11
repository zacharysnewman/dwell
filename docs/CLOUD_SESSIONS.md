# Cloud sessions

How the maintainer runs Claude Code cloud sessions for parallel, logic-only work alongside the one
local heavy job (owner, 2026-11-11). A cloud session runs in its own container with a fresh clone,
builds and tests there, and hands back a PR.

## Launching

From the repo root:

```sh
script -q /tmp/cloud-<name>.log claude --cloud "<brief>" < /dev/null
```

- Plain `claude --cloud "<brief>"` needs a TTY; `script -q <log>` gives it one and keeps a log, and
  `< /dev/null` stops it waiting on the terminal.
- The Agent tool's `isolation: "remote"` silently ran locally instead and is not used.
- The brief is the session's only context: the item (e.g. `docs/WORK_QUEUE.md` item N), the branch,
  what to read first (`docs/AGENT_QUICKSTART.md` rather than all of `CLAUDE.md`), the checks to run
  (`npm run check:<area>`), the commit trailers and the PR footer, and "don't change
  `client/package.json`'s version or `shared/releases/changes.json`; put a one-line release note in
  the PR body under 'Release note'".

Follow-ups go to the same session by its id:

```sh
claude -p "<message>" --cloud <session-id> --output-format json
```

## The PR and merging

1. The session opens a PR against `main` (not a draft), its release note in the body.
2. The maintainer reviews it.
3. The maintainer merges `main` into it, assigns the version (`client/package.json`, the next patch
   or MINOR per `docs/RELEASES.md`) and adds the note to `shared/releases/changes.json`, then merges.

Sessions never pick a version themselves: parallel PRs would all claim the same one.

## What suits the cloud

| Suits                                               | Doesn't                                                        |
| --------------------------------------------------- | -------------------------------------------------------------- |
| Rust logic and its tests (sim, worldgen, protocol)  | GPU work: the browser on a real GPU, screenshots, the bench    |
| data files and their generators                     | judging looks (the software renderer draws at a few fps)       |
| scripts (`client/scripts`) and their Vitest tests   | anything needing local-only assets or the owner's devices      |
| docs                                                | e2e debugging that only reproduces on the owner's machine      |

## Limits

- Up to **3 cloud sessions at once**, alongside **one local heavy job** (owner, 2026-11-11). The
  cloud's CPU does not count against the local one-heavy-job rule.
- Pick items that touch different areas, so the PRs don't conflict.

## Timings measured so far

| Step (in a cloud session)                  | Time   |
| ------------------------------------------ | ------ |
| cold build (first `cargo` build or test)   | ~52 s  |
| incremental build after an edit            | ~10 s  |
