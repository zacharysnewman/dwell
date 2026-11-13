// The resurrection shrines on the big map (docs/SHRINES.md; owner, 2026-11-12: shrines "show on the
// map" and the map can "toggle them off (filters)"). The client crate gives the shrines round the
// view (`mapShrines`, sited from the seed as the server sites them); this shows those in explored
// ground (Guild Wars' map reveal: a shrine is known once its cell is explored) or all of them
// (`reveal` "all", or the `mapRevealed` ability), as the markers' vector icon, behind the map's
// filters (the shrines and each marker kind, remembered per browser). Its looks and reveal rule are
// data (shared/worldgen/shrines.json `map`).

import data from '../../../shared/worldgen/shrines.json';

export interface ShrineMapRules {
  reveal: 'explored' | 'all';
  /** A vector icon of shared/ui/markers.json. */
  icon: string;
  color: string;
  iconPx: number;
  /** Icons nearer than this on screen (px, each axis) merge into one with a count. */
  clusterPx: number;
  /** None show when the grid's spacing draws under this many px (zoomed far out). */
  minSpacingPx: number;
  /** The most time (ms) a draw spends siting cells not sited before; the rest the next draws. */
  siteMsPerDraw: number;
}

/** The map's rules for shrines (shrines.json `map`; `gen.mjs` validates them). */
export const SHRINE_MAP: ShrineMapRules = data.map as ShrineMapRules;

/** The shrine grid's spacing (m): the map asks for the cells round its view's centre. */
export const SHRINE_SPACING: number = data.spacing;

/** A shrine on the map: its face-local x and z (m). */
export interface MapShrine {
  x: number;
  z: number;
}

/** `mapShrines`' flat [x, z]* as shrines. */
export function parseShrines(flat: ArrayLike<number>): MapShrine[] {
  const out: MapShrine[] = [];
  for (let n = 0; n + 1 < flat.length; n += 2) out.push({ x: flat[n] ?? 0, z: flat[n + 1] ?? 0 });
  return out;
}

/** A face-local box (m). */
export interface MapBounds {
  x0: number;
  z0: number;
  x1: number;
  z1: number;
}

/**
 * The shrine grid's cells (i, k) the box `b` touches, on the disc of `radiusM` (a cell whose
 * nearest point lies off the disc holds no shrine): the whole view, however far out it is zoomed
 * (BUGS.md: the map once asked for a square of at most 4 cells round the view's centre).
 */
export function shrineCellsIn(
  b: MapBounds,
  radiusM: number,
  spacing: number = SHRINE_SPACING,
): [number, number][] {
  const near = (lo: number, hi: number) => Math.max(lo, Math.min(0, hi));
  const i0 = Math.floor(Math.max(b.x0, -radiusM) / spacing);
  const i1 = Math.ceil(Math.min(b.x1, radiusM) / spacing) - 1;
  const k0 = Math.floor(Math.max(b.z0, -radiusM) / spacing);
  const k1 = Math.ceil(Math.min(b.z1, radiusM) / spacing) - 1;
  const out: [number, number][] = [];
  for (let k = k0; k <= k1; k++) {
    for (let i = i0; i <= i1; i++) {
      const nx = near(i * spacing, (i + 1) * spacing);
      const nz = near(k * spacing, (k + 1) * spacing);
      if (nx * nx + nz * nz <= radiusM * radiusM) out.push([i, k]);
    }
  }
  return out;
}

/** Whether the map shows shrines at `pxPerM`: the grid's spacing draws at least `minSpacingPx`. */
export function shrinesShownAt(
  pxPerM: number,
  rules: ShrineMapRules = SHRINE_MAP,
  spacing: number = SHRINE_SPACING,
): boolean {
  return spacing * pxPerM >= rules.minSpacingPx;
}

/** An icon the map draws for one shrine or several merged (`count`), at screen pixel (x, y). */
export interface ShrineCluster {
  x: number;
  y: number;
  count: number;
}

/**
 * Screen points merged so no two icons lie nearer than `clusterPx` on either axis: each point joins
 * the first icon within that, else starts its own (in order, so a view draws the same each time).
 */
export function clusterShrines(
  points: readonly { x: number; y: number }[],
  clusterPx: number,
): ShrineCluster[] {
  const out: ShrineCluster[] = [];
  const buckets = new Map<string, ShrineCluster[]>();
  const key = (bi: number, bk: number) => `${String(bi)},${String(bk)}`;
  for (const p of points) {
    const bi = Math.floor(p.x / clusterPx);
    const bk = Math.floor(p.y / clusterPx);
    let into: ShrineCluster | undefined;
    for (let dk = -1; dk <= 1 && !into; dk++) {
      for (let di = -1; di <= 1 && !into; di++) {
        into = buckets
          .get(key(bi + di, bk + dk))
          ?.find((c) => Math.abs(c.x - p.x) < clusterPx && Math.abs(c.y - p.y) < clusterPx);
      }
    }
    if (into) {
      into.count++;
      continue;
    }
    const c = { x: p.x, y: p.y, count: 1 };
    out.push(c);
    const k = key(bi, bk);
    const list = buckets.get(k);
    if (list) list.push(c);
    else buckets.set(k, [c]);
  }
  return out;
}

