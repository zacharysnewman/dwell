// The version and release notes, assigned at merge (RELEASES.md §3, "Pending release notes"): a PR
// carries what it changed in `shared/releases/pending/<slug>.json` instead of raising package.json
// and editing changes.json, so parallel PRs don't conflict. On main, the Release workflow runs
//   release.ts assign RELEASES.json
// (`npm run release:assign -- RELEASES.json`), which gives each pending note the next patch (in file
// name order), writes its changes.json entry and package.json, deletes the notes, and prints
// `assigned=<the newest version>` (empty when there were none); the workflow commits that to main
// and releases it. Pure functions here, tested in releaseAssign.test.ts; Node's type stripping.
import { existsSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { latestStable } from './release.ts';
import { compareVersionText, isStable, parseVersion } from '../src/version/semver.ts';

/** As src/version/changes.ts's MAX_CHANGES and MAX_CHANGE_LENGTH (a test keeps them equal). */
export const NOTE_MAX_LINES = 6;
export const NOTE_MAX_LENGTH = 240;

export interface PendingNote {
  /** The file's name, e.g. `claude-lava.json`: the notes are assigned in this order. */
  name: string;
  changes: string[];
}

/** A pending note's JSON, checked; throws naming the file. A `breaking` note is refused. */
export function parsePendingNote(name: string, json: unknown): PendingNote {
  const fail = (m: string): never => {
    throw new Error(`shared/releases/pending/${name}: ${m}`);
  };
  if (!/^[a-z0-9][a-z0-9._-]*\.json$/.test(name)) {
    fail('name it <slug>.json (lower case letters, digits, ".", "_", "-")');
  }
  if (typeof json !== 'object' || json === null || Array.isArray(json)) {
    return fail('must be an object like {"changes": ["…"]}');
  }
  const note = json as Record<string, unknown>;
  for (const key of Object.keys(note)) {
    if (key !== 'changes' && key !== 'breaking' && key !== '$comment') {
      fail(`unknown field "${key}" (changes, and optionally breaking)`);
    }
  }
  if (note.breaking !== undefined && note.breaking !== false) {
    fail(
      'a breaking change starts a new compatibility line, which the owner raises by hand: raise ' +
        "client/package.json to the next MINOR and shared/version/line.json, add the version's " +
        'entry to shared/releases/changes.json, and delete this note (CLAUDE.md, docs/RELEASES.md §3)',
    );
  }
  const changes = note.changes;
  if (!Array.isArray(changes) || changes.length === 0 || changes.length > NOTE_MAX_LINES) {
    return fail(`changes must be 1–${String(NOTE_MAX_LINES)} lines`);
  }
  for (const c of changes as unknown[]) {
    if (typeof c !== 'string' || c.trim() === '' || c.length > NOTE_MAX_LENGTH) {
      fail(`each change is a line of text up to ${String(NOTE_MAX_LENGTH)} characters`);
    }
  }
  return { name, changes: changes as string[] };
}

function nextPatch(version: string): string {
  const v = parseVersion(version);
  if (!v) throw new Error(`not a version: ${version}`);
  return `${String(v.major)}.${String(v.minor)}.${String(v.patch + 1)}`;
}

/** `changes.json`'s newest entry's version, or null. */
function newestEntry(changesText: string): string | null {
  const data = JSON.parse(changesText) as { versions?: { version?: unknown }[] };
  const v = data.versions?.[0]?.version;
  return typeof v === 'string' ? v : null;
}

/** `package.json`'s text with its (first) version replaced, the rest kept as written. */
export function withPackageVersion(packageText: string, version: string): string {
  const out = packageText.replace(/("version"\s*:\s*")[^"]*(")/, `$1${version}$2`);
  if (out === packageText && !packageText.includes(`"version": "${version}"`)) {
    throw new Error('package.json has no version field');
  }
  return out;
}

/** `changes.json`'s text with `entry` added as the newest, the rest kept as written. */
export function withChangesEntry(
  changesText: string,
  entry: { version: string; changes: string[] },
): string {
  const at = /"versions"\s*:\s*\[\s*\n/.exec(changesText);
  if (!at) throw new Error('changes.json: no "versions": [ list');
  const lines = entry.changes.map((c) => `        ${JSON.stringify(c)}`).join(',\n');
  const empty = /^\s*\]/.test(changesText.slice(at.index + at[0].length));
  const block = `    {\n      "version": "${entry.version}",\n      "changes": [\n${lines}\n      ]\n    }${empty ? '' : ','}\n`;
  const end = at.index + at[0].length;
  return changesText.slice(0, end) + block + changesText.slice(end);
}

export interface Assignment {
  /** One version per note, oldest first; the last is the release. Empty: nothing pending. */
  assigned: { version: string; note: string; changes: string[] }[];
  packageText: string;
  changesText: string;
}

/**
 * Gives each note (in file name order) the next patch: after the newest release, or from
 * `package.json`'s version if that is higher (a new line raised by hand), and after changes.json's
 * newest entry (a PR that still raised package.json by hand and wrote its own entry keeps it). The
 * versions must stay on `line` (shared/version/line.json).
 */
export function assignVersions(input: {
  packageText: string;
  changesText: string;
  latestStable: string | null;
  line: string;
  notes: PendingNote[];
}): Assignment {
  const { latestStable: latest, line } = input;
  let { packageText, changesText } = input;
  const notes = [...input.notes].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  if (notes.length === 0) return { assigned: [], packageText, changesText };
  const pkgVersion = (JSON.parse(packageText) as { version: string }).version;
  const pkg = parseVersion(pkgVersion);
  if (!pkg || !isStable(pkg) || pkg.build !== null) {
    throw new Error(`package.json's version must be a release version, not ${pkgVersion}`);
  }
  let version =
    latest === null || compareVersionText(nextPatch(latest), pkgVersion) < 0
      ? pkgVersion
      : nextPatch(latest);
  const newest = newestEntry(changesText);
  if (newest !== null && compareVersionText(newest, version) >= 0) version = nextPatch(newest);
  const assigned: Assignment['assigned'] = [];
  for (const note of notes) {
    const v = parseVersion(version);
    if (!v || `${String(v.major)}.${String(v.minor)}` !== line) {
      throw new Error(
        `${version} is not on the line in shared/version/line.json (${line}): a new line is raised by hand`,
      );
    }
    changesText = withChangesEntry(changesText, { version, changes: note.changes });
    assigned.push({ version, note: note.name, changes: note.changes });
    version = nextPatch(version);
  }
  const release = assigned[assigned.length - 1]?.version ?? pkgVersion;
  packageText = withPackageVersion(packageText, release);
  return { assigned, packageText, changesText };
}

/** `releaseAssign.ts [RELEASES.json]`, run in client/: applies the assignment to the files. */
export function assignMain(args: string[]): number {
  const [releasesFile] = args;
  const pendingDir = '../shared/releases/pending';
  try {
    const tags = releasesFile
      ? (JSON.parse(readFileSync(releasesFile, 'utf8')) as { tagName: string }[]).map(
          (r) => r.tagName,
        )
      : [];
    const names = existsSync(pendingDir)
      ? readdirSync(pendingDir).filter((n) => n.endsWith('.json'))
      : [];
    const notes = names.map((n) =>
      parsePendingNote(n, JSON.parse(readFileSync(join(pendingDir, n), 'utf8')) as unknown),
    );
    const latest = latestStable(tags);
    const { line } = JSON.parse(readFileSync('../shared/version/line.json', 'utf8')) as {
      line: string;
    };
    const out = assignVersions({
      packageText: readFileSync('package.json', 'utf8'),
      changesText: readFileSync('../shared/releases/changes.json', 'utf8'),
      latestStable: latest,
      line,
      notes,
    });
    if (out.assigned.length > 0) {
      writeFileSync('package.json', out.packageText);
      writeFileSync('../shared/releases/changes.json', out.changesText);
      for (const n of names) rmSync(join(pendingDir, n));
      for (const a of out.assigned) console.error(`${a.version}: ${a.note}`);
    }
    console.log(`assigned=${out.assigned.at(-1)?.version ?? ''}`);
    return 0;
  } catch (err) {
    console.error(`::error::${err instanceof Error ? err.message : String(err)}`);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = assignMain(process.argv.slice(2));
}
