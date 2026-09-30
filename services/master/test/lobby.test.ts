import { exports } from 'cloudflare:workers';
import { describe, expect, it } from 'vitest';
import { RECEIPT_AFTER_RESOLVE_MS, VERIFIED_PLAYERS } from '../src/directory';

// VERIFIED_PLAYERS is at least 2: one player's receipts alone never verify a server.
import { matchesQuery, parseListQuery, parseServerReport, type ServerReport } from '../src/servers';
import { newKey, signedRequest, type TestKey } from './sign';

const BASE = 'https://master.test';
const fetchWorker = (request: Request) => exports.default.fetch(request);

function report(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    port: 4433,
    cert: 'cd'.repeat(32),
    lan: ['192.168.7.9'],
    name: 'Lobby server',
    motd: 'Hello',
    players: 1,
    maxPlayers: 8,
    protocol: 10,
    visibility: 'public',
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
  return (await res.json<{ code: string }>()).code;
}

interface Listed {
  servers: { code: string; name: string; verified: boolean; host: string; tags: string[] }[];
  worlds: { code: string; name: string; players: number; maxPlayers: number }[];
}

async function list(query: string, ip = '198.51.100.99'): Promise<Listed> {
  const res = await fetchWorker(
    new Request(`${BASE}/v1/servers?${query}`, { headers: { 'cf-connecting-ip': ip } }),
  );
  expect(res.status).toBe(200);
  return res.json<Listed>();
}

/** A player joins the server with this code through the master and posts its receipt. */
async function joinAndReceipt(code: string, ip: string): Promise<Response> {
  const player = await newKey();
  expect((await post(player, '/v1/resolve', { code }, ip)).status).toBe(200);
  return post(player, '/v1/receipts', { code }, ip);
}

describe('lobby-list queries', () => {
  it('read search words and filters from the URL', () => {
    const q = parseListQuery(
      new URLSearchParams('q=Big  Castle&tag=PvE&protocol=10&notFull=1&new=true&limit=500'),
    );
    expect(q).toEqual({
      words: ['big', 'castle'],
      tag: 'pve',
      protocol: 10,
      notFull: true,
      hasPlayers: false,
      fresh: true,
      limit: 100,
    });
    expect(parseListQuery(new URLSearchParams('protocol=x&limit=-1'))).toMatchObject({
      protocol: null,
      limit: 100,
    });
  });

  it('match name, MOTD and tags, player counts and the protocol', () => {
    const entry = {
      name: 'Big Castle',
      motd: 'Builders welcome',
      tags: ['pve', 'creative'],
      players: 3,
      maxPlayers: 3,
      protocol: 10,
    };
    const q = (s: string) => parseListQuery(new URLSearchParams(s));
    expect(matchesQuery(entry, q('q=castle builders'))).toBe(true);
    expect(matchesQuery(entry, q('q=creative'))).toBe(true);
    expect(matchesQuery(entry, q('q=survival'))).toBe(false);
    expect(matchesQuery(entry, q('tag=pve'))).toBe(true);
    expect(matchesQuery(entry, q('tag=pvp'))).toBe(false);
    expect(matchesQuery(entry, q('protocol=9'))).toBe(false);
    expect(matchesQuery(entry, q('notFull=1'))).toBe(false);
    expect(matchesQuery({ ...entry, players: 0 }, q('hasPlayers=1'))).toBe(false);
  });

  it('server tags are cleaned: lower case, a-z 0-9 -, at most 8', () => {
    const r = parseServerReport(
      report({ tags: ['PvE', 'pve', 'no spaces', 'x'.repeat(25), ...'abcdefghij'.split('')] }),
    ) as ServerReport;
    expect(r.tags).toEqual(['pve', 'a', 'b', 'c', 'd', 'e', 'f', 'g']);
  });
});