/**
 * The big map's shrine cells: which hold explored ground (so a fogged cell is never sited), and
 * which were sited (the crate caches them, so asking again is cheap) — new cells are sited within
 * a budget a draw, the rest the next draws.
 */
export class ShrineSites {
  private readonly explored = new Set<string>();
  private readonly sited = new Set<string>();

  constructor(private readonly spacing: number = SHRINE_SPACING) {}

  private key(face: number, i: number, k: number): string {
    return `${String(face)},${String(i)},${String(k)}`;
  }

  /** Explored ground at face-local (x, z) on `face`. */
  noteExplored(face: number, x: number, z: number): void {
    const s = this.spacing;
    this.explored.add(this.key(face, Math.floor(x / s), Math.floor(z / s)));
  }

  hasExplored(face: number, i: number, k: number): boolean {
    return this.explored.has(this.key(face, i, k));
  }

  /**
   * The shrines of `cells` on `face`: `site(x, z)` gives a cell's (asked at its centre; `mapShrines`
   * with no cells round it), a cell not sited before only while `overBudget()` is false; `pending`
   * counts those left. A shrine two cells give (the spawn's) is listed once.
   */
  gather(
    face: number,
    cells: readonly (readonly [number, number])[],
    site: (x: number, z: number) => ArrayLike<number>,
    overBudget: () => boolean,
  ): { shrines: MapShrine[]; pending: number } {
    const s = this.spacing;
    const seen = new Set<string>();
    const shrines: MapShrine[] = [];
    let pending = 0;
    for (const [i, k] of cells) {
      const key = this.key(face, i, k);
      if (!this.sited.has(key)) {
        if (overBudget()) {
          pending++;
          continue;
        }
        this.sited.add(key);
      }
      for (const sh of parseShrines(site((i + 0.5) * s, (k + 0.5) * s))) {
        const at = `${String(sh.x)},${String(sh.z)}`;
        if (seen.has(at)) continue;
        seen.add(at);
        shrines.push(sh);
      }
    }
    return { shrines, pending };
  }
}

/**
 * The shrines the map shows: none with the filter off; with it on, every one when the rule is
 * "all" or the map is revealed, else those whose explored cell (`explored(x, z)`) is known.
 */
export function visibleShrines(
  sites: MapShrine[],
  on: boolean,
  explored: (x: number, z: number) => boolean,
  revealed: boolean,
  rules: ShrineMapRules = SHRINE_MAP,
): MapShrine[] {
  if (!on) return [];
  if (revealed || rules.reveal === 'all') return sites;
  return sites.filter((s) => explored(s.x, s.z));
}

/** A layer of the map the filters show or hide: the shrines, each auto marker kind, the player's own. */
export interface FilterLayer {
  key: string;
  label: string;
}

/** The player's own markers' layer key (a marker with no auto kind). */
export const OWN_MARKERS = 'own';

/** The map's filter layers: the shrines, then the marker kinds by data, then the player's own. */
export function filterLayers(auto: readonly { kind: string; label: string }[]): FilterLayer[] {
  return [
    { key: 'shrine', label: 'Shrines' },
    ...auto.map((a) => ({ key: a.kind, label: a.label })),
    { key: OWN_MARKERS, label: 'My markers' },
  ];
}

/** The filter layer a marker belongs to. */
export function markerLayer(m: { auto: string }): string {
  return m.auto === '' ? OWN_MARKERS : m.auto;
}

/** The filters' key in this browser's storage (a per-viewer convenience). */
export const FILTERS_KEY = 'dwell.map.filters';

function readFilters(storage: Pick<Storage, 'getItem'> | null): Record<string, boolean> {
  try {
    const v: unknown = JSON.parse(storage?.getItem(FILTERS_KEY) ?? '{}');
    if (typeof v !== 'object' || v === null || Array.isArray(v)) return {};
    const out: Record<string, boolean> = {};
    for (const [k, on] of Object.entries(v)) if (typeof on === 'boolean') out[k] = on;
    return out;
  } catch {
    return {};
  }
}

/** The browser's storage, or null where it is missing or blocked. */
export function browserStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/**
 * The map's filters (owner, 2026-11-12: the map can "toggle them off (filters)"): each layer shown
 * unless turned off, remembered in `storage` (unknown layers' choices kept; storage may fail).
 */
export class MapFilters {
  private readonly hidden: Record<string, boolean>;
  constructor(private readonly storage: Pick<Storage, 'getItem' | 'setItem'> | null) {
    this.hidden = Object.fromEntries(
      Object.entries(readFilters(storage)).map(([k, on]) => [k, !on]),
    );
  }
  shows(key: string): boolean {
    return this.hidden[key] !== true;
  }
  set(key: string, on: boolean): void {
    this.hidden[key] = !on;
    try {
      const saved = readFilters(this.storage);
      saved[key] = on;
      this.storage?.setItem(FILTERS_KEY, JSON.stringify(saved));
    } catch {
      // Private windows and blocked storage: the choice lasts the page.
    }
  }
  toggle(key: string): boolean {
    this.set(key, !this.shows(key));
    return this.shows(key);
  }
}
