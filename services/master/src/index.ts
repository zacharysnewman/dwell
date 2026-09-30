// The Dwell master server (ARCHITECTURE.md §10.3, ADR 0013): a Cloudflare Worker routing /v1/ to
// handlers and to its Durable Objects. No game traffic passes through it.
import { verifyRequest } from './auth';
import type { Env } from './env';
import { allowedOrigins, corsHeaders, json, problem, withHeaders } from './http';
import { formatCode, newCode, normalizeCode } from './codes';
import { API_VERSION, MAX_BODY_BYTES } from './limits';
import {
  byPlayersThenName,
  matchesQuery,
  parseAddress,
  parseListQuery,
  parseServerReport,
} from './servers';
import { iceServers } from './turn';

export { Directory } from './directory';
export { Room } from './room';

/** The Directory instance every request shares. */
function directory(env: Env) {
  return env.DIRECTORY.get(env.DIRECTORY.idFromName('global'));
}

async function readBody(request: Request): Promise<Uint8Array | null> {
  const declared = Number(request.headers.get('content-length') ?? '0');
  if (declared > MAX_BODY_BYTES) return null;
  const body = new Uint8Array(await request.arrayBuffer());
  return body.length > MAX_BODY_BYTES ? null : body;
}

/** Verifies the signature and applies the per-key and per-IP limits; the signer's key or an error. */
async function signed(
  request: Request,
  env: Env,
  body: Uint8Array,
): Promise<{ key: string } | Response> {
  const now = Date.now();
  const verified = await verifyRequest(request, body, now);
  if (!verified.ok) return problem(401, verified.code, verified.message);
  const ip = request.headers.get('cf-connecting-ip') ?? 'unknown';
  const admission = await directory(env).admit(verified.key, ip, now);
  if (!admission.ok) {
    const response = problem(429, 'rate_limited', 'Too many requests; try again shortly.');
    response.headers.set('retry-after', String(admission.retryAfterS));
    return response;
  }
  return { key: verified.key };
}

function room(env: Env, code: string) {
  return env.ROOM.get(env.ROOM.idFromName(code));
}

function limited(retryAfterS: number): Response {
  const response = problem(429, 'rate_limited', 'Too many requests; try again shortly.');
  response.headers.set('retry-after', String(retryAfterS));
  return response;
}

/** A signed POST: its key and parsed JSON body (an object), or an error response. */
async function signedPost(
  request: Request,
  env: Env,
): Promise<{ key: string; body: Record<string, unknown> } | Response> {
  if (request.method !== 'POST') return problem(405, 'method', 'Use POST.');
  const bytes = await readBody(request);
  if (!bytes) return problem(413, 'too_large', 'The request body is too large.');
  const auth = await signed(request, env, bytes);
  if (auth instanceof Response) return auth;
  let body: unknown = {};
  if (bytes.length > 0) {
    try {
      body = JSON.parse(new TextDecoder().decode(bytes));
    } catch {
      return problem(400, 'bad_json', 'The request body is not JSON.');
    }
  }
  if (typeof body !== 'object' || body === null || Array.isArray(body)) {
    return problem(400, 'bad_json', 'The request body must be a JSON object.');
  }
  return { key: auth.key, body: body as Record<string, unknown> };
}

const clientIp = (request: Request) => request.headers.get('cf-connecting-ip') ?? 'unknown';

/**
 * POST /v1/rooms {maxGuests, visibility?, name?, protocol?}: a new friend-world room and its host
 * token (§10.2). Visibility "network" also lists it to players on the host's network (5d);
 * "public" lists it there and in the lobby list (5e).
 */
async function createRoom(request: Request, env: Env): Promise<Response> {
  const req = await signedPost(request, env);
  if (req instanceof Response) return req;
  const maxGuests = typeof req.body.maxGuests === 'number' ? req.body.maxGuests : 8;
  const now = Date.now();
  for (let attempt = 0; attempt < 5; attempt++) {
    const code = newCode();
    if (await directory(env).isServerCode(code)) continue; // a dedicated server's code
    const opened = await room(env, code).open(req.key, maxGuests, now);
    if (opened) {
      const visibility = req.body.visibility;
      if (visibility === 'network' || visibility === 'public') {
        const name =
          typeof req.body.name === 'string' && req.body.name.trim() !== ''
            ? req.body.name.replace(/\p{Cc}/gu, '').slice(0, 64)
            : 'Friend world';
        const protocol =
          typeof req.body.protocol === 'number' && Number.isInteger(req.body.protocol)
            ? req.body.protocol
            : null;
        await directory(env).addListedRoom(
          code,
          clientIp(request),
          name,
          { public: visibility === 'public', protocol },
          now,
        );
      }
      return json({ code, display: formatCode(code), hostToken: opened.hostToken }, 201);
    }
  }
  return problem(503, 'busy', 'No free join code; try again.');
}

