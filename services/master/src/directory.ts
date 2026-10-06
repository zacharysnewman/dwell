// The Directory Durable Object (ARCHITECTURE.md §10.3, ADR 0013): one instance ("global") holding
// the master's shared state: its SQLite schema (with migrations), the rate limits of signed
// requests (5b), and the registered dedicated servers with their stable join codes and the friend
// worlds shown to their own network (5d), and the lobby list's public friend worlds and join
// receipts (5e).
import { DurableObject } from 'cloudflare:workers';
import { formatCode, newCode } from './codes';
import type { Env } from './env';
import { JOIN_LIMIT, LIST_LIMIT, RateLimiter, SIGNED_LIMITS } from './rateLimit';
import {
  byPlayersThenName,
  matchesQuery,
  MAX_LIST,
  type ListQuery,
  type ServerReport,
} from './servers';

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
  // 5e: friend worlds listed to their network or, when public, in the lobby list too.
  `ALTER TABLE nearby_rooms RENAME TO listed_rooms`,
  `ALTER TABLE listed_rooms ADD COLUMN public INTEGER NOT NULL DEFAULT 0`,
  `ALTER TABLE listed_rooms ADD COLUMN protocol INTEGER`,
  // 5e: a player's latest join of a server through the master (resolving its code), which
  // entitles it to post one receipt for that server.
  `CREATE TABLE resolutions (player_key TEXT NOT NULL, server_key TEXT NOT NULL,
     at INTEGER NOT NULL, PRIMARY KEY (player_key, server_key))`,
  // 5e: join receipts — a player says it joined a server; distinct recent ones verify it.
  `CREATE TABLE receipts (server_key TEXT NOT NULL, player_key TEXT NOT NULL,
     at INTEGER NOT NULL, PRIMARY KEY (server_key, player_key))`,
  // Phase 6: friend worlds carry the host's app version (RELEASES.md §7).
  `ALTER TABLE listed_rooms ADD COLUMN app_version TEXT`,
];

/** A public server is verified once this many distinct players joined it within the window. */
export const VERIFIED_PLAYERS = 2;
export const RECEIPT_WINDOW_MS = 7 * 24 * 60 * 60 * 1000;
/** A receipt counts only this soon after the player resolved the server's code or address. */
export const RECEIPT_AFTER_RESOLVE_MS = 10 * 60 * 1000;

/** A registered server as players see it: how to reach it, and what it is. */
export interface ServerEntry {
  code: string;
  display: string;
  name: string;
  motd: string;
  players: number;
  maxPlayers: number;
  protocol: number;
  /** The server's app version (RELEASES.md §7); null if it predates versioned releases. */
  appVersion: string | null;
  /** Where to connect: its LAN address for a player on its network, else its public one. */
  host: string;
  port: number;
  cert: string;
  rtcPort: number | null;
  ice: string | null;
  tags: string[];
}

/** A public server in the lobby list: verified once distinct players have joined it (5e). */
export interface ListedServer extends ServerEntry {
  verified: boolean;
}

/** A public friend world in the lobby list, before its room adds the player count. */
export interface ListedRoom {
  code: string;
  display: string;
  name: string;
  protocol: number | null;
  appVersion: string | null;
}

export interface NearbyRoom {
  code: string;
  display: string;
  name: string;
  appVersion: string | null;
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

