// The site's assembly (RELEASES.md §4), run by the Pages workflow and by `npm run site:fixture`:
//   site.ts plan RELEASES.json [DEV_KEEP [OPEN_PRS.json]]   which releases the site holds, and which to prune
//   site.ts manifest SITE_DIR               writes SITE_DIR/versions.json from SITE_DIR/v/*/build.json
//   site.ts size SITE_DIR                   the site's size against Pages' 1 GB limit
// Runs under Node's type stripping (`node --experimental-strip-types`): erasable TypeScript only,
// and `.ts` in relative imports.
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import type { BuildEntry, Manifest } from '../launcher/src/manifest.ts';
import {
  compareVersionText,
  isPreviewVersion,
  isStable,
  parseVersion,
  withoutBuild,
} from '../src/version/semver.ts';

/** Dev pre-releases kept on the site and as releases; older ones and their tags are pruned. */
export const DEV_KEEP = 10;
/** Preview builds of pull requests kept on the site: the newest of each open PR, at most this many. */
export const PREVIEW_KEEP = 5;
/** The most a Pages site may hold. */
export const PAGES_LIMIT_BYTES = 1_000_000_000;

export interface ReleaseInfo {
  tagName: string;
}

export interface SitePlan {
  /** Tags whose builds go on the site: every stable release and the newest dev builds. */
  keep: string[];
  /** Dev tags beyond `DEV_KEEP`, and previews that are superseded, over the cap or of a closed PR:
   * their releases and tags are deleted. */
  prune: string[];
  /** Tags that are not `v<version>`, left alone. */
  ignored: string[];
}

/** The version a release tag names (`v0.1.0` → `0.1.0`), or null: no build metadata, no `v` less. */
export function tagVersion(tag: string): string | null {
  if (!tag.startsWith('v')) return null;
  const version = tag.slice(1);
  const parsed = parseVersion(version);
  return parsed && parsed.build === null ? version : null;
}

/** The pull request and run a preview tag names (`v0.2.1-pr.63.9` → 63, 9), or null. */
export function previewOf(tag: string): { pr: number; run: number } | null {
  const m = /^v\d+\.\d+\.\d+-pr\.([1-9]\d*)\.([1-9]\d*)$/.exec(tag);
  return m ? { pr: Number(m[1]), run: Number(m[2]) } : null;
}

export interface PlanOptions {
  devKeep?: number;
  /** Numbers of the open pull requests; a preview of any other is pruned. Absent: all are open. */
  openPrs?: number[];
  previewKeep?: number;
}

export function planSite(releases: ReleaseInfo[], options: PlanOptions = {}): SitePlan {
  const { devKeep = DEV_KEEP, openPrs, previewKeep = PREVIEW_KEEP } = options;
  const stable: string[] = [];
  const dev: string[] = [];
  const previews: { tag: string; pr: number; run: number }[] = [];
  const ignored: string[] = [];
  for (const { tagName } of releases) {
    const version = tagVersion(tagName);
    const parsed = version ? parseVersion(version) : null;
    const preview = previewOf(tagName);
    if (!version || !parsed) ignored.push(tagName);
    else if (preview) previews.push({ tag: tagName, ...preview });
    else (isStable(parsed) ? stable : dev).push(tagName);
  }
  const newestFirst = (a: string, b: string) => compareVersionText(b.slice(1), a.slice(1));
  stable.sort(newestFirst);
  dev.sort(newestFirst);
  // A preview stays while its pull request is open and it is the PR's newest build; the newest
  // `previewKeep` of those are kept (previews come and go with their PRs, never with `devKeep`).
  const live = new Map<number, { tag: string; run: number }>();
  for (const p of previews) {
    if (openPrs && !openPrs.includes(p.pr)) continue;
    const seen = live.get(p.pr);
    if (!seen || p.run > seen.run) live.set(p.pr, p);
  }
  const kept = [...live.values()].sort((a, b) => b.run - a.run).slice(0, previewKeep);
  const keptTags = new Set(kept.map((p) => p.tag));
  return {
    keep: [...stable, ...dev.slice(0, devKeep), ...kept.map((p) => p.tag)],
    prune: [
      ...dev.slice(devKeep),
      ...previews.filter((p) => !keptTags.has(p.tag)).map((p) => p.tag),
    ],
    ignored,
  };
}

/** A build's `build.json`, as the Vite build writes it (client/vite.config.ts). */
export interface BuildJson {
  version: string;
  channel: string;
  date: string;
  commit: string;
  protocolVersion: number;
  minLauncher: number;
}

/**
 * The manifest of the builds in `site/v/<version>/`, newest first. Throws for a build whose
 * `build.json` is damaged or does not match its directory: a bad build must not reach the site.
 */
