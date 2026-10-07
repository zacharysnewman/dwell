// What a run of the Release workflow builds (RELEASES.md §3), decided in one tested place:
//   release.ts EVENT REF RUN SHA [RELEASES.json [REQUESTED]]   (EVENT: push | tag | dispatch | preview),
// reading ./package.json and the releases (`gh release list --json tagName`)
// prints `key=value` lines for $GITHUB_OUTPUT. Runs under Node's type stripping.
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { compareVersionText, isStable, parseVersion, sameLine } from '../src/version/semver.ts';

export type ReleaseEvent = 'push' | 'tag' | 'dispatch' | 'preview';

export interface ReleasePlan {
  /** The release's version, as its tag and `versions.json` name it: no build metadata. */
  version: string;
  /** The version the build embeds: a dev build adds `+<sha>`. */
  buildVersion: string;
  tag: string;
  channel: 'stable' | 'dev';
  /** The release archive attached to the GitHub Release. */
  archive: string;
}

/** The version after a release: its next patch. */
function nextPatch(version: string): string {
  const v = parseVersion(version);
  if (!v) throw new Error(`not a version: ${version}`);
  return `${String(v.major)}.${String(v.minor)}.${String(v.patch + 1)}`;
}

/**
 * The highest stable version among release tags (`v<version>`), or null; with `line`, the highest
 * on that version's compatibility line.
 */
export function latestStable(tags: string[], line?: string): string | null {
  const versions = tags
    .filter((t) => t.startsWith('v'))
    .map((t) => t.slice(1))
    .filter((v) => {
      const parsed = parseVersion(v);
      return (
        parsed !== null &&
        isStable(parsed) &&
        parsed.build === null &&
        (line === undefined || sameLine(v, line))
      );
    })
    .sort(compareVersionText);
  return versions.at(-1) ?? null;
}

/**
 * The version the next release will have: the next patch after the newest release, or
 * `package.json`'s version if that is higher (a breaking change raised it, RELEASES.md §3) or if
 * nothing is released yet (the baseline). Nobody bumps `package.json` after a release.
 */
export function nextVersion(floor: string, latest: string | null): string {
  if (latest === null) return floor;
  const after = nextPatch(latest);
  return compareVersionText(after, floor) >= 0 ? after : floor;
}

/**
 * - `push` (to main): a **stable release of `package.json`'s version if that is above the newest
 *   release** (raising it is the deliberate act that starts a release: the baseline, or a new
 *   line); otherwise a dev build, a pre-release of the next version (`nextVersion`):
 *   `<next>-dev.<run>`, with the commit as build metadata.
 * - `preview` (a pull request, `refs/pull/<n>/merge`): a pre-release of the next version,
 *   `<next>-pr.<n>.<run>`, with the PR's head commit as build metadata. It is never a release,
 *   whatever `package.json` says, and the launcher never picks it by itself (RELEASES.md §3).
 * - `dispatch` (the manual workflow): a stable release of the next version, or of `requested` (a
 *   minor or major bump that package.json does not carry yet).
 * - `tag` (pushing `v<version>`): a stable release of the version the tag names.
 * A named version must be a release version, newer than the newest release and not below
 * `package.json`'s (the floor) — or a **backport**: a patch of an older compatibility line, named
 * by a tag or the manual run on that line's maintenance branch (`release/<line>`, whose
 * `package.json` is on the line), newer than the line's newest release (RELEASES.md §3). The
 * manual run on a maintenance branch releases the next patch of its line, never another line's.
 */
