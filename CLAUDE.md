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
- `docs/adr/` — architecture decision records.
- `docs/FUTURE.md` — out-of-scope future plans; items move into the architecture only via an ADR.

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
