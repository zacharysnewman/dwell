// Which build the launcher opens (RELEASES.md §5): the world's, the host's, or the latest.
import {
  canOpenWorld,
  compareVersionText,
  compatibilityLine,
  isVersion,
  sameLine,
} from '../../src/version/semver';
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
export const LAUNCHER_PARAMS = ['version'];

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
    return e
      ? open(e, 'pinned')
      : { kind: 'error', message: `Version ${pinned} is not published.` };
  }

  // A world from the menu: the newest build that may open it (same line, not older).
  const play = params.get('play');
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

/** The query to forward to the build: everything but the launcher's own parameters. */
export function forwardedSearch(search: string): string {
  const params = new URLSearchParams(search);
  for (const key of LAUNCHER_PARAMS) params.delete(key);
  const query = params.toString();
  return query ? `?${query}` : '';
}
