// Greedy mesher for LOD sections (ARCHITECTURE.md §6.6): a section's 34³ cells (its 32³ and a
// one-cell apron, lod/grid.ts `lodCell` order — the chunk mesher's padded layout) → flat-coloured
// geometry in cell units. Every material but air and liquids is a full cube at LOD; each face has
// its material's flat colour (the average of its texture) with the chunk mesher's face shading.
// Faces on the section's border that the apron hides go to a per-side *skirt* instead of being
// dropped: the renderer shows a side's skirt when the neighbour there is not drawn at the same
// level, which closes the cracks between levels. Pure data, so it runs in the meshing workers.
import { LOD_PAD, LOD_VOLUME, SECTION_CELLS } from '../lod/grid';
import { averageTileColor, linearToSrgbByte, srgbToLinear } from '../render/textures';
import { materialStyle } from '../world/materials';

export interface FlatMesh {
  positions: Float32Array<ArrayBuffer>;
  normals: Float32Array<ArrayBuffer>;
  /** Shading × flat material colour. */
  colors: Float32Array<ArrayBuffer>;
  indices: Uint32Array<ArrayBuffer>;
}

export interface SectionMeshes {
  opaque: FlatMesh;
  /** Liquids, drawn see-through after the opaque pass. */
  water: FlatMesh;
  /** Border faces hidden by the apron, per face index (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z). */
  skirts: FlatMesh[];
}

const FACES: readonly (readonly [number, number])[] = [
  [0, 1],
  [0, -1],
  [1, 1],
  [1, -1],
  [2, 1],
  [2, -1],
];

const cellIndex = (x: number, y: number, z: number): number =>
  x + 1 + LOD_PAD * (z + 1 + LOD_PAD * (y + 1));

const liquidIds = new Map<number, boolean>();
function isLiquid(m: number): boolean {
  let l = liquidIds.get(m);
  if (l === undefined) {
    l = materialStyle(m).look === 'water';
    liquidIds.set(m, l);
  }
  return l;
}

/**
 * How a section's liquids are drawn: `translucent` — see-through surfaces over the floor (fine
 * levels, like the chunks); `tint` — not drawn, the floor under them recoloured as seen through
 * the near water (its colour blended over the floor at its opacity): coarse levels, where a
 * see-through surface over a coarse floor would sort and blend badly.
 */
export type LiquidMode = 'translucent' | 'tint';

const colors = new Map<number, number>();
/** A material's flat colour on a face group (0 top, 1 side, 2 bottom). */
export function lodColor(m: number, group: number): number {
  const key = m * 3 + group;
  let c = colors.get(key);
  if (c === undefined) {
    const style = materialStyle(m);
    const t = style.textures;
    c = t ? averageTileColor(group === 0 ? t.top : group === 2 ? t.bottom : t.side) : style.color;
    colors.set(key, c);
  }
  return c;
}

const tints = new Map<number, number>();
/** A floor face's colour seen through liquid `l` (sRGB): blended in linear light at its opacity. */
export function tintedColor(m: number, group: number, l: number): number {
  const key = (m * 3 + group) * 65536 + l;
  let c = tints.get(key);
  if (c === undefined) {
    const floor = lodColor(m, group);
    const water = lodColor(l, 0);
    const a = materialStyle(l).opacity;
    const mix = (shift: number): number =>
      linearToSrgbByte(
        srgbToLinear((floor >> shift) & 0xff) * (1 - a) + srgbToLinear((water >> shift) & 0xff) * a,
      );
    c = (mix(16) << 16) | (mix(8) << 8) | mix(0);
    tints.set(key, c);
  }
  return c;
}

class Builder {
  private readonly positions: number[] = [];
  private readonly normals: number[] = [];
  private readonly colors: number[] = [];
  private readonly indices: number[] = [];

