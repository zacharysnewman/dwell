// Greedy mesher for LOD sections (ARCHITECTURE.md §6.6): a section's 34³ cells (its 32³ and a
// one-cell apron, lod/grid.ts `lodCell` order — the chunk mesher's padded layout) → flat-coloured
// geometry in cell units. Every material but air and liquids is a full cube at LOD; each face has
// its material's flat colour (the average of its texture) with the chunk mesher's face shading.
// Faces on the section's border that the apron hides go to a per-side *skirt* instead of being
// dropped: the renderer shows a side's skirt when the neighbour there is not drawn at the same
// level, which closes the cracks between levels. Pure data, so it runs in the meshing workers.
import { LOD_PAD, LOD_VOLUME, SECTION_CELLS } from '../lod/grid';
import { faceTint } from '../render/look';
import { averageTileColor, srgbToLinear } from '../render/textures';
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
    const tint = faceTint(axis, sign);
    // Linear, like the chunks' texels (their sRGB texture is decoded before lighting).
    const r = srgbToLinear((color >> 16) & 0xff) * tint[0];
    const g = srgbToLinear((color >> 8) & 0xff) * tint[1];
    const b = srgbToLinear(color & 0xff) * tint[2];
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

/** Floats per column of a section's surface (height in cells, material, flags). */
export const SURFACE_STRIDE = 3;
/** Surface heights are drawn in steps of 1/SURFACE_STEPS of a cell. */
export const SURFACE_STEPS = 2;
const SURFACE_VALID = 1;
const SURFACE_WET = 2;

/**
 * Meshes a section's cells (LOD_VOLUME, `lodCell` order); positions in cells, 0..32. Liquids are
 * see-through surfaces (the `water` mesh) over the floor, as the chunks draw them.
 *
 * `surface` (optional; generated sections): each column's exact surface, 34² × SURFACE_STRIDE
 * floats in (z + 1) · 34 + (x + 1) order — height in cells from the section's bottom, material,
 * flags (1 valid, 2 wet). A cell is filled from its bottom voxel, so its top lifts the ground by
 * up to a cell (kilometres far away); where a column's topmost solid cell holds its surface, that
 * cell's top is drawn at the surface instead, with walls down to lower neighbours.
 */
export interface MeshSectionOptions {
  /** Column surfaces (see above). */
  surface?: Float32Array | null;
  /**
   * How far (in cells) a liquid's top face sits below its cell's top — the chunks draw water's
   * surface at 7/8 of a block, so at sea level it is 1/8 m below the LOD cells' grid.
   */
  waterDrop?: number;
}

export function meshSection(cells: Uint16Array, options: MeshSectionOptions = {}): SectionMeshes {
  const surface = options.surface ?? null;
  const waterDrop = options.waterDrop ?? 0;
  if (cells.length !== LOD_VOLUME) throw new RangeError('section cells must be LOD_VOLUME');
  const N = SECTION_CELLS;
  const opaque = new Builder();
  const waterMesh = new Builder();
  const skirts = FACES.map(() => new Builder());
  const special = findSurfaces(cells, surface);
  // Merge key per slice cell: 0 none, else (material + 1) · 2 + (1 for a skirt face).
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
          if (m === 0) continue;
          // A column's surface cell: its top and sides are drawn at the surface (below).
          const sy = special.y[col(cell[0] ?? 0, cell[2] ?? 0)];
          if (face !== 3 && sy === cell[1]) continue;
          // Under a sea floor drawn in the water cell above, the top is covered — and where the
          // floor lies on the cell's bottom, in the same plane (two tops there z-fight).
          if (face === 2 && sy === (cell[1] ?? 0) + 1) continue;
          cell[axis] = d + sign;
          const n = cells[cellIndex(cell[0] ?? 0, cell[1] ?? 0, cell[2] ?? 0)] ?? 0;
          const liquid = isLiquid(m);
          const nSolid = n !== 0 && !isLiquid(n);
          const hidden = nSolid || (liquid && n !== 0);
          if (hidden && (!border || liquid)) continue;
          mask[i + N * j] = (m + 1) * 2 + (hidden ? 1 : 0);
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
          const m = (key >> 1) - 1;
          const skirt = (key & 1) === 1;
          const target = skirt ? skirts[face] : isLiquid(m) ? waterMesh : opaque;
          // Water's surface where the chunks draw it (waterDrop below the cell's top).
          const drop = axis === 1 && sign > 0 && isLiquid(m) ? waterDrop : 0;
          target?.quad(
            axis,
            sign,
            (sign > 0 ? d + 1 : d) - drop,
            i,
            i + w,
            j,
            j + h,
            lodColor(m, group),
          );
          i += w;
        }
      }
    }
  }
  emitSurfaces(cells, special, opaque, waterMesh, skirts, waterDrop);
  return {
    opaque: opaque.finish(),
    water: waterMesh.finish(),
    skirts: skirts.map((s) => s.finish()),
  };
}

