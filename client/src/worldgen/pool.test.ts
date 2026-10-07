import { afterEach, describe, expect, it, vi } from 'vitest';
import type { ChunkCoord } from '../protocol/messages';
import { defaultWorkerCount, WorldgenPool, type WorkerLike } from './pool';

/** A worker that replies when the test says so. */
class FakeWorker implements WorkerLike {
  onmessage: ((e: MessageEvent) => void) | null = null;
  received: { t: string; id?: number; coord?: ChunkCoord }[] = [];
  terminated = false;
  postMessage(message: unknown): void {
    this.received.push(message as { t: string });
  }
  terminate(): void {
    this.terminated = true;
  }
  reply(data: unknown): void {
    this.onmessage?.({ data } as MessageEvent);
  }
  jobs(): ChunkCoord[] {
    return this.received.flatMap((m) => (m.t === 'generate' && m.coord ? [m.coord] : []));
  }
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe('WorldgenPool', () => {
  it('initialises every worker with the world, then runs jobs in request order', async () => {
    const workers = [new FakeWorker(), new FakeWorker()];
    const pool = new WorldgenPool(workers, 2, 99n);
    for (const w of workers) {
      expect(w.received[0]).toEqual({ t: 'init', generatorVersion: 2, worldSeed: 99n });
    }
    const results: number[] = [];
    for (const x of [1, 2, 3])
      void pool.generate([x, 0, 0]).then((c) => results.push(c.voxels[0] ?? -1));
    expect(pool.pending).toBe(3);
    // Nothing runs before the workers are ready.
    expect(workers.every((w) => w.jobs().length === 0)).toBe(true);
    for (const w of workers) w.reply({ t: 'ready' });
    // The first worker ready takes the queue (up to its limit); later jobs go to the least busy.
    const [w0, w1] = workers as [FakeWorker, FakeWorker];
    expect(w0.jobs().map((c) => c[0])).toEqual([1, 2, 3]);
    void pool.generate([4, 0, 0]).then((c) => results.push(c.voxels[0] ?? -1));
    expect(w1.jobs().map((c) => c[0])).toEqual([4]);
    // Each worker answers in order.
    w0.reply({ t: 'chunk', voxels: new Uint16Array(4).fill(1), hash: 1n });
    await flush();
    expect(results).toEqual([1]);
    w1.reply({ t: 'chunk', voxels: new Uint16Array(4).fill(4), hash: 4n });
    w0.reply({ t: 'chunk', voxels: new Uint16Array(4).fill(2), hash: 2n });
    w0.reply({ t: 'chunk', voxels: new Uint16Array(4).fill(3), hash: 3n });
    await flush();
    expect(results).toEqual([1, 4, 2, 3]);
    expect(pool.pending).toBe(0);
  });

  it('holds at most a few jobs per worker; the rest wait (and can be cancelled)', () => {
    const w = new FakeWorker();
    const pool = new WorldgenPool([w], 2, 0n);
    w.reply({ t: 'ready' });
    for (let x = 0; x < 10; x++) void pool.generate([x, 0, 0]);
    expect(w.jobs().length).toBe(4);
    expect(pool.pending).toBe(10);
    expect(pool.cancel([9, 0, 0])).toBe(true);
    expect(pool.pending).toBe(9);
  });

  it('cancels queued jobs but not running ones', () => {
    const w = new FakeWorker();
    const pool = new WorldgenPool([w], 2, 0n);
    w.reply({ t: 'ready' });
    for (let x = 1; x <= 5; x++) void pool.generate([x, 0, 0]);
    expect(pool.cancel([5, 0, 0])).toBe(true); // queued
    expect(pool.cancel([1, 0, 0])).toBe(false); // handed to the worker
    expect(pool.pending).toBe(4);
  });

  it('rejects jobs when a worker cannot load the generator', async () => {
    const w = new FakeWorker();
    const pool = new WorldgenPool([w], 2, 0n);
    const job = pool.generate([0, 0, 0]);
    w.reply({ t: 'error', message: 'no wasm' });
    await expect(job).rejects.toThrow('no wasm');
    await expect(pool.generate([1, 0, 0])).rejects.toThrow('no wasm');
    pool.terminate();
    expect(w.terminated).toBe(true);
  });
});

describe('worker count', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('is cores − 2 (1 to 4), and at most 2 on phones (each worker counts against the tab)', () => {
    for (const [cores, desktop, phone] of [
      [2, 1, 1],
      [4, 2, 2],
      [6, 4, 2],
      [12, 4, 2],
    ] as const) {
      vi.stubGlobal('navigator', { hardwareConcurrency: cores });
      expect(defaultWorkerCount()).toBe(desktop);
      expect(defaultWorkerCount(true)).toBe(phone);
    }
  });

  it("asks a worker for a chunk column's tint grid and resolves with its bytes", async () => {
    const w = new FakeWorker();
    const pool = new WorldgenPool([w], 3, 0n);
    w.reply({ t: 'ready' });
    const got = pool.tint(7, -2);
    const sent = w.received.find((m) => m.t === 'tint') as unknown as {
      id: number;
      cx: number;
      cz: number;
    };
    expect([sent.cx, sent.cz]).toEqual([7, -2]);
    const bytes = new Uint8Array([9, 8, 7]);
    w.reply({ t: 'tint', id: sent.id, bytes });
    await expect(got).resolves.toBe(bytes);
    // A generator without biomes answers null.
    const none = pool.tint(0, 0);
    const second = w.received.filter((m) => m.t === 'tint')[1] as unknown as { id: number };
    w.reply({ t: 'tint', id: second.id, bytes: null });
    await expect(none).resolves.toBeNull();
  });
});
