import { describe, expect, it } from 'vitest';
import { loadOrCreateDeviceKey, MemoryKeyStore } from './deviceKey';

describe('device key', () => {
  it('creates a key once and reuses it', async () => {
    const store = new MemoryKeyStore();
    const a = await loadOrCreateDeviceKey(store);
    const b = await loadOrCreateDeviceKey(store);
    expect(a.publicKey).toHaveLength(32);
    expect(b.publicKey).toEqual(a.publicKey);
  });

  it('produces verifiable Ed25519 signatures with a non-extractable private key', async () => {
    const store = new MemoryKeyStore();
    const key = await loadOrCreateDeviceKey(store);
    const data = new TextEncoder().encode('transcript');
    const sig = await key.sign(data);
    expect(sig).toHaveLength(64);
    const pub = await crypto.subtle.importKey('raw', key.publicKey, 'Ed25519', false, ['verify']);
    expect(await crypto.subtle.verify('Ed25519', pub, sig, data)).toBe(true);
    const pair = await store.load();
    expect(pair?.privateKey.extractable).toBe(false);
  });
});