export function buildManifest(builds: Map<string, BuildJson>, generated: string): Manifest {
  const versions: BuildEntry[] = [];
  for (const [dir, b] of builds) {
    // A build.json may carry the commit as build metadata ("0.1.0-dev.1+d9a097d"); the site lists
    // and serves the version without it, as the tag and the directory name it.
    const parsed = parseVersion(b.version);
    if (!parsed) throw new Error(`${dir}: bad version ${b.version}`);
    if (withoutBuild(parsed) !== dir) throw new Error(`${dir}: build.json says ${b.version}`);
    const channel = isStable(parsed) ? 'stable' : 'dev';
    if (b.channel !== channel) throw new Error(`${dir}: channel ${b.channel}, expected ${channel}`);
    if (!Number.isInteger(b.protocolVersion) || !Number.isInteger(b.minLauncher)) {
      throw new Error(`${dir}: build.json lacks protocolVersion or minLauncher`);
    }
    versions.push({
      version: dir,
      channel,
      date: b.date,
      commit: b.commit,
      protocolVersion: b.protocolVersion,
      minLauncher: b.minLauncher,
    });
  }
  versions.sort((a, b) => compareVersionText(b.version, a.version));
  return {
    schema: 1,
    generated,
    versions,
    latestStable: versions.find((v) => v.channel === 'stable')?.version ?? null,
    // A preview is a dev build the launcher never picks by itself.
    latestDev:
      versions.find((v) => v.channel === 'dev' && !isPreviewVersion(v.version))?.version ?? null,
  };
}

export function readBuilds(siteDir: string): Map<string, BuildJson> {
  const root = join(siteDir, 'v');
  const builds = new Map<string, BuildJson>();
  if (!existsSync(root)) return builds;
  for (const dir of readdirSync(root).sort()) {
    const file = join(root, dir, 'build.json');
    if (!existsSync(file)) throw new Error(`${dir}: no build.json`);
    builds.set(dir, JSON.parse(readFileSync(file, 'utf8')) as BuildJson);
  }
  return builds;
}

export function directorySize(dir: string): number {
  let total = 0;
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const info = statSync(path);
    total += info.isDirectory() ? directorySize(path) : info.size;
  }
  return total;
}

const mb = (bytes: number) => `${(bytes / 1e6).toFixed(1)} MB`;

/** A Markdown line for the job summary: the site's size against Pages' limit. */
export function sizeReport(bytes: number, builds: number): string {
  const percent = ((bytes / PAGES_LIMIT_BYTES) * 100).toFixed(1);
  const warning = bytes > PAGES_LIMIT_BYTES * 0.8 ? ' ⚠️ over 80% of the limit: prune builds' : '';
  const each = builds > 0 ? `, about ${mb(bytes / builds)} per build` : '';
  return `Site: ${mb(bytes)} of ${mb(PAGES_LIMIT_BYTES)} (${percent}%), ${String(builds)} builds${each}${warning}`;
}

function main(args: string[]): number {
  const [command, arg, extra, openFile] = args;
  if (command === 'plan' && arg) {
    const releases = JSON.parse(readFileSync(arg, 'utf8')) as ReleaseInfo[];
    const openPrs = openFile
      ? (JSON.parse(readFileSync(openFile, 'utf8')) as { number: number }[]).map((p) => p.number)
      : undefined;
    const plan = planSite(releases, {
      devKeep: extra ? Number(extra) : DEV_KEEP,
      ...(openPrs ? { openPrs } : {}),
    });
    console.log(JSON.stringify(plan, null, 2));
    return 0;
  }
  if (command === 'manifest' && arg) {
    const manifest = buildManifest(readBuilds(arg), new Date().toISOString());
    writeFileSync(join(arg, 'versions.json'), `${JSON.stringify(manifest, null, 2)}\n`);
    console.log(
      `versions.json: ${String(manifest.versions.length)} builds, ` +
        `stable ${manifest.latestStable ?? '-'}, dev ${manifest.latestDev ?? '-'}`,
    );
    return 0;
  }
  if (command === 'size' && arg) {
    const bytes = directorySize(arg);
    const builds = readBuilds(arg).size;
    console.log(sizeReport(bytes, builds));
    if (bytes > PAGES_LIMIT_BYTES) {
      console.error('The site is over the 1 GB Pages limit: prune builds.');
      return 1;
    }
    return 0;
  }
  console.error(
    'usage: site.ts plan RELEASES.json [DEV_KEEP [OPEN_PRS.json]] | manifest SITE_DIR | size SITE_DIR',
  );
  return 2;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = main(process.argv.slice(2));
}
