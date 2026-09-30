import { describe, expect, it } from 'vitest';
import type { KeyValueStore } from '../local/worldIndex';
import { loadRecent, MAX_RECENT, RECENT_KEY, rememberServer } from './recentServers';

function memoryStore(): KeyValueStore & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => {
      data.set(k, v);
    },
  };
}

const invite = (host: string, cert = 'aa') => ({ join: host, cert });

describe('recent servers', () => {
  it('lists joined servers newest first, one entry per address', () => {
    const store = memoryStore();
    rememberServer(store, invite('a:1'), 1);
    rememberServer(store, invite('b:1'), 2);
    rememberServer(store, invite('a:1', 'bb'), 3);
    expect(loadRecent(store)).toEqual([
      { invite: invite('a:1', 'bb'), label: 'a:1', joinedAt: 3 },
      { invite: invite('b:1'), label: 'b:1', joinedAt: 2 },
    ]);
  });

  it(`keeps at most ${String(MAX_RECENT)}`, () => {
    const store = memoryStore();
    for (let i = 0; i < MAX_RECENT + 3; i++) rememberServer(store, invite(`h:${String(i)}`), i);
    const recent = loadRecent(store);
    expect(recent).toHaveLength(MAX_RECENT);
    expect(recent[0]?.label).toBe(`h:${String(MAX_RECENT + 2)}`);
  });

  it('ignores damaged storage', () => {
    const store = memoryStore();
    store.setItem(
      RECENT_KEY,
      '[{"label":1}, "x", {"invite":{"join":"a:1"},"label":"a:1","joinedAt":1}]',
    );
    expect(loadRecent(store)).toEqual([]);
    store.setItem(RECENT_KEY, 'nope');
    expect(loadRecent(store)).toEqual([]);
    expect(loadRecent(null)).toEqual([]);
  });
});
