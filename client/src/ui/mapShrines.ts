// The resurrection shrines on the big map (docs/SHRINES.md; owner, 2026-11-12: shrines "show on the
// map" and the map can "toggle them off (filters)"). The client crate gives the shrines round the
// view (`mapShrines`, sited from the seed as the server sites them); this shows those in explored
// ground (Guild Wars' map reveal: a shrine is known once its cell is explored) or all of them
// (`reveal` "all", or the `mapRevealed` ability), as icons, behind a filter toggle remembered per
// browser. Its looks and reveal rule are data (shared/worldgen/shrines.json `map`).

import data from '../../../shared/worldgen/shrines.json';

export interface ShrineMapRules {
  reveal: 'explored' | 'all';
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

/** The filter's key in this browser's storage (a per-viewer convenience). */
export const SHRINE_FILTER_KEY = 'dwell.map.shrines';

function readFilter(storage: Pick<Storage, 'getItem'> | null): string | null {
  try {
    return storage?.getItem(SHRINE_FILTER_KEY) ?? null;
  } catch {
    return null;
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

/** The shrine layer's toggle, remembered in `storage` (on unless turned off; storage may fail). */
export class ShrineFilter {
  on: boolean;
  constructor(private readonly storage: Pick<Storage, 'getItem' | 'setItem'> | null) {
    this.on = readFilter(storage) !== 'off';
  }
  toggle(): boolean {
    this.on = !this.on;
    try {
      this.storage?.setItem(SHRINE_FILTER_KEY, this.on ? 'on' : 'off');
    } catch {
      // Private windows and blocked storage: the toggle lasts the page.
    }
    return this.on;
  }
}
