// Which build the launcher opens (RELEASES.md §5): the world's, the host's, or the latest.
import {
  canOpenWorld,
  compareVersionText,
  compatibilityLine,
  isVersion,
  sameLine,
} from '../../src/version/semver';
import { CHANGES, changesBetween, type VersionChanges } from '../../src/version/changes';
import type { BuildEntry, Manifest } from './manifest';
import { LAUNCHER_VERSION } from './manifest';

export type Channel = 'stable' | 'dev';

export type Choice =
  | { kind: 'open'; version: string; why: 'world' | 'host' | 'pinned' | 'latest' }
  /** Nothing can be opened for the request: `message` says why; the latest build is still offered. */
  | { kind: 'error'; message: string };

export interface Request {
  /** The page's query string (`?play=…`, `?join=…&v=…`, `?code=…`). */
  search: string;
  /** The app version that last played a local world, if the browser has it. */
  worldVersion(id: string): string | null;
  /** The player's channel setting. */
  channel: Channel;
}

/** Parameters that choose a version, not forwarded: they are the launcher's. */
export const LAUNCHER_PARAMS = ['version', 'versions'];

/** The newest published build of a channel, falling back to the other one. */
export function latestBuild(manifest: Manifest, channel: Channel): BuildEntry | null {
  const wanted = channel === 'dev' ? manifest.latestDev : manifest.latestStable;
  const fallback = channel === 'dev' ? manifest.latestStable : manifest.latestDev;
  const name = wanted ?? fallback;
  return manifest.versions.find((e) => e.version === name) ?? null;
}

/** The newest published build that satisfies `ok`, ignoring builds this launcher cannot serve. */
function newest(manifest: Manifest, ok: (version: string) => boolean): BuildEntry | null {
  return (
    manifest.versions
      .filter((e) => e.minLauncher <= LAUNCHER_VERSION && ok(e.version))
      .sort((a, b) => compareVersionText(b.version, a.version))[0] ?? null
  );
}

export function choose(manifest: Manifest, req: Request): Choice {
  const params = new URLSearchParams(req.search);
  const open = (e: BuildEntry, why: 'world' | 'host' | 'pinned' | 'latest'): Choice => ({
    kind: 'open',
    version: e.version,
    why,
  });

  // A developer pinning a version: ?version=0.1.0
  const pinned = params.get('version');
  if (pinned) {
    const e = manifest.versions.find((b) => b.version === pinned);
    if (e) return open(e, 'pinned');
    return { kind: 'error', message: notOnSite(manifest, pinned) };
  }

  // A world from the menu: the newest build that may open it (same line, not older). Regenerating
  // one is the same build's to do.
  const play = params.get('play') ?? params.get('regenerate');
  const worldVersion = play ? req.worldVersion(play) : null;
  if (worldVersion && isVersion(worldVersion)) {
    const e = newest(manifest, (v) => canOpenWorld(v, worldVersion));
    return e
      ? open(e, 'world')
      : {
          kind: 'error',
          message: `No published build can open this world: it was last saved by Dwell ${worldVersion}, and a build on the ${compatibilityLine(worldVersion) ?? '?'} line, ${worldVersion} or newer, is needed.`,
        };
  }

  // A host's version (an invite, a code that the game looked up): any build on its line can join.
  const host = params.get('v');
  if (host && isVersion(host)) {
    const e = newest(manifest, (v) => sameLine(v, host));
    return e
      ? open(e, 'host')
      : {
          kind: 'error',
          message: `No published build can join this game: the host runs Dwell ${host}, and a build on the ${compatibilityLine(host) ?? '?'} line is needed.`,
        };
  }

  // Everything else — the main menu, an unversioned invite or code, a world the browser doesn't
  // know — opens the latest, which explains itself.
  const latest = latestBuild(manifest, req.channel);
  return latest
    ? open(latest, 'latest')
    : { kind: 'error', message: 'No version of Dwell is published yet.' };
}

/** Where every release stays, including the ones retired from the site (RELEASES.md §4). */
export const RELEASES_URL = 'https://github.com/zacharysnewman/dwell/releases';

/**
 * Why a pinned version is not here. The site keeps the newest build of each compatibility line and
 * retires older patches (RELEASES.md §4): for one of those, name the build that opens its worlds.
 */
export function notOnSite(manifest: Manifest, version: string): string {
  const instead = isVersion(version) ? newest(manifest, (v) => canOpenWorld(v, version)) : null;
  return instead
    ? `Dwell ${version} is not on the site any more: older patches are retired. Dwell ${instead.version}, on the same line, opens its worlds.`
    : `Version ${version} is not published.`;
}

