// The Directory Durable Object (ARCHITECTURE.md §10.3, ADR 0013): one instance ("global") holding
// the master's shared state. Phase 5b: its SQLite schema (with migrations) and the rate limits of
// signed requests. Join codes (5c), registered dedicated servers (5d) and join receipts (5e) are
// added as schema migrations.
import { DurableObject } from 'cloudflare:workers';
import type { Env } from './env';
import { JOIN_LIMIT, RateLimiter, SIGNED_LIMITS } from './rateLimit';

/** Schema migrations, applied in order; the index + 1 is the schema version. */
export const MIGRATIONS: readonly string[] = [
  `CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)`,
];

export type Admission = { ok: true } | { ok: false; retryAfterS: number };

export class Directory extends DurableObject<Env> {
  /** In memory: limits restart from full if the object is evicted, which errs on allowing. */
  private readonly limits = new RateLimiter();

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    void ctx.blockConcurrencyWhile(() => {
      this.migrate();
      return Promise.resolve();
    });
  }

  private migrate(): void {
    const sql = this.ctx.storage.sql;
    sql.exec(MIGRATIONS[0] ?? '');
    const row = sql
      .exec<{ value: string }>(`SELECT value FROM meta WHERE key = 'schema'`)
      .toArray();
    let version = row[0] ? Number(row[0].value) : 0;
    for (; version < MIGRATIONS.length; version++) sql.exec(MIGRATIONS[version] ?? '');
    sql.exec(
      `INSERT INTO meta (key, value) VALUES ('schema', ?)
       ON CONFLICT(key) DO UPDATE SET value = excluded.value`,
      String(version),
    );
  }

  /** The schema version in storage (tests, health). */
  schemaVersion(): number {
    const row = this.ctx.storage.sql
      .exec<{ value: string }>(`SELECT value FROM meta WHERE key = 'schema'`)
      .one();
    return Number(row.value);
  }

  /** Admits a join-code lookup from `ip` (JOIN_LIMIT), or says how long to wait. */
  admitJoin(ip: string, now: number): Admission {
    if (this.limits.take(`j:${ip}`, JOIN_LIMIT, now)) return { ok: true };
    return { ok: false, retryAfterS: Math.ceil(1 / JOIN_LIMIT.perSecond) };
  }

  /** Admits a signed request from `key` at `ip`, or says how long to wait. */
  admit(key: string, ip: string, now: number): Admission {
    const byKey = this.limits.take(`k:${key}`, SIGNED_LIMITS.key, now);
    const byIp = this.limits.take(`i:${ip}`, SIGNED_LIMITS.ip, now);
    if (byKey && byIp) return { ok: true };
    const spec = byKey ? SIGNED_LIMITS.ip : SIGNED_LIMITS.key;
    return { ok: false, retryAfterS: Math.ceil(1 / spec.perSecond) };
  }
}
