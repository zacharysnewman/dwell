// The terrain generator (server/wasm/worldgen_api.cpp, the server's own C++ compiled to WASM) for
// one worldgen worker (§5.1, §6.3). Also used directly by tests under Node.
import { CHUNK_VOLUME } from '../protocol/chunkVoxels';
import type { ChunkCoord } from '../protocol/messages';

/** Module surface of dwell_worldgen.js (-sMODULARIZE -sEXPORT_ES6). */
export interface DwellWorldgenModule {
  HEAPU16: Uint16Array;
  HEAPU32: Uint32Array;
  _malloc(size: number): number;
  _free(ptr: number): void;
  _dwell_worldgen_create(generatorVersion: number, seedLo: number, seedHi: number): number;
  _dwell_worldgen_generate(cx: number, cy: number, cz: number): number;
  _dwell_worldgen_hash(outPtr: number): void;
}

export type DwellWorldgenFactory = () => Promise<DwellWorldgenModule>;

/** URL of the worldgen module's JS loader, served from `public/wasm` at the site base. */
export function dwellWorldgenUrl(): string {
  return `${import.meta.env.BASE_URL}wasm/dwell_worldgen.js`;
}

export class ChunkGenerator {
  private constructor(private readonly m: DwellWorldgenModule) {}

  static async load(
    factory: DwellWorldgenFactory,
    generatorVersion: number,
    worldSeed: bigint,
  ): Promise<ChunkGenerator> {
    const m = await factory();
    m._dwell_worldgen_create(
      generatorVersion,
      Number(BigInt.asUintN(32, worldSeed)),
      Number(BigInt.asUintN(32, worldSeed >> 32n)),
    );
    return new ChunkGenerator(m);
  }

  /** Generates a chunk: CHUNK_VOLUME materials in chunk index order, in a fresh buffer. */
  generate(coord: ChunkCoord): Uint16Array<ArrayBuffer> {
    const ptr = this.m._dwell_worldgen_generate(coord[0], coord[1], coord[2]);
    return this.m.HEAPU16.slice(ptr >> 1, (ptr >> 1) + CHUNK_VOLUME);
  }

  /** ChunkHash (FNV-1a 64) of the chunk generated last: the WorldgenCheck value. */
  lastHash(): bigint {
    const out = this.m._malloc(8);
    try {
      this.m._dwell_worldgen_hash(out);
      const lo = BigInt(this.m.HEAPU32[out >> 2] ?? 0);
      const hi = BigInt(this.m.HEAPU32[(out >> 2) + 1] ?? 0);
      return (hi << 32n) | lo;
    } finally {
      this.m._free(out);
    }
  }
}
