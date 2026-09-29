// Chunk voxels on the wire: palette + RLE (ARCHITECTURE.md §6.1). Mirrors WriteVoxels / ReadVoxels
// in server/core/src/protocol/messages.cpp; pinned by the chunk_data_* golden vectors.
//
// Voxels travel in layer order (x fastest, then z, then y) so horizontal strata make long runs.
// Palette: u16 count (1..CHUNK_VOLUME), then the materials in order of first appearance. Runs until
// the chunk is full: LEB128 length (1..CHUNK_VOLUME, minimal encoding), then the palette index as
// u8 (palette ≤ 256 entries) or u16. The encoding is canonical: encode(decode(bytes)) == bytes.
import type { ByteReader, ByteWriter } from './bytes';
import { CHUNK_SIZE, Lod } from './constants.gen';

export const CHUNK_VOLUME = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;
/** Cells of a LOD section with its one-cell apron (§6.6), already in layer order. */
export const LOD_CELL_COUNT = (Lod.sectionCells + 2) ** 3;

/** Chunk index (x | y << 5 | z << 10) of the i-th voxel in wire (layer) order. */
function wireToIndex(i: number): number {
  return (i & 31) | (((i >> 10) & 31) << 5) | (((i >> 5) & 31) << 10);
}

/** Palette + RLE over `count` cells taken in wire order through `at`. */
function writeCells(w: ByteWriter, count: number, at: (i: number) => number): void {
  const palette: number[] = [];
  const index = new Map<number, number>();
  for (let i = 0; i < count; i++) {
    const m = at(i);
    if (!index.has(m)) {
      index.set(m, palette.length);
      palette.push(m);
    }
  }
  w.u16(palette.length);
  for (const m of palette) w.u16(m);
  const wide = palette.length > 256;
  for (let i = 0; i < count;) {
    const m = at(i);
    let run = 1;
    while (i + run < count && at(i + run) === m) run++;
    for (let v = run; ; v >>>= 7) {
      if (v < 0x80) {
        w.u8(v);
        break;
      }
      w.u8((v & 0x7f) | 0x80);
    }
    const idx = index.get(m) ?? 0;
    if (wide) w.u16(idx);
    else w.u8(idx);
    i += run;
  }
}

export function writeVoxels(w: ByteWriter, voxels: Uint16Array): void {
  if (voxels.length !== CHUNK_VOLUME) throw new RangeError('chunk voxels must be CHUNK_VOLUME');
  writeCells(w, CHUNK_VOLUME, (i) => voxels[wireToIndex(i)] ?? 0);
}

/** A LOD section's 34³ cells (LodData Explicit, §6.6), taken in their own (layer) order. */
export function writeLodCells(w: ByteWriter, cells: Uint16Array): void {
  if (cells.length !== LOD_CELL_COUNT) throw new RangeError('LOD cells must be LOD_CELL_COUNT');
  writeCells(w, LOD_CELL_COUNT, (i) => cells[i] ?? 0);
}

function readVarint(r: ByteReader): number {
  let v = 0;
  for (let shift = 0; shift < 21; shift += 7) {
    const b = r.u8();
    r.check(shift === 0 || b !== 0, 'non-minimal run length');
    v |= (b & 0x7f) << shift;
    if ((b & 0x80) === 0) return v;
  }
  r.check(false, 'run length too long');
  return 0;
}

function readCells(r: ByteReader, volume: number, put: (i: number, m: number) => void): void {
  const count = r.u16();
  r.check(count >= 1 && count <= Math.min(volume, 0xffff), 'palette size');
  const palette = new Uint16Array(count);
  for (let i = 0; i < count; i++) palette[i] = r.u16();
  const wide = count > 256;
  for (let filled = 0; filled < volume;) {
    const run = readVarint(r);
    const i = wide ? r.u16() : r.u8();
    r.check(run >= 1 && run <= volume - filled, 'run length');
    r.check(i < count, 'palette index');
    const m = palette[i] ?? 0;
    for (let k = 0; k < run; k++) put(filled++, m);
  }
}

export function readVoxels(r: ByteReader): Uint16Array {
  const voxels = new Uint16Array(CHUNK_VOLUME);
  readCells(r, CHUNK_VOLUME, (i, m) => {
    voxels[wireToIndex(i)] = m;
  });
  return voxels;
}

export function readLodCells(r: ByteReader): Uint16Array<ArrayBuffer> {
  const cells = new Uint16Array(LOD_CELL_COUNT);
  readCells(r, LOD_CELL_COUNT, (i, m) => {
    cells[i] = m;
  });
  return cells;
}