export function releasePlan(input: {
  event: ReleaseEvent;
  ref: string;
  run: number;
  sha: string;
  packageVersion: string;
  /** The newest stable release so far, or null. */
  latestStable: string | null;
  /** The newest stable release on `package.json`'s compatibility line, or null (for backports). */
  lineLatest?: string | null;
  /** A version asked for by the manual workflow's input (empty or absent: the next one). */
  requested?: string;
}): ReleasePlan {
  const { event, ref, run, sha, packageVersion, latestStable: latest } = input;
  const pkg = parseVersion(packageVersion);
  if (!pkg || !isStable(pkg) || pkg.build !== null) {
    throw new Error(
      `package.json's version must be a release version like 0.1.0, not ${packageVersion}`,
    );
  }
  const next = nextVersion(packageVersion, latest);
  if (event === 'preview') {
    const pr = /^refs\/pull\/([1-9]\d*)\//.exec(ref)?.[1];
    if (!pr) throw new Error(`a preview is built for a pull request ref, not ${ref}`);
    if (!Number.isInteger(run) || run < 1) throw new Error(`bad run number ${String(run)}`);
    if (!/^[0-9a-f]{7,40}$/.test(sha)) throw new Error(`bad commit ${sha}`);
    const version = `${next}-pr.${pr}.${String(run)}`;
    return {
      version,
      buildVersion: `${version}+${sha.slice(0, 7)}`,
      tag: `v${version}`,
      channel: 'dev',
      archive: `dwell-${version}.tar.gz`,
    };
  }
  const raised = latest === null || compareVersionText(packageVersion, latest) > 0;
  if (event === 'tag' || event === 'dispatch' || raised) {
    const named =
      event === 'tag'
        ? ref.replace(/^refs\/tags\/v?/, '')
        : event === 'dispatch'
          ? (input.requested ?? '')
          : packageVersion;
    // A run by hand on a maintenance branch releases the next patch of its own line.
    const maintenance = event === 'dispatch' && ref.startsWith('refs/heads/release/');
    const version =
      named !== ''
        ? named
        : maintenance
          ? nextVersion(packageVersion, input.lineLatest ?? null)
          : next;
    if (maintenance && !sameLine(version, packageVersion)) {
      throw new Error(`${version} is not on this branch's line (package.json ${packageVersion}).`);
    }
    const parsed = parseVersion(version);
    if (!parsed || !isStable(parsed) || parsed.build !== null) {
      throw new Error(`${version} is not a release version like 0.1.0`);
    }
    // A backport: a version named for package.json's line while a newer line is out.
    const backport =
      (named !== '' || maintenance) &&
      latest !== null &&
      compareVersionText(version, latest) < 0 &&
      sameLine(version, packageVersion) &&
      !sameLine(version, latest);
    const newest = backport ? (input.lineLatest ?? null) : latest;
    if (newest !== null && compareVersionText(version, newest) <= 0) {
      throw new Error(
        backport
          ? `${version} is already released (the newest release on its line is ${newest}).`
          : `${version} is already released (the newest release is ${newest}).`,
      );
    }
    if (compareVersionText(version, packageVersion) < 0) {
      throw new Error(
        `${version} is below package.json's version ${packageVersion}: lower package.json first if that is intended.`,
      );
    }
    return {
      version,
      buildVersion: version,
      tag: `v${version}`,
      channel: 'stable',
      archive: `dwell-${version}.tar.gz`,
    };
  }
  if (!Number.isInteger(run) || run < 1) throw new Error(`bad run number ${String(run)}`);
  if (!/^[0-9a-f]{7,40}$/.test(sha)) throw new Error(`bad commit ${sha}`);
  const version = `${next}-dev.${String(run)}`;
  return {
    version,
    buildVersion: `${version}+${sha.slice(0, 7)}`,
    tag: `v${version}`,
    channel: 'dev',
    archive: `dwell-${version}.tar.gz`,
  };
}

function main(args: string[]): number {
  const [event, ref = '', run = '0', sha = '', releasesFile, requested] = args;
  if (event !== 'push' && event !== 'tag' && event !== 'dispatch' && event !== 'preview') {
    console.error(
      'usage: release.ts push|tag|dispatch|preview REF RUN SHA [RELEASES.json [REQUESTED]]',
    );
    return 2;
  }
  const { version: packageVersion } = JSON.parse(readFileSync('package.json', 'utf8')) as {
    version: string;
  };
  try {
    const releases = releasesFile
      ? (JSON.parse(readFileSync(releasesFile, 'utf8')) as { tagName: string }[])
      : [];
    const tags = releases.map((r) => r.tagName);
    const plan = releasePlan({
      event,
      ref,
      run: Number(run),
      sha,
      packageVersion,
      latestStable: latestStable(tags),
      lineLatest: latestStable(tags, packageVersion),
      ...(requested ? { requested } : {}),
    });
    for (const [key, value] of Object.entries(plan) as [string, string][]) {
      console.log(`${key}=${value}`);
    }
    return 0;
  } catch (err) {
    console.error(`::error::${err instanceof Error ? err.message : String(err)}`);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = main(process.argv.slice(2));
}