  quad(
    axis: number,
    sign: number,
    plane: number,
    u0: number,
    u1: number,
    v0: number,
    v1: number,
    color: number,
  ): void {
    const u = (axis + 1) % 3;
    const v = (axis + 2) % 3;
    const base = this.positions.length / 3;
    const shade = axis === 1 ? (sign > 0 ? 1 : 0.55) : axis === 0 ? 0.8 : 0.7;
    // Linear, like the chunks' texels (their sRGB texture is decoded before lighting).
    const r = srgbToLinear((color >> 16) & 0xff) * shade;
    const g = srgbToLinear((color >> 8) & 0xff) * shade;
    const b = srgbToLinear(color & 0xff) * shade;
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
      this.positions.push(p[0] ?? 0, p[1] ?? 0, p[2] ?? 0);
      this.normals.push(axis === 0 ? sign : 0, axis === 1 ? sign : 0, axis === 2 ? sign : 0);
      this.colors.push(r, g, b);
    }
    if (sign > 0) this.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
    else this.indices.push(base, base + 2, base + 1, base, base + 3, base + 2);
  }

  finish(): FlatMesh {
    return {
      positions: Float32Array.from(this.positions),
      normals: Float32Array.from(this.normals),
      colors: Float32Array.from(this.colors),
      indices: Uint32Array.from(this.indices),
    };
  }
}

/**
 * Meshes a section's cells (LOD_VOLUME, `lodCell` order); positions in cells, 0..32. Liquids per
 * `liquids` (LiquidMode): with `tint` they are left out and the faces they cover take the
 * tinted colour.
 */
export function meshSection(
  cells: Uint16Array,
  liquids: LiquidMode = 'translucent',
): SectionMeshes {
  const translucent = liquids === 'translucent';
  const tint = liquids === 'tint';
  if (cells.length !== LOD_VOLUME) throw new RangeError('section cells must be LOD_VOLUME');
  const N = SECTION_CELLS;
  const opaque = new Builder();
  const water = new Builder();
  const skirts = FACES.map(() => new Builder());
  // Merge key per slice cell: 0 none, else ((material + 1) · 1024 + tinting liquid) · 2 + (1 for
  // a skirt face).
  const TINT_IDS = 1024;
  const mask = new Int32Array(N * N);
  const cell = [0, 0, 0];

  for (let face = 0; face < 6; face++) {
    const [axis = 0, sign = 1] = FACES[face] ?? [];
    const u = (axis + 1) % 3;
    const v = (axis + 2) % 3;
    const group = axis === 1 ? (sign > 0 ? 0 : 2) : 1;
    for (let d = 0; d < N; d++) {
      mask.fill(0);
      let any = false;
      const border = sign > 0 ? d === N - 1 : d === 0;
      for (let j = 0; j < N; j++) {
        for (let i = 0; i < N; i++) {
          cell[axis] = d;
          cell[u] = i;
          cell[v] = j;
          const m = cells[cellIndex(cell[0] ?? 0, cell[1] ?? 0, cell[2] ?? 0)] ?? 0;
          if (m === 0 || (tint && isLiquid(m))) continue;
          cell[axis] = d + sign;
          const n = cells[cellIndex(cell[0] ?? 0, cell[1] ?? 0, cell[2] ?? 0)] ?? 0;
          const liquid = translucent && isLiquid(m);
          const nSolid = n !== 0 && !isLiquid(n);
          const hidden = nSolid || (liquid && n !== 0);
          if (hidden && (!border || liquid)) continue;
          const tinting = tint && n !== 0 && n < TINT_IDS && isLiquid(n) ? n : 0;
          mask[i + N * j] = ((m + 1) * TINT_IDS + tinting) * 2 + (hidden ? 1 : 0);
          any = true;
        }
      }
      if (!any) continue;
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
          const m = Math.floor((key >> 1) / TINT_IDS) - 1;
          const tinting = (key >> 1) % TINT_IDS;
          const skirt = (key & 1) === 1;
          const target = skirt ? skirts[face] : translucent && isLiquid(m) ? water : opaque;
          const color = tinting ? tintedColor(m, group, tinting) : lodColor(m, group);
          target?.quad(axis, sign, sign > 0 ? d + 1 : d, i, i + w, j, j + h, color);
          i += w;
        }
      }
    }
  }
  return { opaque: opaque.finish(), water: water.finish(), skirts: skirts.map((s) => s.finish()) };
}

/** The buffers of a result, for transferring it between threads. */
export function sectionBuffers(m: SectionMeshes): ArrayBuffer[] {
  return [m.opaque, m.water, ...m.skirts].flatMap((a) => [
    a.positions.buffer,
    a.normals.buffer,
    a.colors.buffer,
    a.indices.buffer,
  ]);
}

/** GPU bytes of a result (for the LOD cache budget). */
export function sectionBytes(m: SectionMeshes): number {
  return [m.opaque, m.water, ...m.skirts].reduce(
    (n, a) =>
      n +
      a.positions.byteLength +
      a.normals.byteLength +
      a.colors.byteLength +
      a.indices.byteLength,
    0,
  );
}
