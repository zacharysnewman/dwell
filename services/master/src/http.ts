// JSON responses and CORS for the master's HTTP API (ARCHITECTURE.md §10.3).

export const SIGNATURE_HEADERS = ['x-dwell-key', 'x-dwell-time', 'x-dwell-signature'] as const;

export function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json; charset=utf-8', ...headers },
  });
}

/** An error response: `{ "error": code, "message": text }`. */
export function problem(status: number, code: string, message: string): Response {
  return json({ error: code, message }, status);
}

export function allowedOrigins(list: string): Set<string> {
  return new Set(
    list
      .split(',')
      .map((o) => o.trim())
      .filter((o) => o !== ''),
  );
}

/**
 * CORS headers for a request from `origin`: only listed origins get any (a browser page elsewhere
 * cannot read the answers). Requests without an Origin (dwell_server, curl) need none.
 */
export function corsHeaders(origin: string | null, allowed: Set<string>): Record<string, string> {
  if (!origin || !allowed.has(origin)) return {};
  return {
    'access-control-allow-origin': origin,
    'access-control-allow-methods': 'GET, POST, DELETE, OPTIONS',
    'access-control-allow-headers': ['content-type', ...SIGNATURE_HEADERS].join(', '),
    'access-control-max-age': '86400',
    vary: 'origin',
  };
}

export function withHeaders(response: Response, headers: Record<string, string>): Response {
  if (Object.keys(headers).length === 0) return response;
  const out = new Response(response.body, response);
  for (const [k, v] of Object.entries(headers)) out.headers.set(k, v);
  return out;
}
