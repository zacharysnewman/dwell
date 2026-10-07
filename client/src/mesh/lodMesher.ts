// Greedy mesher for LOD sections (ARCHITECTURE.md §6.6): a section's 34³ cells (its 32³ and a
// one-cell apron, lod/grid.ts `lodCell` order — the chunk mesher's padded layout) → flat-coloured
// geometry in cell units. Every material but air and liquids is a full cube at LOD; each face has
// its material's flat colour (the average of its texture) with the chunk mesher's face shading.
// Faces on the section's border that the apron hides go to a per-side *skirt* instead of being
// dropped: the renderer shows a side's skirt when the neighbour there is not drawn at the same
// level, which closes the cracks between levels. With `slopes`, the surface cells are drawn as the
// slope pieces of the terrain generator (SLOPE_BLOCKS.md §3.2): the heights of the columns around a
// corner decide its height, in halves of a cell, so distant hills read as facets, not terraces.
// Pure data, so it runs in the meshing workers.
import { LOD_PAD, LOD_VOLUME, SECTION_CELLS, SURFACE_STRIDE } from '../lod/grid';
import { faceTint, normalTint } from '../render/look';
import { averageTileColor, srgbToLinear } from '../render/textures';
import { PATTERN_SHAPE, patternCorners, patternIndex, pieceFor } from '../world/slopePieces';
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

/** The generator's tint unit (biomes.h kTintUnit): 64 is a multiplier of 1. */
const TINT_UNIT = 64;

/**
 * The biome tint of a section's columns (the surface data's last two values, 0xRRGGBB in 1/64: the
 * grass and the foliage colour, smoothed by the generator), looked up bilinearly between column
 * centres at a vertex. Nothing is tinted without surface data (modified sections, flat worlds).
 */
export class LodTint {
  private readonly grass: Float32Array | null;
  private readonly foliage: Float32Array | null;

  constructor(surface: Float32Array | null) {
    const n = LOD_PAD * LOD_PAD;
    if (!surface || surface.length < n * SURFACE_STRIDE) {
      this.grass = this.foliage = null;
      return;
    }
    const unpack = (offset: number): Float32Array => {
      const out = new Float32Array(n * 3);
      for (let c = 0; c < n; c++) {
        const packed = surface[c * SURFACE_STRIDE + offset] ?? 0;
        if (packed === 0) {
          out.fill(1, c * 3, c * 3 + 3);
        } else {
          out[c * 3] = ((packed >> 16) & 0xff) / TINT_UNIT;
          out[c * 3 + 1] = ((packed >> 8) & 0xff) / TINT_UNIT;
          out[c * 3 + 2] = (packed & 0xff) / TINT_UNIT;
        }
      }
      return out;
    };
    this.grass = unpack(4);
    this.foliage = unpack(5);
  }

  /** Multiplies `rgb` (linear) by the tint of `kind` at section position (x, z) in cells. */
  apply(kind: 'grass' | 'foliage', x: number, z: number, rgb: number[]): void {
    const t = kind === 'grass' ? this.grass : this.foliage;
    if (!t) return;
    const lo = -1;
    const hi = SECTION_CELLS;
    const u = Math.min(Math.max(x - 0.5, lo), hi - 1e-6);
    const v = Math.min(Math.max(z - 0.5, lo), hi - 1e-6);
    const i = Math.floor(u);
    const j = Math.floor(v);
    const fx = u - i;
    const fz = v - j;
    for (let c = 0; c < 3; c++) {
      const at = (di: number, dj: number): number => t[col(i + di, j + dj) * 3 + c] ?? 1;
      const a = at(0, 0) + (at(1, 0) - at(0, 0)) * fx;
      const b = at(0, 1) + (at(1, 1) - at(0, 1)) * fx;
      rgb[c] = (rgb[c] ?? 1) * (a + (b - a) * fz);
    }
  }
}

const NO_TINT = new LodTint(null);

/**
 * Step sides on slopes: full detail shows the side faces of one-block steps on a hillside, about a
 * third of the gradient as a share of its surface (measured over generated terrain: 5 % at a
 * gradient of 0.2, 15 % at 0.5, ~40 % on the steepest cells, alike at 16–64 m cells), which a
 * distant cell's smooth top does not have. Tops are tinted toward the material's side colour by
 * that share, in TINT_STEPS steps (so equal tops still merge).
 */
