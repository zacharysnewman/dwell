// What a run of the Release workflow builds (RELEASES.md §3), decided in one tested place:
//   release.ts EVENT REF RUN SHA [RELEASES.json [REQUESTED [PREVIOUS_PACKAGE_VERSION [CHANNEL]]]]
//     (EVENT: push | tag | dispatch; CHANNEL: the manual run's stable | dev, default stable),
//     reading ./package.json and the releases (`gh release list --json tagName`), prints
//     `key=value` lines for $GITHUB_OUTPUT (the plan, and the release's `title` and `notes`);
//   release.ts publish CHANNEL EVENT SHA TAG_COMMIT RELEASE_EXISTS
//     (TAG_COMMIT: the remote tag's commit or `-`; RELEASE_EXISTS: true | false), checked again just
//     before publishing, prints `action=create|skip` (`publishAction`);
//   release.ts site REF DEFAULT_BRANCH
//     prints `site=call|dispatch` (`siteDeploy`): how the run deploys the site.
// Runs under Node's type stripping.
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { compareVersionText, isStable, parseVersion, sameLine } from '../src/version/semver.ts';

export type ReleaseEvent = 'push' | 'tag' | 'dispatch';

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
 * - `dispatch` (the manual workflow): a stable release of the next version, or of `requested` (a
 *   minor or major bump that package.json does not carry yet) — or, with `channel: 'dev'`, a dev
 *   build of whatever branch it runs on, numbered as a push's (`<next>-dev.<run>`), never stable.
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
  /**
   * `package.json`'s version before this push (its first parent's): a push releases
   * `package.json`'s version only if it raised it. Absent: not known (counts as raised).
   */
  previousPackageVersion?: string;
  /** The manual run's channel (absent: stable): `dev` builds a dev pre-release of any branch. */
  channel?: 'stable' | 'dev';
  /**
   * `shared/releases/changes.json`, parsed (RELEASES.md §3): a stable release needs its version's
   * entry. Absent: not checked.
   */
  changes?: unknown;
}): ReleasePlan {
  const { event, ref, run, sha, packageVersion, latestStable: latest } = input;
  const devByHand = input.channel === 'dev';
  if (devByHand && event !== 'dispatch') {
    throw new Error(`the dev channel is for a run by hand, not a ${event}`);
  }
  if (devByHand && (input.requested ?? '') !== '') {
    throw new Error('a dev build is numbered by its run: leave the version empty');
  }
  const pkg = parseVersion(packageVersion);
  if (!pkg || !isStable(pkg) || pkg.build !== null) {
    throw new Error(
      `package.json's version must be a release version like 0.1.0, not ${packageVersion}`,
    );
  }
  const next = nextVersion(packageVersion, latest);
  // Only the push that raised package.json releases it: a later push still above the newest
  // release (its run started before the release was published) is a dev build.
  const raisedHere =
    input.previousPackageVersion === undefined ||
    compareVersionText(packageVersion, input.previousPackageVersion) > 0;
  const raised =
    (latest === null || compareVersionText(packageVersion, latest) > 0) &&
    (event !== 'push' || raisedHere);
  if (!devByHand && (event === 'tag' || event === 'dispatch' || raised)) {
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
    if (input.changes !== undefined && !hasChanges(input.changes, version)) {
      throw new Error(
        `${version} has no entry in shared/releases/changes.json: say what changed before releasing it.`,
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

/**
 * Whether `changes` (`shared/releases/changes.json`, parsed) lists what changed in `version`: an
 * entry with at least one line. Its full checks are the client's (`src/version/changes.ts`).
 */
export function hasChanges(changes: unknown, version: string): boolean {
  const versions = (changes as { versions?: unknown } | null)?.versions;
  return (
    Array.isArray(versions) &&
    versions.some((e: unknown) => {
      const entry = e as { version?: unknown; changes?: unknown } | null;
      return (
        entry?.version === version &&
        Array.isArray(entry.changes) &&
        entry.changes.some((c: unknown) => typeof c === 'string' && c.trim() !== '')
      );
    })
  );
}

/**
 * The GitHub Release's title and notes. A stable release's notes are generated by GitHub (empty
 * here). A dev build run by hand names its branch, so it is clearly not one of main's.
 */
export function releaseNotes(
  plan: ReleasePlan,
  event: ReleaseEvent,
  ref: string,
  sha: string,
): { title: string; notes: string } {
  if (plan.channel === 'stable') return { title: `Dwell ${plan.version}`, notes: '' };
  if (event !== 'dispatch') {
    return { title: `Dwell ${plan.version} (dev)`, notes: `Dev build of ${sha}.` };
  }
  const branch = ref.replace(/^refs\/(heads|tags)\//, '');
  return {
    title: `Dwell ${plan.version} (dev, ${branch})`,
    notes: `Dev build of ${sha} on ${branch} (run by hand).`,
  };
}

/**
 * What the publish step does, decided again just before it publishes (another run may have
 * published the version since this one planned): `create` the release, or `skip` it.
 * - Already released from this commit (the tag at `sha` and its release exist): skip, it is done.
 * - The tag is at another commit: for a push, that commit is the release and this push is not
 *   (skip); a pushed tag or a run by hand named a version that is taken (an error).
 * - Otherwise create it (a pushed tag without its release yet, or a new tag at this commit).
 */
export function publishAction(input: {
  channel: 'stable' | 'dev';
  event: ReleaseEvent;
  sha: string;
  /** The commit the remote tag points at, or null when there is no such tag. */
  tagCommit: string | null;
  releaseExists: boolean;
}): 'create' | 'skip' {
  const { channel, event, sha, tagCommit, releaseExists } = input;
  if (tagCommit !== null && tagCommit !== sha) {
    if (channel === 'dev' || event === 'push') return 'skip';
    throw new Error(`the tag is already at another commit (${tagCommit.slice(0, 7)})`);
  }
  return releaseExists ? 'skip' : 'create';
}

/**
 * How a release run updates the site (RELEASES.md §4): `call` the Pages workflow in this run, or
 * `dispatch` it on the default branch. The github-pages environment deploys only from the default
 * branch (its deployment branch policy), so a run on a tag — or by hand on another branch — would
 * have its deploy rejected (v0.13.0, 2026-10-16); it hands the deploy to a Pages run on the default
 * branch instead, which assembles the site from every release, its own included.
 */
export function siteDeploy(ref: string, defaultBranch: string): 'call' | 'dispatch' {
  return ref === `refs/heads/${defaultBranch}` ? 'call' : 'dispatch';
}

function publishMain(args: string[]): number {
  const [channel, event, sha = '', tagCommit = '-', releaseExists = 'false'] = args;
  if (
    (channel !== 'stable' && channel !== 'dev') ||
    (event !== 'push' && event !== 'tag' && event !== 'dispatch')
  ) {
    console.error(
      'usage: release.ts publish stable|dev push|tag|dispatch SHA TAG_COMMIT|- true|false',
    );
    return 2;
  }
  try {
    const action = publishAction({
      channel,
      event,
      sha,
      tagCommit: tagCommit === '-' || tagCommit === '' ? null : tagCommit,
      releaseExists: releaseExists === 'true',
    });
    console.log(`action=${action}`);
    return 0;
  } catch (err) {
    console.error(`::error::${err instanceof Error ? err.message : String(err)}`);
    return 1;
  }
}

function main(args: string[]): number {
  if (args[0] === 'publish') return publishMain(args.slice(1));
  if (args[0] === 'site') {
    const [ref, defaultBranch] = args.slice(1);
    if (!ref || !defaultBranch) {
      console.error('usage: release.ts site REF DEFAULT_BRANCH');
      return 2;
    }
    console.log(`site=${siteDeploy(ref, defaultBranch)}`);
    return 0;
  }
  const [event, ref = '', run = '0', sha = '', releasesFile, requested, previous, channel] = args;
  if (
    (event !== 'push' && event !== 'tag' && event !== 'dispatch') ||
    (channel && channel !== 'stable' && channel !== 'dev')
  ) {
    console.error(
      'usage: release.ts push|tag|dispatch REF RUN SHA [RELEASES.json [REQUESTED [PREVIOUS [stable|dev]]]]',
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
      ...(previous ? { previousPackageVersion: previous } : {}),
      ...(channel ? { channel: channel as 'stable' | 'dev' } : {}),
      changes: JSON.parse(readFileSync('../shared/releases/changes.json', 'utf8')) as unknown,
    });
    const out = { ...plan, ...releaseNotes(plan, event, ref, sha) };
    for (const [key, value] of Object.entries(out)) {
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
