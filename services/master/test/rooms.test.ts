import { exports } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { CODE_ALPHABET, formatCode, newCode, normalizeCode } from '../src/codes';
import { JOIN_LIMIT } from '../src/rateLimit';
import { iceServers, parseIceServers, STUN_ONLY } from '../src/turn';
import { newKey, signedRequest, type TestKey } from './sign';

const BASE = 'https://master.test';
const fetchWorker = (request: Request) => exports.default.fetch(request);

async function createRoom(key: TestKey, maxGuests = 4) {
  const res = await fetchWorker(
    await signedRequest(key, 'POST', `${BASE}/v1/rooms`, JSON.stringify({ maxGuests })),
  );
  expect(res.status).toBe(201);
  return res.json<{ code: string; display: string; hostToken: string }>();
}

async function join(key: TestKey, code: string, ip = '198.51.100.1') {
  return fetchWorker(
    await signedRequest(key, 'POST', `${BASE}/v1/rooms/${code}/join`, '{}', {
      headers: { 'cf-connecting-ip': ip },
    }),
  );
}

/** Opens a room WebSocket, as a browser does (with an Origin header). */
async function connect(code: string, token: string): Promise<Socket> {
  const res = await fetchWorker(
    new Request(`${BASE}/v1/rooms/${code}/ws?token=${token}`, {
      headers: { upgrade: 'websocket', origin: 'https://dropkickarcade.com' },
    }),
  );
  expect(res.status).toBe(101);
  const ws = res.webSocket;
  if (!ws) throw new Error('no WebSocket');
  ws.accept();
  return new Socket(ws);
}

/** A WebSocket with a queue of received messages. */
class Socket {
  private readonly received: unknown[] = [];
  private waiting: ((m: unknown) => void) | null = null;
  closed = false;

  constructor(readonly ws: WebSocket) {
    ws.addEventListener('message', (e) => {
      const m: unknown = JSON.parse(e.data as string);
      if (this.waiting) {
        const w = this.waiting;
        this.waiting = null;
        w(m);
      } else {
        this.received.push(m);
      }
    });
    ws.addEventListener('close', () => {
      this.closed = true;
    });
  }

  send(m: unknown): void {
    this.ws.send(JSON.stringify(m));
  }

  next(): Promise<unknown> {
    const m = this.received.shift();
    if (m !== undefined) return Promise.resolve(m);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error('no message'));
      }, 2000);
      this.waiting = (msg) => {
        clearTimeout(timer);
        resolve(msg);
      };
    });
  }
}

describe('join codes', () => {
  it('are six characters without look-alikes', () => {
    const code = newCode();
    expect(code).toMatch(/^[A-HJKMNP-Z2-9]{6}$/);
    expect(CODE_ALPHABET).not.toMatch(/[ILO01]/);
    expect(newCode(() => 0)).toBe('AAAAAA');
    expect(formatCode('KQ7XM4')).toBe('KQ7-XM4');
  });

  it('are read regardless of case, spaces and dashes; anything else is rejected', () => {
    expect(normalizeCode('kq7-xm4')).toBe('KQ7XM4');
    expect(normalizeCode(' KQ7 XM4 ')).toBe('KQ7XM4');
    expect(normalizeCode('KQ7XM')).toBeNull();
    expect(normalizeCode('KQ7XM40')).toBeNull();
    expect(normalizeCode('KO7XM4')).toBeNull(); // O is not in the alphabet
  });
});