/**
 * POST /v1/servers: a dedicated server registers or heartbeats (signed with its server key, §10.1).
 * Its public address is the request's unless it advertises one. Answers with its join code.
 */
async function registerServer(request: Request, env: Env): Promise<Response> {
  const req = await signedPost(request, env);
  if (req instanceof Response) return req;
  const report = parseServerReport(req.body);
  if (typeof report === 'string') return problem(400, 'bad_report', report);
  const { code, display } = await directory(env).registerServer(
    req.key,
    clientIp(request),
    report,
    Date.now(),
  );
  return json({ code, display, heartbeatS: report.heartbeatS });
}

/**
 * POST /v1/resolve {code} or {address}: what a join code or typed address leads to — a dedicated
 * server (how to connect) or a friend world's room. Limited per IP like room joins (guessing).
 */
async function resolve(request: Request, env: Env): Promise<Response> {
  const req = await signedPost(request, env);
  if (req instanceof Response) return req;
  const ip = clientIp(request);
  const now = Date.now();
  const admission = await directory(env).admitJoin(ip, now);
  if (!admission.ok) return limited(admission.retryAfterS);
  if (typeof req.body.code === 'string') {
    const code = normalizeCode(req.body.code);
    if (!code) return problem(404, 'not_found', 'That is not a join code.');
    const server = await directory(env).serverByCode(code, ip, now);
    if (server) {
      await directory(env).noteResolution(req.key, server.code, now);
      return json({ kind: 'server', server });
    }
    if (await room(env, code).isOpen()) {
      return json({ kind: 'room', code, display: formatCode(code) });
    }
    return problem(404, 'not_found', 'Nothing is being hosted with that code.');
  }
  if (typeof req.body.address === 'string') {
    const address = parseAddress(req.body.address);
    if (!address) return problem(400, 'bad_address', 'That is not a server address.');
    const server = await directory(env).serverByAddress(address.host, address.port, ip, now);
    if (server) {
      await directory(env).noteResolution(req.key, server.code, now);
      return json({ kind: 'server', server });
    }
    return problem(404, 'not_found', 'No server at that address is registered with the master.');
  }
  return problem(400, 'bad_request', 'Send a code or an address.');
}

/**
 * POST /v1/nearby: dedicated servers and friend worlds on the player's network (the same public
 * IP) — LAN discovery for browsers (5d). Friend-world listings whose room has closed are dropped.
 */
async function nearby(request: Request, env: Env): Promise<Response> {
  const req = await signedPost(request, env);
  if (req instanceof Response) return req;
  const ip = clientIp(request);
  const now = Date.now();
  const dir = directory(env);
  const servers = await dir.nearbyServers(ip, now);
  const worlds = [];
  for (const w of await dir.nearbyRooms(ip, now)) {
    if (await room(env, w.code).isOpen()) worlds.push(w);
    else await dir.removeListedRoom(w.code);
  }
  return json({ servers, worlds });
}

/**
 * GET /v1/servers?q=&tag=&protocol=&notFull=1&hasPlayers=1&new=1&limit=: the lobby list (5e) —
 * public dedicated servers (verified by players' receipts; unverified ones only with `new=1`) and
 * public friend worlds, most players first. Unsigned; limited per IP.
 */
async function listServers(request: Request, env: Env): Promise<Response> {
  const ip = clientIp(request);
  const now = Date.now();
  const dir = directory(env);
  const admission = await dir.admitList(ip, now);
  if (!admission.ok) return limited(admission.retryAfterS);
  const query = parseListQuery(new URL(request.url).searchParams);
  const servers = await dir.listServers(query, ip, now);
  const worlds = [];
  if (!query.fresh) {
    for (const w of await dir.publicRooms(now)) {
      const listing = await room(env, w.code).listing();
      if (listing.state === 'closed') await dir.removeListedRoom(w.code);
      if (listing.state !== 'open') continue;
      const world = { ...w, players: listing.players, maxPlayers: listing.maxPlayers };
      if (matchesQuery({ ...world, motd: '', tags: [] }, query)) worlds.push(world);
    }
    worlds.sort(byPlayersThenName);
  }
  return json({ servers, worlds: worlds.slice(0, query.limit) });
}

