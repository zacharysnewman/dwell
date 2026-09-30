import { env, exports } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { normalizeCode } from '../src/codes';
import { parseAddress, parseServerReport, type ServerReport } from '../src/servers';
import { newKey, signedRequest, type TestKey } from './sign';

const BASE = 'https://master.test';
const fetchWorker = (request: Request) => exports.default.fetch(request);
const CERT = 'ab'.repeat(32);
const ICE = `ufrag123:${'p'.repeat(24)}`;

function report(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    port: 4433,
    rtcPort: 4434,
    ice: ICE,
    cert: CERT,
    lan: ['192.168.1.50'],
    name: 'Home server',
    motd: 'Welcome',
    players: 2,
    maxPlayers: 16,
    protocol: 10,
    visibility: 'unlisted',
    heartbeatS: 30,
    ...overrides,
  };
}

async function post(key: TestKey, path: string, body: unknown, ip: string): Promise<Response> {
  return fetchWorker(
    await signedRequest(key, 'POST', `${BASE}${path}`, JSON.stringify(body), {
      headers: { 'cf-connecting-ip': ip },
    }),
  );
}

async function register(key: TestKey, ip: string, overrides: Record<string, unknown> = {}) {
  const res = await post(key, '/v1/servers', report(overrides), ip);
  expect(res.status).toBe(200);
  return res.json<{ code: string; display: string; heartbeatS: number }>();
}

describe('server reports', () => {
  it('are checked and clamped', () => {
    const ok = parseServerReport(
      report({ name: 'A\u0007B', heartbeatS: 1, lan: ['10.0.0.2', 'x'] }),
    );
    expect(typeof ok).toBe('object');
    const r = ok as ServerReport;
    expect(r.name).toBe('AB');
    expect(r.heartbeatS).toBe(2);
    expect(r.lan).toEqual(['10.0.0.2']);
    expect(parseServerReport(report({ port: 0 }))).toMatch(/port/);
    expect(parseServerReport(report({ cert: 'nope' }))).toMatch(/cert/);
    expect(parseServerReport(report({ ice: 'bad' }))).toMatch(/ice/);
    expect(parseServerReport(report({ advertise: 'not a host!' }))).toMatch(/advertise/);
  });

  it('typed addresses: host, host:port, IPv6 with or without brackets', () => {
    expect(parseAddress('192.168.1.50')).toEqual({ host: '192.168.1.50', port: null });
    expect(parseAddress(' 192.168.1.50:4433 ')).toEqual({ host: '192.168.1.50', port: 4433 });
    expect(parseAddress('Play.Example.com:5000')).toEqual({ host: 'play.example.com', port: 5000 });
    expect(parseAddress('[::1]:4433')).toEqual({ host: '::1', port: 4433 });
    expect(parseAddress('fe80::1')).toEqual({ host: 'fe80::1', port: null });
    expect(parseAddress('host:99999')).toBeNull();
    expect(parseAddress('http://x')).toBeNull();
  });
});

