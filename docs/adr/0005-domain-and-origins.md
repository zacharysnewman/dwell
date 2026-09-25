# 0005. Domain and web origins: `dwell.dropkickarcade.com`

- Status: Accepted
- Date: 2026-09-25

## Context

The GitHub Pages user site `zacharysnewman.github.io` uses the custom domain
`dropkickarcade.com`. A project site in this repo would therefore default to
`https://dropkickarcade.com/dwell/`, i.e. the **same origin** as every other game published
there.

The browser origin matters to Dwell because it scopes:
- IndexedDB / OPFS / localStorage — local-mode world saves and the player's **device key**
  (ADR 0004). A shared origin lets any other page on `dropkickarcade.com` read Dwell's saves and
  *use* its non-extractable key.
- Storage quota, service workers, and permissions, all shared across the games on that origin.
- Future passkeys and OAuth redirect URIs, which are bound to a domain and cannot move later.

Moving origins after launch strands browser-stored saves and keys, so this is settled up front.

## Decision

Serve the Dwell client from its own origin: **`https://dwell.dropkickarcade.com/`**.

- This repository's Pages site sets the custom domain `dwell.dropkickarcade.com` (a `CNAME` file
  in the Pages artifact). DNS: `CNAME dwell → zacharysnewman.github.io`. Pages provides HTTPS.
- Vite `base` becomes `'/'` (was `'/dwell/'`). Old client versions live at `/v/<version>/`
  (ADR 0003).
- Related hostnames, when needed:
  - `api.dwell.dropkickarcade.com` — master server (ADR 0003).
  - `turn.dwell.dropkickarcade.com` — TURN relay.
  - `*.servers.dwell.dropkickarcade.com` — optional master-issued hostnames with trusted
    certificates for player servers (follow-up in ADR 0003).
- Until DNS is in place, the client can still be built and served at the default Pages URL for
  development; nothing is stored in browsers against that origin intentionally.

## Consequences

- Dwell's browser storage and keys are isolated from other dropkickarcade.com games.
- One DNS record is needed now; the others come with the master server and relay.
- Master-issued server hostnames need programmatic DNS (ACME DNS-01) on the zone; the DNS
  provider is an open decision for that follow-up.
