// Runs the real client sim (WASM) under Node: streamed chunks, prediction from a snapshot, input,
// block edits and targeting, voxels for meshing.
// Skipped until `npm run build:wasm` has been run, unless DWELL_REQUIRE_WASM is set (CI).
import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { ControllerFlags, GroundKind, MessageType, PlayerState } from '../protocol/constants.gen';
import { encode, type ControllerState } from '../protocol/messages';
import { IDLE_INPUT, quantizeInput } from '../predict/input';
import { paddedIndex } from '../mesh/mesher';
import { ClientCore } from './clientCore';
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

  /** Starts prediction at `position` (capsule centre) from a snapshot. */
  function start(core: ClientCore, position: [number, number, number]): void {
    const snapshot = encode({
      type: MessageType.PhysicsSnapshot,
      serverTick: 3,
      ackInputSeq: 0,
      local: {
        position,
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
  }

  it('returns a chunk with its apron for meshing', async () => {
    const core = await load();
    const padded = core.paddedChunk(0, -1, 0);
    // The playground's grass top layer is y = −1, i.e. local y 31 of chunk row −1.
    expect(padded[paddedIndex(5, 31, 5)]).toBe(4);
    expect(padded[paddedIndex(5, 32, 5)]).toBe(0); // the apron above: chunk row 0, air
    expect(padded[paddedIndex(-1, 30, 5)]).toBe(3); // the apron at −X: chunk −1's dirt
    expect(core.paddedChunk(40, 10, 40).every((m) => m === 0)).toBe(true); // not streamed: air
  });

  it('applies voxel edits and targets blocks', async () => {
    const core = await load();
    // A block on the ground at (8, 0, −5), looked at from (8.5, 1.6, −8.5).
    core.editChunk([0, 0, -1], 1, Uint16Array.of(8 | (0 << 5) | (27 << 10), 2));
    expect(core.voxel(8, 0, -5)).toBe(2);
    const hit = core.target([8.5, 1.6, -8.5], [0, -0.2, 0.98], 5);
    expect(hit).toEqual({ cell: [8, 0, -5], face: 5 });
    expect(core.target([8.5, 1.6, -8.5], [0, 1, 0], 5)).toBeNull();
    // An edit in a chunk the client does not hold (e.g. sent as Air) creates it.
    core.editChunk([0, 5, 0], 1, Uint16Array.of(0, 2));
    expect(core.voxel(0, 160, 0)).toBe(2);
  });

  it('collides with a block right after the edit arrives', async () => {
    const core = await load();
    start(core, [8.5, 0.9, -8.5]);
    // A wall across the path at z = −5 (y 0..1, x 7..9), then walk into it.
    const pairs: number[] = [];
    for (let x = 7; x <= 9; x++)
      for (let y = 0; y <= 1; y++) pairs.push(x | (y << 5) | (27 << 10), 2);
    core.editChunk([0, 0, -1], 1, Uint16Array.from(pairs));
    for (let i = 0; i < 120; i++)
      core.tick(quantizeInput({ ...IDLE_INPUT, moveY: 1 }, core.nextSeq()));
    const z = core.state().position[2];
    expect(z).toBeLessThan(-5 - 0.29);
    expect(z).toBeGreaterThan(-5.5);
  });

  it('drops removed chunks', async () => {
    const core = await load();
    expect(core.voxel(5, -1, 5)).toBe(4);
    core.removeChunk([0, -1, 0]);
    expect(core.chunkCount()).toBe(31);
    expect(core.voxel(5, -1, 5)).toBe(0);
  });
});
