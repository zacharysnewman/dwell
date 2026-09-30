// The master server's client side (ARCHITECTURE.md §10.3, ADR 0013): where it is, and signing
// requests with the device key (§10.4). The message format matches the Worker's
// (services/master/src/auth.ts); shared/master/vectors.json pins it for both. Friend worlds (5c) use
// its rooms (join codes and signaling) and TURN credentials; dedicated servers (5d) are found by
// code or typed address, and listed to players on their own network; public ones, and public
// friend worlds, are in the lobby list (5e), and players confirm their joins with receipts.
import type { DeviceKey } from '../identity/deviceKey';

export const SIGNATURE_CONTEXT = 'dwell-master-v1';

/**
 * The master's base URL: `?master=<url>` (e.g. a local `wrangler dev`), else the one the build
 * was configured with (`VITE_MASTER_URL`); null when neither is a valid http(s) URL.
 */
export function masterUrl(search: string, configured: string | undefined): string | null {
  const chosen = new URLSearchParams(search).get('master') ?? configured ?? '';
  try {
    const url = new URL(chosen);
    if (url.protocol !== 'https:' && url.protocol !== 'http:') return null;
    return url.href.replace(/\/+$/, '');
  } catch {
    return null;
  }
}

/** The master this page uses (see masterUrl). */
export function configuredMasterUrl(): string | null {
  return masterUrl(location.search, import.meta.env.VITE_MASTER_URL);
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/**
 * The bytes a request's signature covers: UTF-8 of the context, method, path with query and time
 * (each followed by a newline), then the SHA-256 of the body.
 */
export async function signingMessage(
  method: string,
  path: string,
  time: number,
  body: Uint8Array<ArrayBuffer>,
): Promise<Uint8Array<ArrayBuffer>> {
  const head = new TextEncoder().encode(
    `${SIGNATURE_CONTEXT}\n${method.toUpperCase()}\n${path}\n${String(time)}\n`,
  );
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', body));
  const message = new Uint8Array(head.length + digest.length);
  message.set(head);
  message.set(digest, head.length);
  return message;
}

/** The headers that sign a request: X-Dwell-Key, X-Dwell-Time, X-Dwell-Signature. */
export async function signatureHeaders(
  key: DeviceKey,
  method: string,
  path: string,
  body: Uint8Array<ArrayBuffer>,
  time: number,
): Promise<Record<string, string>> {
  const signature = await key.sign(await signingMessage(method, path, time, body));
  return {
    'x-dwell-key': hex(key.publicKey),
    'x-dwell-time': String(time),
    'x-dwell-signature': hex(signature),
  };
}

/** A dedicated server registered with the master, as a player sees it (§10.3, Phase 5d). */
export interface ServerEntry {
  code: string;
  display: string;
  name: string;
  motd: string;
  players: number;
  maxPlayers: number;
  protocol: number;
  host: string;
  port: number;
  cert: string;
  rtcPort: number | null;
  ice: string | null;
  /** Lobby-list tags (5e; absent from older masters). */
  tags?: string[];
}

/** A public server in the lobby list: verified once distinct players have joined it (5e). */
export interface ListedServer extends ServerEntry {
  verified: boolean;
}

/** A public friend world in the lobby list (players include the host). */
export interface ListedWorld {
  code: string;
  display: string;
  name: string;
  players: number;
  maxPlayers: number;
  /** The host's protocol version, when it said. */
  protocol: number | null;
}

/** The lobby list: verified public servers and public friend worlds, or the "new" servers. */
export interface Lobby {
  servers: ListedServer[];
  worlds: ListedWorld[];
}

/** A lobby-list search (all optional). */
export interface LobbyQuery {
  /** Words to find in names, MOTDs and tags. */
  q?: string;
  tag?: string;
  /** Only servers running this protocol version. */
  protocol?: number;
  notFull?: boolean;
  hasPlayers?: boolean;
  /** Unverified ("new") servers instead. */
  fresh?: boolean;
}

/** The lobby-list query string for a search (`GET /v1/servers?…`). */
export function lobbyQueryString(query: LobbyQuery): string {
  const p = new URLSearchParams();
  if (query.q?.trim()) p.set('q', query.q.trim());
  if (query.tag) p.set('tag', query.tag);
  if (query.protocol !== undefined) p.set('protocol', String(query.protocol));
  if (query.notFull) p.set('notFull', '1');
  if (query.hasPlayers) p.set('hasPlayers', '1');
  if (query.fresh) p.set('new', '1');
  const s = p.toString();
  return s === '' ? '' : `?${s}`;
}

/** What a join code or typed address leads to. */
export type Resolved =
  { kind: 'server'; server: ServerEntry } | { kind: 'room'; code: string; display: string };

/** Servers and friend worlds on the player's network (same public IP). */
export interface Nearby {
  servers: ServerEntry[];
  worlds: { code: string; display: string; name: string }[];
}

/** Invite-link parameters (`?join=…&cert=…[&rtc=…&ice=…]`) for a resolved server. */
export function serverInvite(s: ServerEntry): Record<string, string> {
  const host = s.host.includes(':') ? `[${s.host}]` : s.host;
  const route: Record<string, string> = { join: `${host}:${String(s.port)}`, cert: s.cert };
  if (s.rtcPort !== null && s.ice) {
    route.rtc = String(s.rtcPort);
    route.ice = s.ice;
  }
  return route;
}

export class MasterError extends Error {
  override name = 'MasterError';
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

/** Calls the master's HTTP API. */
export class MasterClient {
  constructor(
    readonly baseUrl: string,
    private readonly key: DeviceKey,
    private readonly now: () => number = Date.now,
    private readonly fetchFn: typeof fetch = (input, init) => fetch(input, init),
  ) {}

  health(): Promise<{ ok: boolean; api: number }> {
    return this.request('GET', '/v1/health', null);
  }

  /** Checks signing end to end: the master answers with the key it verified. */
  whoami(): Promise<{ key: string }> {
    return this.request('POST', '/v1/whoami', {});
  }

  /**
   * Opens a room for a friend world: its join code, and the host's token for the room socket.
   * Visibility "network" also lists it (by `name`) to players on the host's network; "public"
   * there and in the lobby list, with the host's protocol version.
   */
  createRoom(
    maxGuests: number,
    visibility: 'code' | 'network' | 'public' = 'code',
    name = '',
    protocol?: number,
  ): Promise<{ code: string; display: string; hostToken: string }> {
    return this.request('POST', '/v1/rooms', { maxGuests, visibility, name, protocol });
  }

  /** Asks to join the room with this code: a one-use token for the room socket. */
  joinRoom(code: string): Promise<{ token: string; peer: number }> {
    return this.request('POST', `/v1/rooms/${encodeURIComponent(code)}/join`, {});
  }

  /** ICE servers for WebRTC: STUN, and TURN when the master has a TURN key. */
  async turn(): Promise<RTCIceServer[]> {
    const { iceServers } = await this.request<{ iceServers: RTCIceServer[] }>(
      'POST',
      '/v1/turn',
      {},
    );
    return iceServers;
  }

  /** What a join code (a friend world's or a server's) or a typed server address leads to. */
  resolve(query: { code: string } | { address: string }): Promise<Resolved> {
    return this.request('POST', '/v1/resolve', query);
  }

  /** Servers and friend worlds on this player's network. */
  nearby(): Promise<Nearby> {
    return this.request('POST', '/v1/nearby', {});
  }

  /** The lobby list (5e): public servers and friend worlds matching the search. */
  lobby(query: LobbyQuery = {}): Promise<Lobby> {
    return this.request('GET', `/v1/servers${lobbyQueryString(query)}`, null);
  }

  /**
   * Confirms this player joined the server with this code (after resolving it): the receipts of
   * distinct players verify a public server (ADR 0013).
   */
  receipt(code: string): Promise<{ ok: boolean; verified: boolean }> {
    return this.request('POST', '/v1/receipts', { code });
  }

  /** The room socket's URL for a host or guest token. */
  roomSocketUrl(code: string, token: string): string {
    return roomSocketUrl(this.baseUrl, code, token);
  }

  private async request<T>(method: string, path: string, body: unknown): Promise<T> {
    const bytes = new TextEncoder().encode(body === null ? '' : JSON.stringify(body));
    const headers: Record<string, string> =
      method === 'GET' ? {} : await signatureHeaders(this.key, method, path, bytes, this.now());
    if (body !== null) headers['content-type'] = 'application/json';
    const res = await this.fetchFn(this.baseUrl + path, {
      method,
      headers,
      ...(body === null ? {} : { body: bytes }),
    });
    const data = (await res.json().catch(() => ({}))) as Record<string, unknown>;
    if (!res.ok) {
      const code = typeof data.error === 'string' ? data.error : 'http';
      const message =
        typeof data.message === 'string' ? data.message : `HTTP ${String(res.status)}`;
      throw new MasterError(res.status, code, message);
    }
    return data as T;
  }
}

/** A room's WebSocket URL: the master's origin with ws(s), `/v1/rooms/<code>/ws?token=`. */
export function roomSocketUrl(baseUrl: string, code: string, token: string): string {
  const url = new URL(`${baseUrl}/v1/rooms/${encodeURIComponent(code)}/ws`);
  url.protocol = url.protocol === 'http:' ? 'ws:' : 'wss:';
  url.searchParams.set('token', token);
  return url.href;
}
