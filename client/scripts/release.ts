// What a run of the Release workflow builds (RELEASES.md §3), decided in one tested place:
//   release.ts EVENT REF RUN SHA     (EVENT: push | tag | dispatch), reading ./package.json
// prints `key=value` lines for $GITHUB_OUTPUT. Runs under Node's type stripping.
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { isStable, parseVersion } from '../src/version/semver.ts';

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

/**
 * - `tag` (pushing `v<version>`): a stable release; the tag must name `package.json`'s version.
 * - `dispatch` (the manual workflow): a stable release of `package.json`'s version.
 * - `push` (to main): a dev build, a pre-release of the version the next release will have:
 *   `<package.json version>-dev.<run>`, with the commit as build metadata.
 */
export function releasePlan(input: {
  event: ReleaseEvent;
  ref: string;
  run: number;
  sha: string;
  packageVersion: string;
}): ReleasePlan {
  const { event, ref, run, sha, packageVersion } = input;
  const pkg = parseVersion(packageVersion);
  if (!pkg || !isStable(pkg) || pkg.build !== null) {
    throw new Error(
      `package.json's version must be a release version like 0.1.0, not ${packageVersion}`,
    );
  }
  if (event === 'tag') {
    const tag = ref.replace(/^refs\/tags\//, '');
    if (tag !== `v${packageVersion}`) {
      throw new Error(
        `The tag ${tag} does not match package.json's version ${packageVersion}: ` +
          `bump client/package.json first, then tag v<that version>.`,
      );
    }
  }
  if (event === 'tag' || event === 'dispatch') {
    return {
      version: packageVersion,
      buildVersion: packageVersion,
      tag: `v${packageVersion}`,
      channel: 'stable',
      archive: `dwell-${packageVersion}.tar.gz`,
    };
  }
  if (!Number.isInteger(run) || run < 1) throw new Error(`bad run number ${String(run)}`);
  if (!/^[0-9a-f]{7,40}$/.test(sha)) throw new Error(`bad commit ${sha}`);
  const version = `${packageVersion}-dev.${String(run)}`;
  return {
    version,
    buildVersion: `${version}+${sha.slice(0, 7)}`,
    tag: `v${version}`,
    channel: 'dev',
    archive: `dwell-${version}.tar.gz`,
  };
}

function main(args: string[]): number {
  const [event, ref = '', run = '0', sha = ''] = args;
  if (event !== 'push' && event !== 'tag' && event !== 'dispatch') {
    console.error('usage: release.ts push|tag|dispatch REF RUN SHA');
    return 2;
  }
  const { version: packageVersion } = JSON.parse(readFileSync('package.json', 'utf8')) as {
    version: string;
  };
  try {
    const plan = releasePlan({ event, ref, run: Number(run), sha, packageVersion });
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
