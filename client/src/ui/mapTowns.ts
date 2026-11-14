// The town sites on the maps (docs/BLUEPRINTS.md §6; owner, 2026-11-14: "Yes let's get town
// markers"): a derived layer like the shrines (mapShrines.ts). The client crate gives the sites round
// the view (`mapTowns`, sited from the seed as the server sites them, each with its state); this
// shows those in explored ground (or all: `reveal` "all", or the `mapRevealed` ability) as the
// markers' vector `town` icon, a founded town still being built in its own colour, behind the map's
// filter "Towns". Its looks and reveal rule are data (shared/blueprints/blueprints.json `towns.map`).

import data from '../../../shared/blueprints/blueprints.json';
import type { ShrineMapRules } from './mapShrines';

export interface TownMapRules extends ShrineMapRules {
  /** A founded town whose blueprints still stand (being built). */
  buildingColor: string;
}

/** The maps' rules for town sites (blueprints.json `towns.map`; `gen.mjs` validates them). */
export const TOWN_MAP: TownMapRules = data.towns.map as TownMapRules;

/** The town grid's spacing (m): the map asks for the cells round its view. */
export const TOWN_SPACING: number = data.towns.spacingM;

/** The filter layer of the town sites. */
export const TOWN_LAYER = 'town';

/** A town site on the map: its centre (face-local m) and whether it is being built. */
export interface MapTown {
  x: number;
  z: number;
  building: boolean;
}

/** `mapTowns`' flat [x, z, state]* as sites. */
export function parseTowns(flat: ArrayLike<number>): MapTown[] {
  const out: MapTown[] = [];
  for (let n = 0; n + 2 < flat.length; n += 3) {
    out.push({ x: flat[n] ?? 0, z: flat[n + 1] ?? 0, building: (flat[n + 2] ?? 0) > 0 });
  }
  return out;
}

/** A site's icon colour: building or not. */
export function townColor(t: { building: boolean }, rules: TownMapRules = TOWN_MAP): string {
  return t.building ? rules.buildingColor : rules.color;
}
