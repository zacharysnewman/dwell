// Chunk voxels on the wire: palette + RLE (ARCHITECTURE.md §6.1). Mirrors WriteVoxels / ReadVoxels
// in server/core/src/protocol/messages.cpp; pinned by the chunk_data_* golden vectors.
//
// Voxels travel in layer order (x fastest, then z, then y) so horizontal strata make long runs.
// Palette: u16 count (1..CHUNK_VOLUME), then the materials in order of first appearance. Runs until
// the chunk is full: LEB128 length (1..CHUNK_VOLUME, minimal encoding), then the palette index as
// u8 (palette ≤ 256 entries) or u16. The encoding is canonical: encode(decode(bytes)) == bytes.
import type { ByteReader, ByteWriter } from './bytes';
import { CHUNK_SIZE } from './constants.gen';

export const CHUNK_VOLUME = CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE;

/** Chunk index (x | y << 5 | z << 10) of the i-th voxel in wire (layer) order. */
function wireToIndex(i: number): number {
  return (i & 31) | (((i >> 10) & 31) << 5) | (((i >> 5) & 31) << 10);
}

export function writeVoxels(w: ByteWriter, voxels: Uint16Array): void {
  if (voxels.length !== CHUNK_VOLUME) throw new RangeError('chunk voxels must be CHUNK_VOLUME');
  const at = (i: number): number => voxels[wireToIndex(i)] ?? 0;
  const palette: number[] = [];
  const index = new Map<number, number>();
  for (let i = 0; i < CHUNK_VOLUME; i++) {
    const m = at(i);
    if (!index.has(m)) {
      index.set(m, palette.length);
      palette.push(m);
    }
  }
  w.u16(palette.length);
  for (const m of palette) w.u16(m);
  const wide = palette.length > 256;
  for (let i = 0; i < CHUNK_VOLUME;) {
    const m = at(i);
    let run = 1;
    while (i + run < CHUNK_VOLUME && at(i + run) === m) run++;
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

export function readVoxels(r: ByteReader): Uint16Array {
  const count = r.u16();
  r.check(count >= 1 && count <= CHUNK_VOLUME, 'palette size');
  const palette = new Uint16Array(count);
  for (let i = 0; i < count; i++) palette[i] = r.u16();
  const wide = count > 256;
  const voxels = new Uint16Array(CHUNK_VOLUME);
  for (let filled = 0; filled < CHUNK_VOLUME;) {
    const run = readVarint(r);
    const i = wide ? r.u16() : r.u8();
    r.check(run >= 1 && run <= CHUNK_VOLUME - filled, 'run length');
    r.check(i < count, 'palette index');
    const m = palette[i] ?? 0;
    for (let k = 0; k < run; k++) voxels[wireToIndex(filled++)] = m;
  }
  return voxels;
}
