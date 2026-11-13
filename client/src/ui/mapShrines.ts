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

/** How many grid cells round the view's centre the map asks for to cover a view `spanM` across. */
export function cellsFor(spanM: number, spacing: number = SHRINE_SPACING): number {
  return Math.min(4, Math.max(1, Math.ceil(spanM / 2 / spacing)));
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
