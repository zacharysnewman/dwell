import { isStable, parseVersion, withoutBuild } from './version/semver';

export interface BuildInfo {
  /** The app version (RELEASES.md §3), e.g. "0.1.0" or "0.2.0-dev.42+ab12cd3". */
  version: string;
  sha: string;
  time: string;
}

export const buildInfo: BuildInfo = {
  version: __APP_VERSION__,
  sha: __BUILD_SHA__,
  time: __BUILD_TIME__,
};

/** The version's channel: a release is stable, anything with a pre-release part is dev. */
export function channelOf(version: string): 'stable' | 'dev' {
  const v = parseVersion(version);
  return v && isStable(v) ? 'stable' : 'dev';
}

/** The app version without its build metadata, as a world records it ("0.2.0-dev.42"). */
export function recordedVersion(version: string = buildInfo.version): string {
  const v = parseVersion(version);
  return v ? withoutBuild(v) : version;
}

/** One-line label for the build-info overlay, e.g. "dwell 0.1.0 · 1a2b3c4 · 2026-09-25". */
export function formatBuildInfo(info: BuildInfo): string {
  const sha = /^[0-9a-f]{7,40}$/i.test(info.sha) ? info.sha.slice(0, 7) : info.sha;
  const date = info.time.slice(0, 10);
  return `dwell ${recordedVersion(info.version)} · ${sha} · ${date}`;
}
