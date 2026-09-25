# Dwell — Future Plans

Ideas that are **out of scope for the current implementation** (`IMPLEMENTATION_PLAN.md`
Phases 0–7). Nothing here is part of the architecture until it is promoted: promoting an item
means an ADR, an update to `ARCHITECTURE.md`, and a phase in the implementation plan.

Each entry notes what the current architecture already does to keep the option open.

---

## Friend-world host migration

**Today:** a friend world lives only while its host plays; when the host leaves, the session ends
(ADR 0009).

**Idea:** when the host leaves, hand the world to another participant (or to a dedicated server)
so the session continues.

**What keeps it open:** a world is a single SQLite file (ADR 0006) and the integrated server is the
same sim core on every client, so a snapshot can be transferred and resumed. Would need: a
consistent mid-session snapshot, transfer to the new host, re-signaling all guests through the
master server, and rules for who becomes host.

## Cloud-hosted worlds (paid)

**Idea:** a paid feature that stores a world in the cloud so players can "join" it anytime, even
when its owner is offline — the world is started on demand on managed infrastructure when someone
joins and shut down when empty.

**What keeps it open:**
- World = one SQLite file (ADR 0006) → store it in object storage or a cloud database.
- The dedicated server ships as a Docker image (ADR 0003) → run it on demand on a container
  platform that supports UDP.
- The master server already lists servers and resolves join codes (ADR 0003) → it can start a
  cloud world and return its address when a player joins.
- Accounts (ADR 0004, later layer) → ownership, billing, and access control.

Would need: accounts, billing, a world store, an on-demand server orchestrator, idle shutdown, and
import/export between cloud, dedicated, and friend worlds. This is also the natural home for any
"official" hosting tier.

## Own subdomain for the web client

**Today:** the client is served at `https://dropkickarcade.com/dwell/` (ADR 0005), sharing an
origin with other dropkickarcade.com games.

**Idea:** move to `https://dwell.dropkickarcade.com/` to isolate storage and keys — ideally before
passkey-based accounts ship. Needs a one-time data-migration flow using world and device-key
export/import.

## Trusted hostnames for player servers

**Idea:** the master server issues stable hostnames with publicly trusted certificates to
registered dedicated servers (e.g. `<id>.servers.dropkickarcade.com`, ACME DNS-01 on the zone).

**Why it's optional:** reachability no longer depends on it (ADR 0008: WebRTC fallback with DTLS
fingerprints). It would only add stable, human-readable addresses. Needs programmatic DNS.
