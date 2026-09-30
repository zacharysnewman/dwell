import { env, exports } from 'cloudflare:workers';
import { runInDurableObject } from 'cloudflare:test';
import { describe, expect, it } from 'vitest';
import { MIGRATIONS, type Directory } from '../src/directory';
import { MAX_BODY_BYTES } from '../src/limits';
import { SIGNED_LIMITS } from '../src/rateLimit';
import { newKey, signedRequest } from './sign';

const BASE = 'https://master.test';
const fetchWorker = (request: Request) => exports.default.fetch(request);

describe('master worker', () => {
  it('exports only handlers and Durable Object classes (workerd refuses anything else)', async () => {
    const main: Record<string, unknown> = { ...(await import('../src/index')) };
    for (const [name, value] of Object.entries(main)) {
      expect(
        typeof value === 'function' || (typeof value === 'object' && value !== null),
        name,
      ).toBe(true);
    }
  });

  it('answers health checks', async () => {
    const res = await fetchWorker(new Request(`${BASE}/v1/health`));
    expect(res.status).toBe(200);
    expect(await res.json()).toMatchObject({ ok: true, service: 'dwell-master', api: 1 });
    expect((await fetchWorker(new Request(`${BASE}/v1/health`, { method: 'POST' }))).status).toBe(
      405,
    );
  });

  it('treats repeated slashes as one (a base URL ending in / plus /v1/…)', async () => {
    for (const path of ['//v1/health', '/v1//health', '/v1/health/']) {
      const res = await fetchWorker(new Request(`${BASE}${path}`));
      expect(res.status, path).toBe(200);
    }
  });

  it('answers unknown paths with a JSON 404', async () => {
    const res = await fetchWorker(new Request(`${BASE}/nope`));
    expect(res.status).toBe(404);
    expect(await res.json()).toMatchObject({ error: 'not_found' });
  });

  it('allows CORS from the listed origins only', async () => {
    const preflight = (origin: string) =>
      fetchWorker(
        new Request(`${BASE}/v1/whoami`, {
          method: 'OPTIONS',
          headers: { origin, 'access-control-request-method': 'POST' },
        }),
      );
    const ok = await preflight('https://dropkickarcade.com');
    expect(ok.status).toBe(204);
    expect(ok.headers.get('access-control-allow-origin')).toBe('https://dropkickarcade.com');
    expect(ok.headers.get('access-control-allow-headers')).toContain('x-dwell-signature');
    const other = await preflight('https://example.com');
    expect(other.headers.get('access-control-allow-origin')).toBeNull();
    const get = await fetchWorker(
      new Request(`${BASE}/v1/health`, { headers: { origin: 'http://localhost:5173' } }),
    );
    expect(get.headers.get('access-control-allow-origin')).toBe('http://localhost:5173');
  });

  it('answers a signed request with the signer’s key', async () => {
    const key = await newKey();
    const res = await fetchWorker(await signedRequest(key, 'POST', `${BASE}/v1/whoami`, '{}'));
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ key: key.publicHex });
  });

  it('refuses unsigned, stale and oversized requests', async () => {
    const key = await newKey();
    const unsigned = await fetchWorker(new Request(`${BASE}/v1/whoami`, { method: 'POST' }));
    expect(unsigned.status).toBe(401);
    expect(await unsigned.json()).toMatchObject({ error: 'unsigned' });
    const stale = await signedRequest(key, 'POST', `${BASE}/v1/whoami`, '', {
      time: Date.now() - 120_000,
    });
    expect(await (await fetchWorker(stale)).json()).toMatchObject({ error: 'clock' });
    const big = await signedRequest(
      key,
      'POST',
      `${BASE}/v1/whoami`,
      'x'.repeat(MAX_BODY_BYTES + 1),
    );
    expect((await fetchWorker(big)).status).toBe(413);
  });

  it('rate-limits each key', async () => {
    const key = await newKey();
    const statuses: number[] = [];
    for (let i = 0; i <= SIGNED_LIMITS.key.burst; i++) {
      const req = await signedRequest(key, 'POST', `${BASE}/v1/whoami`, '', {
        headers: { 'cf-connecting-ip': `10.0.0.${String(i)}` },
      });
      statuses.push((await fetchWorker(req)).status);
    }
    expect(statuses.slice(0, -1).every((s) => s === 200)).toBe(true);
    expect(statuses.at(-1)).toBe(429);
    // Another key is unaffected.
    const other = await signedRequest(await newKey(), 'POST', `${BASE}/v1/whoami`);
    expect((await fetchWorker(other)).status).toBe(200);
  });
});

describe('Directory', () => {
  it('migrates its schema to the latest version', async () => {
    const stub = env.DIRECTORY.get(env.DIRECTORY.idFromName('global'));
    expect(await stub.schemaVersion()).toBe(MIGRATIONS.length);
    await runInDurableObject(stub, (instance: Directory) => {
      expect(instance.schemaVersion()).toBe(MIGRATIONS.length);
    });
  });

  it('limits each key and each IP (at a fixed time, so no refill)', async () => {
    const stub = env.DIRECTORY.get(env.DIRECTORY.idFromName('limits-test'));
    const now = 1_000_000;
    for (let i = 0; i < SIGNED_LIMITS.key.burst; i++) {
      expect(await stub.admit('key-a', `ip-${String(i)}`, now)).toEqual({ ok: true });
    }
    expect(await stub.admit('key-a', 'ip-x', now)).toEqual({ ok: false, retryAfterS: 1 });
    for (let i = 0; i < SIGNED_LIMITS.ip.burst; i++) {
      expect(await stub.admit(`key-${String(i)}`, 'ip-shared', now)).toEqual({ ok: true });
    }
    expect(await stub.admit('key-new', 'ip-shared', now)).toMatchObject({ ok: false });
    // A second later, one more of each.
    expect(await stub.admit('key-a', 'ip-y', now + 1000)).toEqual({ ok: true });
  });

  it('has a Room class ready for signaling (Phase 5c)', async () => {
    const room = env.ROOM.get(env.ROOM.idFromName('test'));
    expect((await room.fetch(new Request(`${BASE}/`))).status).toBe(501);
  });
});
