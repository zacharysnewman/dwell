# 0009. Friend worlds end with their host; migration and cloud worlds deferred

- Status: Accepted
- Date: 2026-09-25
- Resolves: ARCHITECTURE.md Open Decisions #15

## Context

A friend world runs on its host's device (ADR 0003). Host migration — handing the running world to
another participant or a dedicated server when the host leaves — was raised as a possible
improvement over sessions ending with the host. It affects mid-session snapshotting, transfer,
and re-signaling of guests.

## Decision

- **No host migration in the current implementation.** A friend world exists while its host
  plays; when the host quits, the session ends for everyone. When the host backgrounds the
  app/tab, the world pauses and guests are notified (unchanged from ADR 0003).
- The world is saved on the host's device as usual (ADR 0006) and can be exported to a dedicated
  server manually.
- Host migration and **cloud-hosted worlds** (a paid feature that keeps a world joinable anytime)
  are recorded in `docs/FUTURE.md` as out-of-scope future plans.

## Consequences

- Friend worlds stay simple: no snapshot-transfer protocol, no host election.
- Nothing in the architecture precludes migration or cloud worlds later: single-file worlds, the
  shared sim core, the Docker server image, and the master server are the building blocks.
