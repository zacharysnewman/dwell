// TURN credentials (ARCHITECTURE.md §10.3, ADR 0013): the master mints short-lived ICE servers for
// signed, rate-limited requests from Cloudflare's managed TURN service. Without a TURN key
// (development, CI) it hands out STUN only, which works on the same network and most home routers.

export interface IceServer {
  urls: string | string[];
  username?: string;
  credential?: string;
}

export const STUN_ONLY: IceServer[] = [{ urls: 'stun:stun.cloudflare.com:3478' }];
/** How long minted credentials last (the API allows up to 48 h). */
export const TURN_TTL_SECONDS = 6 * 60 * 60;

function isIceServer(v: unknown): v is IceServer {
  if (typeof v !== 'object' || v === null) return false;
  const s = v as Record<string, unknown>;
  const urls = s.urls;
  const urlsOk =
    typeof urls === 'string' ||
    (Array.isArray(urls) && urls.length > 0 && urls.every((u) => typeof u === 'string'));
  return (
    urlsOk &&
    (s.username === undefined || typeof s.username === 'string') &&
    (s.credential === undefined || typeof s.credential === 'string')
  );
}

/** The ICE servers in Cloudflare's answer (`iceServers` as a list, or a single object), or null. */
export function parseIceServers(body: unknown): IceServer[] | null {
  if (typeof body !== 'object' || body === null) return null;
  const raw = (body as { iceServers?: unknown }).iceServers;
  const list = Array.isArray(raw) ? raw : raw === undefined ? [] : [raw];
  const servers = list.filter(isIceServer);
  return servers.length > 0 ? servers : null;
}

/**
 * Mints ICE servers with the TURN key, falling back to STUN only when there is no key or the
 * service fails (so a friend world still works where a direct connection does).
 */
export async function iceServers(
  keyId: string | undefined,
  apiToken: string | undefined,
  fetchFn: typeof fetch = fetch,
): Promise<IceServer[]> {
  if (!keyId || !apiToken) return STUN_ONLY;
  try {
    const res = await fetchFn(
      `https://rtc.live.cloudflare.com/v1/turn/keys/${encodeURIComponent(keyId)}/credentials/generate-ice-servers`,
      {
        method: 'POST',
        headers: { authorization: `Bearer ${apiToken}`, 'content-type': 'application/json' },
        body: JSON.stringify({ ttl: TURN_TTL_SECONDS }),
      },
    );
    if (!res.ok) {
      console.error('master: TURN credentials failed', res.status);
      return STUN_ONLY;
    }
    return parseIceServers(await res.json()) ?? STUN_ONLY;
  } catch (err) {
    console.error('master: TURN credentials failed', err);
    return STUN_ONLY;
  }
}
