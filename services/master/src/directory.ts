// The Directory Durable Object (ARCHITECTURE.md §10.3, ADR 0013): one instance ("global") holding
// the master's shared state: its SQLite schema (with migrations), the rate limits of signed
// requests (5b), and the registered dedicated servers with their stable join codes and the friend
// worlds shown to their own network (5d). Join receipts (5e) come as a later migration.
import { DurableObject } from 'cloudflare:workers';
import { formatCode, newCode } from './codes';
import type { Env } from './env';
import { JOIN_LIMIT, RateLimiter, SIGNED_LIMITS } from './rateLimit';
import type { ServerReport } from './servers';

/** Schema migrations, applied in order; the index + 1 is the schema version. */
export const MIGRATIONS: readonly string[] = [
  `CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)`,
  // 5d: a dedicated server's join code, kept while its record comes and goes (stable per key).
  `CREATE TABLE server_codes (key TEXT PRIMARY KEY, code TEXT NOT NULL UNIQUE)`,
  // 5d: registered servers; a record lives until two heartbeats are missed (expires, ms).
  `CREATE TABLE servers (key TEXT PRIMARY KEY, public_ip TEXT NOT NULL, advertise TEXT,
     report TEXT NOT NULL, expires INTEGER NOT NULL)`,
  `CREATE INDEX servers_public_ip ON servers (public_ip)`,
  `CREATE INDEX servers_advertise ON servers (advertise)`,
  // 5d: friend worlds hosted with "code + same network" visibility, by the host's public IP.
  `CREATE TABLE nearby_rooms (code TEXT PRIMARY KEY, public_ip TEXT NOT NULL, name TEXT NOT NULL,
     created INTEGER NOT NULL)`,
  `CREATE INDEX nearby_rooms_public_ip ON nearby_rooms (public_ip)`,
];

/** A registered server as players see it: how to reach it, and what it is. */
export interface ServerEntry {
  code: string;
  display: string;
  name: string;
  motd: string;
  players: number;
  maxPlayers: number;
  protocol: number;
  /** Where to connect: its LAN address for a player on its network, else its public one. */
  host: string;
  port: number;
  cert: string;
  rtcPort: number | null;
  ice: string | null;
}

export interface NearbyRoom {
  code: string;
  display: string;
  name: string;
}

interface ServerRow extends Record<string, SqlStorageValue> {
  key: string;
  public_ip: string;
  advertise: string | null;
  report: string;
  expires: number;
}

