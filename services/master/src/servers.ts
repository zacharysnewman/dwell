// Dedicated servers on the master (ARCHITECTURE.md §10.1, §10.3, Phase 5d): what a server sends
// when it registers and heartbeats, checked here before the Directory stores it, and how a typed
// address is read. Pure functions, so the rules are tested on their own.

/** Heartbeat interval a server may ask for (s); its record expires after two missed ones. */
export const MIN_HEARTBEAT_S = 2;
export const MAX_HEARTBEAT_S = 60;
export const DEFAULT_HEARTBEAT_S = 30;
/** LAN addresses a server may report. */
export const MAX_LAN_ADDRESSES = 8;
export const MAX_NAME = 64;
export const MAX_MOTD = 256;
/** Tags a server may report (5e), each 1–24 of `a-z`, `0-9` and `-`. */
export const MAX_TAGS = 8;
export const MAX_TAG = 24;

export type ServerVisibility = 'public' | 'unlisted';

/** A registration or heartbeat, as validated. */
export interface ServerReport {
  port: number;
  rtcPort: number | null;
  /** WebRTC ICE-lite credentials "ufrag:pwd", or null (no WebRTC fallback). */
  ice: string | null;
  /** SHA-256 of the current certificate, 64 hex. */
  cert: string;
  /** The address players use, if the server advertises one; else its public IP. */
  advertise: string | null;
  lan: string[];
  name: string;
  motd: string;
  players: number;
  maxPlayers: number;
  protocol: number;
  visibility: ServerVisibility;
  /** Lower-case tags for the lobby list's search (5e). */
  tags: string[];
  heartbeatS: number;
}

const IPV4 = /^(25[0-5]|2[0-4]\d|1?\d?\d)(\.(25[0-5]|2[0-4]\d|1?\d?\d)){3}$/;

/** An IP literal (v4 dotted, or v6 hex groups), normalized to lower case; else null. */
export function ipLiteral(text: string): string | null {
  const t = text.trim().toLowerCase();
  if (IPV4.test(t)) return t;
  // IPv6 has at least two colons ("host:port" has one).
  if ((t.match(/:/g) ?? []).length >= 2 && /^[0-9a-f:.]+$/.test(t) && t.length <= 45) return t;
  return null;
}

/** A host name or IP a server advertises: an IP literal or a DNS name. */
function hostName(text: string): string | null {
  const t = text.trim().toLowerCase();
  if (ipLiteral(t)) return ipLiteral(t);
  return /^[a-z0-9]([a-z0-9-]{0,62})(\.[a-z0-9]([a-z0-9-]{0,62}))*$/.test(t) && t.length <= 253
    ? t
    : null;
}

const port = (v: unknown): number | null =>
  typeof v === 'number' && Number.isInteger(v) && v >= 1 && v <= 65535 ? v : null;
const count = (v: unknown, max: number): number =>
  typeof v === 'number' && Number.isFinite(v) ? Math.max(0, Math.min(max, Math.floor(v))) : 0;
const text = (v: unknown, max: number): string =>
  typeof v === 'string' ? v.replace(/\p{Cc}/gu, '').slice(0, max) : '';

/** A server's report from its JSON body, or an error message. */
export function parseServerReport(body: Record<string, unknown>): ServerReport | string {
  const p = port(body.port);
  if (p === null) return 'port must be 1–65535.';
  const cert = typeof body.cert === 'string' ? body.cert.toLowerCase() : '';
  if (!/^[0-9a-f]{64}$/.test(cert)) return 'cert must be the certificate SHA-256 in hex.';
  const rtcPort = body.rtcPort === undefined || body.rtcPort === null ? null : port(body.rtcPort);
  if (body.rtcPort !== undefined && body.rtcPort !== null && rtcPort === null) {
    return 'rtcPort must be 1–65535.';
  }
  let ice: string | null = null;
  if (typeof body.ice === 'string' && body.ice !== '') {
    if (!/^[A-Za-z0-9+/]{4,256}:[A-Za-z0-9+/]{22,256}$/.test(body.ice)) return 'ice is malformed.';
    ice = body.ice;
  }
  let advertise: string | null = null;
  if (typeof body.advertise === 'string' && body.advertise !== '') {
    advertise = hostName(body.advertise);
    if (!advertise) return 'advertise must be an IP address or host name.';
  }
  const lan: string[] = [];
  if (Array.isArray(body.lan)) {
    for (const a of body.lan.slice(0, MAX_LAN_ADDRESSES)) {
      const ip = typeof a === 'string' ? ipLiteral(a) : null;
      if (ip && !lan.includes(ip)) lan.push(ip);
    }
  }
  const tags: string[] = [];
  if (Array.isArray(body.tags)) {
    for (const t of body.tags) {
      const tag = typeof t === 'string' ? t.trim().toLowerCase() : '';
      if (new RegExp(`^[a-z0-9-]{1,${String(MAX_TAG)}}$`).test(tag) && !tags.includes(tag)) {
        tags.push(tag);
      }
      if (tags.length === MAX_TAGS) break;
    }
  }
  const visibility = body.visibility === 'public' ? 'public' : 'unlisted';
  const heartbeat =
    typeof body.heartbeatS === 'number' && Number.isFinite(body.heartbeatS)
      ? body.heartbeatS
      : DEFAULT_HEARTBEAT_S;
  return {
    port: p,
    rtcPort,
    ice,
    cert,
    advertise,
    lan,
    name: text(body.name, MAX_NAME) || 'Dwell Server',
    motd: text(body.motd, MAX_MOTD),
    players: count(body.players, 65535),
    maxPlayers: count(body.maxPlayers, 65535),
    protocol: count(body.protocol, 65535),
    visibility,
    tags,
    heartbeatS: Math.max(MIN_HEARTBEAT_S, Math.min(MAX_HEARTBEAT_S, heartbeat)),
  };
}

