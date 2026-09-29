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

/** Floats per column of a section's surface (height in cells, material, flags). */
export const SURFACE_STRIDE = 3;
/** Surface heights are drawn in steps of 1/SURFACE_STEPS of a cell. */
export const SURFACE_STEPS = 2;
const SURFACE_VALID = 1;
const SURFACE_WET = 2;

/** The first liquid material (the water the tinted floor is seen through). */
let waterId = -1;
function waterMaterial(): number {
  if (waterId < 0) {
    for (let m = 1; m < 256 && waterId < 0; m++) if (isLiquid(m)) waterId = m;
  }
  return waterId;
}

/**
 * Meshes a section's cells (LOD_VOLUME, `lodCell` order); positions in cells, 0..32. Liquids per
 * `liquids` (LiquidMode): with `tint` they are left out and the faces they cover take the
 * tinted colour.
 *
 * `surface` (optional; generated sections): each column's exact surface, 34² × SURFACE_STRIDE
 * floats in (z + 1) · 34 + (x + 1) order — height in cells from the section's bottom, material,
 * flags (1 valid, 2 wet). A cell is filled from its bottom voxel, so its top lifts the ground by
 * up to a cell (kilometres far away); where a column's topmost solid cell holds its surface, that
 * cell's top is drawn at the surface instead, with walls down to lower neighbours.
 */
export function meshSection(
  cells: Uint16Array,
  liquids: LiquidMode = 'translucent',
  surface: Float32Array | null = null,
): SectionMeshes {
  const translucent = liquids === 'translucent';
  const tint = liquids === 'tint';
  if (cells.length !== LOD_VOLUME) throw new RangeError('section cells must be LOD_VOLUME');
  const N = SECTION_CELLS;
  const opaque = new Builder();
  const waterMesh = new Builder();
  const skirts = FACES.map(() => new Builder());
  const special = findSurfaces(cells, tint, surface);
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
          // A column's surface cell: its top and sides are drawn at the surface (below).
          if (face !== 3 && special.y[col(cell[0] ?? 0, cell[2] ?? 0)] === cell[1]) continue;
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
          const target = skirt ? skirts[face] : translucent && isLiquid(m) ? waterMesh : opaque;
          const color = tinting ? tintedColor(m, group, tinting) : lodColor(m, group);
          target?.quad(axis, sign, sign > 0 ? d + 1 : d, i, i + w, j, j + h, color);
          i += w;
        }
      }
    }
  }
  emitSurfaces(cells, tint, special, opaque, skirts);
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
  wet: Uint8Array;
}
const NO_SURFACE = -1000;

/** Which cell of each column holds its surface, where the surface data says so. */
function findSurfaces(cells: Uint16Array, tint: boolean, surface: Float32Array | null): Surfaces {
  const n = LOD_PAD * LOD_PAD;
  const out: Surfaces = {
    y: new Int16Array(n).fill(NO_SURFACE),
    h: new Float32Array(n),
    m: new Uint16Array(n),
    wet: new Uint8Array(n),
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
        if (tint && m !== 0 && flags & SURFACE_WET && h >= y && h <= y + 1) break;
      }
      if (y < -1 || y === top || h < y - 1e-3 || h > y + 1 + 1e-3) continue;
      const m = cells[cellIndex(x, y, z)] ?? 0;
      // In 1/SURFACE_STEPS of a cell (at most 1/4 cell off: a pixel or two, as cells are a few
      // pixels on screen; finer steps cost far more triangles). At the cell's top the cell is
      // drawn as usual (and merges).
      const steps = Math.round(Math.min(Math.max(h - y, 0), 1) * SURFACE_STEPS);
      if (steps === SURFACE_STEPS && !isLiquid(m)) continue;
      out.y[c] = y;
      out.h[c] = y + steps / SURFACE_STEPS;
      out.m[c] = isLiquid(m) ? (surface[c * SURFACE_STRIDE + 1] ?? m) : m;
      out.wet[c] = flags & SURFACE_WET ? 1 : 0;
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
  color: number;
}

