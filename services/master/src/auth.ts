// Signed requests (ARCHITECTURE.md §10.3, ADR 0013). Every mutating request carries an Ed25519
// public key (a player's device key, §10.4, or a dedicated server's key), a timestamp and a
// signature over
//
//   UTF-8("dwell-master-v1\n" ‖ METHOD ‖ "\n" ‖ path+query ‖ "\n" ‖ time ‖ "\n") ‖ SHA-256(body)
//
// in the headers X-Dwell-Key (64 hex), X-Dwell-Time (Unix milliseconds, decimal) and
// X-Dwell-Signature (128 hex). The client builds the same message (client/src/net/master.ts);
// shared/master/vectors.json pins it for both.

export const SIGNATURE_CONTEXT = 'dwell-master-v1';
/** How far a request's time may be from the master's clock. */
export const MAX_CLOCK_SKEW_MS = 60_000;

export function fromHex(hex: string): Uint8Array | null {
  if (!/^(?:[0-9a-f]{2})*$/i.test(hex)) return null;
  return Uint8Array.from(hex.match(/../g) ?? [], (h) => parseInt(h, 16));
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** The bytes a request's signature covers. */
export async function signingMessage(
  method: string,
  path: string,
  time: number,
  body: Uint8Array,
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

export type Verified = { ok: true; key: string } | { ok: false; code: string; message: string };

/**
 * Checks a request's signature headers against its method, path, query and body. `key` is the
 * signer's public key (lower-case hex).
 */
export async function verifyRequest(
  request: Request,
  body: Uint8Array,
  now: number,
): Promise<Verified> {
  const keyHex = request.headers.get('x-dwell-key') ?? '';
  const timeText = request.headers.get('x-dwell-time') ?? '';
  const signature = fromHex(request.headers.get('x-dwell-signature') ?? '');
  const key = fromHex(keyHex);
  if (key?.length !== 32 || signature?.length !== 64 || !/^\d{1,16}$/.test(timeText)) {
    return { ok: false, code: 'unsigned', message: 'This request must be signed.' };
  }
  const time = Number(timeText);
  if (Math.abs(now - time) > MAX_CLOCK_SKEW_MS) {
    return { ok: false, code: 'clock', message: 'The device clock is too far off.' };
  }
  const url = new URL(request.url);
  const message = await signingMessage(request.method, url.pathname + url.search, time, body);
  let valid: boolean;
  try {
    const publicKey = await crypto.subtle.importKey(
      'raw',
      new Uint8Array(key),
      { name: 'Ed25519' },
      false,
      ['verify'],
    );
    valid = await crypto.subtle.verify('Ed25519', publicKey, new Uint8Array(signature), message);
  } catch {
    valid = false; // not a valid point
  }
  if (!valid) return { ok: false, code: 'signature', message: 'The signature does not match.' };
  return { ok: true, key: keyHex.toLowerCase() };
}
