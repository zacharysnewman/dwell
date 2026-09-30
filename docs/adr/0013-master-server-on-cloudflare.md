# 0013. Master server on Cloudflare Workers + Durable Objects; Cloudflare TURN

- Status: Accepted
- Date: 2026-09-30
- Resolves: ARCHITECTURE.md Open Decisions #10 (master server platform, database, hostname) and
  #11 (TURN relay)

## Context

ADR 0003 makes the master server required infrastructure for join codes, friend-world signaling,
TURN credentials, cert-hash distribution and the public listing. The project is web-first: the
first multiplayer players are on the GitHub Pages site, on desktop and mobile browsers, where a
page cannot accept inbound connections, so hosting from a browser depends on signaling and, on
mobile data, a TURN relay. The constraint is **$0 during development**, with no servers to
maintain. The master carries no game traffic: HTTPS JSON plus a WebSocket per hosting session.

## Options considered

1. **Cloudflare Workers + Durable Objects.** No servers to run; free plan includes Workers and
   SQLite-backed Durable Objects; Durable Objects give strongly consistent state and WebSocket
   rooms (the Hibernation API keeps idle rooms free); Cloudflare runs a managed TURN service;
   `wrangler dev` runs everything locally for development and CI. Cons: no outbound UDP, so the
   master cannot probe a dedicated server's QUIC/WebRTC ports — reachability must be attested by
   players; vendor-specific runtime.
2. **Free-tier VM + Rust service + `coturn`.** UDP probes for reachability, one language with the
   server's network crate, self-hosted TURN. Cons: a machine to patch and monitor, free tiers with
   tight bandwidth (TURN relays game traffic), and more setup before anything works.
3. **Workers + D1** instead of Durable Object storage. D1 suits relational queries, but signaling
   needs Durable Objects anyway, and one storage kind is simpler.

## Decision

- The master server (`services/master`) is a **TypeScript Cloudflare Worker** with two
  **SQLite-backed Durable Object** classes: `Directory` (a single instance holding registered
  dedicated servers, join codes, join receipts and rate-limit state; heartbeat expiry via alarms)
  and `Room` (one per hosted friend world: WebSocket signaling between the host and its guests).
  No D1. It runs on the **Workers Free** plan.
- **Hostname:** the account's `workers.dev` subdomain (`dwell-master.<subdomain>.workers.dev`)
  at first. The URL is a build setting of the client (`VITE_MASTER_URL`) and a flag of
  `dwell_server` (`--master`), so moving to `master.dropkickarcade.com` later (a custom domain,
  once the zone is on Cloudflare) is a configuration change.
- **Requests are signed** with Ed25519 — the player's device key (ADR 0004) or the dedicated
  server's own key — so rate limits, receipts and registrations are keyed by public key as well
  as by IP.
- **Reachability is player-attested:** after a successful join made through the master, the
  client posts a signed receipt; a public server is listed as verified once distinct players
  have joined it recently.
- **TURN:** Cloudflare's managed TURN service; the master mints short-lived credentials for
  signed, rate-limited requests. STUN only when no TURN key is configured (development, CI).
- **Same-network discovery:** the master sees each request's public IP, so it can list servers and
  friend worlds that share the requester's public IP ("On your network") and resolve LAN
  addresses among them — LAN discovery for browsers, which cannot scan the local network.

## Consequences

- Browser hosting, join codes, join by address and the lobby list need no infrastructure beyond a
  Cloudflare account, with no running cost within free limits; over-limit requests fail rather
  than bill (no payment method on the account).
- Account setup (API token, TURN key, secrets) is manual and documented in the implementation
  plan (Phase 5).
- A new public server is unverified until players join it; unverified servers are shown only under
  a "new" filter.
- The master is a trusted party for friend-world signaling: it relays the host's DTLS fingerprint,
  so a compromised master could intercept friend-world sessions (not dedicated servers joined by
  invite link). Accepted for now; accounts (ADR 0004) could later sign host fingerprints.
- Leaving Cloudflare means porting the Worker (plain `fetch` handlers) and replacing Durable
  Objects with a stateful service; the HTTP API and client code stay the same.
