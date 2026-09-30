import { describe, expect, it } from 'vitest';
import vectors from '../../../shared/master/vectors.json';
import type { DeviceKey } from '../identity/deviceKey';
import {
  MasterClient,
  MasterError,
  masterUrl,
  roomSocketUrl,
  signatureHeaders,
  signingMessage,
} from './master';

const fromHex = (hex: string) => Uint8Array.from(hex.match(/../g) ?? [], (h) => parseInt(h, 16));
const toHex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
const utf8 = (s: string) => new TextEncoder().encode(s);

/** The vectors' fixed key as a DeviceKey. */
async function vectorKey(): Promise<DeviceKey> {
  const privateKey = await crypto.subtle.importKey(
    'pkcs8',
    fromHex(vectors.pkcs8),
    'Ed25519',
    false,
    ['sign'],
  );
  return {
    publicKey: fromHex(vectors.publicKey),
    sign: async (data) => new Uint8Array(await crypto.subtle.sign('Ed25519', privateKey, data)),
  };
}

describe('master requests', () => {
  it('build the shared vectors’ messages and signatures', async () => {
    const key = await vectorKey();
    for (const c of vectors.cases) {
      expect(toHex(await signingMessage(c.method, c.path, c.time, utf8(c.body)))).toBe(c.message);
      expect(await signatureHeaders(key, c.method, c.path, utf8(c.body), c.time)).toEqual({
        'x-dwell-key': vectors.publicKey,
        'x-dwell-time': String(c.time),
        'x-dwell-signature': c.signature,
      });
    }
  });

  it('find the master from ?master= or the build setting', () => {
    expect(masterUrl('', 'https://dwell-master.example.workers.dev/')).toBe(
      'https://dwell-master.example.workers.dev',
    );
    expect(masterUrl('?master=http://localhost:8787', 'https://x.test')).toBe(
      'http://localhost:8787',
    );
    expect(masterUrl('', undefined)).toBeNull();
    expect(masterUrl('', '')).toBeNull();
    expect(masterUrl('?master=javascript:alert(1)', 'https://x.test')).toBeNull();
    expect(masterUrl('?master=not a url', undefined)).toBeNull();
  });

  it('sign POSTs, not GETs, and report errors', async () => {
    const key = await vectorKey();
    const seen: { url: string; init: RequestInit | undefined }[] = [];
    const replies = [
      new Response(JSON.stringify({ ok: true, api: 1 })),
      new Response(JSON.stringify({ key: vectors.publicKey })),
      new Response(
        JSON.stringify({ error: 'clock', message: 'The device clock is too far off.' }),
        {
          status: 401,
        },
      ),
    ];
    const client = new MasterClient(
      'https://m.test',
      key,
      () => 1790000000000,
      (url, init) => {
        seen.push({ url: url as string, init });
        const reply = replies.shift();
        return reply ? Promise.resolve(reply) : Promise.reject(new Error('no reply'));
      },
    );
    expect(await client.health()).toEqual({ ok: true, api: 1 });
    expect(seen[0]?.init?.headers).toEqual({});
    expect(await client.whoami()).toEqual({ key: vectors.publicKey });
    const headers = seen[1]?.init?.headers as Record<string, string>;
    expect(seen[1]?.url).toBe('https://m.test/v1/whoami');
    expect(headers['x-dwell-key']).toBe(vectors.publicKey);
    expect(headers['x-dwell-time']).toBe('1790000000000');
    const error = await client.whoami().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(MasterError);
    expect(error).toMatchObject({ status: 401, code: 'clock' });
  });
});

describe('friend-world rooms on the master', () => {
  it('create and join rooms, fetch ICE servers, and build the room socket URL', async () => {
    const key = await vectorKey();
    const seen: string[] = [];
    const replies = [
      { code: 'KQ7XM4', display: 'KQ7-XM4', hostToken: 'h' },
      { token: 't', peer: 3 },
      { iceServers: [{ urls: 'stun:stun.cloudflare.com:3478' }] },
    ];
    const client = new MasterClient(
      'https://m.test',
      key,
      () => 1790000000000,
      (url, init) => {
        seen.push(`${init?.method ?? 'GET'} ${url as string} ${String(init?.body && 'body')}`);
        return Promise.resolve(new Response(JSON.stringify(replies.shift())));
      },
    );
    expect(await client.createRoom(8)).toEqual({
      code: 'KQ7XM4',
      display: 'KQ7-XM4',
      hostToken: 'h',
    });
    expect(await client.joinRoom('KQ7XM4')).toEqual({ token: 't', peer: 3 });
    expect(await client.turn()).toEqual([{ urls: 'stun:stun.cloudflare.com:3478' }]);
    expect(seen).toEqual([
      'POST https://m.test/v1/rooms body',
      'POST https://m.test/v1/rooms/KQ7XM4/join body',
      'POST https://m.test/v1/turn body',
    ]);
    expect(roomSocketUrl('https://m.test', 'KQ7XM4', 'a+b')).toBe(
      'wss://m.test/v1/rooms/KQ7XM4/ws?token=a%2Bb',
    );
    expect(roomSocketUrl('http://localhost:8787', 'KQ7XM4', 't')).toBe(
      'ws://localhost:8787/v1/rooms/KQ7XM4/ws?token=t',
    );
  });
});
