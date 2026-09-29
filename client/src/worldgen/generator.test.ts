// The client's worldgen module (WASM) against the server's golden chunk hashes: the same seed gives
// bit-identical chunks natively, in local mode, and in the client's workers (§6.3, ADR 0010).
// Skipped until `npm run build:wasm` has been run, unless DWELL_REQUIRE_WASM is set (CI).
import { existsSync, readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { GENERATORS } from '../local/world';
import { CHUNK_VOLUME } from '../protocol/chunkVoxels';
import { World } from '../protocol/constants.gen';
import { mapColumn } from '../ui/mapOverlay';
import type { ChunkCoord } from '../protocol/messages';
import { ChunkGenerator, type DwellWorldgenFactory } from './generator';

const wasmJs = new URL('../../public/wasm/dwell_worldgen.js', import.meta.url);
const skip = !existsSync(wasmJs) && !process.env.DWELL_REQUIRE_WASM;
const GENERATOR_TERRAIN = GENERATORS.terrain;

async function loadGenerator(version: number, seed: bigint): Promise<ChunkGenerator> {
  const mod = (await import(/* @vite-ignore */ wasmJs.href)) as { default: DwellWorldgenFactory };
  return ChunkGenerator.load(mod.default, version, seed);
}

function fnv1a64(voxels: Uint16Array): bigint {
  let h = 0xcbf29ce484222325n;
  for (const m of voxels) {
    for (const byte of [m & 0xff, m >> 8]) {
      h ^= BigInt(byte);
      h = BigInt.asUintN(64, h * 0x100000001b3n);
    }
  }
  return h;
}

const golden = readFileSync(
  new URL('../../../server/tests/worldgen/golden/chunk-hashes.txt', import.meta.url),
  'utf8',
)
  .split('\n')
  .filter((l) => l.trim() && !l.startsWith('#'))
  .map((l) => {
    const [seed = '0', x = '0', y = '0', z = '0', hash = '0'] = l.trim().split(/\s+/);
    return {
      seed: BigInt(seed),
      coord: [Number(x), Number(y), Number(z)] as ChunkCoord,
      hash: BigInt(`0x${hash}`),
    };
  });

describe.skipIf(skip)('worldgen module (WASM)', () => {
  it('reproduces the golden chunk hashes', async () => {
    expect(golden.length).toBeGreaterThan(10);
    const generators = new Map<bigint, ChunkGenerator>();
    for (const g of golden) {
      let gen = generators.get(g.seed);
      if (!gen) {
        gen = await loadGenerator(GENERATOR_TERRAIN, g.seed);
        generators.set(g.seed, gen);
      }
      const voxels = gen.generate(g.coord);
      expect(voxels.length).toBe(CHUNK_VOLUME);
      expect({ coord: g.coord, hash: gen.lastHash() }).toEqual({ coord: g.coord, hash: g.hash });
      // The copied-out voxels are the hashed ones.
      expect(fnv1a64(voxels)).toBe(g.hash);
    }
  });

  it('samples the terrain map (debug overlay): land at the spawn, the void beyond the rim', async () => {
    const gen = await loadGenerator(GENERATOR_TERRAIN, 0n);
    const bytes = gen.map(-16, -16, 8, 4);
    expect(bytes?.length).toBe(64);
    expect(mapColumn(bytes ?? new Uint8Array(64), 4, 2, 2).biome).not.toBe(0); // not ocean
    const rim = gen.map(World.worldRadius + 100, 0, 8, 2);
    expect(mapColumn(rim ?? new Uint8Array(16), 2, 0, 0).outside).toBe(true);
    // Generators without a terrain map (flat, playground) have none.
    expect((await loadGenerator(GENERATORS.flat, 0n)).map(0, 0, 8, 4)).toBeNull();
  });
});
