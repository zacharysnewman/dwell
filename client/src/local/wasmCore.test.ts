// Runs the real WASM server core under Node. Skipped until `npm run build:wasm` has been run,
// unless DWELL_REQUIRE_WASM is set (CI), in which case a missing build fails the test.
import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { Channel, MessageType, TransportKind } from '../protocol/constants.gen';
import { decode, encode } from '../protocol/messages';
import { LocalCore, OutgoingKind, type DwellCoreFactory } from './wasmCore';

const wasmJs = new URL('../../public/wasm/dwell_core.js', import.meta.url);

const skip = !existsSync(wasmJs) && !process.env.DWELL_REQUIRE_WASM;

describe.skipIf(skip)('WASM local core', () => {
  async function load(): Promise<LocalCore> {
    const mod = (await import(/* @vite-ignore */ wasmJs.href)) as { default: DwellCoreFactory };
    return LocalCore.load(mod.default);
  }

  it('answers a status query and advances ticks', async () => {
    const core = await load();
    core.connected(1, TransportKind.Loopback, new Uint8Array(32));
    core.reliable(1, Channel.control, encode({ type: MessageType.StatusRequest }));
    const out = core.takeOutbox();
    expect(out).toHaveLength(1);
    expect(out[0]?.kind).toBe(OutgoingKind.Reliable);
    const reply = decode(out[0]?.bytes ?? new Uint8Array());
    expect(reply.type).toBe(MessageType.StatusResponse);
    expect(core.advance(0.5)).toBe(8); // capped at 8 steps per advance
  });
});

describe('a world file locked to another version (RELEASES.md §6)', () => {
  /** A stand-in core whose `dwell_local_create` returns `result`, with `message` as the storage error. */
  function fakeFactory(result: number, message: string): DwellCoreFactory {
    const heap = new Uint8Array(256);
    const bytes = new TextEncoder().encode(message);
    heap.set(bytes, 16);
    return (() =>
      Promise.resolve({
        HEAPU8: heap,
        _dwell_local_create: () => result,
        _dwell_local_storage_error: () => 16,
      })) as unknown as DwellCoreFactory;
  }

  it('refuses to start, with the reason', async () => {
    const message = 'this world was last saved by Dwell 0.1.1, newer than this build (0.1.0)';
    await expect(
      LocalCore.load(fakeFactory(3, message), 1, 4, { files: new Map() } as never),
    ).rejects.toThrow(message);
  });

  it('starts a world that merely could not be persisted, in memory', async () => {
    const core = await LocalCore.load(fakeFactory(1, 'no space'), 1, 4, {} as never);
    expect(core.persisted).toBe(false);
    expect(core.storageError).toBe('no space');
  });
});