describe('rooms', () => {
  it('relay signaling between a host and a guest, and close when the host leaves', async () => {
    const hostKey = await newKey();
    const { code, display, hostToken } = await createRoom(hostKey);
    expect(normalizeCode(display)).toBe(code);

    // Nobody can join before the host's socket is connected.
    expect((await join(await newKey(), code)).status).toBe(404);

    const host = await connect(code, hostToken);
    const joined = await join(await newKey(), code);
    expect(joined.status).toBe(200);
    const { token, peer } = await joined.json<{ token: string; peer: number }>();
    const guest = await connect(code, token);
    expect(await host.next()).toEqual({ t: 'guest', peer });

    guest.send({ t: 'signal', data: { type: 'offer', sdp: 'v=0…' } });
    expect(await host.next()).toEqual({
      t: 'signal',
      from: peer,
      data: { type: 'offer', sdp: 'v=0…' },
    });
    host.send({ t: 'signal', to: peer, data: { type: 'answer', sdp: 'v=0…' } });
    expect(await guest.next()).toEqual({ t: 'signal', data: { type: 'answer', sdp: 'v=0…' } });

    // A guest's token works once.
    const reuse = await fetchWorker(
      new Request(`${BASE}/v1/rooms/${code}/ws?token=${token}`, {
        headers: { upgrade: 'websocket' },
      }),
    );
    expect(reuse.status).toBe(404);

    host.ws.close(1000, 'bye');
    expect(await guest.next()).toEqual({ t: 'host-left' });
    expect((await join(await newKey(), code)).status).toBe(404);
  });

  it('turn guests away once full', async () => {
    const { code, hostToken } = await createRoom(await newKey(), 1);
    await connect(code, hostToken);
    expect((await join(await newKey(), code)).status).toBe(200);
    const second = await join(await newKey(), code);
    expect(second.status).toBe(409);
    expect(await second.json()).toMatchObject({ error: 'full' });
  });

  it('tell the host when a guest leaves', async () => {
    const { code, hostToken } = await createRoom(await newKey());
    const host = await connect(code, hostToken);
    const { token, peer } = await (
      await join(await newKey(), code)
    ).json<{ token: string; peer: number }>();
    const guest = await connect(code, token);
    expect(await host.next()).toEqual({ t: 'guest', peer });
    guest.ws.close(1000, 'bye');
    expect(await host.next()).toEqual({ t: 'guest-left', peer });
  });

  it('refuse unsigned requests, malformed codes and unknown tokens', async () => {
    const unsigned = await fetchWorker(new Request(`${BASE}/v1/rooms`, { method: 'POST' }));
    expect(unsigned.status).toBe(401);
    expect((await join(await newKey(), 'NOT-A-CODE')).status).toBe(404);
    const { code } = await createRoom(await newKey());
    const bad = await fetchWorker(
      new Request(`${BASE}/v1/rooms/${code}/ws?token=nope`, { headers: { upgrade: 'websocket' } }),
    );
    expect(bad.status).toBe(404);
  });

  it('limit join-code lookups per IP (guessing)', async () => {
    const key = await newKey();
    const statuses: number[] = [];
    for (let i = 0; i <= JOIN_LIMIT.burst; i++) {
      statuses.push((await join(key, newCode(), '203.0.113.77')).status);
    }
    expect(statuses.slice(0, -1).every((s) => s === 404)).toBe(true);
    expect(statuses.at(-1)).toBe(429);
  });
});

describe('TURN credentials', () => {
  it('fall back to STUN without a TURN key', async () => {
    const res = await fetchWorker(
      await signedRequest(await newKey(), 'POST', `${BASE}/v1/turn`, '{}'),
    );
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ iceServers: STUN_ONLY });
  });

  it('mint ICE servers with the key, and fall back when the service fails', async () => {
    const minted = {
      iceServers: [
        { urls: ['stun:stun.cloudflare.com:3478'] },
        { urls: ['turn:turn.cloudflare.com:3478?transport=udp'], username: 'u', credential: 'c' },
      ],
    };
    let seen: Request | null = null;
    const ok = ((input: RequestInfo | URL, init?: RequestInit) => {
      seen = new Request(input, init);
      return Promise.resolve(new Response(JSON.stringify(minted), { status: 201 }));
    }) as typeof fetch;
    expect(await iceServers('key-id', 'secret', ok)).toEqual(minted.iceServers);
    const request = seen as Request | null;
    expect(request?.url).toBe(
      'https://rtc.live.cloudflare.com/v1/turn/keys/key-id/credentials/generate-ice-servers',
    );
    expect(request?.headers.get('authorization')).toBe('Bearer secret');

    const failing = (() => Promise.resolve(new Response('no', { status: 500 }))) as typeof fetch;
    expect(await iceServers('key-id', 'secret', failing)).toEqual(STUN_ONLY);
    const throwing = (() => Promise.reject(new Error('offline'))) as typeof fetch;
    expect(await iceServers('key-id', 'secret', throwing)).toEqual(STUN_ONLY);
  });

  it('read either answer shape and drop malformed entries', () => {
    expect(
      parseIceServers({ iceServers: { urls: 'turn:x', username: 'u', credential: 'c' } }),
    ).toEqual([{ urls: 'turn:x', username: 'u', credential: 'c' }]);
    expect(parseIceServers({ iceServers: [{ urls: 7 }, { urls: ['stun:y'] }] })).toEqual([
      { urls: ['stun:y'] },
    ]);
    expect(parseIceServers({ iceServers: [] })).toBeNull();
    expect(parseIceServers('nope')).toBeNull();
  });
});
