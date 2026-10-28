// What changed in each build (docs/RELEASES.md §3): `shared/releases/changes.json`, a few short
// lines per stable version, newest first. Read by the launcher's version page and the main menu's
// Versions card, and checked here: a bad entry fails with its version named, and the release
// script refuses a version without one.
import data from '../../../shared/releases/changes.json';
import { compareVersionText, isStable, parseVersion, withoutBuild } from './semver';

export interface VersionChanges {
  version: string;
  changes: string[];
}

/** The longest change line (characters): a line, not a paragraph. */
export const MAX_CHANGE_LENGTH = 240;
/** The most lines one version lists. */
export const MAX_CHANGES = 6;

/** The changes in parsed JSON, checked; throws naming the first bad entry. */
export function parseChanges(json: unknown): VersionChanges[] {
  const fail = (m: string): never => {
    throw new Error(`changes.json: ${m}`);
  };
  if (typeof json !== 'object' || json === null) return fail('must be an object');
  const list = (json as { versions?: unknown }).versions;
  if (!Array.isArray(list) || list.length === 0) return fail('versions must be a non-empty list');
  const out: VersionChanges[] = [];
  list.forEach((entry: unknown, i) => {
    const e = (entry ?? {}) as { version?: unknown; changes?: unknown };
    const at = typeof e.version === 'string' ? e.version : `versions[${String(i)}]`;
    const parsed = typeof e.version === 'string' ? parseVersion(e.version) : null;
    if (!parsed || !isStable(parsed) || withoutBuild(parsed) !== e.version) {
      fail(`${at}: version must be a stable version, e.g. 0.38.6`);
    }
    const version = e.version as string;
    const previous = out[out.length - 1];
    if (previous && compareVersionText(previous.version, version) <= 0) {
      fail(`${version}: listed after ${previous.version} (newest first, each once)`);
    }
    const changes = e.changes;
    if (!Array.isArray(changes) || changes.length === 0 || changes.length > MAX_CHANGES) {
      fail(`${version}: changes must be 1–${String(MAX_CHANGES)} lines`);
    }
    for (const c of changes as unknown[]) {
      if (typeof c !== 'string' || c.trim() === '' || c.length > MAX_CHANGE_LENGTH) {
        fail(
          `${version}: each change is a line of text up to ${String(MAX_CHANGE_LENGTH)} characters`,
        );
      }
    }
    out.push({ version, changes: changes as string[] });
  });
  return out;
}

/** The build's changes data. */
export const CHANGES: VersionChanges[] = parseChanges(data);

/** What changed in `version` (a dev build: its base version's, once written), or null. */
export function changesFor(version: string, all: VersionChanges[] = CHANGES): string[] | null {
  const parsed = parseVersion(version);
  if (!parsed) return null;
  const base = `${String(parsed.major)}.${String(parsed.minor)}.${String(parsed.patch)}`;
  return all.find((e) => e.version === base)?.changes ?? null;
}

/**
 * The versions after `after` (none: from the first) up to and including `upTo`, newest first: what
 * a build brings over an older one, retired patches between them included.
 */
export function changesBetween(
  after: string | null,
  upTo: string,
  all: VersionChanges[] = CHANGES,
): VersionChanges[] {
  return all.filter(
    (e) =>
      compareVersionText(e.version, upTo) <= 0 &&
      (after === null || compareVersionText(e.version, after) > 0),
  );
}