describe('the lobby list', () => {
  it('shows a public server as verified only once enough distinct players have joined it', async () => {
    const name = `Receipts ${crypto.randomUUID()}`;
    const code = await register(await newKey(), '203.0.113.70', { name, tags: ['pve'] });
    const find = async (query: string) =>
      (await list(`${query}&q=${encodeURIComponent(name)}`)).servers.map((s) => s.code);

    // Without receipts it is only under "new".
    expect(await find('')).toEqual([]);
    expect(await find('new=1')).toEqual([code]);

    // A receipt without having resolved the server first does not count.
    const stranger = await newKey();
    expect((await post(stranger, '/v1/receipts', { code }, '198.51.100.3')).status).toBe(404);

    // The same player twice is still one player.
    const player = await newKey();
    for (let i = 0; i < 2; i++) {
      await post(player, '/v1/resolve', { code }, '198.51.100.4');
      const res = await post(player, '/v1/receipts', { code }, '198.51.100.4');
      expect(await res.json()).toEqual({ ok: true, verified: false });
    }
    // One resolve entitles one receipt.
    expect((await post(player, '/v1/receipts', { code }, '198.51.100.4')).status).toBe(404);
    expect(await find('')).toEqual([]);

    for (let i = 1; i < VERIFIED_PLAYERS; i++) {
      const res = await joinAndReceipt(code, `198.51.100.${String(10 + i)}`);
      expect(await res.json()).toEqual({ ok: true, verified: i + 1 >= VERIFIED_PLAYERS });
    }
    const listed = await list(`q=${encodeURIComponent(name)}`);
    expect(listed.servers).toEqual([
      expect.objectContaining({ code, name, verified: true, tags: ['pve'], host: '203.0.113.70' }),
    ]);
    expect(await find('new=1')).toEqual([]);
    expect(await find('tag=pvp')).toEqual([]);
  });

  it('never lists unlisted servers, and a receipt must follow its resolve closely', async () => {
    const name = `Hidden ${crypto.randomUUID()}`;
    const code = await register(await newKey(), '203.0.113.71', { name, visibility: 'unlisted' });
    expect((await list(`new=1&q=${encodeURIComponent(name)}`)).servers).toEqual([]);
    expect((await list(`q=${encodeURIComponent(name)}`)).servers).toEqual([]);
    expect(RECEIPT_AFTER_RESOLVE_MS).toBeGreaterThanOrEqual(60_000);
    // Unlisted servers still take receipts (they may become public later).
    expect((await joinAndReceipt(code, '198.51.100.20')).status).toBe(200);
  });

  it('shows public friend worlds with their players while the host is connected', async () => {
    const name = `World ${crypto.randomUUID()}`;
    const host = await newKey();
    const created = await post(
      host,
      '/v1/rooms',
      { maxGuests: 3, visibility: 'public', name, protocol: 10 },
      '203.0.113.80',
    );
    expect(created.status).toBe(201);
    const room = await created.json<{ code: string; hostToken: string }>();
    // A "network" world is not in the lobby list.
    await post(host, '/v1/rooms', { maxGuests: 3, visibility: 'network', name }, '203.0.113.80');
    const q = `q=${encodeURIComponent(name)}`;
    expect((await list(q)).worlds).toEqual([]); // the host has not connected yet

    const ws = await fetchWorker(
      new Request(`${BASE}/v1/rooms/${room.code}/ws?token=${room.hostToken}`, {
        headers: { upgrade: 'websocket' },
      }),
    );
    ws.webSocket?.accept();
    expect((await list(q)).worlds).toEqual([
      expect.objectContaining({ code: room.code, name, players: 1, maxPlayers: 4 }),
    ]);
    expect((await list(`${q}&protocol=9`)).worlds).toEqual([]);
    expect((await list(`${q}&new=1`)).worlds).toEqual([]);
    // Listed to the host's network too.
    const nearby = await (
      await post(await newKey(), '/v1/nearby', {}, '203.0.113.80')
    ).json<{ worlds: { code: string }[] }>();
    expect(nearby.worlds.map((w) => w.code)).toContain(room.code);

    ws.webSocket?.close(1000, 'bye');
    await new Promise((r) => setTimeout(r, 50));
    expect((await list(q)).worlds).toEqual([]);
  });

  it('limits queries per IP', async () => {
    const ip = '198.51.100.250';
    const results = await Promise.all(
      Array.from({ length: 40 }, () =>
        fetchWorker(new Request(`${BASE}/v1/servers`, { headers: { 'cf-connecting-ip': ip } })),
      ),
    );
    expect(results.some((r) => r.status === 429)).toBe(true);
  });
});
