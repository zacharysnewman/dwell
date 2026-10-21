// The site's assembly (RELEASES.md §4), run by the Pages workflow and by `npm run site:fixture`:
//   site.ts plan RELEASES.json [POLICY]     which releases the site holds, and which to prune
//   site.ts manifest SITE_DIR               writes SITE_DIR/versions.json from SITE_DIR/v/*/build.json
//   site.ts media SITE_DIR                  moves the builds' music to SITE_DIR/media/, once
//   site.ts size SITE_DIR [SUMMARY]         the site's size against the budget and Pages' 1 GB limit
// POLICY is the retention policy, shared/version/site-retention.json by default.
// Runs under Node's type stripping (`node --experimental-strip-types`): erasable TypeScript only,
// and `.ts` in relative imports.
import {
  appendFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import type { BuildEntry, Manifest } from '../launcher/src/manifest.ts';
import {
  compareVersionText,
  compatibilityLine,
  isStable,
  parseVersion,
  withoutBuild,
} from '../src/version/semver.ts';

/** The most a Pages site may hold. */
export const PAGES_LIMIT_BYTES = 1_000_000_000;

/** The retention policy's file (RELEASES.md §4), relative to this script. */
export const RETENTION_FILE = join(
  dirname(fileURLToPath(import.meta.url)),
  '../../shared/version/site-retention.json',
);

/**
 * Which releases the site holds (RELEASES.md §4, `shared/version/site-retention.json`). A stable
 * release is kept if any rule keeps it; the rest are retired from the site, their releases (and
 * tarballs) left on GitHub. Dev pre-releases follow `devKeep` alone.
 */
export interface RetentionPolicy {
  /** Keep the newest release of every compatibility line, so every line's worlds still open. */
  newestPerLine: boolean;
  /** Keep this many of the newest stable releases. */
  newestOverall: number;
  /** Always kept: a compatibility line ("0.10", "1": all of its releases) or an exact release. */
  pinned: string[];
  /** Dev pre-releases kept on the site and as releases; older ones and their tags are deleted. */
  devKeep: number;
  /** Above this the size check warns loudly (the hard stop is Pages' 1 GB). */
  sizeBudgetMB: number;
}

const POLICY_KEYS = ['newestPerLine', 'newestOverall', 'pinned', 'devKeep', 'sizeBudgetMB'];
/** A compatibility line as a pin names it: `0.MINOR` before 1.0, `MAJOR` after. */
const LINE_PIN = /^(0\.(0|[1-9]\d*)|[1-9]\d*)$/;

/** Whether `pin` names a compatibility line or an exact stable release; null if neither. */
export function pinKind(pin: string): 'line' | 'version' | null {
  if (LINE_PIN.test(pin)) return 'line';
  const v = parseVersion(pin);
  return v && isStable(v) && v.build === null ? 'version' : null;
}

/** The policy in parsed JSON; throws, naming the field, for anything it does not understand. */
export function parseRetention(json: unknown, source = 'retention policy'): RetentionPolicy {
  const fail = (message: string): never => {
    throw new Error(`${source}: ${message}`);
  };
  if (typeof json !== 'object' || json === null || Array.isArray(json)) fail('not an object');
  const p = json as Record<string, unknown>;
  for (const key of Object.keys(p)) {
    if (key !== 'comment' && !POLICY_KEYS.includes(key)) fail(`unknown field "${key}"`);
  }
  const count = (key: string): number => {
    const n = p[key];
    if (typeof n !== 'number' || !Number.isInteger(n) || n < 0) {
      return fail(`"${key}" must be a whole number of 0 or more`);
    }
    return n;
  };
  if (typeof p.newestPerLine !== 'boolean') fail('"newestPerLine" must be true or false');
  if (!Array.isArray(p.pinned)) fail('"pinned" must be a list');
  const pinned = p.pinned as unknown[];
  for (const pin of pinned) {
    if (typeof pin !== 'string' || pinKind(pin) === null) {
      fail(
        `pin ${JSON.stringify(pin)} is neither a compatibility line ("0.10", "1") nor a release ("0.17.2")`,
      );
    }
  }
  const sizeBudgetMB = count('sizeBudgetMB');
  if (sizeBudgetMB === 0 || sizeBudgetMB * 1e6 > PAGES_LIMIT_BYTES) {
    fail(
      `"sizeBudgetMB" must be above 0 and at most Pages' limit (${String(PAGES_LIMIT_BYTES / 1e6)})`,
    );
  }
  return {
    newestPerLine: p.newestPerLine as boolean,
    newestOverall: count('newestOverall'),
    pinned: pinned as string[],
    devKeep: count('devKeep'),
    sizeBudgetMB,
  };
}

export function loadRetention(file: string = RETENTION_FILE): RetentionPolicy {
  return parseRetention(JSON.parse(readFileSync(file, 'utf8')), file);
}

export interface ReleaseInfo {
  tagName: string;
}

export interface SitePlan {
  /** Tags whose builds go on the site, newest first: the stable releases the policy keeps, then the newest dev builds. */
  keep: string[];
  /** Stable tags the policy leaves off the site; their releases stay on GitHub. */
  retired: string[];
  /** Dev tags beyond `devKeep`: their releases and tags are deleted. */
  prune: string[];
  /** Pins that matched no release (a typo, or a release not made yet). */
  unmatchedPins: string[];
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

export function planSite(releases: ReleaseInfo[], policy: RetentionPolicy): SitePlan {
  const stable: string[] = [];
  const dev: string[] = [];
  const ignored: string[] = [];
  for (const { tagName } of releases) {
    const version = tagVersion(tagName);
    const parsed = version ? parseVersion(version) : null;
    if (!version || !parsed) ignored.push(tagName);
    else (isStable(parsed) ? stable : dev).push(tagName);
  }
  const newestFirst = (a: string, b: string) => compareVersionText(b.slice(1), a.slice(1));
  stable.sort(newestFirst);
  dev.sort(newestFirst);

  const kept = new Set<string>(stable.slice(0, policy.newestOverall));
  const lines = new Set<string>();
  const matched = new Set<string>();
  for (const tag of stable) {
    const version = tag.slice(1);
    const line = compatibilityLine(version) ?? version;
    if (policy.newestPerLine && !lines.has(line)) kept.add(tag); // newest first: the first is the newest
    lines.add(line);
    for (const pin of policy.pinned) {
      if (pin === version || pin === line) {
        kept.add(tag);
        matched.add(pin);
      }
    }
  }
  return {
    keep: [...stable.filter((t) => kept.has(t)), ...dev.slice(0, policy.devKeep)],
    retired: stable.filter((t) => !kept.has(t)),
    prune: dev.slice(policy.devKeep),
    unmatchedPins: policy.pinned.filter((p) => !matched.has(p)),
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
    latestDev: versions.find((v) => v.channel === 'dev')?.version ?? null,
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

/**
 * Moves every build's `v/<version>/media/*` (the music, scripts/media.ts) to the site's `media/`,
 * where release builds load it: one copy per site, not per build. Names carry their content hash,
 * so a name already there is the same file and the build's copy is dropped; a size that differs
 * means a damaged build, and throws.
 */
export function hoistMedia(siteDir: string): { moved: number; shared: number } {
  const root = join(siteDir, 'v');
  const into = join(siteDir, 'media');
  let moved = 0;
  let shared = 0;
  if (!existsSync(root)) return { moved, shared };
  for (const dir of readdirSync(root).sort()) {
    const media = join(root, dir, 'media');
    if (!existsSync(media)) continue;
    mkdirSync(into, { recursive: true });
    for (const name of readdirSync(media).sort()) {
      const from = join(media, name);
      const to = join(into, name);
      if (existsSync(to)) {
        if (statSync(to).size !== statSync(from).size) {
          throw new Error(`${dir}: media/${name} differs from the one already on the site`);
        }
        rmSync(from);
        shared++;
      } else {
        renameSync(from, to);
        moved++;
      }
    }
    rmdirSync(media);
  }
  return { moved, shared };
}

const mb = (bytes: number) => `${(bytes / 1e6).toFixed(1)} MB`;

/** A Markdown line for the job summary: the site's size against the budget and Pages' limit. */
export function sizeReport(bytes: number, builds: number, budgetMB: number): string {
  const percent = ((bytes / PAGES_LIMIT_BYTES) * 100).toFixed(1);
  const warning =
    bytes > PAGES_LIMIT_BYTES
      ? ' ⛔ over the Pages limit: the deploy is stopped'
      : bytes > budgetMB * 1e6
        ? ` ⚠️ over the ${String(budgetMB)} MB budget: lower newestOverall or the pins in shared/version/site-retention.json`
        : '';
  const each = builds > 0 ? `, about ${mb(bytes / builds)} per build` : '';
  return `Site: ${mb(bytes)} of ${mb(PAGES_LIMIT_BYTES)} (${percent}%; budget ${String(budgetMB)} MB), ${String(builds)} builds${each}${warning}`;
}

/** What the size check does: 'ok', 'warn' over the budget (the deploy goes on), 'fail' over the limit. */
export function sizeVerdict(bytes: number, budgetMB: number): 'ok' | 'warn' | 'fail' {
  if (bytes > PAGES_LIMIT_BYTES) return 'fail';
  return bytes > budgetMB * 1e6 ? 'warn' : 'ok';
}

function main(args: string[]): number {
  const [command, arg, extra] = args;
  if (command === 'plan' && arg) {
    const releases = JSON.parse(readFileSync(arg, 'utf8')) as ReleaseInfo[];
    const plan = planSite(releases, loadRetention(extra));
    console.log(JSON.stringify(plan, null, 2));
    for (const pin of plan.unmatchedPins) {
      console.error(`::warning::site-retention.json pins ${pin}, which matches no release`);
    }
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
  if (command === 'media' && arg) {
    const { moved, shared } = hoistMedia(arg);
    console.log(`media/: ${String(moved)} files moved, ${String(shared)} shared copies dropped`);
    return 0;
  }
  if (command === 'size' && arg) {
    const { sizeBudgetMB } = loadRetention();
    const bytes = directorySize(arg);
    const report = sizeReport(bytes, readBuilds(arg).size, sizeBudgetMB);
    console.log(report);
    if (extra) appendFileSync(extra, `${report}\n`); // the job summary
    const verdict = sizeVerdict(bytes, sizeBudgetMB);
    // GitHub Actions reads these as annotations on the run.
    if (verdict === 'warn') console.log(`::warning title=Site over budget::${report}`);
    if (verdict === 'fail') {
      console.log(`::error title=Site over the Pages limit::${report}`);
      return 1;
    }
    return 0;
  }
  console.error(
    'usage: site.ts plan RELEASES.json [POLICY] | manifest SITE_DIR | media SITE_DIR | size SITE_DIR [SUMMARY]',
  );
  return 2;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = main(process.argv.slice(2));
}
