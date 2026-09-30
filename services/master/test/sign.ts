// Signs requests for tests the way clients do (client/src/net/master.ts).
import { signingMessage, toHex } from '../src/auth';

export interface TestKey {
  publicHex: string;
  privateKey: CryptoKey;
}

export async function newKey(): Promise<TestKey> {
  const pair = (await crypto.subtle.generateKey({ name: 'Ed25519' }, true, [
    'sign',
    'verify',
  ])) as CryptoKeyPair;
  const raw = new Uint8Array((await crypto.subtle.exportKey('raw', pair.publicKey)) as ArrayBuffer);
  return { publicHex: toHex(raw), privateKey: pair.privateKey };
}

export async function signedRequest(
  key: TestKey,
  method: string,
  url: string,
  body = '',
  options: { time?: number; headers?: Record<string, string> } = {},
): Promise<Request> {
  const time = options.time ?? Date.now();
  const bytes = new TextEncoder().encode(body);
  const u = new URL(url);
  const message = await signingMessage(method, u.pathname + u.search, time, bytes);
  const signature = new Uint8Array(await crypto.subtle.sign('Ed25519', key.privateKey, message));
  return new Request(url, {
    method,
    ...(method === 'GET' ? {} : { body }),
    headers: {
      'x-dwell-key': key.publicHex,
      'x-dwell-time': String(time),
      'x-dwell-signature': toHex(signature),
      ...options.headers,
    },
  });
}
