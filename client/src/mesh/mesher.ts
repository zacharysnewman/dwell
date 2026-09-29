// Greedy chunk mesher (ARCHITECTURE.md §5, §6.1): turns a chunk's voxels, with a one-voxel apron
// from its neighbours, into render geometry. Visible faces of full cubes and water merge into
// rectangles of one material per slice; slabs and ladders stay one quad per face. Textures repeat
// once per block across a merged quad: `uv` carries positions in blocks and `tile` the atlas
// rectangle, and the chunk shader samples tile + fract(uv) × size (render/three). Pure data (no
// WebGL), so it runs in the meshing workers and in tests.
import { CHUNK_SIZE } from '../protocol/constants.gen';
import { tileRect, type TileRect } from '../render/textures';
import { materialStyle, type MaterialStyle } from '../world/materials';

/** Edge of the padded voxel block: the chunk plus one voxel on each side. */
export const PAD = CHUNK_SIZE + 2;
export const PADDED_VOLUME = PAD * PAD * PAD;

/** Index into padded voxels of chunk-local (x, y, z), each in −1..CHUNK_SIZE. */
export const paddedIndex = (x: number, y: number, z: number): number =>
  x + 1 + PAD * (z + 1 + PAD * (y + 1));

export interface MeshArrays {
  positions: Float32Array<ArrayBuffer>;
  normals: Float32Array<ArrayBuffer>;
  /** Shading × (untextured) material colour; multiplies the texture. */
  colors: Float32Array<ArrayBuffer>;
  /** Texture coordinates in blocks: the shader repeats the tile once per unit. */
  uvs: Float32Array<ArrayBuffer>;
  /** Atlas rectangle per vertex: u0, v0, width, height (render/textures.ts). */
  tiles: Float32Array<ArrayBuffer>;
  indices: Uint32Array<ArrayBuffer>;
}

export interface ChunkMeshes {
  opaque: MeshArrays;
  /** Water and other see-through materials, drawn after the opaque pass. */
  transparent: MeshArrays;
}

/** Face index → (axis, sign): 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z. */
const FACES: readonly (readonly [number, number])[] = [
  [0, 1],
  [0, -1],
  [1, 1],
  [1, -1],
  [2, 1],
  [2, -1],
];

const LADDER_INSET = 0.05; // the plate sits this far in front of the cell's back face
const WATER_SURFACE = 0.875;

/** How a material takes part in face culling (mirrors the collision shapes of voxel.h). */
const enum Shape {
  Empty,
  Full,
  Slab,
}

interface Kind {
  shape: Shape;
  liquid: boolean;
  /** Ladders: the one face drawn; −1 otherwise. */
  ladderFace: number;
  style: MaterialStyle;
}

const kinds = new Map<number, Kind>();
function kindOf(id: number): Kind {
  let k = kinds.get(id);
  if (!k) {
    const style = materialStyle(id);
    k = {
      shape:
        id === 0 || style.look === 'water' || style.look === 'ladder'
          ? Shape.Empty
          : style.look === 'slab'
            ? Shape.Slab
            : Shape.Full,
      liquid: style.look === 'water',
      ladderFace: style.look === 'ladder' ? (style.ladderFace ?? 4) : -1,
      style,
    };
    kinds.set(id, k);
  }
  return k;
}

/** Is face `face` of a voxel of material `m` hidden by neighbour `n`? */
function hidden(m: number, n: number, face: number): boolean {
  const km = kindOf(m);
  const kn = kindOf(n);
  if (km.shape === Shape.Slab) {
    if (face === 2) return false; // the top of a slab is always open
    if (face !== 3 && kn.shape === Shape.Slab) return true;
  }
  return kn.shape === Shape.Full || (km.liquid && n === m);
}

class Builder {
  private readonly positions: number[] = [];
  private readonly normals: number[] = [];
  private readonly colors: number[] = [];
  private readonly uvs: number[] = [];
  private readonly tiles: number[] = [];
  private readonly indices: number[] = [];

  /** Quad on `axis = plane`, spanning [u0,u1] × [v0,v1] (u = axis+1, v = axis+2), facing `sign`. */
  quad(
    axis: number,
    sign: number,
    plane: number,
    u0: number,
    u1: number,
    v0: number,
    v1: number,
    color: number,
    tile: TileRect,
  ): void {
    const u = (axis + 1) % 3;
    const v = (axis + 2) % 3;
    const base = this.positions.length / 3;
    const shade = axis === 1 ? (sign > 0 ? 1 : 0.55) : axis === 0 ? 0.8 : 0.7;
    const r = (((color >> 16) & 0xff) / 255) * shade;
    const g = (((color >> 8) & 0xff) / 255) * shade;
    const b = ((color & 0xff) / 255) * shade;
    const p = [0, 0, 0];
    for (const [cu, cv] of [
      [u0, v0],
      [u1, v0],
      [u1, v1],
      [u0, v1],
    ] as const) {
      p[axis] = plane;
      p[u] = cu;
      p[v] = cv;
      const [px = 0, py = 0, pz = 0] = p;
      this.positions.push(px, py, pz);
      this.normals.push(axis === 0 ? sign : 0, axis === 1 ? sign : 0, axis === 2 ? sign : 0);
      this.colors.push(r, g, b);
      // In blocks: s across, t up the face (world y on sides), tops and bottoms in x/z.
      if (axis === 1) this.uvs.push(px, pz);
      else if (axis === 0) this.uvs.push(pz, py);
      else this.uvs.push(px, py);
      this.tiles.push(tile.u0, tile.v0, tile.u1 - tile.u0, tile.v1 - tile.v0);
    }
    // e_u × e_v = e_axis: (0, 1, 2) is counter-clockwise seen from +axis.
    if (sign > 0) this.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
    else this.indices.push(base, base + 2, base + 1, base, base + 3, base + 2);
  }