/** A typed server address: "host", "host:port", "[v6]:port" or a bare v6 literal. */
export interface TypedAddress {
  host: string;
  port: number | null;
}

export function parseAddress(input: string): TypedAddress | null {
  const t = input.trim().toLowerCase();
  const bracketed = /^\[([0-9a-f:.]+)\](?::(\d{1,5}))?$/.exec(t);
  if (bracketed?.[1]) {
    const host = ipLiteral(bracketed[1]);
    const p = bracketed[2] === undefined ? null : port(Number(bracketed[2]));
    return host && (bracketed[2] === undefined || p !== null) ? { host, port: p } : null;
  }
  if (ipLiteral(t)?.includes(':')) return { host: t, port: null }; // a bare v6 literal
  const m = /^([^:/\s]+)(?::(\d{1,5}))?$/.exec(t);
  if (!m?.[1]) return null;
  const host = hostName(m[1]);
  const p = m[2] === undefined ? null : port(Number(m[2]));
  return host && (m[2] === undefined || p !== null) ? { host, port: p } : null;
}

// --- the lobby list (5e) ---------------------------------------------------------------------

/** Most entries a lobby-list query answers with. */
export const MAX_LIST = 100;

/** A lobby-list query (`GET /v1/servers?…`): search text and filters. */
export interface ListQuery {
  /** Words that must each appear in the name, MOTD or tags (case-insensitive). */
  words: string[];
  /** A tag the entry must have. */
  tag: string | null;
  /** Only entries running this protocol version (compatible with the player's client). */
  protocol: number | null;
  /** Only entries with room for another player. */
  notFull: boolean;
  /** Only entries with someone playing. */
  hasPlayers: boolean;
  /** Unverified servers ("new") instead of verified servers and friend worlds. */
  fresh: boolean;
  limit: number;
}

/** A lobby-list query from the URL's parameters (unknown or malformed ones are ignored). */
export function parseListQuery(params: URLSearchParams): ListQuery {
  const q = (params.get('q') ?? '').toLowerCase().slice(0, 100);
  const tag = (params.get('tag') ?? '').trim().toLowerCase();
  const protocol = Number(params.get('protocol') ?? 'NaN');
  const limit = Number(params.get('limit') ?? 'NaN');
  const flag = (name: string) => params.get(name) === '1' || params.get(name) === 'true';
  return {
    words: q.split(/\s+/).filter((w) => w !== ''),
    tag: tag === '' ? null : tag,
    protocol: Number.isInteger(protocol) && protocol >= 0 ? protocol : null,
    notFull: flag('notFull'),
    hasPlayers: flag('hasPlayers'),
    fresh: flag('new'),
    limit: Number.isInteger(limit) && limit >= 1 ? Math.min(limit, MAX_LIST) : MAX_LIST,
  };
}

/** What a listed entry offers to the query. */
export interface Listable {
  name: string;
  motd: string;
  tags: string[];
  players: number;
  maxPlayers: number;
  protocol: number | null;
}

/** Whether an entry matches a query's search words and filters (not `fresh`: the caller's). */
export function matchesQuery(entry: Listable, query: ListQuery): boolean {
  const haystack = [entry.name, entry.motd, ...entry.tags].join('\n').toLowerCase();
  if (!query.words.every((w) => haystack.includes(w))) return false;
  if (query.tag !== null && !entry.tags.includes(query.tag)) return false;
  if (query.protocol !== null && entry.protocol !== query.protocol) return false;
  if (query.notFull && entry.players >= entry.maxPlayers) return false;
  if (query.hasPlayers && entry.players === 0) return false;
  return true;
}

/** Most players first, then by name: the lobby list's order. */
export function byPlayersThenName(
  a: Pick<Listable, 'players' | 'name'>,
  b: Pick<Listable, 'players' | 'name'>,
): number {
  return b.players - a.players || a.name.localeCompare(b.name);
}