/** Column index in the 34 × 34 padded layout, x and z each −1..32. */
const col = (x: number, z: number): number => x + 1 + LOD_PAD * (z + 1);

interface Surfaces {
  /** The surface cell's y per column (NO_SURFACE: none): its top is drawn at `h`. */
  y: Int16Array;
  /** Surface height in cells from the section's bottom. */
  h: Float32Array;
  /** The surface cell's material (for a sea's floor drawn in a water cell, the floor's). */
  m: Uint16Array;
}
const NO_SURFACE = -1000;

/** Which cell of each column holds its surface, where the surface data says so. */
function findSurfaces(cells: Uint16Array, surface: Float32Array | null): Surfaces {
  const n = LOD_PAD * LOD_PAD;
  const out: Surfaces = {
    y: new Int16Array(n).fill(NO_SURFACE),
    h: new Float32Array(n),
    m: new Uint16Array(n),
  };
  if (!surface || surface.length < n * SURFACE_STRIDE) return out;
  const top: number = SECTION_CELLS;
  for (let z = -1; z <= top; z++) {
    for (let x = -1; x <= top; x++) {
      const c = col(x, z);
      const flags = surface[c * SURFACE_STRIDE + 2] ?? 0;
      if (!(flags & SURFACE_VALID)) continue;
      const h = surface[c * SURFACE_STRIDE] ?? 0;
      // The topmost solid cell (above it only air or liquid), and it must hold the surface.
      let y: number = top;
      for (; y >= -1; y--) {
        const m = cells[cellIndex(x, y, z)] ?? 0;
        if (m !== 0 && !isLiquid(m)) break;
        // A sea's floor within one of its water cells (a cell taller than the sea is deep).
        if (m !== 0 && flags & SURFACE_WET && h >= y && h <= y + 1) break;
      }
      if (y < -1 || y === top || h < y - 1e-3 || h > y + 1 + 1e-3) continue;
      const m = cells[cellIndex(x, y, z)] ?? 0;
      // In 1/SURFACE_STEPS of a cell (at most 1/4 cell off: a pixel or two, as cells are a few
      // pixels on screen; finer steps cost far more triangles). At the cell's top the cell is
      // drawn as usual (and merges).
      let steps = Math.round(Math.min(Math.max(h - y, 0), 1) * SURFACE_STEPS);
      if (steps === SURFACE_STEPS && !isLiquid(m)) continue;
      // A sea floor is never drawn at its water cell's top, level with the water surface (only
      // waterDrop, a sliver of a cell, from it: they z-fight) when that surface is this cell's.
      if (isLiquid(m) && (cells[cellIndex(x, y + 1, z)] ?? 0) === 0)
        steps = Math.min(steps, SURFACE_STEPS - 1);
      out.y[c] = y;
      out.h[c] = y + steps / SURFACE_STEPS;
      out.m[c] = isLiquid(m) ? (surface[c * SURFACE_STRIDE + 1] ?? m) : m;
    }
  }
  return out;
}