/** Friend worlds listed on their network stay at most this long without being seen open. */
const NEARBY_ROOM_MAX_MS = 24 * 60 * 60 * 1000;

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

  // --- dedicated servers (5d) ----------------------------------------------------------------

  /**
   * Registers a server or records its heartbeat: its report and public IP, until two heartbeats
   * are missed. Answers with its join code, the same every time for the same key.
   */
  async registerServer(
    key: string,
    publicIp: string,
    report: ServerReport,
    now: number,
  ): Promise<{ code: string; display: string }> {
    const sql = this.ctx.storage.sql;
    let code = sql
      .exec<{ code: string }>(`SELECT code FROM server_codes WHERE key = ?`, key)
      .toArray()[0]?.code;
    for (let attempt = 0; !code && attempt < 8; attempt++) {
      const candidate = newCode();
      const taken =
        sql.exec(`SELECT 1 FROM server_codes WHERE code = ?`, candidate).toArray().length > 0 ||
        (await this.env.ROOM.get(this.env.ROOM.idFromName(candidate)).isOpen());
      if (!taken) {
        sql.exec(`INSERT INTO server_codes (key, code) VALUES (?, ?)`, key, candidate);
        code = candidate;
      }
    }
    if (!code) throw new Error('no free join code');
    const expires = now + report.heartbeatS * 2 * 1000;
    sql.exec(
      `INSERT INTO servers (key, public_ip, advertise, report, expires) VALUES (?, ?, ?, ?, ?)
       ON CONFLICT(key) DO UPDATE SET public_ip = excluded.public_ip,
         advertise = excluded.advertise, report = excluded.report, expires = excluded.expires`,
      key,
      publicIp,
      report.advertise,
      JSON.stringify(report),
      expires,
    );
    await this.scheduleCleanup(expires);
    return { code, display: formatCode(code) };
  }

  /** A server shutting down: gone at once (its code stays its own). */
  leaveServer(key: string): void {
    this.ctx.storage.sql.exec(`DELETE FROM servers WHERE key = ?`, key);
  }

  /** Whether a join code belongs to a dedicated server (friend-world rooms must avoid them). */
  isServerCode(code: string): boolean {
    return (
      this.ctx.storage.sql.exec(`SELECT 1 FROM server_codes WHERE code = ?`, code).toArray()
        .length > 0
    );
  }

  /** The live server with this join code, as seen from `requesterIp`. */
  serverByCode(code: string, requesterIp: string, now: number): ServerEntry | null {
    const row = this.ctx.storage.sql
      .exec<ServerRow>(
        `SELECT s.* FROM servers s JOIN server_codes c ON c.key = s.key
         WHERE c.code = ? AND s.expires > ?`,
        code,
        now,
      )
      .toArray()[0];
    return row ? entry(row, code, requesterIp) : null;
  }

  /**
   * The live server at a typed address: its public IP or advertised host, or — for a player on
   * its network (same public IP) — one of its LAN addresses. Without a port, the default
   * WebTransport port 4433 is preferred.
   */
  serverByAddress(
    host: string,
    port: number | null,
    requesterIp: string,
    now: number,
  ): ServerEntry | null {
    const rows = this.ctx.storage.sql
      .exec<ServerRow>(
        `SELECT * FROM servers WHERE expires > ? AND (public_ip = ? OR advertise = ? OR public_ip = ?)`,
        now,
        host,
        host,
        requesterIp,
      )
      .toArray();
    const matches: { row: ServerRow; report: ServerReport; lan: boolean }[] = [];
    for (const row of rows) {
      const report = JSON.parse(row.report) as ServerReport;
      if (port !== null && report.port !== port) continue;
      if (row.advertise === host || (!row.advertise && row.public_ip === host)) {
        matches.push({ row, report, lan: false });
      } else if (row.public_ip === requesterIp && report.lan.includes(host)) {
        matches.push({ row, report, lan: true });
      }
    }
    matches.sort((a, b) => Number(b.report.port === 4433) - Number(a.report.port === 4433));
    const best = matches[0];
    if (!best) return null;
    const code = this.codeOf(best.row.key);
    const e = entry(best.row, code, requesterIp);
    return { ...e, host: best.lan ? host : e.host };
  }

  /** Servers on the requester's network (same public IP): LAN discovery for browsers. */
  nearbyServers(requesterIp: string, now: number): ServerEntry[] {
    return this.ctx.storage.sql
      .exec<ServerRow>(
        `SELECT * FROM servers WHERE public_ip = ? AND expires > ? ORDER BY key`,
        requesterIp,
        now,
      )
      .toArray()
      .map((row) => entry(row, this.codeOf(row.key), requesterIp));
  }

  /** Lists a friend world to its host's network ("code + same network" visibility). */
  addNearbyRoom(code: string, publicIp: string, name: string, now: number): void {
    this.ctx.storage.sql.exec(
      `INSERT OR REPLACE INTO nearby_rooms (code, public_ip, name, created) VALUES (?, ?, ?, ?)`,
      code,
      publicIp,
      name,
      now,
    );
  }

  /** Friend worlds listed to this network (the caller checks each room is still open). */
  nearbyRooms(requesterIp: string, now: number): NearbyRoom[] {
    return this.ctx.storage.sql
      .exec<{ code: string; name: string }>(
        `SELECT code, name FROM nearby_rooms WHERE public_ip = ? AND created > ? ORDER BY created`,
        requesterIp,
        now - NEARBY_ROOM_MAX_MS,
      )
      .toArray()
      .map((r) => ({ code: r.code, display: formatCode(r.code), name: r.name }));
  }

  removeNearbyRoom(code: string): void {
    this.ctx.storage.sql.exec(`DELETE FROM nearby_rooms WHERE code = ?`, code);
  }

  private codeOf(key: string): string {
    return this.ctx.storage.sql
      .exec<{ code: string }>(`SELECT code FROM server_codes WHERE key = ?`, key)
      .one().code;
  }

  /** Runs the cleanup alarm by `at` (at the latest), if none is set sooner. */
  private async scheduleCleanup(at: number): Promise<void> {
    const current = await this.ctx.storage.getAlarm();
    if (current === null || current > at) await this.ctx.storage.setAlarm(at);
  }

  /** Deletes expired server records and old room listings; runs again while servers remain. */
  override async alarm(): Promise<void> {
    const now = Date.now();
    const sql = this.ctx.storage.sql;
    sql.exec(`DELETE FROM servers WHERE expires <= ?`, now);
    sql.exec(`DELETE FROM nearby_rooms WHERE created <= ?`, now - NEARBY_ROOM_MAX_MS);
    const next = sql
      .exec<{ next: number | null }>(`SELECT MIN(expires) AS next FROM servers`)
      .one().next;
    if (next !== null) await this.ctx.storage.setAlarm(next);
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

/** A server row as a player at `requesterIp` sees it. */
function entry(row: ServerRow, code: string, requesterIp: string): ServerEntry {
  const r = JSON.parse(row.report) as ServerReport;
  const local = row.public_ip === requesterIp ? r.lan[0] : undefined;
  return {
    code,
    display: formatCode(code),
    name: r.name,
    motd: r.motd,
    players: r.players,
    maxPlayers: r.maxPlayers,
    protocol: r.protocol,
    host: local ?? row.advertise ?? row.public_ip,
    port: r.port,
    cert: r.cert,
    rtcPort: r.rtcPort,
    ice: r.ice,
  };
}
