// The Dwell master server (ARCHITECTURE.md §10.3, ADR 0013): a Cloudflare Worker routing /v1/ to
// handlers and to its Durable Objects. No game traffic passes through it.
import { verifyRequest } from './auth';
import type { Env } from './env';
import { allowedOrigins, corsHeaders, json, problem, withHeaders } from './http';
import { formatCode, newCode, normalizeCode } from './codes';
import { API_VERSION, MAX_BODY_BYTES } from './limits';
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

/** POST /v1/rooms {maxGuests}: a new friend-world room and its host token (§10.2). */
async function createRoom(request: Request, env: Env): Promise<Response> {
  const req = await signedPost(request, env);
  if (req instanceof Response) return req;
  const maxGuests = typeof req.body.maxGuests === 'number' ? req.body.maxGuests : 8;
  for (let attempt = 0; attempt < 5; attempt++) {
    const code = newCode();
    const opened = await room(env, code).open(req.key, maxGuests, Date.now());
    if (opened) {
      return json({ code, display: formatCode(code), hostToken: opened.hostToken }, 201);
    }
  }
  return problem(503, 'busy', 'No free join code; try again.');
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
