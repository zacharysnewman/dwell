// Runs the real client sim (WASM) under Node: prediction from a snapshot, input, render faces.
// Skipped until `npm run build:wasm` has been run, unless DWELL_REQUIRE_WASM is set (CI).
import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { ControllerFlags, GroundKind, MessageType, PlayerState } from '../protocol/constants.gen';
import { encode, type ControllerState } from '../protocol/messages';
import { IDLE_INPUT, quantizeInput } from '../predict/input';
import { ClientCore, RENDER_FACE_BYTES } from './clientCore';
import type { DwellCoreFactory } from './module';

const wasmJs = new URL('../../public/wasm/dwell_core.js', import.meta.url);
const skip = !existsSync(wasmJs) && !process.env.DWELL_REQUIRE_WASM;

const controller: ControllerState = {
  flags: ControllerFlags.grounded,
  currentX: 0,
  currentZ: 0,
  externalX: 0,
  externalZ: 0,
  contributionX: 0,
  contributionZ: 0,
  accumulatedY: 0,
  platformY: 0,
  targetY: 0,
  groundVelocityY: 0,
  groundKind: GroundKind.Terrain,
  groundId: 0,
  bufferTicks: 0,
  coyoteTicks: 0,
  stepGrace: 0,
  ladder: [0, 0, 0],
  released: [0, 0],
};

describe.skipIf(skip)('client sim core (WASM)', () => {
  async function load(): Promise<ClientCore> {
    const mod = (await import(/* @vite-ignore */ wasmJs.href)) as { default: DwellCoreFactory };
    return ClientCore.load(mod.default, 1);
  }

  it('starts from a snapshot and predicts movement from input', async () => {
    const core = await load();
    expect(core.state().active).toBe(false);
    const snapshot = encode({
      type: MessageType.PhysicsSnapshot,
      serverTick: 3,
      ackInputSeq: 0,
      local: {
        position: [8.5, 0.9, -8.5],
        velocity: [0, 0, 0],
        flags: 1,
        health: 100,
        state: PlayerState.Idle,
        inputBuffer: 0,
        lastKnockbackSeq: 0,
        controller,
      },
      remotes: [],
    });
    expect(core.snapshot(snapshot)).toBe(true);
    expect(core.state().active).toBe(true);
    expect(core.nextSeq()).toBe(1);
    for (let i = 0; i < 60; i++)
      core.tick(quantizeInput({ ...IDLE_INPUT, moveY: 1 }, core.nextSeq()));
    const s = core.state();
    expect(s.position[2]).toBeGreaterThan(-4.5); // walked ~4.9 m along +Z
    expect(s.state).toBe(PlayerState.Walking);
    expect(s.velocity[2]).toBeCloseTo(5, 1);
    expect(s.eyeHeight).toBeCloseTo(1.62);
  });

  it('returns render faces for the playground', async () => {
    const core = await load();
    const faces = core.chunkFaces(0, 0, 0);
    expect(faces.length % RENDER_FACE_BYTES).toBe(0);
    expect(faces.length / RENDER_FACE_BYTES).toBeGreaterThan(50);
    expect(core.chunkFaces(40, 10, 40).length).toBe(0); // open sky
  });
});