describe('dedicated servers', () => {
  it('keep one join code per server key across heartbeats', async () => {
    const key = await newKey();
    const first = await register(key, '203.0.113.10');
    expect(normalizeCode(first.display)).toBe(first.code);
    const again = await register(key, '203.0.113.10', { players: 3 });
    expect(again.code).toBe(first.code);
    const other = await register(await newKey(), '203.0.113.11');
    expect(other.code).not.toBe(first.code);
  });

  it('resolve by code, by public address, and by LAN address only on their own network', async () => {
    const server = await newKey();
    const { code } = await register(server, '203.0.113.20');
    const player = await newKey();
    const resolve = (body: unknown, ip: string) => post(player, '/v1/resolve', body, ip);

    // From elsewhere: the public address.
    let res = await resolve({ code }, '198.51.100.7');
    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({
      kind: 'server',
      server: { host: '203.0.113.20', port: 4433, cert: CERT, rtcPort: 4434, ice: ICE, code },
    });
    res = await resolve({ address: '203.0.113.20' }, '198.51.100.7');
    expect(await res.json()).toMatchObject({ server: { host: '203.0.113.20', code } });
    expect((await resolve({ address: '203.0.113.20:5000' }, '198.51.100.7')).status).toBe(404);
    // A LAN address means nothing from another network…
    expect((await resolve({ address: '192.168.1.50' }, '198.51.100.7')).status).toBe(404);
    // …but works from the server's own (and the code then leads to the LAN address).
    res = await resolve({ address: '192.168.1.50:4433' }, '203.0.113.20');
    expect(await res.json()).toMatchObject({ server: { host: '192.168.1.50', code } });
    res = await resolve({ code }, '203.0.113.20');
    expect(await res.json()).toMatchObject({ server: { host: '192.168.1.50' } });
  });

  it('use an advertised host instead of the request address', async () => {
    const { code } = await register(await newKey(), '203.0.113.30', {
      advertise: 'play.example.com',
      lan: [],
    });
    const res = await post(
      await newKey(),
      '/v1/resolve',
      { address: 'play.example.com' },
      '1.2.3.4',
    );
    expect(await res.json()).toMatchObject({ server: { host: 'play.example.com', code } });
  });

  it('disappear when they leave, and after two missed heartbeats', async () => {
    const player = await newKey();
    const leaving = await newKey();
    const { code } = await register(leaving, '203.0.113.40');
    expect((await post(leaving, '/v1/servers/leave', {}, '203.0.113.40')).status).toBe(200);
    expect((await post(player, '/v1/resolve', { code }, '9.9.9.9')).status).toBe(404);
    // Its code stays its own when it comes back.
    expect((await register(leaving, '203.0.113.40')).code).toBe(code);

    const stub = env.DIRECTORY.get(env.DIRECTORY.idFromName('global'));
    const quiet = await newKey();
    const r = parseServerReport(report({ heartbeatS: 5 })) as ServerReport;
    const t0 = 1_000_000;
    const { code: quietCode } = await stub.registerServer(quiet.publicHex, '203.0.113.41', r, t0);
    expect(await stub.serverByCode(quietCode, 'x', t0 + 9_999)).not.toBeNull();
    expect(await stub.serverByCode(quietCode, 'x', t0 + 10_000)).toBeNull();
  });

  it('list servers and "same network" friend worlds to players on that network', async () => {
    const ip = '203.0.113.50';
    const server = await newKey();
    const { code } = await register(server, ip, { name: 'Basement' });
    const host = await newKey();
    const created = await post(
      host,
      '/v1/rooms',
      { maxGuests: 4, visibility: 'network', name: 'Bravo' },
      ip,
    );
    expect(created.status).toBe(201);
    const room = await created.json<{ code: string; hostToken: string }>();
    // A code-only world is not listed.
    await post(host, '/v1/rooms', { maxGuests: 4, name: 'Secret' }, ip);
    // The room is open once its host connects.
    const ws = await fetchWorker(
      new Request(`${BASE}/v1/rooms/${room.code}/ws?token=${room.hostToken}`, {
        headers: { upgrade: 'websocket' },
      }),
    );
    ws.webSocket?.accept();

    const player = await newKey();
    const here = await (
      await post(player, '/v1/nearby', {}, ip)
    ).json<{
      servers: { code: string; name: string; host: string }[];
      worlds: { code: string; name: string }[];
    }>();
    expect(here.servers).toEqual([
      expect.objectContaining({ code, name: 'Basement', host: '192.168.1.50' }),
    ]);
    expect(here.worlds).toEqual([expect.objectContaining({ code: room.code, name: 'Bravo' })]);
    const elsewhere = await (
      await post(player, '/v1/nearby', {}, '198.51.100.1')
    ).json<{
      servers: unknown[];
      worlds: unknown[];
    }>();
    expect(elsewhere).toEqual({ servers: [], worlds: [] });

    // A code resolves to the room it names.
    const res = await post(player, '/v1/resolve', { code: room.code }, '198.51.100.1');
    expect(await res.json()).toMatchObject({ kind: 'room', code: room.code });

    // Once the host leaves, the world is no longer listed.
    ws.webSocket?.close(1000, 'bye');
    await new Promise((r) => setTimeout(r, 50));
    const after = await (await post(player, '/v1/nearby', {}, ip)).json<{ worlds: unknown[] }>();
    expect(after.worlds).toEqual([]);
  });

  it('refuse malformed reports and unsigned registrations', async () => {
    expect(
      (await post(await newKey(), '/v1/servers', report({ cert: 'x' }), '1.1.1.1')).status,
    ).toBe(400);
    const unsigned = await fetchWorker(
      new Request(`${BASE}/v1/servers`, { method: 'POST', body: JSON.stringify(report()) }),
    );
    expect(unsigned.status).toBe(401);
  });
});
