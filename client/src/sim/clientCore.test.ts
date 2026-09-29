// Runs the real client sim (WASM) under Node: streamed chunks, prediction from a snapshot, input,
// render faces.
// Skipped until `npm run build:wasm` has been run, unless DWELL_REQUIRE_WASM is set (CI).
import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { ControllerFlags, GroundKind, MessageType, PlayerState } from '../protocol/constants.gen';
import { encode, type ControllerState } from '../protocol/messages';
import { IDLE_INPUT, quantizeInput } from '../predict/input';
import { ClientCore, RENDER_FACE_BYTES } from './clientCore';
import type { DwellCoreFactory } from './module';
import { ChunkGenerator, type DwellWorldgenFactory } from '../worldgen/generator';

const wasmJs = new URL('../../public/wasm/dwell_core.js', import.meta.url);
const worldgenJs = new URL('../../public/wasm/dwell_worldgen.js', import.meta.url);
const skip = !existsSync(wasmJs) && !process.env.DWELL_REQUIRE_WASM;
const GENERATOR_PLAYGROUND = 1;

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
  /** A client sim holding the playground's chunks around the origin, as streaming would. */
  async function load(): Promise<ClientCore> {
    const mod = (await import(/* @vite-ignore */ wasmJs.href)) as { default: DwellCoreFactory };
    const core = await ClientCore.load(mod.default);
    const wg = (await import(/* @vite-ignore */ worldgenJs.href)) as {
      default: DwellWorldgenFactory;
    };
    const gen = await ChunkGenerator.load(wg.default, GENERATOR_PLAYGROUND, 0n);
    for (let y = -1; y <= 0; y++)
      for (let z = -2; z <= 1; z++)
        for (let x = -2; x <= 1; x++) core.setChunk([x, y, z], 0, gen.generate([x, y, z]));
    return core;
  }

  it('starts from a snapshot and predicts movement from input', async () => {
    const core = await load();
    expect(core.chunkCount()).toBe(32);
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
    expect(core.chunkFaces(40, 10, 40).length).toBe(0); // not streamed: air
  });

  it('drops removed chunks', async () => {
    const core = await load();
    expect(core.chunkFaces(0, -1, 0).length).toBeGreaterThan(0);
    core.removeChunk([0, -1, 0]);
    expect(core.chunkCount()).toBe(31);
    expect(core.chunkFaces(0, -1, 0).length).toBe(0);
  });
});
