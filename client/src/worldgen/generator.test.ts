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
import { LOD_VOLUME } from '../lod/grid';
import { ChunkGenerator, type DwellWorldgenFactory } from './generator';

const wasmJs = new URL('../../public/wasm/dwell_worldgen.js', import.meta.url);
const skip = !existsSync(wasmJs) && !process.env.DWELL_REQUIRE_WASM;
const GENERATOR_TERRAIN = GENERATORS.terrain;
const SEA_BIOMES = 2; // the last sea biome id: 0 ocean, 1 deep ocean, 2 frozen ocean

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

const lodGolden = readFileSync(
  new URL('../../../server/tests/worldgen/golden/lod-hashes.txt', import.meta.url),
  'utf8',
)
  .split('\n')
  .filter((l) => l.trim() && !l.startsWith('#'))
  .map((l) => {
    const [seed = '0', level = '0', i = '0', j = '0', k = '0', kind = '0', hash = '0'] = l
      .trim()
      .split(/\s+/);
    return {
      seed: BigInt(seed),
      coord: [Number(level), Number(i), Number(j), Number(k)] as const,
      kind: Number(kind),
      hash: BigInt(`0x${hash}`),
    };
  });

describe.skipIf(skip)('worldgen module (WASM)', () => {
  it('reproduces the golden LOD section hashes (GenerateLod, §6.6)', async () => {
    expect(lodGolden.length).toBeGreaterThan(5);
    const generators = new Map<bigint, ChunkGenerator>();
    for (const g of lodGolden) {
      let gen = generators.get(g.seed);
      if (!gen) {
        gen = await loadGenerator(GENERATOR_TERRAIN, g.seed);
        generators.set(g.seed, gen);
      }
      const s = gen.lod(g.coord);
      expect(s.cells.length).toBe(LOD_VOLUME);
      // LodHash: the kind byte, then the cells.
      let h = 0xcbf29ce484222325n;
      h = BigInt.asUintN(64, (h ^ BigInt(s.kind)) * 0x100000001b3n);
      for (const m of s.cells) {
        for (const byte of [m & 0xff, m >> 8])
          h = BigInt.asUintN(64, (h ^ BigInt(byte)) * 0x100000001b3n);
      }
      expect({ coord: g.coord, kind: s.kind, hash: h }).toEqual({
        coord: g.coord,
        kind: g.kind,
        hash: g.hash,
      });
    }
    const b = generators.get(0n)?.lodBounds(2, 65536, 65536);
    expect(b?.anyInside).toBe(true);
    expect((b?.hi ?? 0) > (b?.lo ?? 0)).toBe(true);
  });

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
    expect(mapColumn(bytes ?? new Uint8Array(64), 4, 2, 2).biome).toBeGreaterThan(SEA_BIOMES); // not sea
    const rim = gen.map(World.worldRadius + 100, 0, 8, 2);
    expect(mapColumn(rim ?? new Uint8Array(16), 2, 0, 0).outside).toBe(true);
    // Generators without a terrain map (flat, playground) have none.
    expect((await loadGenerator(GENERATORS.flat, 0n)).map(0, 0, 8, 4)).toBeNull();
  });

  it('samples the whole disc (the map zoomed all the way out): land at the origin, mostly ocean, a void corner', async () => {
    const gen = await loadGenerator(GENERATOR_TERRAIN, 0n);
    const n = 32;
    const step = (2 * World.worldRadius) / n;
    const bytes = gen.map(-World.worldRadius, -World.worldRadius, step, n) ?? new Uint8Array(0);
    expect(bytes.length).toBe(n * n * 4);
    expect(mapColumn(bytes, n, n / 2, n / 2).biome).toBeGreaterThan(SEA_BIOMES); // the origin is on land
    expect(mapColumn(bytes, n, 0, 0).outside).toBe(true); // the corner is beyond the rim
    let ocean = 0;
    let inside = 0;
    for (let j = 0; j < n; j++)
      for (let i = 0; i < n; i++) {
        const c = mapColumn(bytes, n, i, j);
        if (c.outside) continue;
        inside++;
        if (c.biome <= SEA_BIOMES) ocean++; // ocean, deep ocean, frozen ocean (biomes.h)
      }
    expect(ocean / inside).toBeGreaterThan(0.5); // the continents cover a quarter to a third
  });
});
