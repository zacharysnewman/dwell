# Dwell master server

The master server (ARCHITECTURE.md §10.3, ADR 0013): a Cloudflare Worker with SQLite-backed
Durable Objects, on the Workers Free plan. It carries no game traffic. It holds friend worlds'
join codes and signaling and hands out TURN credentials (Phase 5c); registered dedicated servers
and the lobby list follow (Phase 5d–5e).

| Path               | What                                                                 |
| ------------------ | -------------------------------------------------------------------- |
| `src/index.ts`     | Routes under `/v1/`, CORS, signed-request checks                     |
| `src/auth.ts`      | Ed25519 request signatures (the format is in the file's header)      |
| `src/directory.ts` | `Directory` Durable Object: schema migrations, rate limits           |
| `src/room.ts`      | `Room` Durable Object: one per friend world, named by its join code  |
| `src/codes.ts`     | Join codes: alphabet, parsing, display                               |
| `src/turn.ts`      | ICE servers from Cloudflare's TURN service (STUN without a key)      |
| `wrangler.jsonc`   | Worker, bindings, Durable Object migrations, allowed browser origins |

API so far (details in ARCHITECTURE.md §10.3):

- `GET /v1/health`; `POST /v1/whoami` (signed; answers with the key that signed, to check a
  client's signing and clock).
- `POST /v1/rooms` (signed, `{ maxGuests }`) → `201 { code, display, hostToken }`: a host opens a
  friend world's room.
- `POST /v1/rooms/<code>/join` (signed, rate-limited per IP) → `{ token, peer }`, `404` or
  `409 full`.
- `GET /v1/rooms/<code>/ws?token=…`: the room's signaling WebSocket, for the host or a guest.
- `POST /v1/turn` (signed) → `{ iceServers }`.

## Develop

```sh
npm ci
npm run dev            # http://localhost:8787 (Durable Objects included, state in .wrangler/)
npm test               # Vitest inside the Workers runtime (@cloudflare/vitest-pool-workers)
npm run lint && npm run typecheck && npm run format:check
```

Point a client at the local master with `?master=http://localhost:8787`. After changing the
signing format, regenerate the shared vectors with `node shared/master/make_vectors.mjs` (from the
repository root); the client's and the master's tests both check them.

`.npmrc` sets `legacy-peer-deps`: npm's resolver otherwise crashes on optional peers in the
Vitest/Vite tree.

## Deploy

`.github/workflows/master.yml` runs `wrangler deploy` on pushes to `main` that touch this
directory, once the account is set up (the manual steps in `docs/IMPLEMENTATION_PLAN.md`,
Phase 5): repository secrets `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID`, and the
repository variable `VITE_MASTER_URL` (also read by the Pages build). Without the secrets the
workflow skips the deploy with a notice.

- Worker URL: `https://dwell-master.dropkick.workers.dev` (the account's `workers.dev`
  subdomain is `dropkick`); `GET /v1/health` checks it.
- TURN: without the Worker secrets `TURN_KEY_ID` and `TURN_KEY_API_TOKEN` the master hands out
  STUN only, which connects most home networks but not every mobile one. Create a TURN key in the
  Cloudflare dashboard (Realtime → TURN Server) and set both secrets on the Worker
  (`npx wrangler secret put TURN_KEY_ID`, then `TURN_KEY_API_TOKEN`, here). Cloudflare's TURN
  service includes a free monthly allowance of relayed traffic (see its pricing page).
