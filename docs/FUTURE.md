# Dwell — Future Plans

Ideas that are **out of scope for the current implementation** (`IMPLEMENTATION_PLAN.md`
Phases 0–17). Nothing here is part of the architecture until it is promoted: promoting an item
means an ADR, an update to `ARCHITECTURE.md`, and a phase in the implementation plan.

Each entry notes what the current architecture already does to keep the option open.

---

## Rivers filling the lower land beside them (owner, 2026-10-27)

*"Rivers should fill adjacent 'lower' valleys, creating wider rivers/lakes/pools."* Today a river's
water stays in its channel and the generator raises a levee where the land beside it is lower than
the water (WORLD_GENERATION.md §4, generator 25). Instead the water would spread over that land at
the river's level: wider stretches, backwaters and pools along the river. It needs a design pass
before building (questions for the owner first): how far the water may spread, whether it is decided
on the world map (a flood of each river's level over its cells, like the lakes) or per column, how it
stays a pure function of the chunk, and how the LOD shows it. A new compatibility line when built.

## Friend-world host migration

**Today:** a friend world lives only while its host plays; when the host leaves, the session ends
(ADR 0009).

**Idea:** when the host leaves, hand the world to another participant (or to a dedicated server)
so the session continues.

**What keeps it open:** a world is a single redb file (ADR 0006, ADR 0028 §8) and the integrated server is the
same sim core on every client, so a snapshot can be transferred and resumed. Would need: a
consistent mid-session snapshot, transfer to the new host, re-signaling all guests through the
master server, and rules for who becomes host.

## Cloud-hosted worlds (paid)

**Idea:** a paid feature that stores a world in the cloud so players can "join" it anytime, even
when its owner is offline — the world is started on demand on managed infrastructure when someone
joins and shut down when empty.

**What keeps it open:**
- World = one redb file (ADR 0006, ADR 0028 §8) → store it in object storage or a cloud database.
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

## Sun and moon moving separately (sky phase 2)

**Today:** the sky is one sphere whose frame (day zenith, sun, moon) turns as a unit, client-side
only (`dwell-look` `sky_frame.rs`).

**Idea:** the sun and the moon move separately, with the gradient weighted toward each; the
meshers' baked face shading (warm tops, cool sides, blue undersides) moves into the shader so it
follows the light; and the angle comes from the server's tick, so every player shares it.

## Sky, fog and light by the sun's position (owner, 2026-10-26)

**Today:** the sun, moon and stars are world directions, but the sky's colours, the fog and the light
are the viewer's face's (blended across the flip band; DAY_NIGHT.md §5), and each face's geometry is
lit by its own face's light. So what one sees of the other face (through a portal, past the rim) is
coloured by one's own face's time of day, except the sky behind the world below the horizon, which
takes the other face's sky since 0.28.5 (docs/BUGS.md 10).

**Goal:** the sky, the fog and the lighting worked out per point from the sun's height above that
point's face (its face-local horizon), so every look across the faces matches where the sun is.
Needs a design pass: what the fog and the sky colour are along a ray that crosses the midplane, and
what that costs per pixel.

## World upgrades across app versions

**Today (planned, Phase 6):** a world records the app version that saved it and opens only in
builds of the same compatibility line — that version or a later compatible one
([`RELEASES.md`](./RELEASES.md) §6).

**Idea:** let a world move forward across a breaking change, to a newer compatibility line — upgrade its file (block-registry aliases and
upgrade rules, and a placeholder that keeps unknown block states, all deferred from Phase 8), and
decide what happens to terrain generated by an older generator (keep generated chunks as saved,
regenerate untouched areas, or blend at the border).

**What keeps it open:** the world file records both versions (created, last opened); the block
registry stores states as strings (Phase 8); generator versions are recorded per
world; the newest build of every line in use stays published, so a world can always be opened.


## Game modes: adventure and creative

Dwell has no game modes yet; the owner wants two, adventure and creative (2026-10-19). A mode is a
world (or player) setting that gates abilities. The first known use: creative allows the
VoxelSniper-style brushes (`docs/BRUSHES.md` §5, question 2) in terrain worlds, which today only the
sandbox allows. Allowing brushes for everyone in creative is acceptable if a finer permission is
logically complicated. Needs an ADR (modes as data, per world or per player, what each gates)
before it is built.
