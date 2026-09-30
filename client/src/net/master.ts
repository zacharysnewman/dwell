// The master server's client side (ARCHITECTURE.md §10.3, ADR 0013): where it is, and signing
// requests with the device key (§10.4). The message format matches the Worker's
// (services/master/src/auth.ts); shared/master/vectors.json pins it for both. Phase 5c onwards adds
// the calls that use it (join codes, signaling, TURN credentials).
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
