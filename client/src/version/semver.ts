// The app version (RELEASES.md §3, §6): Semantic Versioning 2.0.0, and the compatibility lines that
// lock worlds to builds. Pure functions, shared by the game, the launcher and the site scripts; the
// C++ copy (server/core/src/app_version.cpp) is checked against the same vectors
// (shared/version/vectors.txt).

export interface Version {
  major: number;
  minor: number;
  patch: number;
  /** Pre-release identifiers ("dev", "42" for `-dev.42`); empty for a release. */
  pre: string[];
  /** Build metadata after `+`, ignored by precedence; null if none. */
  build: string | null;
}

const NUMERIC = '(0|[1-9]\\d*)';
const PRE_ID = '(?:0|[1-9]\\d*|\\d*[a-zA-Z-][0-9a-zA-Z-]*)';
const BUILD_ID = '[0-9a-zA-Z-]+';
const SEMVER = new RegExp(
  `^${NUMERIC}\\.${NUMERIC}\\.${NUMERIC}` +
    `(?:-(${PRE_ID}(?:\\.${PRE_ID})*))?(?:\\+(${BUILD_ID}(?:\\.${BUILD_ID})*))?$`,
);

/** Parses a version, strictly (no leading "v", no missing parts); null if it is not valid. */
export function parseVersion(text: string): Version | null {
  const m = SEMVER.exec(text);
  if (!m) return null;
  const [major, minor, patch] = [Number(m[1]), Number(m[2]), Number(m[3])];
  if (![major, minor, patch].every(Number.isSafeInteger)) return null;
  return { major, minor, patch, pre: m[4] ? m[4].split('.') : [], build: m[5] ?? null };
}

export function isVersion(text: string): boolean {
  return parseVersion(text) !== null;
}

/** A release (no pre-release part): the stable channel; dev builds are pre-releases. */
export function isStable(v: Version): boolean {
  return v.pre.length === 0;
}

function comparePre(a: string[], b: string[]): number {
  // A release outranks its pre-releases (SemVer §11.3).
  if (a.length === 0 || b.length === 0) return a.length === 0 ? (b.length === 0 ? 0 : 1) : -1;
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i];
    const y = b[i];
    if (x === undefined) return -1;
    if (y === undefined) return 1;
    const xn = /^\d+$/.test(x);
    const yn = /^\d+$/.test(y);
    if (xn && yn) {
      if (Number(x) !== Number(y)) return Number(x) < Number(y) ? -1 : 1;
    } else if (xn !== yn) {
      return xn ? -1 : 1; // numeric identifiers rank below alphanumeric ones
    } else if (x !== y) {
      return x < y ? -1 : 1;
    }
  }
  return 0;
}

/** SemVer precedence of `a` against `b`: -1, 0 or 1. Build metadata is ignored. */
export function compareVersions(a: Version, b: Version): -1 | 0 | 1 {
  for (const key of ['major', 'minor', 'patch'] as const) {
    if (a[key] !== b[key]) return a[key] < b[key] ? -1 : 1;
  }
  const pre = comparePre(a.pre, b.pre);
  return pre < 0 ? -1 : pre > 0 ? 1 : 0;
}

/** Orders version strings, oldest first; invalid ones sort before every valid one. */
export function compareVersionText(a: string, b: string): number {
  const x = parseVersion(a);
  const y = parseVersion(b);
  if (!x || !y) return x ? 1 : y ? -1 : a < b ? -1 : a > b ? 1 : 0;
  return compareVersions(x, y);
}

/** The version without its build metadata ("0.2.0-dev.42+ab12cd3" → "0.2.0-dev.42"). */
export function withoutBuild(v: Version): string {
  const pre = v.pre.length > 0 ? `-${v.pre.join('.')}` : '';
  return `${String(v.major)}.${String(v.minor)}.${String(v.patch)}${pre}`;
}

/**
 * The compatibility line of a version (RELEASES.md §6): versions sharing a line have the same world
 * format, generated terrain and protocols. From 1.0.0 it is MAJOR; before, MAJOR.MINOR ("0.1");
 * a pre-release promises nothing, so its line is that exact version. Null for an invalid version.
 */
export function compatibilityLine(text: string): string | null {
  const v = parseVersion(text);
  if (!v) return null;
  if (!isStable(v)) return withoutBuild(v);
  return v.major === 0 ? `0.${String(v.minor)}` : String(v.major);
}

/**
 * Whether a build may open a world last saved by `worldLast`: on the same compatibility line and
 * not older (a compatible release may add data an older build would not understand).
 */
export function canOpenWorld(build: string, worldLast: string): boolean {
  const b = parseVersion(build);
  const w = parseVersion(worldLast);
  if (!b || !w) return false;
  return compatibilityLine(build) === compatibilityLine(worldLast) && compareVersions(b, w) >= 0;
}

/** Whether two versions are on one compatibility line (can talk to each other, RELEASES.md §7). */
export function sameLine(a: string, b: string): boolean {
  const line = compatibilityLine(a);
  return line !== null && line === compatibilityLine(b);
}
