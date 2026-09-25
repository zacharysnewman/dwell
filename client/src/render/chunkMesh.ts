// Builds chunk geometry arrays from the sim core's visible faces (RenderFace: x, y, z, face,
// u16 material). Pure data, so it is testable without WebGL; ThreeRenderer uploads the result.
import { materialStyle, type MaterialStyle } from '../world/materials';
import { tileRect, type TileRect } from './textures';

export interface MeshArrays {
  positions: Float32Array;
  normals: Float32Array;
  /** Shading × (untextured) material colour; multiplies the texture. */
  colors: Float32Array;
  /** Atlas coordinates (render/textures.ts); untextured faces sample the plain white tile. */
  uvs: Float32Array;
  indices: Uint32Array;
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

class Builder {
  private readonly positions: number[] = [];
  private readonly normals: number[] = [];
  private readonly colors: number[] = [];
  private readonly uvs: number[] = [];
  private readonly indices: number[] = [];

  /**
   * Quad on `axis = plane`, spanning [u0,u1] × [v0,v1] (u = axis+1, v = axis+2), facing `sign`.
   * `cell` is the voxel's min corner: texture coordinates are the position within the cell (sides
   * with t up, tops and bottoms in x/z) mapped into `tile`.
   */
  quad(
    axis: number,
    sign: number,
    plane: number,
    u0: number,
    u1: number,
    v0: number,
    v1: number,
    color: number,
    cell: readonly number[],
    tile: TileRect,
  ): void {
    const u = (axis + 1) % 3;
    const v = (axis + 2) % 3;
    const base = this.positions.length / 3;
    const shade = axis === 1 ? (sign > 0 ? 1 : 0.55) : axis === 0 ? 0.8 : 0.7;
    const r = (((color >> 16) & 0xff) / 255) * shade;
    const g = (((color >> 8) & 0xff) / 255) * shade;
    const b = ((color & 0xff) / 255) * shade;
    for (const [cu, cv] of [
      [u0, v0],
      [u1, v0],
      [u1, v1],
      [u0, v1],
    ] as const) {
      const p = [0, 0, 0];
      p[axis] = plane;
      p[u] = cu;
      p[v] = cv;
      this.positions.push(p[0] ?? 0, p[1] ?? 0, p[2] ?? 0);
      const n = [0, 0, 0];
      n[axis] = sign;
      this.normals.push(n[0] ?? 0, n[1] ?? 0, n[2] ?? 0);
      this.colors.push(r, g, b);
      // In-cell texture coordinates: s across, t up the face (world y on sides).
      const px = (p[0] ?? 0) - (cell[0] ?? 0);
      const py = (p[1] ?? 0) - (cell[1] ?? 0);
      const pz = (p[2] ?? 0) - (cell[2] ?? 0);
      const [st, tt] = axis === 1 ? [px, pz] : axis === 0 ? [pz, py] : [px, py];
      this.uvs.push(tile.u0 + (tile.u1 - tile.u0) * st, tile.v0 + (tile.v1 - tile.v0) * tt);
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

export function buildChunkMeshes(faces: Uint8Array): ChunkMeshes {
  const opaque = new Builder();
  const transparent = new Builder();
  const view = new DataView(faces.buffer, faces.byteOffset, faces.byteLength);
  for (let i = 0; i + 8 <= faces.length; i += 8) {
    const x = view.getUint8(i);
    const y = view.getUint8(i + 1);
    const z = view.getUint8(i + 2);
    const face = view.getUint8(i + 3);
    const style = materialStyle(view.getUint16(i + 4, true));
    const [axis, sign] = FACES[face] ?? [1, 1];
    const lo = [x, y, z];
    const hi = [x + 1, y + 1, z + 1];
    if (style.look === 'slab') hi[1] = y + 0.5;
    if (style.look === 'water' && face === 2) hi[1] = y + WATER_SURFACE;
    const u = (axis + 1) % 3;
    const v = (axis + 2) % 3;
    let plane = sign > 0 ? (hi[axis] ?? 0) : (lo[axis] ?? 0);
    if (style.look === 'ladder') {
      // A thin plate against the cell's back face, facing out of the facing side.
      plane = sign > 0 ? (lo[axis] ?? 0) + LADDER_INSET : (hi[axis] ?? 0) - LADDER_INSET;
    }
    const target = style.opacity < 1 ? transparent : opaque;
    const [color, tile] = surface(style, axis, sign);
    target.quad(axis, sign, plane, lo[u] ?? 0, hi[u] ?? 0, lo[v] ?? 0, hi[v] ?? 0, color, lo, tile);
  }
  return { opaque: opaque.finish(), transparent: transparent.finish() };
}
