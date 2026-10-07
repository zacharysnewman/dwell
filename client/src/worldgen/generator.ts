// The terrain generator (server/wasm/worldgen_api.cpp, the server's own C++ compiled to WASM) for
// one worldgen worker (§5.1, §6.3). Also used directly by tests under Node.
import { CHUNK_VOLUME } from '../protocol/chunkVoxels';
import type { ChunkCoord } from '../protocol/messages';
import {
  LOD_PAD,
  LOD_VOLUME,
  type LodBounds,
  type LodCoord,
  type LodKind,
  SURFACE_STRIDE,
} from '../lod/grid';
import { wasmUrl } from '../sim/wasmUrl';

/** Bytes of a chunk column's tint grid (mesh/mesher.ts TINT_GRID_BYTES). */
const TINT_GRID_BYTES = 3 * 3 * 6;

/** Module surface of dwell_worldgen.js (-sMODULARIZE -sEXPORT_ES6). */
export interface DwellWorldgenModule {
  HEAPU8: Uint8Array;
  HEAPU16: Uint16Array;
  HEAPU32: Uint32Array;
  HEAPF32: Float32Array;
  HEAPF64: Float64Array;
  _malloc(size: number): number;
  _free(ptr: number): void;
  _dwell_worldgen_create(generatorVersion: number, seedLo: number, seedHi: number): number;
  _dwell_worldgen_generate(cx: number, cy: number, cz: number): number;
  _dwell_worldgen_hash(outPtr: number): void;
  _dwell_worldgen_map(x0: number, z0: number, step: number, n: number, face: number): number;
  _dwell_worldgen_tint(cx: number, cz: number, face: number): number;
  _dwell_worldgen_lod(level: number, i: number, j: number, k: number): number;
  _dwell_worldgen_lod_cells(): number;
  _dwell_worldgen_lod_surface(face: number): number;
  _dwell_worldgen_lod_bounds(level: number, i: number, k: number, outPtr: number): void;
}

/** A generated LOD section (§6.6): its kind and 34³ cells (lod/grid.ts `lodCell` order). */
export interface GeneratedSection {
  kind: LodKind;
  cells: Uint16Array<ArrayBuffer>;
  /**
   * Each column's exact surface (C++ core::LodSurface), 34² × SURFACE_STRIDE floats in
   * (z + 1) · 34 + (x + 1) order: height (m), material, flags (1 valid, 2 wet), and where wet the
   * water's level (m). Null when the generator has none. Face A's, in world terms.
   */
  surface?: Float32Array<ArrayBuffer> | null;
  /**
   * The same for face B, in face-local terms (heights as on face A: sea level 0, sky toward +h;
   * the section's rows mirrored): null where the section has no rows of face B.
   */
  surfaceB?: Float32Array<ArrayBuffer> | null;
}

export type DwellWorldgenFactory = (options?: {
  locateFile?: (path: string, prefix: string) => string;
}) => Promise<DwellWorldgenModule>;

/** URL of the worldgen module's JS loader, served from `public/wasm` at the site base (versioned). */
export function dwellWorldgenUrl(): string {
  return wasmUrl('dwell_worldgen.js');
}

export class ChunkGenerator {
  private constructor(private readonly m: DwellWorldgenModule) {}

  /** Bytes of the generator's WebAssembly memory (it grows, never shrinks). */
  heapBytes(): number {
    return this.m.HEAPU8.buffer.byteLength;
  }

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

  /**
   * The terrain's biome/height map (n × n columns from (x0, z0) every `step` m, 4 bytes each:
   * i16 height, u8 biome, u8 flags) of a face (0 = A, 1 = B; heights are face-local), or null for
   * generators without one.
   */
  map(x0: number, z0: number, step: number, n: number, face = 0): Uint8Array<ArrayBuffer> | null {
    const ptr = this.m._dwell_worldgen_map(x0, z0, step, n, face);
    return ptr ? this.m.HEAPU8.slice(ptr, ptr + n * n * 4) : null;
  }

  /**
   * The biome tint of chunk column (cx, cz) of a face (0 = A, 1 = B): TINT_GRID_BYTES (3 × 3
   * points 16 m apart, each the grass then the foliage colour as r, g, b in 1/64), or null for
   * generators without biomes.
   */
  tint(cx: number, cz: number, face = 0): Uint8Array<ArrayBuffer> | null {
    const ptr = this.m._dwell_worldgen_tint(cx, cz, face);
    return ptr ? this.m.HEAPU8.slice(ptr, ptr + TINT_GRID_BYTES) : null;
  }

  /** GenerateLod (§6.6): the section as the generator leaves it, at its level's resolution. */
  lod(c: LodCoord): GeneratedSection {
    const kind = this.m._dwell_worldgen_lod(c[0], c[1], c[2], c[3]) as LodKind;
    const ptr = this.m._dwell_worldgen_lod_cells();
    const cells = this.m.HEAPU16.slice(ptr >> 1, (ptr >> 1) + LOD_VOLUME);
    const read = (face: number): Float32Array<ArrayBuffer> | null => {
      const sp = this.m._dwell_worldgen_lod_surface(face);
      return sp
        ? this.m.HEAPF32.slice(sp >> 2, (sp >> 2) + LOD_PAD * LOD_PAD * SURFACE_STRIDE)
        : null;
    };
    const surface = read(0);
    const surfaceB = read(1);
    return { kind, cells, surface, surfaceB };
  }

  /** Height bounds of the column of sections (level, i, ·, k). */
  lodBounds(level: number, i: number, k: number): LodBounds {
    const out = this.m._malloc(48);
    try {
      this.m._dwell_worldgen_lod_bounds(level, i, k, out);
      const f = this.m.HEAPF64;
      return {
        lo: f[out >> 3] ?? 0,
        hi: f[(out >> 3) + 1] ?? 0,
        anyInside: (f[(out >> 3) + 2] ?? 0) !== 0,
        loB: f[(out >> 3) + 3] ?? 0,
        hiB: f[(out >> 3) + 4] ?? 0,
        bifacial: (f[(out >> 3) + 5] ?? 1) !== 0,
      };
    } finally {
      this.m._free(out);
    }
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
