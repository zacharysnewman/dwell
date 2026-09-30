// The Dwell master server (ARCHITECTURE.md §10.3, ADR 0013): a Cloudflare Worker routing /v1/ to
// handlers and to its Durable Objects. No game traffic passes through it.
import { verifyRequest } from './auth';
import type { Env } from './env';
import { allowedOrigins, corsHeaders, json, problem, withHeaders } from './http';
import { API_VERSION, MAX_BODY_BYTES } from './limits';

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

async function route(request: Request, env: Env): Promise<Response> {
  const url = new URL(request.url);
  const path = url.pathname.replace(/\/+$/, '');
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
    return withHeaders(response, cors);
  },
} satisfies ExportedHandler<Env>;
