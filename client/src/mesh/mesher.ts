// Greedy chunk mesher (ARCHITECTURE.md §5, §6.1): turns a chunk's voxels, with a one-voxel apron
// from its neighbours, into render geometry. Visible faces of full cubes and water merge into
// rectangles of one material per slice; slabs, slopes (the shapes of world/blocks.ts, drawn with
// their true normals) and ladders are emitted polygon by polygon. Textures repeat
// once per block across a merged quad: `uv` carries positions in blocks and `tile` the atlas
// rectangle, and the chunk shader samples tile + fract(uv) × size (render/three). Pure data (no
// WebGL), so it runs in the meshing workers and in tests.
import { CHUNK_SIZE } from '../protocol/constants.gen';
import { faceTint, normalTint } from '../render/look';
import { tileRect, type TileRect } from '../render/textures';
import { SHAPES, stateId, type ShapeDef, type ShapeFace } from '../world/blocks';
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
  /** Face tint (render/look.ts) × (untextured) material colour; multiplies the texture. */
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
const FLOOD_INSET = 0.002; // water in a flooded cell sits this far inside its faces
/** The (only) water state: how flooded cells are drawn. */
const WATER_STATE = stateId('dwell:water');

/** How a material takes part in the meshing. */
const enum Shape {
  Empty,
  Full,
  /** A slab or slope: its polygons come from the shape table. */
  Shaped,
}

/** The full cube's shape: what a full face is tested against, for cubes and water alike. */
const CUBE: ShapeDef = SHAPES.find(
  (sh) => !sh.inverted && sh.corners.every((h) => h === 2),
) as ShapeDef;

interface Kind {
  shape: Shape;
  /** The solid's geometry (Full: the cube; Shaped: its shape; otherwise none). */
  def: ShapeDef | null;
  liquid: boolean;
  /** Shaped blocks with water in their open part. */
  flooded: boolean;
  /** Ladders: the one face drawn; −1 otherwise. */
  ladderFace: number;
  style: MaterialStyle;
}

const kinds = new Map<number, Kind>();
function kindOf(id: number): Kind {
  let k = kinds.get(id);
  if (!k) {
    const style = materialStyle(id);
    const empty = id === 0 || style.look === 'water' || style.look === 'ladder';
    const shaped = !empty && style.look === 'shaped';
    k = {
      shape: empty ? Shape.Empty : shaped ? Shape.Shaped : Shape.Full,
      def: empty ? null : shaped ? (SHAPES[style.shape] ?? null) : CUBE,
      liquid: style.look === 'water',
      flooded: shaped && style.flooded,
      ladderFace: style.look === 'ladder' ? (style.ladderFace ?? 4) : -1,
      style,
    };
    kinds.set(id, k);
  }
  return k;
}

/**
 * Whether the part of `shape` on its cell face `face` is completely covered by the opposite face of
 * `neighbour` (mirrors FaceCovered in block_shape.cpp). Sloped surfaces are never covered.
 */
export function faceCovered(shape: ShapeDef, face: number, neighbour: ShapeDef | null): boolean {
  if (!neighbour) return false;
  if (face === 2) return !shape.fullTop || neighbour.fullBottom;
  if (face === 3) return !shape.fullBottom || neighbour.fullTop;
  const mine = shape.sides[face < 2 ? face : face - 2];
  const theirs = neighbour.sides[face < 2 ? 1 - face : 5 - face];
  if (!mine || !theirs) return false;
  if (mine[0] === 0 && mine[1] === 0) return true;
  if (shape.inverted === neighbour.inverted) return theirs[0] >= mine[0] && theirs[1] >= mine[1];
  return theirs[0] === 2 && theirs[1] === 2;
}

/** Is the full-cube face `face` of a cube or water voxel `m` hidden by neighbour `n`? */
function hidden(m: number, n: number, face: number): boolean {
  const km = kindOf(m);
  const kn = kindOf(n);
  if (km.liquid && (n === m || kn.flooded)) return true;
  return faceCovered(CUBE, face, kn.def);
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
    const tint = faceTint(axis, sign);
    const r = (((color >> 16) & 0xff) / 255) * tint[0];
    const g = (((color >> 8) & 0xff) / 255) * tint[1];
    const b = ((color & 0xff) / 255) * tint[2];
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

  /**
   * A convex polygon (counter-clockwise seen from outside) of `pts`, offset by (x, y, z). `normal`
   * is its unit normal; uvs project the face along its dominant axis, in blocks.
   */
  polygon(
    pts: readonly (readonly [number, number, number])[],
    x: number,
    y: number,
    z: number,
    normal: readonly [number, number, number],
    tint: readonly [number, number, number],
    color: number,
    tile: TileRect,
  ): void {
    const base = this.positions.length / 3;
    const r = (((color >> 16) & 0xff) / 255) * tint[0];
    const g = (((color >> 8) & 0xff) / 255) * tint[1];
    const b = ((color & 0xff) / 255) * tint[2];
    const [nx, ny, nz] = normal;
    for (const [qx, qy, qz] of pts) {
      const px = qx + x;
      const py = qy + y;
      const pz = qz + z;
      this.positions.push(px, py, pz);
      this.normals.push(nx, ny, nz);
      this.colors.push(r, g, b);
      // The dominant axis of the normal: tops and bottoms (and slopes) in x/z, sides by height.
      if (Math.abs(ny) >= Math.abs(nx) && Math.abs(ny) >= Math.abs(nz)) this.uvs.push(px, pz);
      else if (Math.abs(nx) >= Math.abs(nz)) this.uvs.push(pz, py);
      else this.uvs.push(px, py);
      this.tiles.push(tile.u0, tile.v0, tile.u1 - tile.u0, tile.v1 - tile.v0);
    }
    for (let i = 1; i + 1 < pts.length; i++) this.indices.push(base, base + i, base + i + 1);
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

/** A ladder's plate: one quad against the back of its cell. */
function ladderFace(b: Builder, m: number, face: number, x: number, y: number, z: number): void {
  const style = kindOf(m).style;
  const [axis = 1, sign = 1] = FACES[face] ?? [];
  const lo = [x, y, z];
  const hi = [x + 1, y + 1, z + 1];
  const u = (axis + 1) % 3;
  const v = (axis + 2) % 3;
  // A thin plate against the cell's back face, facing out of the facing side.
  const plane = sign > 0 ? (lo[axis] ?? 0) + LADDER_INSET : (hi[axis] ?? 0) - LADDER_INSET;
  const [color, tile] = surface(style, axis, sign);
  b.quad(axis, sign, plane, lo[u] ?? 0, hi[u] ?? 0, lo[v] ?? 0, hi[v] ?? 0, color, tile);
}

/** The unit normal of a polygon (first three points, counter-clockwise). */
function polygonNormal(f: ShapeFace): [number, number, number] {
  const [a = ORIGIN, b = ORIGIN, c = ORIGIN] = f.pts;
  const e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]] as const;
  const e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]] as const;
  const nx = e1[1] * e2[2] - e1[2] * e2[1];
  const ny = e1[2] * e2[0] - e1[0] * e2[2];
  const nz = e1[0] * e2[1] - e1[1] * e2[0];
  const len = Math.hypot(nx, ny, nz) || 1;
  return [nx / len, ny / len, nz / len];
}
const ORIGIN = [0, 0, 0] as const;

