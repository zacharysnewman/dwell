# 0003. Multiplayer hosting model: player-hosted servers, friend worlds, and a master server

- Status: Accepted
- Date: 2026-09-25
- Resolves: ARCHITECTURE.md Open Decisions #3 (server hosting)

## Context

Dwell will not run official game servers. Like Minecraft, worlds are hosted by players. Browsers
constrain how that can work: pages cannot accept inbound connections, WebTransport needs a
trusted certificate or a pinned certificate hash (≤14-day self-signed ECDSA certs), and the
WebSocket fallback from an HTTPS page needs a trusted certificate. Home and mobile hosts sit
behind NATs, often carrier-grade NAT on mobile data.

Reference model: Minecraft Java (dedicated `server.jar`, direct connect by address, integrated
server + "Open to LAN", no official directory) and Minecraft Bedrock (any device, phones
included, hosts a friend world of about 8 players, joined over LAN or through Xbox network
services with NAT traversal and relays; the world lives only while the host plays).

## Options considered

1. **Official hosted servers only** (VPS / cloud). Rejected: not the intended model.
2. **Dedicated player servers only, direct connect.** Minecraft Java model. Works for desktop
   hosts, but phones and browsers cannot host, and rotating certificate hashes make bare
   addresses insufficient.
3. **Two tiers plus a master server** (chosen): dedicated servers *and* friend worlds hosted by
   any client, brokered by a small master server.

## Decision

Two hosting tiers, one protocol, one sim core:

| | Friend worlds (Bedrock-style) | Dedicated servers (Java-style) |
|---|---|---|
| Host | Any client — browser, phone (Capacitor), desktop — running the sim core as its integrated server | Native server binary (Windows/macOS/Linux), Docker image, or Electron "Host world" |
| Transport | **WebRTC data channels** (unordered/unreliable ↔ datagrams, ordered/reliable ↔ streams); STUN, TURN relay fallback | **WebTransport** (WebSocket fallback) |
| Join | Join code / invite via master server (signaling) | Server browser, join code, or invite link |
| Lifetime | While the host plays; pauses when the host backgrounds | Always on |
| Scale | Small (host profile, e.g. 4–8 players, reduced physics caps) | Large, host-configured |

**Master server** (`api.dwell.dropkickarcade.com`, ADR 0005): a small HTTPS service, no game
traffic.
- Dedicated servers register and heartbeat (~30 s): address, port, current certificate hash,
  name, MOTD, player count, version, tags, visibility (public/unlisted). Clients get address +
  cert hash from it, so self-signed certificates can rotate transparently.
- Public server listing (in-game browser). Listing requires a reachability check from the master.
- Short **join codes** for unlisted servers and friend worlds.
- **WebRTC signaling** for friend worlds, and issuing short-lived TURN credentials.
- Direct invite links (`?join=host:port&cert=<sha256>`) keep working with no master server.

**Integrated server.** Local single-player is already the sim core in a worker (local mode);
friend-world hosting exposes that same integrated server over WebRTC. Electron can alternatively
run the native server binary for better performance ("Host world").

**Operators.** Dedicated servers ship with a config file (max players, visibility, password /
allow-list, online/offline mode per ADR 0004, physics and view-distance caps), admin commands,
UPnP/NAT-PMP port mapping with port-forwarding guidance, and world backups.

**Versioning.** The handshake carries the protocol version; incompatible servers are shown as
such in the browser. Old client builds are kept at versioned paths on GitHub Pages
(`/v/<version>/`) so players can join servers that have not updated.

## Consequences

- The master server becomes required infrastructure for public listing, join codes, and friend
  worlds (not for direct invite links). It is small and HTTP-only; its platform is an open
  decision (leaning Cloudflare Workers + a small database).
- TURN relay bandwidth is a running cost; credentials are only issued to identified players
  (ADR 0004) and are rate-limited.
- Transports grow to four: WebTransport, WebSocket, WebRTC, Loopback — all behind the same
  `Transport` interface; the protocol above it is unchanged.
- Safari/iOS web players without WebTransport can always join friend worlds (WebRTC) but can only
  join dedicated servers that have a trusted certificate — unless dedicated servers also accept
  WebRTC (possible later via a Rust WebRTC stack in `server/net`) or master-issued hostnames with
  trusted certificates are added. Both are follow-ups.
- World persistence (Open Decisions #5) is now required, and saves must be portable between
  friend worlds (browser/app storage) and dedicated servers (disk).
- No official hosting; a Realms-like paid official tier can be added later with the same Docker
  image.