export const SIDE_SHARE_PER_GRADIENT = 1 / 3;
export const SIDE_SHARE_MAX = 0.5;
const TINT_STEPS = 8;

const tops = new Map<number, number>();
/** A material's top colour with `tint` (0..TINT_STEPS) eighths of its side colour mixed in. */
function lodTop(m: number, tint: number): number {
  if (tint === 0) return lodColor(m, 0);
  const key = m * (TINT_STEPS + 1) + tint;
  let c = tops.get(key);
  if (c === undefined) {
    const a = lodColor(m, 0);
    const b = lodColor(m, 1);
    const f = tint / TINT_STEPS;
    c = 0;
    for (const shift of [16, 8, 0]) {
      const v = Math.round(((a >> shift) & 0xff) * (1 - f) + ((b >> shift) & 0xff) * f);
      c |= v << shift;
    }
    tops.set(key, c);
  }
  return c;
}

class Builder {
  private readonly positions: number[] = [];
  private readonly normals: number[] = [];
  private readonly colors: number[] = [];
  private readonly indices: number[] = [];
  private readonly rgb: [number, number, number] = [1, 1, 1];

  constructor(private readonly tint: LodTint = NO_TINT) {}

  /** Pushes a vertex colour: the base (linear) × the biome tint of the material at (x, z). */
  private pushColor(r: number, g: number, b: number, m: number | undefined, x: number, z: number) {
    const kind = m === undefined ? undefined : materialStyle(m).tint;
    if (!kind) {
      this.colors.push(r, g, b);
      return;
    }
    this.rgb[0] = r;
    this.rgb[1] = g;
    this.rgb[2] = b;
    this.tint.apply(kind, x, z, this.rgb);
    this.colors.push(...this.rgb);
  }