const SIDES: readonly (readonly [number, number, number])[] = [
  // [axis, sign, face index] for ±X and ±Z.
  [0, 1, 0],
  [0, -1, 1],
  [2, 1, 4],
  [2, -1, 5],
];

/** A wall between two columns, before merging runs of equal ones. */
interface Wall {
  target: number; // 0 opaque, else skirt face + 1
  sign: number;
  lo: number;
  hi: number;
  material: number;
}

/**
 * Tops at the surface height and the walls between columns (see meshSection), merged like the
 * rest: tops of equal height and material into rectangles, walls along a row into strips. A sea
 * floor drawn in a water cell gets that cell's water surface above it.
 */
function emitSurfaces(
  cells: Uint16Array,
  s: Surfaces,
  opaque: Builder,
  waterMesh: Builder,
  skirts: Builder[],
  waterDrop: number,
): void {
  const N = SECTION_CELLS;
  const builder = (target: number): Builder =>
    target === 0 ? opaque : (skirts[target - 1] ?? opaque);

  // Tops: greedy rectangles over (z, x) of equal height and material.
  const topKey = new Float64Array(N * N);
  for (let z = 0; z < N; z++) {
    for (let x = 0; x < N; x++) {
      const c = col(x, z);
      const y = s.y[c] ?? NO_SURFACE;
      if (y < 0) continue;
      // Height in 1/SURFACE_STEPS cells (integral) and material: exact in a double.
      topKey[z + N * x] = (Math.round((s.h[c] ?? y) * SURFACE_STEPS) + 1) * 0x10000 + (s.m[c] ?? 0);
    }
  }
  for (let x = 0; x < N; x++) {
    for (let z = 0; z < N;) {
      const key = topKey[z + N * x] ?? 0;
      if (key === 0) {
        z++;
        continue;
      }
      let w = 1;
      while (z + w < N && topKey[z + w + N * x] === key) w++;
      let d = 1;
      grow: while (x + d < N) {
        for (let k = 0; k < w; k++) if (topKey[z + k + N * (x + d)] !== key) break grow;
        d++;
      }
      for (let dx = 0; dx < d; dx++) topKey.fill(0, z + N * (x + dx), z + w + N * (x + dx));
      const h = (Math.floor(key / 0x10000) - 1) / SURFACE_STEPS;
      // The top: axis y, u = z, v = x.
      opaque.quad(1, 1, h, z, z + w, x, x + d, lodColor(key % 0x10000, 0));
      z += w;
    }
  }

  // The water surface over floors drawn inside water cells.
  {
    // Greedy rectangles over (z, x) of equal water level and liquid.
    for (let z = 0; z < N; z++) {
      for (let x = 0; x < N; x++) {
        const c = col(x, z);
        const y = s.y[c] ?? NO_SURFACE;
        topKey[z + N * x] = 0;
        if (y < 0) continue;
        const m = cells[cellIndex(x, y, z)] ?? 0;
        const above = cells[cellIndex(x, y + 1, z)] ?? 0;
        if (!isLiquid(m) || above !== 0) continue;
        topKey[z + N * x] = (y + 1) * 0x10000 + m;
      }
    }
    for (let x = 0; x < N; x++) {
      for (let z = 0; z < N;) {
        const key = topKey[z + N * x] ?? 0;
        if (key === 0) {
          z++;
          continue;
        }
        let w = 1;
        while (z + w < N && topKey[z + w + N * x] === key) w++;
        let d = 1;
        grow: while (x + d < N) {
          for (let k = 0; k < w; k++) if (topKey[z + k + N * (x + d)] !== key) break grow;
          d++;
        }
        for (let dx = 0; dx < d; dx++) topKey.fill(0, z + N * (x + dx), z + w + N * (x + dx));
        const top = Math.floor(key / 0x10000);
        waterMesh.quad(1, 1, top - waterDrop, z, z + w, x, x + d, lodColor(key % 0x10000, 0));
        z += w;
      }
    }
  }

  // Walls, per side and slot (see wallOf): computed per column, then runs of equal walls along
  // the row merged.
  for (const [axis, sign, faceIndex] of SIDES) {
    for (let slot = 0; slot < 2; slot++)
      for (let a = 0; a < N; a++) {
        // `a` is the coordinate along the wall's axis (x for ±X walls, z for ±Z), `b` along the row.
        let run: Wall | null = null;
        let runStart = 0;
        const flush = (bEnd: number): void => {
          if (!run || run.hi - run.lo < 1e-4) return;
          const plane = sign > 0 ? a + 1 : a;
          // Axis x: u = y, v = z; axis z: u = x, v = y.
          const color = lodColor(run.material, 1);
          if (axis === 0)
            builder(run.target).quad(0, run.sign, plane, run.lo, run.hi, runStart, bEnd, color);
          else builder(run.target).quad(2, run.sign, plane, runStart, bEnd, run.lo, run.hi, color);
        };
        for (let b = 0; b <= N; b++) {
          let wall: Wall | null = null;
          if (b < N) {
            const x = axis === 0 ? a : b;
            const z = axis === 0 ? b : a;
            wall = wallOf(cells, s, x, z, axis, sign, faceIndex, slot);
          }
          const same =
            wall &&
            run &&
            wall.target === run.target &&
            wall.sign === run.sign &&
            wall.lo === run.lo &&
            wall.hi === run.hi &&
            wall.material === run.material;
          if (same) continue;
          flush(b);
          run = wall;
          runStart = b;
        }
      }
  }
}