  /** Admits a lobby-list query from `ip` (LIST_LIMIT), or says how long to wait. */
  admitList(ip: string, now: number): Admission {
    if (this.limits.take(`l:${ip}`, LIST_LIMIT, now)) return { ok: true };
    return { ok: false, retryAfterS: Math.ceil(1 / LIST_LIMIT.perSecond) };
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

  /**
   * Lists a friend world to its host's network ("code + same network" visibility) and, when
   * public, in the lobby list too.
   */
  addListedRoom(
    code: string,
    publicIp: string,
    name: string,
    listing: { public: boolean; protocol: number | null; appVersion: string | null },
    now: number,
  ): void {
    this.ctx.storage.sql.exec(
      `INSERT OR REPLACE INTO listed_rooms (code, public_ip, name, created, public, protocol, app_version)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
      code,
      publicIp,
      name,
      now,
      listing.public ? 1 : 0,
      listing.protocol,
      listing.appVersion,
    );
  }

  /** Friend worlds listed to this network (the caller checks each room is still open). */
  nearbyRooms(requesterIp: string, now: number): NearbyRoom[] {
    return this.ctx.storage.sql
      .exec<{ code: string; name: string; app_version: string | null }>(
        `SELECT code, name, app_version FROM listed_rooms WHERE public_ip = ? AND created > ?
         ORDER BY created`,
        requesterIp,
        now - NEARBY_ROOM_MAX_MS,
      )
      .toArray()
      .map((r) => ({
        code: r.code,
        display: formatCode(r.code),
        name: r.name,
        appVersion: r.app_version,
      }));
  }

  /** Public friend worlds (the caller checks each room is still open and counts its players). */
  publicRooms(now: number): ListedRoom[] {
    return this.ctx.storage.sql
      .exec<{ code: string; name: string; protocol: number | null; app_version: string | null }>(
        `SELECT code, name, protocol, app_version FROM listed_rooms WHERE public = 1 AND created > ?
         ORDER BY created LIMIT ?`,
        now - NEARBY_ROOM_MAX_MS,
        MAX_LIST * 2,
      )
      .toArray()
      .map((r) => ({
        code: r.code,
        display: formatCode(r.code),
        name: r.name,
        protocol: r.protocol,
        appVersion: r.app_version,
      }));
  }

  removeListedRoom(code: string): void {
    this.ctx.storage.sql.exec(`DELETE FROM listed_rooms WHERE code = ?`, code);
  }

  // --- the lobby list (5e) -------------------------------------------------------------------

  /**
   * Public servers matching the query, as seen from `requesterIp`: verified ones, or with
   * `query.fresh` the unverified ("new") ones.
   */
  listServers(query: ListQuery, requesterIp: string, now: number): ListedServer[] {
    const sql = this.ctx.storage.sql;
    const verified = new Set(
      sql
        .exec<{ server_key: string }>(
          `SELECT server_key FROM receipts WHERE at > ? GROUP BY server_key HAVING COUNT(*) >= ?`,
          now - RECEIPT_WINDOW_MS,
          VERIFIED_PLAYERS,
        )
        .toArray()
        .map((r) => r.server_key),
    );
    const listed: ListedServer[] = [];
    for (const row of sql
      .exec<ServerRow>(`SELECT * FROM servers WHERE expires > ?`, now)
      .toArray()) {
      const report = JSON.parse(row.report) as ServerReport;
      if (report.visibility !== 'public') continue;
      const isVerified = verified.has(row.key);
      if (isVerified === query.fresh) continue;
      const e = entry(row, this.codeOf(row.key), requesterIp);
      if (!matchesQuery(e, query)) continue;
      listed.push({ ...e, verified: isVerified });
    }
    return listed.sort(byPlayersThenName).slice(0, query.limit);
  }

  /** A player resolved a server's code or address: it may post a receipt for it soon. */
  noteResolution(playerKey: string, serverCode: string, now: number): void {
    const sql = this.ctx.storage.sql;
    const server = sql
      .exec<{ key: string }>(`SELECT key FROM server_codes WHERE code = ?`, serverCode)
      .toArray()[0];
    if (!server || server.key === playerKey) return;
    sql.exec(`DELETE FROM resolutions WHERE at <= ?`, now - RECEIPT_AFTER_RESOLVE_MS);
    sql.exec(
      `INSERT OR REPLACE INTO resolutions (player_key, server_key, at) VALUES (?, ?, ?)`,
      playerKey,
      server.key,
      now,
    );
  }

  /**
   * A player's receipt for joining the server with this code (ADR 0013: player-attested
   * reachability). Counted only after the player resolved that server through the master; one
   * per player and server (a later one refreshes it). Answers whether the server is now verified.
   */
  addReceipt(
    playerKey: string,
    serverCode: string,
    now: number,
  ): { ok: true; verified: boolean } | { ok: false } {
    const sql = this.ctx.storage.sql;
    const server = sql
      .exec<{ key: string }>(`SELECT key FROM server_codes WHERE code = ?`, serverCode)
      .toArray()[0];
    if (!server) return { ok: false };
    const resolved = sql
      .exec(
        `DELETE FROM resolutions WHERE player_key = ? AND server_key = ? AND at > ? RETURNING 1`,
        playerKey,
        server.key,
        now - RECEIPT_AFTER_RESOLVE_MS,
      )
      .toArray();
    if (resolved.length === 0) return { ok: false };
    sql.exec(`DELETE FROM receipts WHERE at <= ?`, now - RECEIPT_WINDOW_MS);
    sql.exec(
      `INSERT OR REPLACE INTO receipts (server_key, player_key, at) VALUES (?, ?, ?)`,
      server.key,
      playerKey,
      now,
    );
    const count = sql
      .exec<{ n: number }>(
        `SELECT COUNT(*) AS n FROM receipts WHERE server_key = ? AND at > ?`,
        server.key,
        now - RECEIPT_WINDOW_MS,
      )
      .one().n;
    return { ok: true, verified: count >= VERIFIED_PLAYERS };
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

  /**
   * Deletes expired server records, old room listings, receipts and resolutions; runs again
   * while servers remain.
   */
  override async alarm(): Promise<void> {
    const now = Date.now();
    const sql = this.ctx.storage.sql;
    sql.exec(`DELETE FROM servers WHERE expires <= ?`, now);
    sql.exec(`DELETE FROM listed_rooms WHERE created <= ?`, now - NEARBY_ROOM_MAX_MS);
    sql.exec(`DELETE FROM receipts WHERE at <= ?`, now - RECEIPT_WINDOW_MS);
    sql.exec(`DELETE FROM resolutions WHERE at <= ?`, now - RECEIPT_AFTER_RESOLVE_MS);
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
    appVersion: r.appVersion ?? null, // reports from before versioned releases have none
    host: local ?? row.advertise ?? row.public_ip,
    port: r.port,
    cert: r.cert,
    rtcPort: r.rtcPort,
    ice: r.ice,
    tags: Array.isArray(r.tags) ? r.tags : [], // reports from before 5e have none
  };
}
