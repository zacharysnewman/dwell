---
name: instances
description: List and stop the game instances running on this Mac — vite dev/preview servers, native dwell-servers, automation Chrome (bench, Playwright, the DevTools MCP's), Playwright runners, wrangler. Use after any browser check, bench run, e2e run or native server test, before reporting, and whenever the owner asks to clean up; never touches the owner's own Chrome.
---

# Game instances

Owner (2026-10-21, 2026-11-15, 2026-11-24): stop dev and test instances as soon as they're not in use — they draw on the
Mac's CPU, GPU and power — and keep game browsers muted unless testing sound.

```sh
.claude/skills/instances/scripts/instances.sh            # list: kind, pid, age, port, command
.claude/skills/instances/scripts/instances.sh stop       # stop every kind but `app`
.claude/skills/instances/scripts/instances.sh stop vite browser   # only these kinds
.claude/skills/instances/scripts/instances.sh stop 24535 24602    # only these pids
```

Kinds: `vite` (this repo's client dev and preview servers, with their port), `server` (native `dwell-server`),
`browser` (Chrome/Chromium launched with a remote-debugging flag: bench, Playwright, the DevTools MCP's — the owner's own
Chrome has none and is never listed), `runner` (Playwright test runners), `master` (wrangler, the master server), `app`
(the Dwell Electron app: stopped only when named, it may be the owner's).

When to use:

- **After a browser check or bench run:** stop the `browser` (or close its pages) and any preview server you started. A
  `vite preview` on :4174 under `target/bench-client` is the bench's.
- **Keep what the owner is using:** a dev server you handed the owner a link to stays up until they're done; stop the
  others by pid.
- **Before reporting work done:** list, and stop what you started.

Stopping sends TERM, then KILL to any still running after 2 s, and lists what is left.
