import { describe, expect, it } from 'vitest';
import vectors from '../../../shared/master/vectors.json';
import { fromHex, signingMessage, toHex, verifyRequest } from '../src/auth';

const URL_BASE = 'https://master.test';

function request(c: (typeof vectors.cases)[number], headers: Partial<Record<string, string>> = {}) {
  return new Request(URL_BASE + c.path, {
    method: c.method,
    headers: {
      'x-dwell-key': vectors.publicKey,
      'x-dwell-time': String(c.time),
      'x-dwell-signature': c.signature,
      ...headers,
    } as Record<string, string>,
  });
}

const body = (text: string) => new TextEncoder().encode(text);

describe('signed requests', () => {
  it('builds the shared vectors’ messages', async () => {
    for (const c of vectors.cases) {
      expect(toHex(await signingMessage(c.method, c.path, c.time, body(c.body)))).toBe(c.message);
    }
  });

  it('accepts the shared vectors', async () => {
    for (const c of vectors.cases) {
      expect(await verifyRequest(request(c), body(c.body), c.time)).toEqual({
        ok: true,
        key: vectors.publicKey,
      });
    }
  });

  it('rejects a changed body, path, method or time', async () => {
    const c = vectors.cases[1];
    if (!c) throw new Error('no vector');
    const fail = { ok: false, code: 'signature' };
    expect(await verifyRequest(request(c), body(c.body + ' '), c.time)).toMatchObject(fail);
    const moved = new Request(`${URL_BASE}/v1/servers/register?x=2`, request(c));
    expect(await verifyRequest(moved, body(c.body), c.time)).toMatchObject(fail);
    const method = new Request(request(c), { method: 'PUT' });
    expect(await verifyRequest(method, body(c.body), c.time)).toMatchObject(fail);
    const later = request(c, { 'x-dwell-time': String(c.time + 1) });
    expect(await verifyRequest(later, body(c.body), c.time)).toMatchObject(fail);
  });

  it('rejects requests outside the clock window', async () => {
    const c = vectors.cases[0];
    if (!c) throw new Error('no vector');
    expect(await verifyRequest(request(c), body(c.body), c.time + 60_000)).toMatchObject({
      ok: true,
    });
    expect(await verifyRequest(request(c), body(c.body), c.time + 60_001)).toMatchObject({
      ok: false,
      code: 'clock',
    });
    expect(await verifyRequest(request(c), body(c.body), c.time - 60_001)).toMatchObject({
      code: 'clock',
    });
  });

  it('rejects missing or malformed headers, and keys that are not points', async () => {
    const c = vectors.cases[0];
    if (!c) throw new Error('no vector');
    const unsigned = { ok: false, code: 'unsigned' };
    const plain = new Request(URL_BASE + c.path, { method: c.method });
    expect(await verifyRequest(plain, body(''), c.time)).toMatchObject(unsigned);
    for (const headers of [
      { 'x-dwell-key': vectors.publicKey.slice(2) },
      { 'x-dwell-signature': 'zz' + c.signature.slice(2) },
      { 'x-dwell-time': '-1' },
      { 'x-dwell-time': '1e12' },
    ]) {
      expect(await verifyRequest(request(c, headers), body(c.body), c.time)).toMatchObject(
        unsigned,
      );
    }
    const notAPoint = request(c, { 'x-dwell-key': 'ff'.repeat(32) });
    expect(await verifyRequest(notAPoint, body(c.body), c.time)).toMatchObject({ ok: false });
  });

  it('converts hex both ways', () => {
    expect(toHex(fromHex('00ff10') ?? new Uint8Array())).toBe('00ff10');
    expect(fromHex('abc')).toBeNull();
    expect(fromHex('zz')).toBeNull();
  });
});