/** A slab or slope voxel: each polygon of its shape, but the parts of cell faces a neighbour covers. */
function shapedVoxel(
  b: Builder,
  k: Kind,
  padded: Uint16Array,
  x: number,
  y: number,
  z: number,
): void {
  const def = k.def;
  if (!def) return;
  for (const f of def.faces) {
    if (f.tag < 6) {
      const [axis = 0, sign = 1] = FACES[f.tag] ?? [];
      const n = [x, y, z];
      n[axis] = (n[axis] ?? 0) + sign;
      const [nx = 0, ny = 0, nz = 0] = n;
      if (faceCovered(def, f.tag, kindOf(padded[paddedIndex(nx, ny, nz)] ?? 0).def)) continue;
    }
    const normal = polygonNormal(f);
    const tint =
      f.tag < 6 ? faceTint(Math.floor(f.tag / 2), f.tag % 2 === 0 ? 1 : -1) : normalTint(...normal);
    // Sides take the side tile, a sloped surface the top (or, hanging, the bottom) tile.
    const axis = f.tag < 6 ? Math.floor(f.tag / 2) : 1;
    const sign = f.tag < 6 ? (f.tag % 2 === 0 ? 1 : -1) : normal[1] >= 0 ? 1 : -1;
    const [color, tile] = surface(k.style, axis, sign);
    b.polygon(f.pts, x, y, z, normal, tint, color, tile);
  }
}

/** Water in the open part of a flooded shaped cell: its faces, inset so the solid shows through. */
function floodedWater(b: Builder, padded: Uint16Array, x: number, y: number, z: number): void {
  const water = kindOf(WATER_STATE);
  for (let face = 0; face < 6; face++) {
    const [axis = 0, sign = 1] = FACES[face] ?? [];
    const n = [x, y, z];
    n[axis] = (n[axis] ?? 0) + sign;
    const [nx = 0, ny = 0, nz = 0] = n;
    const m = padded[paddedIndex(nx, ny, nz)] ?? 0;
    const kn = kindOf(m);
    if (kn.liquid || kn.flooded || faceCovered(CUBE, face, kn.def)) continue;
    const u = (axis + 1) % 3;
    const v = (axis + 2) % 3;
    const lo = [x, y, z];
    let plane = (lo[axis] ?? 0) + (sign > 0 ? 1 - FLOOD_INSET : FLOOD_INSET);
    if (face === 2) plane = y + WATER_SURFACE;
    const [color, tile] = surface(water.style, axis, sign);
    b.quad(
      axis,
      sign,
      plane,
      lo[u] ?? 0,
      (lo[u] ?? 0) + 1,
      lo[v] ?? 0,
      (lo[v] ?? 0) + 1,
      color,
      tile,
    );
  }
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
            if (k.ladderFace === face) ladderFace(opaque, m, face, cx, cy, cz);
            continue;
          }
          if (k.shape === Shape.Shaped) continue; // drawn whole, below
          neighbour[0] = cx;
          neighbour[1] = cy;
          neighbour[2] = cz;
          neighbour[axis] = d + sign;
          const [nx = 0, ny = 0, nz = 0] = neighbour;
          if (hidden(m, padded[paddedIndex(nx, ny, nz)] ?? 0, face)) continue;
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
  // Slabs and slopes: whole voxels, not slices.
  for (let cz = 0; cz < N; cz++) {
    for (let cy = 0; cy < N; cy++) {
      for (let cx = 0; cx < N; cx++) {
        const m = padded[paddedIndex(cx, cy, cz)] ?? 0;
        if (m === 0) continue;
        const k = kindOf(m);
        if (k.shape !== Shape.Shaped) continue;
        shapedVoxel(opaque, k, padded, cx, cy, cz);
        if (k.flooded) floodedWater(transparent, padded, cx, cy, cz);
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
