# 0008. Dedicated-server transports: WebTransport primary, WebRTC fallback, no WebSocket

- Status: Accepted
- Date: 2026-09-25
- Resolves: ARCHITECTURE.md Open Decisions #13 and #16
- Supersedes in part: ADR 0001 (WebSocket fallback note), ADR 0003 (dedicated-server transport)

## Context

Dedicated servers speak WebTransport (ADR 0001). A fallback is needed for clients without
WebTransport (chiefly Safari / iOS web). The previously planned WebSocket fallback has two
problems:
- **No unreliable delivery:** inputs and snapshots queue behind lost packets (head-of-line
  blocking), so movement stutters on lossy links.
- **Trusted certificate required** from an HTTPS page: player hosts rarely have one, so Safari
  users still could not join most servers.

The client already needs a WebRTC transport for friend worlds (ADR 0003).

## Options considered

1. **WebTransport + WebRTC, no WebSocket.**
2. **WebTransport + WebSocket** (previous plan).
3. **All three.**

## Decision

Dedicated servers accept **WebTransport (primary)** and **WebRTC (fallback)**. **WebSocket is
removed** from the architecture.

- **Server side:** an ICE-lite WebRTC endpoint using **`str0m`** (sans-I/O Rust WebRTC library,
  driven by our own event loop) in the same crate as WebTransport (`server/net/wt`), behind the
  same C ABI and event queue. Channel mapping is identical to friend worlds: one unordered,
  `maxRetransmits: 0` data channel for datagrams; ordered reliable channels for `control` and
  `world`. Same UDP port as QUIC where practical (demultiplexed by packet type), otherwise a
  second UDP port.
- **Security:** DTLS with the server's self-signed certificate; its SHA-256 fingerprint is
  published exactly like the WebTransport cert hash (master heartbeat, invite link). No CA
  certificate needed.
- **Connecting via the master server:** standard WebRTC signaling through the master.
- **Connecting via invite link (no master):** the link carries address, port, cert fingerprint,
  and the server's ICE credentials (`?join=host:port&cert=<sha256>&ice=<ufrag>:<pwd>`); the
  client synthesizes the remote description locally for the ICE-lite server. This technique must
  be proven in the Phase 1 spike; if it fails, invite-link WebRTC joins go through the master.
- **NAT:** WebRTC lets servers behind strict NAT be reached through the TURN relay.
- **Client selection:** WebTransport when available, else WebRTC.

## Consequences

- Every browser (including iOS Safari) can join every dedicated server, with or without a trusted
  certificate; the platform reachability table has no gaps.
- Unreliable delivery is preserved on the fallback path.
- Master-issued trusted hostnames (Open Decisions #12) are no longer needed for reachability.
- One fewer transport to implement; the client's transports are WebTransport, WebRTC, and
  Loopback. Dedicated servers need only inbound UDP.
- WebRTC on the server adds complexity (ICE-lite, DTLS, SCTP via `str0m`) and a slightly longer
  connection setup than WebTransport.

## Implementation notes (2026-09-25, Phase 1)

- The invite-link path was proven: Chromium connects to the ICE-lite `str0m` endpoint with only
  the invite data. The browser's offer is created locally and the server's answer is synthesized
  from address, ICE credentials, and fingerprint; the server learns the client's ICE username
  from its first STUN request.
- DTLS reuses the WebTransport certificate, so the invite carries a single fingerprint (`cert`).
  Invite form: `?join=host:port&cert=<sha256>&rtc=<port>&ice=<ufrag>:<pwd>`.
- WebRTC runs on a **separate UDP port** (default: WebTransport port + 1) rather than
  demultiplexing one socket.
- `str0m` needs a remote fingerprint on record even with verification disabled; a placeholder is
  set.
- Chrome and Electron don't pair loopback candidates, so WebRTC tests against `127.0.0.1` only
  work in browsers that allow loopback (e.g. Playwright's Chromium); real servers advertise a
  routable IP (`--advertise`).
- Safari has not been tested yet (no Safari in the development environment).
