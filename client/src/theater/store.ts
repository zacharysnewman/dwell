// Theater mode's saved camera paths (docs/THEATER_MODE.md §5): per player, in the browser, under
// the shared-stores rule (CLAUDE.md "versions and releases"): append-only — new fields optional,
// unknown fields and unreadable records kept on rewrite. One localStorage key holding a JSON list
// (paths are a few kB; a synchronous store keeps the toolbar simple), plus export/import as
// `<name>.camera.json`.

import type { PathJson } from './keyframes';

export const STORE_KEY = 'dwell.theater.paths';

/** A saved path: its JSON and where it was made (a hint, not a lock). */
export interface SavedPath {
  name: string;
  savedAt: number;
  world?: string;
  seed?: string;
  path: PathJson;
  [unknown: string]: unknown;
}

/** The storage this needs (localStorage, or a test's map). */
export interface KeyValue {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function readRaw(kv: KeyValue): unknown[] {
  let text: string | null;
  try {
    text = kv.getItem(STORE_KEY);
  } catch {
    return [];
  }
  if (!text) return [];
  try {
    const v = JSON.parse(text) as unknown;
    return Array.isArray(v) ? v : [];
  } catch {
    return [];
  }
}

const isSaved = (v: unknown): v is SavedPath =>
  typeof v === 'object' &&
  v !== null &&
  typeof (v as SavedPath).name === 'string' &&
  typeof (v as SavedPath).path === 'object';

export class PathStore {
  constructor(private readonly kv: KeyValue) {}

  /** The saved paths this build can read, newest first. */
  list(): SavedPath[] {
    return readRaw(this.kv)
      .filter(isSaved)
      .sort((a, b) => b.savedAt - a.savedAt);
  }

  get(name: string): SavedPath | undefined {
    return this.list().find((p) => p.name === name);
  }

  /**
   * Saves `path` under its name (replacing a record of that name, its unknown fields kept);
   * records this build can't read are kept as they were.
   */
  save(path: PathJson, where: { world?: string; seed?: string } = {}, now = Date.now()): void {
    const raw = readRaw(this.kv);
    const i = raw.findIndex((r) => isSaved(r) && r.name === path.name);
    const old = i >= 0 ? (raw[i] as SavedPath) : {};
    const rec: SavedPath = { ...old, ...where, name: path.name, savedAt: now, path };
    if (i >= 0) raw[i] = rec;
    else raw.push(rec);
    this.write(raw);
  }

  /** Deletes the saved path `name` (the player's own choice); the other records are kept. */
  remove(name: string): void {
    this.write(readRaw(this.kv).filter((r) => !(isSaved(r) && r.name === name)));
  }

  private write(raw: unknown[]): void {
    try {
      this.kv.setItem(STORE_KEY, JSON.stringify(raw));
    } catch {
      // A full or blocked store: the path stays exportable.
    }
  }
}

/** A path's export file name: `<name>.camera.json`, the name made safe for a file. */
export function exportName(name: string): string {
  const safe = name.replace(/[^\w.-]+/g, '-').replace(/^-+|-+$/g, '') || 'shot';
  return `${safe}.camera.json`;
}

/** Reads an imported path's text; throws with a message when it isn't a path this build reads. */
export function parseImported(text: string): PathJson {
  let v: unknown;
  try {
    v = JSON.parse(text);
  } catch (e) {
    throw new Error(`not JSON: ${(e as Error).message}`, { cause: e });
  }
  const p = v as Partial<Omit<PathJson, 'formatVersion'>> & { formatVersion?: unknown };
  if (typeof v !== 'object' || v === null) throw new Error('a camera path is an object');
  if (p.formatVersion !== 1)
    throw new Error(`formatVersion ${String(p.formatVersion)}: this build reads 1`);
  if (!Array.isArray(p.keyframes) || p.keyframes.length === 0)
    throw new Error('a camera path needs keyframes');
  return {
    ...p,
    formatVersion: 1,
    keyframes: p.keyframes,
    name: typeof p.name === 'string' ? p.name : 'imported',
    segments: p.segments ?? [],
  };
}