  quad(
    axis: number,
    sign: number,
    plane: number,
    u0: number,
    u1: number,
    v0: number,
    v1: number,
    color: number,
    m?: number,
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
      this.pushColor(r, g, b, m, p[0] ?? 0, p[2] ?? 0);
    }
    if (sign > 0) this.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
    else this.indices.push(base, base + 2, base + 1, base, base + 3, base + 2);
  }

  /**
   * A wall in the plane `axis = plane` (axis 0: u = y, v = z; axis 2: u = x, v = y) over the run
   * [b0, b1] along the wall, from heights lo to hi that may differ at the two ends (a trapezoid).
   */
  wall(
    axis: number,
    sign: number,
    plane: number,
    b0: number,
    b1: number,
    lo: readonly [number, number],
    hi: readonly [number, number],
    color: number,
    m?: number,
  ): void {
    const tint = faceTint(axis, sign);
    const r = srgbToLinear((color >> 16) & 0xff) * tint[0];
    const g = srgbToLinear((color >> 8) & 0xff) * tint[1];
    const b = srgbToLinear(color & 0xff) * tint[2];
    const base = this.positions.length / 3;
    // The same corner order as `quad`: (u0, v0), (u1, v0), (u1, v1), (u0, v1).
    const corners: [number, number, number][] =
      axis === 0
        ? [
            [plane, lo[0], b0],
            [plane, hi[0], b0],
            [plane, hi[1], b1],
            [plane, lo[1], b1],
          ]
        : [
            [b0, lo[0], plane],
            [b1, lo[1], plane],
            [b1, hi[1], plane],
            [b0, hi[0], plane],
          ];
    for (const [x, y, z] of corners) {
      this.positions.push(x, y, z);
      this.normals.push(axis === 0 ? sign : 0, 0, axis === 2 ? sign : 0);
      this.pushColor(r, g, b, m, x, z);
    }
    if (sign > 0) this.indices.push(base, base + 1, base + 2, base, base + 2, base + 3);
    else this.indices.push(base, base + 2, base + 1, base, base + 3, base + 2);
  }

  /** A convex polygon (counter-clockwise seen from outside) lit by its own normal. */
  polygon(pts: readonly (readonly number[])[], color: number, m?: number): void {
    const [a = [0, 0, 0], b = [0, 0, 0], c = [0, 0, 0]] = pts;
    const e1 = [(b[0] ?? 0) - (a[0] ?? 0), (b[1] ?? 0) - (a[1] ?? 0), (b[2] ?? 0) - (a[2] ?? 0)];
    const e2 = [(c[0] ?? 0) - (a[0] ?? 0), (c[1] ?? 0) - (a[1] ?? 0), (c[2] ?? 0) - (a[2] ?? 0)];
    const nx = (e1[1] ?? 0) * (e2[2] ?? 0) - (e1[2] ?? 0) * (e2[1] ?? 0);
    const ny = (e1[2] ?? 0) * (e2[0] ?? 0) - (e1[0] ?? 0) * (e2[2] ?? 0);
    const nz = (e1[0] ?? 0) * (e2[1] ?? 0) - (e1[1] ?? 0) * (e2[0] ?? 0);
    const len = Math.hypot(nx, ny, nz) || 1;
    const tint = normalTint(nx / len, ny / len, nz / len);
    const r = srgbToLinear((color >> 16) & 0xff) * tint[0];
    const g = srgbToLinear((color >> 8) & 0xff) * tint[1];
    const bl = srgbToLinear(color & 0xff) * tint[2];
    const base = this.positions.length / 3;
    for (const p of pts) {
      this.positions.push(p[0] ?? 0, p[1] ?? 0, p[2] ?? 0);
      this.normals.push(nx / len, ny / len, nz / len);
      this.pushColor(r, g, bl, m, p[0] ?? 0, p[2] ?? 0);
    }
    for (let i = 1; i + 1 < pts.length; i++) this.indices.push(base, base + i, base + i + 1);
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

export { SURFACE_STRIDE };
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
 * flags (1 valid, 2 wet), and where wet the water's level in cells. A cell is filled from its
 * bottom voxel, so its top lifts the ground by up to a cell (kilometres far away); where a column's
 * topmost solid cell holds its surface, that cell's top is drawn at the surface instead, with walls
 * down to lower neighbours. Likewise a column's water is drawn at its level, not at its top liquid
 * cell's top: the sea's level is a cell boundary at every level, but a river's or a lake's is not.
 */
export interface MeshSectionOptions {
  /** Column surfaces (see above). */
  surface?: Float32Array | null;
  /**
   * How far (in cells) a liquid's top face sits below its cell's top — the chunks draw water's
   * surface at 7/8 of a block, so at sea level it is 1/8 m below the LOD cells' grid.
   */
  waterDrop?: number;
  /**
   * Draw the surface cells as slope pieces from their corner heights (SLOPE_BLOCKS.md §3.2), at
   * every level, with or without `surface` (a section without it uses each column's top solid
   * cell). Off by default.
   */
  slopes?: boolean;
}

export function meshSection(cells: Uint16Array, options: MeshSectionOptions = {}): SectionMeshes {
  const surface = options.surface ?? null;
  const waterDrop = options.waterDrop ?? 0;
  if (cells.length !== LOD_VOLUME) throw new RangeError('section cells must be LOD_VOLUME');
  const N = SECTION_CELLS;
  const tint = new LodTint(surface);
  const opaque = new Builder(tint);
  const waterMesh = new Builder();
  const skirts = FACES.map(() => new Builder(tint));
  const special = findSurfaces(cells, surface, options.slopes ?? false);
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
          // The top liquid cell of a column with a water level: its top is drawn at the level
          // (emitSurfaces), its sides up to it (below).
          const wc = col(cell[0] ?? 0, cell[2] ?? 0);
          const atLevel = special.wy[wc] === cell[1];
          if (atLevel && face === 2) continue;
          cell[axis] = d + sign;
          const n = cells[cellIndex(cell[0] ?? 0, cell[1] ?? 0, cell[2] ?? 0)] ?? 0;
          const liquid = isLiquid(m);
          const nSolid = n !== 0 && !isLiquid(n);
          const hidden = nSolid || (liquid && n !== 0);
          if (hidden && (!border || liquid)) continue;
          if (atLevel && axis !== 1) {
            const y = cell[1] ?? 0;
            const lid = (special.w[wc] ?? y + 1) - waterDrop;
            const plane = sign > 0 ? d + 1 : d;
            const color = lodColor(m, group);
            if (axis === 0) waterMesh.quad(0, sign, plane, y, lid, j, j + 1, color);
            else waterMesh.quad(2, sign, plane, i, i + 1, y, lid, color);
            continue;
          }
          // An exposed top takes its column's tint (tintSlopes).
          const tint = face === 2 && n === 0 && !liquid ? (special.t[wc] ?? 0) : 0;
          mask[i + N * j] = ((m + 1) * (TINT_STEPS + 1) + tint) * 2 + (hidden ? 1 : 0);
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
          const tint = (key >> 1) % (TINT_STEPS + 1);
          const m = Math.floor((key >> 1) / (TINT_STEPS + 1)) - 1;
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
            group === 0 && tint > 0 ? lodTop(m, tint) : lodColor(m, group),
            m,
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
  /**
   * Sloped columns: the top's corner heights in halves above the cell's floor, NW NE SE SW (four
   * per column); −1 in the first for a flat top at `h`.
   */
  q: Int8Array;
  /** The column's topmost liquid cell (with air above it), whose top is drawn at `w`; else NO_SURFACE. */
  wy: Int16Array;
  /** Its water level in cells from the section's bottom (within that cell). */
  w: Float32Array;
  /** 1 where the column or a neighbour is wet: its ground is drawn at its exact height. */
  shore: Uint8Array;
  /** The column's top tint toward its side colour, in TINT_STEPS steps (see SIDE_SHARE). */
  t: Uint8Array;
}
const NO_SURFACE = -1000;
const FLAT = -1;

/** Which cell of each column holds its surface, where the surface data says so. */
function findSurfaces(cells: Uint16Array, surface: Float32Array | null, slopes: boolean): Surfaces {
  const n = LOD_PAD * LOD_PAD;
  const out: Surfaces = {
    y: new Int16Array(n).fill(NO_SURFACE),
    h: new Float32Array(n),
    m: new Uint16Array(n),
    q: new Int8Array(n * 4).fill(FLAT),
    wy: new Int16Array(n).fill(NO_SURFACE),
    w: new Float32Array(n),
    shore: new Uint8Array(n),
    t: new Uint8Array(n),
  };
  const data = surface && surface.length >= n * SURFACE_STRIDE ? surface : null;
  const top: number = SECTION_CELLS;
  if (data) {
    // Water levels: a wet column's topmost cell, if liquid, has its top at the water's level (which
    // lies within it: the cell above would be water too if the level were higher).
    for (let z = -1; z <= top; z++) {
      for (let x = -1; x <= top; x++) {
        const c = col(x, z);
        const flags = data[c * SURFACE_STRIDE + 2] ?? 0;
        if (!(flags & SURFACE_VALID) || !(flags & SURFACE_WET)) continue;
        let y: number = top;
        while (y >= -1 && (cells[cellIndex(x, y, z)] ?? 0) === 0) y--;
        if (y < -1 || !isLiquid(cells[cellIndex(x, y, z)] ?? 0)) continue;
        const level = data[c * SURFACE_STRIDE + 3] ?? 0;
        if (level <= y + 1e-6 || level > y + 1 + 1e-4) continue; // not this cell's: its top
        out.wy[c] = y;
        out.w[c] = Math.min(level, y + 1);
      }
    }
    // Shores: dry columns beside wet ones. Half-cell steps would put a bank a metre above the water
    // under it, or up to its cell's top (a cell is metres to kilometres tall), so their ground is
    // drawn at its exact height. A column is wet with the wet flag, or when its topmost filled
    // cell is liquid — as the sea in the apron row below a section whose bottom is sea level (its
    // floor is the section below's, so this section's surface data has no wet column there).
    const wet = new Uint8Array(n);
    for (let z = -1; z <= top; z++) {
      for (let x = -1; x <= top; x++) {
        const c = col(x, z);
        let y: number = top;
        while (y >= -1 && (cells[cellIndex(x, y, z)] ?? 0) === 0) y--;
        if (((data[c * SURFACE_STRIDE + 2] ?? 0) & SURFACE_WET) !== 0) wet[c] = 1;
        else if (y >= -1 && isLiquid(cells[cellIndex(x, y, z)] ?? 0)) wet[c] = 1;
      }
    }
    for (let z = -1; z <= top; z++) {
      for (let x = -1; x <= top; x++) {
        const c = col(x, z);
        if (wet[c]) continue;
        let beside = false;
        for (let dz = -1; dz <= 1 && !beside; dz++)
          for (let dx = -1; dx <= 1 && !beside; dx++) {
            const nx = x + dx;
            const nz = z + dz;
            beside = nx >= -1 && nx <= top && nz >= -1 && nz <= top && wet[col(nx, nz)] === 1;
          }
        if (beside) out.shore[c] = 1;
      }
    }
    for (let z = -1; z <= top; z++) {
      for (let x = -1; x <= top; x++) {
        const c = col(x, z);
        const flags = data[c * SURFACE_STRIDE + 2] ?? 0;
        if (!(flags & SURFACE_VALID)) continue;
        const h = data[c * SURFACE_STRIDE] ?? 0;
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
        const level = out.wy[c] === y ? (out.w[c] ?? y + 1) : y + 1;
        // In 1/SURFACE_STEPS of a cell (at most 1/4 cell off: a pixel or two, as cells are a few
        // pixels on screen; finer steps cost far more triangles). At the cell's top the cell is
        // drawn as usual (and merges).
        if (out.shore[c] && !isLiquid(m)) {
          // A bank beside water: at its height (a full cell is drawn as usual, and merges).
          if (h >= y + 1 - 1e-3) continue;
          out.y[c] = y;
          out.h[c] = Math.max(h, y);
          out.m[c] = m;
          continue;
        }
        let steps = Math.round(Math.min(Math.max(h - y, 0), 1) * SURFACE_STEPS);
        if (steps === SURFACE_STEPS && !isLiquid(m)) continue;
        // A sea floor is never drawn at or over its water's surface (only waterDrop, a sliver of a
        // cell, from it: they z-fight; above it the floor hides the water) when that surface is
        // this cell's: the highest step strictly below the level.
        if (isLiquid(m) && (cells[cellIndex(x, y + 1, z)] ?? 0) === 0)
          steps = Math.min(steps, Math.max(0, Math.ceil((level - y) * SURFACE_STEPS) - 1));
        out.y[c] = y;
        out.h[c] = y + steps / SURFACE_STEPS;
        out.m[c] = isLiquid(m) ? (data[c * SURFACE_STRIDE + 1] ?? m) : m;
      }
    }
  }
  if (slopes) applySlopes(cells, data, out);
  tintSlopes(cells, out);
  return out;
}

/** Each dry column's top tint from its gradient: its height against its four neighbours'. */
function tintSlopes(cells: Uint16Array, out: Surfaces): void {
  const top: number = SECTION_CELLS;
  const height = new Float32Array(LOD_PAD * LOD_PAD).fill(Number.NaN);
  for (let z = -1; z <= top; z++) {
    for (let x = -1; x <= top; x++) {
      const c = col(x, z);
      const sy = out.y[c] ?? NO_SURFACE;
      if (sy !== NO_SURFACE) {
        if ((out.q[c * 4] ?? FLAT) !== FLAT) {
          let sum = 0;
          for (let i = 0; i < 4; i++) sum += out.q[c * 4 + i] ?? 0;
          height[c] = sy + sum / 8; // the mean of the corners (halves)
        } else {
          height[c] = out.h[c] ?? sy + 1;
        }
        continue;
      }
      let y: number = top;
      while (y >= -1 && (cells[cellIndex(x, y, z)] ?? 0) === 0) y--;
      if (y < -1 || isLiquid(cells[cellIndex(x, y, z)] ?? 0)) continue; // none, or under water
      height[c] = y + 1;
    }
  }
  for (let z = 0; z < top; z++) {
    for (let x = 0; x < top; x++) {
      const c = col(x, z);
      const h = height[c] ?? Number.NaN;
      if (Number.isNaN(h)) continue;
      let gradient = 0;
      for (const [dx, dz] of [
        [1, 0],
        [-1, 0],
        [0, 1],
        [0, -1],
      ] as const) {
        const n = height[col(x + dx, z + dz)] ?? Number.NaN;
        if (!Number.isNaN(n)) gradient = Math.max(gradient, Math.abs(n - h));
      }
      const share = Math.min(SIDE_SHARE_MAX, gradient * SIDE_SHARE_PER_GRADIENT);
      out.t[c] = Math.round(share * TINT_STEPS);
    }
  }
}

/**
 * Slope pieces for the surface cells (SLOPE_BLOCKS.md §3.2): each column has a surface height — the
 * generated surface where the section carries it and the cell holds it, else the top of its topmost
 * solid cell, which is also what a modified (downsampled) section has — and a corner's height is
 * the mean of the four columns around it, rounded to a half. A column becomes sloped when its nine
 * columns have a surface, its corners lie within a cell of each other (a cliff stays flat, with
 * walls) and the piece is neither empty nor a full cell.
 */
function applySlopes(cells: Uint16Array, data: Float32Array | null, out: Surfaces): void {
  const top: number = SECTION_CELLS;
  const n = LOD_PAD * LOD_PAD;
  const height = new Float32Array(n).fill(Number.NaN);
  const cellY = new Int16Array(n).fill(NO_SURFACE);
  for (let z = -1; z <= top; z++) {
    for (let x = -1; x <= top; x++) {
      const c = col(x, z);
      const flags = data?.[c * SURFACE_STRIDE + 2] ?? 0;
      if (flags & SURFACE_WET) continue; // a sea floor stays flat, under its water
      let y: number = top;
      for (; y >= -1; y--) {
        const m = cells[cellIndex(x, y, z)] ?? 0;
        if (m !== 0 && !isLiquid(m)) break;
      }
      if (y < -1 || y === top) continue;
      let h = y + 1;
      if (flags & SURFACE_VALID) {
        const raw = data?.[c * SURFACE_STRIDE] ?? 0;
        if (raw >= y - 1e-3 && raw <= y + 1 + 1e-3) h = raw;
      }
      height[c] = h;
      cellY[c] = y;
    }
  }
  const hs = [0, 0, 0, 0];
  const h4 = [0, 0, 0, 0];
  for (let z = 0; z < top; z++) {
    for (let x = 0; x < top; x++) {
      const c = col(x, z);
      const y = cellY[c] ?? NO_SURFACE;
      if (y === NO_SURFACE) continue;
      if (out.shore[c]) continue; // beside water: flat at its exact height (findSurfaces)
      // The corner at (x + a, z + b): the mean of columns (x + a − 1 … x + a) × (z + b − 1 … z + b).
      let missing = 0; // columns around the cell with no surface
      const corner = (a: number, b: number): number => {
        const cx = x + a;
        const cz = z + b;
        hs[0] = height[col(cx - 1, cz - 1)] ?? Number.NaN;
        hs[1] = height[col(cx, cz - 1)] ?? Number.NaN;
        hs[2] = height[col(cx - 1, cz)] ?? Number.NaN;
        hs[3] = height[col(cx, cz)] ?? Number.NaN;
        if (Number.isNaN(hs[0] + hs[1] + hs[2] + hs[3])) missing++;
        return Math.floor((hs[0] + hs[1] + hs[2] + hs[3]) * 0.5 + 0.5);
      };
      h4[0] = corner(0, 0); // NW
      h4[1] = corner(1, 0); // NE
      h4[2] = corner(1, 1); // SE
      h4[3] = corner(0, 1); // SW
      if (missing > 0) continue;
      if (Math.max(...h4) - Math.min(...h4) > 2) continue; // a cliff
      const q = h4.map((v) => Math.min(2, Math.max(0, v - 2 * y)));
      const piece = pieceFor(q[0] ?? 0, q[1] ?? 0, q[2] ?? 0, q[3] ?? 0);
      if (piece === 0 || piece === patternIndex(2, 2, 2, 2)) continue; // nothing, or a whole cell
      const corners = patternCorners(piece);
      out.y[c] = y;
      out.m[c] = cells[cellIndex(x, y, z)] ?? 0;
      if (corners[0] === corners[1] && corners[1] === corners[2] && corners[2] === corners[3]) {
        out.h[c] = y + corners[0] / 2; // a slab: flat, as any partial cell
        out.q.fill(FLAT, c * 4, c * 4 + 4);
      } else {
        out.h[c] = y + Math.max(...corners) / 2;
        for (let i = 0; i < 4; i++) out.q[c * 4 + i] = corners[i] ?? 0;
      }
    }
  }
}

/**
 * Heights (in cells) of a column's top along one of its sides, in the order the wall runs: for
 * ±X sides along z (north end first), for ±Z along x (west end first).
 */
function profile(s: Surfaces, c: number, axis: number, sign: number): readonly [number, number] {
  const h = s.h[c] ?? 0;
  if ((s.q[c * 4] ?? FLAT) === FLAT) return [h, h];
  const y = s.y[c] ?? 0;
  const at = (i: number): number => y + (s.q[c * 4 + i] ?? 0) / 2;
  // Corners: 0 NW, 1 NE, 2 SE, 3 SW (north = −z, east = +x).
  if (axis === 0) return sign > 0 ? [at(1), at(2)] : [at(0), at(3)];
  return sign > 0 ? [at(3), at(2)] : [at(0), at(1)];
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
  lo: readonly [number, number];
  hi: readonly [number, number];
  material: number;
}

const flatWall = (w: Wall): boolean => w.lo[0] === w.lo[1] && w.hi[0] === w.hi[1];

/**
 * Tops at the surface height and the walls between columns (see meshSection), merged like the
 * rest: tops of equal height and material into rectangles, walls along a row into strips. A sea
 * floor drawn in a water cell gets that cell's water surface above it. Sloped columns draw their
 * top piece's surface as triangles of their own, and their walls follow the slope's edge.
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
  const topH = new Float32Array(N * N);
  for (let z = 0; z < N; z++) {
    for (let x = 0; x < N; x++) {
      const c = col(x, z);
      const y = s.y[c] ?? NO_SURFACE;
      if (y < 0) continue;
      if ((s.q[c * 4] ?? FLAT) !== FLAT) {
        // A sloped top: the piece's surface (its triangles, in cell coordinates over the cell).
        const pattern = patternIndex(
          s.q[c * 4] ?? 0,
          s.q[c * 4 + 1] ?? 0,
          s.q[c * 4 + 2] ?? 0,
          s.q[c * 4 + 3] ?? 0,
        );
        const color = lodTop(s.m[c] ?? 0, s.t[c] ?? 0);
        for (const face of PATTERN_SHAPE[pattern]?.faces ?? []) {
          if (face.tag !== 6) continue;
          opaque.polygon(
            face.pts.map((p) => [x + p[0], y + p[1], z + p[2]]),
            color,
            s.m[c] ?? 0,
          );
        }
        continue;
      }
      // Material and height (half-cell steps, or exact beside water): merged where both match.
      topKey[z + N * x] = ((s.m[c] ?? 0) + 1) * (TINT_STEPS + 1) + (s.t[c] ?? 0);
      topH[z + N * x] = s.h[c] ?? y;
    }
  }
  const sameTop = (a: number, b: number): boolean => topKey[a] === topKey[b] && topH[a] === topH[b];
  for (let x = 0; x < N; x++) {
    for (let z = 0; z < N;) {
      const at = z + N * x;
      const key = topKey[at] ?? 0;
      if (key === 0) {
        z++;
        continue;
      }
      let w = 1;
      while (z + w < N && sameTop(z + w + N * x, at)) w++;
      let d = 1;
      grow: while (x + d < N) {
        for (let k = 0; k < w; k++) if (!sameTop(z + k + N * (x + d), at)) break grow;
        d++;
      }
      const h = topH[at] ?? 0;
      for (let dx = 0; dx < d; dx++) topKey.fill(0, z + N * (x + dx), z + w + N * (x + dx));
      // The top: axis y, u = z, v = x.
      const material = Math.floor(key / (TINT_STEPS + 1)) - 1;
      opaque.quad(1, 1, h, z, z + w, x, x + d, lodTop(material, key % (TINT_STEPS + 1)), material);
      z += w;
    }
  }

  // Water surfaces at their level: over floors drawn inside water cells, and over the top liquid
  // cell of any column with a water level (rivers and lakes above sea level).
  {
    // Greedy rectangles over (z, x) of equal water level and liquid.
    const level = new Float32Array(N * N);
    for (let z = 0; z < N; z++) {
      for (let x = 0; x < N; x++) {
        const c = col(x, z);
        topKey[z + N * x] = 0;
        let y = s.wy[c] ?? NO_SURFACE;
        let w = s.w[c] ?? 0;
        if (y === NO_SURFACE) {
          // A floor drawn in a water cell with no level of its own (a sea): the cell's top.
          y = s.y[c] ?? NO_SURFACE;
          if (y < 0) continue;
          const m = cells[cellIndex(x, y, z)] ?? 0;
          const above = cells[cellIndex(x, y + 1, z)] ?? 0;
          if (!isLiquid(m) || above !== 0) continue;
          w = y + 1;
        }
        if (y < 0 || y >= N) continue;
        topKey[z + N * x] = (cells[cellIndex(x, y, z)] ?? 0) + 1;
        level[z + N * x] = w;
      }
    }
    const same = (a: number, b: number): boolean =>
      topKey[a] === topKey[b] && level[a] === level[b];
    for (let x = 0; x < N; x++) {
      for (let z = 0; z < N;) {
        const at = z + N * x;
        const key = topKey[at] ?? 0;
        if (key === 0) {
          z++;
          continue;
        }
        let w = 1;
        while (z + w < N && same(z + w + N * x, at)) w++;
        let d = 1;
        grow: while (x + d < N) {
          for (let k = 0; k < w; k++) if (!same(z + k + N * (x + d), at)) break grow;
          d++;
        }
        const top = level[at] ?? 0;
        for (let dx = 0; dx < d; dx++) topKey.fill(0, z + N * (x + dx), z + w + N * (x + dx));
        waterMesh.quad(1, 1, top - waterDrop, z, z + w, x, x + d, lodColor(key - 1, 0));
        z += w;
      }
    }
  }

  // Walls, per side and slot (see wallOf): computed per column, then runs of equal flat walls
  // along the row merged; sloped walls stand alone.
  for (const [axis, sign, faceIndex] of SIDES) {
    for (let slot = 0; slot < 2; slot++)
      for (let a = 0; a < N; a++) {
        // `a` is the coordinate along the wall's axis (x for ±X walls, z for ±Z), `b` along the row.
        let run: Wall | null = null;
        let runStart = 0;
        const flush = (bEnd: number): void => {
          if (!run || (run.hi[0] - run.lo[0] < 1e-4 && run.hi[1] - run.lo[1] < 1e-4)) return;
          const plane = sign > 0 ? a + 1 : a;
          builder(run.target).wall(
            axis,
            run.sign,
            plane,
            runStart,
            bEnd,
            run.lo,
            run.hi,
            lodColor(run.material, 1),
            run.material,
          );
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
            flatWall(wall) &&
            flatWall(run) &&
            wall.target === run.target &&
            wall.sign === run.sign &&
            wall.lo[0] === run.lo[0] &&
            wall.hi[0] === run.hi[0] &&
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
  const ours = profile(s, c, axis, sign);
  const nx = axis === 0 ? x + sign : x;
  const nz = axis === 2 ? z + sign : z;
  const nc = col(nx, nz);
  const border = nx < 0 || nx >= N || nz < 0 || nz >= N;
  const nm = cells[cellIndex(nx, y, nz)] ?? 0;
  const nSurfaceHere = (s.y[nc] ?? NO_SURFACE) === y;
  const nFilled = nSurfaceHere || (nm !== 0 && !isLiquid(nm));
  const material = s.m[c] ?? 0;
  const skirt = face + 1;
  const floor: readonly [number, number] = [y, y];
  if (!nFilled) return slot === 0 ? { target: 0, sign, lo: floor, hi: ours, material } : null; // open
  if (nSurfaceHere) {
    // Both surfaces in this row: the higher one's side shows down to the lower (and below that,
    // on the border, a skirt). The neighbour's top along the same edge is its opposite side.
    const theirs = profile(s, nc, axis, -sign);
    const lo: readonly [number, number] = [
      Math.min(theirs[0], ours[0]),
      Math.min(theirs[1], ours[1]),
    ];
    if (slot === 0) {
      return lo[0] < ours[0] || lo[1] < ours[1]
        ? { target: 0, sign, lo, hi: ours, material }
        : null;
    }
    return border && (lo[0] > y || lo[1] > y)
      ? { target: skirt, sign, lo: floor, hi: lo, material }
      : null;
  }
  // A full neighbour cell stands taller: its face towards us, above our surface (and on the
  // border, our side below as a skirt).
  if (slot === 0) return { target: 0, sign: -sign, lo: ours, hi: [y + 1, y + 1], material: nm };
  return border ? { target: skirt, sign, lo: floor, hi: ours, material } : null;
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
