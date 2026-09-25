# 0005. Domain and web origin: the default Pages path `dropkickarcade.com/dwell/`

- Status: Accepted
- Date: 2026-09-25

## Context

The GitHub Pages user site `zacharysnewman.github.io` uses the custom domain
`dropkickarcade.com`. This repository's project site is therefore served by default at
`https://dropkickarcade.com/dwell/`, the **same origin** as the other games published there.

The origin scopes browser storage (IndexedDB / OPFS / localStorage), storage quota, service
workers, and permissions — and in Dwell that storage holds local-mode world saves and the
player's device key (ADR 0004). Passkeys and OAuth redirect URIs (for later accounts) are also
bound to the domain.

## Options considered

1. **Default project path `dropkickarcade.com/dwell/`** — no DNS or Pages configuration; shares
   the origin with the other games.
2. **Own subdomain `dwell.dropkickarcade.com`** — isolates storage and keys; needs a DNS record
   and a Pages custom domain for this repo.

## Decision

Use the **default project path**: `https://dropkickarcade.com/dwell/`. Vite `base: '/dwell/'`.
Older client builds live at `/dwell/v/<version>/` (ADR 0003).

Because the origin is shared, Dwell's client:
- namespaces everything it stores (IndexedDB database names, OPFS directories, localStorage
  keys) under a `dwell` prefix, and never reads or clears other games' data;
- keeps the device key **non-extractable** (WebCrypto `CryptoKey`), so other pages on the origin
  cannot copy it (they could still invoke it, which is accepted for now);
- scopes any service worker to `/dwell/`;
- requests persistent storage (`navigator.storage.persist()`) and keeps its footprint visible
  to the player, since quota is shared.

Service hostnames (master server, TURN relay) are chosen when those services are built
(Phase 7); they do not affect the client origin.

## Future option

Moving the client to its own subdomain (`dwell.dropkickarcade.com`) remains a planned option,
e.g. before accounts/passkeys ship. Browser-stored data does not follow an origin change, so the
move would ship with a migration path: the existing world export/import and device-key
export/import, surfaced as a one-time "move your data" flow on the old path.

## Consequences

- Zero setup: the site is live at the default URL as soon as Pages deploys.
- Storage, quota, and key usage are shared with other dropkickarcade.com games; mitigated by the
  rules above.
- A later origin move costs a migration flow; passkey-based accounts should wait for that
  decision.
