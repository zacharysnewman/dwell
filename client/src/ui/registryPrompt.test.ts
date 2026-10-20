import { describe, expect, it } from 'vitest';
import {
  GROUP_TITLES,
  REGISTRY_TRUST_KEY,
  promptGroups,
  rememberTrust,
  trustRecord,
  trustedHash,
  type RegistryDifferences,
} from './registryPrompt';

function store(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => {
      data.set(k, v);
    },
  };
}

const registry: RegistryDifferences = {
  hash: '0123456789abcdef',
  different: [{ name: 'dwell:sand', fields: ['density', 'shape'] }],
  added: [{ name: 'test:glow', fields: [] }],
  clientOnly: ['dwell:ladder'],
};

describe('registry prompt', () => {
  it('lists all three groups, each item with what differs', () => {
    const groups = promptGroups(registry);
    expect(groups.map((g) => g.title)).toEqual([
      'Different on this server',
      'New on this server (shown as placeholders)',
      'Only in your version (not used here)',
    ]);
    expect(groups[0]?.items).toEqual([{ name: 'dwell:sand', detail: 'density, shape' }]);
    expect(groups[1]?.items).toEqual([{ name: 'test:glow', detail: '' }]);
    expect(groups[2]?.items).toEqual([{ name: 'dwell:ladder', detail: '' }]);
  });

  it('leaves out empty groups', () => {
    const groups = promptGroups({ ...registry, different: [], clientOnly: [] });
    expect(groups.map((g) => g.title)).toEqual([GROUP_TITLES.added]);
  });

  it("'don't ask again' holds only for the same server and hash", () => {
    const s = store();
    expect(trustedHash(s, 'invite:a:1')).toBeNull();
    rememberTrust(s, 'invite:a:1', registry.hash, 5);
    expect(trustedHash(s, 'invite:a:1')).toBe(registry.hash);
    expect(trustedHash(s, 'invite:b:1')).toBeNull();
    // The session compares it with the server's hash: a changed server asks again.
    expect(trustedHash(s, 'invite:a:1') === 'fedcba9876543210').toBe(false);
  });

  it('keeps other fields and servers on rewrite, and never overwrites an unreadable record', () => {
    const stored = {
      future: { x: 1 },
      servers: { 'code:K': { hash: 'aaaaaaaaaaaaaaaa', at: 1, note: 'kept' } },
    };
    const next = trustRecord(stored, 'code:K', registry.hash, 9);
    expect(next.future).toEqual({ x: 1 });
    expect((next.servers as Record<string, unknown>)['code:K']).toEqual({
      hash: registry.hash,
      at: 9,
      note: 'kept',
    });
    const s = store({ [REGISTRY_TRUST_KEY]: JSON.stringify(stored) });
    rememberTrust(s, 'local:w', registry.hash, 2);
    const saved = JSON.parse(s.data.get(REGISTRY_TRUST_KEY) ?? '{}') as typeof stored;
    expect(saved.future).toEqual({ x: 1 });
    expect(Object.keys(saved.servers)).toEqual(['code:K', 'local:w']);
    const broken = store({ [REGISTRY_TRUST_KEY]: '{not json' });
    rememberTrust(broken, 'local:w', registry.hash, 2);
    expect(broken.data.get(REGISTRY_TRUST_KEY)).toBe('{not json');
    expect(trustedHash(broken, 'local:w')).toBeNull();
  });
});