/**
 * The wall on one side of column (x, z) with a surface, if any (see meshSection). Slot 0 is what
 * shows; slot 1 the skirt below it on the section's border, which the apron hides.
 *
 * Every wall is drawn by exactly one section, including those on its border: a step between our
 * surface and a neighbour's across the border is ours to draw (the neighbour section's cells see
 * our apron cell as solid and draw only a skirt there, shown only when we are not).
 */
function wallOf(
  cells: Uint16Array,
  s: Surfaces,
  x: number,
  z: number,
  axis: number,
  sign: number,
  face: number,
  slot: number,
): Wall | null {
  const N = SECTION_CELLS;
  const c = col(x, z);
  const y = s.y[c] ?? NO_SURFACE;
  if (y < 0) return null;
  const h = s.h[c] ?? y + 1;
  const nx = axis === 0 ? x + sign : x;
  const nz = axis === 2 ? z + sign : z;
  const nc = col(nx, nz);
  const border = nx < 0 || nx >= N || nz < 0 || nz >= N;
  const nm = cells[cellIndex(nx, y, nz)] ?? 0;
  const nSurfaceHere = (s.y[nc] ?? NO_SURFACE) === y;
  const nFilled = nSurfaceHere || (nm !== 0 && !isLiquid(nm));
  const material = s.m[c] ?? 0;
  const skirt = face + 1;
  if (!nFilled) return slot === 0 ? { target: 0, sign, lo: y, hi: h, material } : null; // open
  if (nSurfaceHere) {
    // Both surfaces in this row: the higher one's side shows down to the lower (and below that,
    // on the border, a skirt).
    const lo = Math.min(s.h[nc] ?? y + 1, h);
    if (slot === 0) return lo < h ? { target: 0, sign, lo, hi: h, material } : null;
    return border && lo > y ? { target: skirt, sign, lo: y, hi: lo, material } : null;
  }
  // A full neighbour cell stands taller: its face towards us, above our surface (and on the
  // border, our side below as a skirt).
  if (slot === 0) return { target: 0, sign: -sign, lo: h, hi: y + 1, material: nm };
  return border ? { target: skirt, sign, lo: y, hi: h, material } : null;
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