/** The query to forward to the build: everything but the launcher's own parameters. */
export function forwardedSearch(search: string): string {
  const params = new URLSearchParams(search);
  for (const key of LAUNCHER_PARAMS) params.delete(key);
  const query = params.toString();
  return query ? `?${query}` : '';
}

/** The query that opens the version page instead of a build (RELEASES.md §5). */
export const VERSIONS_PARAM = 'versions';

/** A local world as the version page needs it: from the menu's index, `dwell.worlds`. */
export interface WorldRef {
  name: string;
  /** The app version that last saved it; null for a save from before versioned releases. */
  appVersion: string | null;
}

/** The worlds in the index's JSON text; tolerant of damage and of fields it does not know. */
export function parseWorldIndex(text: string | null): WorldRef[] {
  try {
    const raw: unknown = JSON.parse(text ?? '[]');
    if (!Array.isArray(raw)) return [];
    const worlds: WorldRef[] = [];
    for (const entry of raw as unknown[]) {
      if (typeof entry !== 'object' || entry === null) continue;
      const r = entry as { name?: unknown; appVersion?: unknown };
      if (typeof r.name !== 'string') continue;
      const v = typeof r.appVersion === 'string' && isVersion(r.appVersion) ? r.appVersion : null;
      worlds.push({ name: r.name, appVersion: v });
    }
    return worlds;
  } catch {
    return [];
  }
}

export interface VersionRow {
  version: string;
  channel: Channel;
  date: string;
  /** The compatibility line, e.g. `0.2.x`, or the exact build for a dev build. */
  line: string;
  /** The newest stable build: the one the launcher opens by default. */
  recommended: boolean;
  /** False when the build needs a newer launcher than this one. */
  openable: boolean;
  /** Names of the player's worlds this build can open (same line, not older than their last save). */
  worlds: string[];
  /**
   * What changed (shared/releases/changes.json), newest first: this build's, then a stable
   * build's retired patches since the next older build listed (RELEASES.md §4).
   */
  changes: VersionChanges[];
}

/** Every published build for the version page, newest first; dev builds only when `showDev`. */
export function listBuilds(
  manifest: Manifest,
  showDev: boolean,
  worlds: WorldRef[],
  changes: VersionChanges[] = CHANGES,
): VersionRow[] {
  const listed = manifest.versions
    .filter((e) => showDev || e.channel === 'stable')
    .sort((a, b) => compareVersionText(b.version, a.version));
  const stable = listed.filter((e) => e.channel === 'stable').map((e) => e.version);
  const changesOf = (e: BuildEntry): VersionChanges[] => {
    if (e.channel === 'dev') return [];
    const older = stable[stable.indexOf(e.version) + 1] ?? null;
    return changesBetween(older, e.version, changes);
  };
  return listed.map((e) => ({
    version: e.version,
    channel: e.channel,
    date: e.date,
    line: e.channel === 'dev' ? e.version : `${compatibilityLine(e.version) ?? '?'}.x`,
    recommended: e.version === manifest.latestStable,
    openable: e.minLauncher <= LAUNCHER_VERSION,
    worlds: worlds
      .filter((w) => w.appVersion !== null && canOpenWorld(e.version, w.appVersion))
      .map((w) => w.name),
    changes: changesOf(e),
  }));
}

/** Query parameters that route to a particular build or game: a visit with none is "plain". */
const ROUTING_PARAMS = [
  'play',
  'join',
  'code',
  'v',
  'version',
  VERSIONS_PARAM,
  // The menu's new and regenerated worlds, and local worlds by link (src/ui/launch.ts).
  'new',
  'regenerate',
  'local',
  'world',
  'seed',
];

/** Whether the address asks for nothing in particular (the site root, maybe `?debug=1`). */
export function isPlainVisit(search: string): boolean {
  const params = new URLSearchParams(search);
  return !ROUTING_PARAMS.some((key) => params.has(key));
}

/**
 * The first builds that carry out the menu's routes (`?new=…`, `?regenerate=<id>`, src/ui/launch.ts)
 * and publish `tools.js`: the launcher owns the menu from these on (RELEASES.md §5).
 */
export const MENU_ROUTES_SINCE = '0.10.3-0';

export function supportsMenuRoutes(version: string): boolean {
  return compareVersionText(version, MENU_ROUTES_SINCE) >= 0;
}

/** Whether a world can be regenerated: the build that would open it carries `?regenerate=` out. */
export function canRegenerate(manifest: Manifest, worldVersion: string | null): boolean {
  if (!worldVersion || !isVersion(worldVersion)) return false;
  const e = newest(manifest, (v) => canOpenWorld(v, worldVersion));
  return e !== null && supportsMenuRoutes(e.version);
}