/**
 * Tops at the surface height and the walls between columns (see meshSection), merged like the
 * rest: tops of equal height and colour into rectangles, walls along a row into strips.
 */
function emitSurfaces(
  cells: Uint16Array,
  tint: boolean,
  s: Surfaces,
  opaque: Builder,
  skirts: Builder[],
): void {
  const N = SECTION_CELLS;
  const colorOf = (m: number, group: number, wet: boolean): number =>
    tint && wet ? tintedColor(m, group, waterMaterial()) : lodColor(m, group);
  const builder = (target: number): Builder =>
    target === 0 ? opaque : (skirts[target - 1] ?? opaque);

  // Tops: greedy rectangles over (z, x) of equal height and colour.
  const topKey = new Float64Array(N * N);
  for (let z = 0; z < N; z++) {
    for (let x = 0; x < N; x++) {
      const c = col(x, z);
      const y = s.y[c] ?? NO_SURFACE;
      if (y < 0) continue;
      const color = colorOf(s.m[c] ?? 0, 0, s.wet[c] === 1);
      // Height in 1/SURFACE_STEPS cells (integral), then colour: exact in a double.
      topKey[z + N * x] = (Math.round((s.h[c] ?? y) * SURFACE_STEPS) + 1) * 0x1000000 + color;
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
      const h = (Math.floor(key / 0x1000000) - 1) / SURFACE_STEPS;
      // The top: axis y, u = z, v = x.
      opaque.quad(1, 1, h, z, z + w, x, x + d, key % 0x1000000);
      z += w;
    }
  }

  // Walls, per side: computed per column, then runs of equal walls along the row merged.
  for (const [axis, sign, face] of SIDES) {
    for (let a = 0; a < N; a++) {
      // `a` is the coordinate along the wall's axis (x for ±X walls, z for ±Z), `b` along the row.
      let run: Wall | null = null;
      let runStart = 0;
      const flush = (bEnd: number): void => {
        if (!run || run.hi - run.lo < 1e-4) return;
        const plane = sign > 0 ? a + 1 : a;
        // Axis x: u = y, v = z; axis z: u = x, v = y.
        if (axis === 0)
          builder(run.target).quad(0, run.sign, plane, run.lo, run.hi, runStart, bEnd, run.color);
        else
          builder(run.target).quad(2, run.sign, plane, runStart, bEnd, run.lo, run.hi, run.color);
      };
      for (let b = 0; b <= N; b++) {
        let wall: Wall | null = null;
        if (b < N) {
          const x = axis === 0 ? a : b;
          const z = axis === 0 ? b : a;
          wall = wallOf(cells, s, x, z, axis, sign, face, colorOf);
        }
        const same =
          wall &&
          run &&
          wall.target === run.target &&
          wall.sign === run.sign &&
          wall.lo === run.lo &&
          wall.hi === run.hi &&
          wall.color === run.color;
        if (same) continue;
        flush(b);
        run = wall;
        runStart = b;
      }
    }
  }
}

/** The wall on one side of column (x, z) with a surface, if any (see meshSection). */
function wallOf(
  cells: Uint16Array,
  s: Surfaces,
  x: number,
  z: number,
  axis: number,
  sign: number,
  face: number,
  colorOf: (m: number, group: number, wet: boolean) => number,
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
  const side = colorOf(s.m[c] ?? 0, 1, s.wet[c] === 1);
  if (!nFilled) return { target: 0, sign, lo: y, hi: h, color: side }; // open: up to our surface
  if (nSurfaceHere) {
    // Both surfaces in this row: the higher one's side shows down to the lower.
    const lo = s.h[nc] ?? y + 1;
    return lo < h ? { target: border ? face + 1 : 0, sign, lo, hi: h, color: side } : null;
  }
  // A full neighbour cell stands taller: its face towards us, above our surface (a neighbour
  // section's cell draws its own).
  if (!border) return { target: 0, sign: -sign, lo: h, hi: y + 1, color: lodColor(nm, 1) };
  return { target: face + 1, sign, lo: y, hi: h, color: side }; // hidden by the apron: a skirt
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
