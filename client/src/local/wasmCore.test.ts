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