/**
 * POST /v1/receipts {code}: a player joined the server with this code through the master (after
 * resolving it) and says so (ADR 0013); enough distinct players verify a public server (5e).
 */
async function postReceipt(request: Request, env: Env): Promise<Response> {
  const req = await signedPost(request, env);
  if (req instanceof Response) return req;
  const code = typeof req.body.code === 'string' ? normalizeCode(req.body.code) : null;
  if (!code) return problem(400, 'bad_request', "Send the server's code.");
  const receipt = await directory(env).addReceipt(req.key, code, Date.now());
  if (!receipt.ok) {
    return problem(404, 'not_found', 'No recent join of a server with that code to confirm.');
  }
  return json({ ok: true, verified: receipt.verified });
}

/** POST /v1/rooms/<code>/join: a guest's one-use token for the room's WebSocket. */
async function joinRoom(request: Request, env: Env, code: string): Promise<Response> {
  const req = await signedPost(request, env);
  if (req instanceof Response) return req;
  const ip = request.headers.get('cf-connecting-ip') ?? 'unknown';
  const admission = await directory(env).admitJoin(ip, Date.now());
  if (!admission.ok) return limited(admission.retryAfterS);
  const joined = await room(env, code).join(req.key, Date.now());
  if (joined.ok) return json({ token: joined.token, peer: joined.peer });
  return joined.code === 'full'
    ? problem(409, 'full', 'That world is full.')
    : problem(404, 'not_found', 'No world is being hosted with that code.');
}

async function route(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  // Repeated and trailing slashes don't matter (a base URL ending in "/" plus "/v1/…"). Signatures
  // still cover the path exactly as sent.
  const path = url.pathname.replace(/\/{2,}/g, '/').replace(/\/+$/, '');
  if (path === '/v1/health') {
    if (request.method !== 'GET') return problem(405, 'method', 'Use GET.');
    return json({ ok: true, service: 'dwell-master', api: API_VERSION, time: Date.now() });
  }
  if (path === '/v1/whoami') {
    // Checks a client's signing (and clock) end to end: answers with the key that signed.
    if (request.method !== 'POST') return problem(405, 'method', 'Use POST.');
    const body = await readBody(request);
    if (!body) return problem(413, 'too_large', 'The request body is too large.');
    const auth = await signed(request, env, body);
    if (auth instanceof Response) return auth;
    return json({ key: auth.key });
  }
  if (path === '/v1/turn') {
    // Short-lived ICE servers (TURN when configured) for a signed player (§10.3).
    const req = await signedPost(request, env);
    if (req instanceof Response) return req;
    return json({ iceServers: await iceServers(env.TURN_KEY_ID, env.TURN_KEY_API_TOKEN) });
  }
  if (path === '/v1/rooms') return createRoom(request, env);
  if (path === '/v1/servers') {
    return request.method === 'GET' ? listServers(request, env) : registerServer(request, env);
  }
  if (path === '/v1/receipts') return postReceipt(request, env);
  if (path === '/v1/servers/leave') {
    const req = await signedPost(request, env);
    if (req instanceof Response) return req;
    await directory(env).leaveServer(req.key);
    return json({ ok: true });
  }
  if (path === '/v1/resolve') return resolve(request, env);
  if (path === '/v1/nearby') return nearby(request, env);
  const roomPath = /^\/v1\/rooms\/([^/]+)\/(join|ws)$/.exec(path);
  if (roomPath?.[1] && roomPath[2]) {
    const code = normalizeCode(decodeURIComponent(roomPath[1]));
    if (!code) return problem(404, 'not_found', 'That is not a join code.');
    if (roomPath[2] === 'join') return joinRoom(request, env, code);
    if (request.method !== 'GET') return problem(405, 'method', 'Use GET.');
    return room(env, code).fetch(request);
  }
  return problem(404, 'not_found', 'No such endpoint.');
}

export default {
  async fetch(request, env): Promise<Response> {
    const cors = corsHeaders(request.headers.get('origin'), allowedOrigins(env.ALLOWED_ORIGINS));
    if (request.method === 'OPTIONS') return new Response(null, { status: 204, headers: cors });
    let response: Response;
    try {
      response = await route(request, env);
    } catch (err) {
      console.error('master: unhandled error', err);
      response = problem(500, 'internal', 'Something went wrong.');
    }
    // A WebSocket upgrade (101) is returned untouched: it needs no CORS headers.
    return response.status === 101 ? response : withHeaders(response, cors);
  },
} satisfies ExportedHandler<Env>;
