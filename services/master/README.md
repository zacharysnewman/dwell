# Dwell master server

The master server (ARCHITECTURE.md §10.3, ADR 0013): a Cloudflare Worker with SQLite-backed
Durable Objects, on the Workers Free plan. It carries no game traffic; it will hold join codes,
friend-world signaling, TURN credentials, registered dedicated servers and the lobby list
(Phase 5c–5e).

| Path               | What                                                                 |
| ------------------ | -------------------------------------------------------------------- |
| `src/index.ts`     | Routes under `/v1/`, CORS, signed-request checks                     |
| `src/auth.ts`      | Ed25519 request signatures (the format is in the file's header)      |
| `src/directory.ts` | `Directory` Durable Object: schema migrations, rate limits           |
| `src/room.ts`      | `Room` Durable Object: friend-world signaling (Phase 5c)             |
| `wrangler.jsonc`   | Worker, bindings, Durable Object migrations, allowed browser origins |

API so far: `GET /v1/health`, and `POST /v1/whoami` (signed; answers with the key that signed,
to check a client's signing and clock).

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

- Worker URL: `https://dwell-master.<workers.dev subdomain>.workers.dev` — record it here once
  chosen.
- Free TURN allowance: record it here when the TURN key is created (Phase 5c).