  finish(): MeshArrays {
    return {
      positions: Float32Array.from(this.positions),
      normals: Float32Array.from(this.normals),
      colors: Float32Array.from(this.colors),
      uvs: Float32Array.from(this.uvs),
      tiles: Float32Array.from(this.tiles),
      indices: Uint32Array.from(this.indices),
    };
  }
}

const PLAIN = tileRect('plain');

/** Vertex colour and texture tile of a face: textured faces take their colour from the texture. */
function surface(style: MaterialStyle, axis: number, sign: number): [number, TileRect] {
  const t = style.textures;
  if (!t) return [style.color, PLAIN];
  const name = axis !== 1 ? t.side : sign > 0 ? t.top : t.bottom;
  return [0xffffff, tileRect(name)];
}

/** A slab or ladder face: one quad in its cell. */
function singleFace(b: Builder, m: number, face: number, x: number, y: number, z: number): void {
  const style = kindOf(m).style;
  const [axis = 1, sign = 1] = FACES[face] ?? [];
  const lo = [x, y, z];
  const hi = [x + 1, y + 1, z + 1];
  if (style.look === 'slab') hi[1] = y + 0.5;
  const u = (axis + 1) % 3;
  const v = (axis + 2) % 3;
  let plane = sign > 0 ? (hi[axis] ?? 0) : (lo[axis] ?? 0);
  if (style.look === 'ladder') {
    // A thin plate against the cell's back face, facing out of the facing side.
    plane = sign > 0 ? (lo[axis] ?? 0) + LADDER_INSET : (hi[axis] ?? 0) - LADDER_INSET;
  }
  const [color, tile] = surface(style, axis, sign);
  b.quad(axis, sign, plane, lo[u] ?? 0, hi[u] ?? 0, lo[v] ?? 0, hi[v] ?? 0, color, tile);
}

/**
 * Meshes a chunk from its padded voxels (PADDED_VOLUME materials, `paddedIndex` order). Positions
 * are chunk-local.
 */
export function meshChunk(padded: Uint16Array): ChunkMeshes {
  if (padded.length !== PADDED_VOLUME) throw new RangeError('padded voxels must be PADDED_VOLUME');
  const opaque = new Builder();
  const transparent = new Builder();
  const N = CHUNK_SIZE;
  // Merge mask for one slice: material + 1 of a visible mergeable face, 0 for none.
  const mask = new Int32Array(N * N);
  const cell = [0, 0, 0];
  const neighbour = [0, 0, 0];

  for (let face = 0; face < 6; face++) {
    const [axis = 0, sign = 1] = FACES[face] ?? [];
    const u = (axis + 1) % 3;
    const v = (axis + 2) % 3;
    for (let d = 0; d < N; d++) {
      mask.fill(0);
      let any = false;
      for (let j = 0; j < N; j++) {
        for (let i = 0; i < N; i++) {
          cell[axis] = d;
          cell[u] = i;
          cell[v] = j;
          const [cx = 0, cy = 0, cz = 0] = cell;
          const m = padded[paddedIndex(cx, cy, cz)] ?? 0;
          if (m === 0) continue;
          const k = kindOf(m);
          if (k.ladderFace >= 0) {
            if (k.ladderFace === face) singleFace(opaque, m, face, cx, cy, cz);
            continue;
          }
          neighbour[0] = cx;
          neighbour[1] = cy;
          neighbour[2] = cz;
          neighbour[axis] = d + sign;
          const [nx = 0, ny = 0, nz = 0] = neighbour;
          if (hidden(m, padded[paddedIndex(nx, ny, nz)] ?? 0, face)) continue;
          if (k.shape === Shape.Slab) {
            singleFace(opaque, m, face, cx, cy, cz);
            continue;
          }
          mask[i + N * j] = m + 1;
          any = true;
        }
      }
      if (!any) continue;
      // Greedy rectangles: widest run along u, then as many rows along v as match it.
      for (let j = 0; j < N; j++) {
        for (let i = 0; i < N;) {
          const key = mask[i + N * j] ?? 0;
          if (key === 0) {
            i++;
            continue;
          }
          let w = 1;
          while (i + w < N && mask[i + w + N * j] === key) w++;
          let h = 1;
          grow: while (j + h < N) {
            for (let k = 0; k < w; k++) if (mask[i + k + N * (j + h)] !== key) break grow;
            h++;
          }
          for (let dv = 0; dv < h; dv++) mask.fill(0, i + N * (j + dv), i + w + N * (j + dv));
          const m = key - 1;
          const style = kindOf(m).style;
          let plane = sign > 0 ? d + 1 : d;
          if (style.look === 'water' && face === 2) plane = d + WATER_SURFACE;
          const [color, tile] = surface(style, axis, sign);
          const target = style.opacity < 1 ? transparent : opaque;
          target.quad(axis, sign, plane, i, i + w, j, j + h, color, tile);
          i += w;
        }
      }
    }
  }
  return { opaque: opaque.finish(), transparent: transparent.finish() };
}

/** The buffers of a result, for transferring it between threads. */
export function meshBuffers(m: ChunkMeshes): ArrayBuffer[] {
  return [m.opaque, m.transparent].flatMap((a) => [
    a.positions.buffer,
    a.normals.buffer,
    a.colors.buffer,
    a.uvs.buffer,
    a.tiles.buffer,
    a.indices.buffer,
  ]);
}
