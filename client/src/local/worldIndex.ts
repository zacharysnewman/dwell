// The player's local worlds (ARCHITECTURE.md §2.1, Phase 5a): a small index kept in local storage
// listing each world's name, type, seed and when it was last played. The world file in OPFS
// (`dwell/worlds/<id>.dwellworld`, §6.4) stays the source of truth for the seed and generator;
// the index is what the main menu shows.
import { recordedVersion } from '../buildInfo';
import { isVersion } from '../version/semver';
import { GENERATORS } from './world';

export type WorldType = keyof typeof GENERATORS;
export const WORLD_TYPES: readonly WorldType[] = ['terrain', 'playground', 'flat'];

export interface WorldMeta {
  /** File name in `dwell/worlds/` without `.dwellworld`. */
  id: string;
  name: string;
  type: WorldType;
  seed: number;
  generatorVersion: number;
  createdAt: number;
  lastPlayedAt: number;
  /**
   * The app version that last played the world (RELEASES.md §6): it locks the world to that
   * version's compatibility line. Absent on worlds saved before versioned releases, which are
   * ignored (the menu offers to delete them).
   */
  appVersion?: string;
}

/** The part of `Storage` the index uses. */
export interface KeyValueStore {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export const INDEX_KEY = 'dwell.worlds';
export const MAX_NAME_LENGTH = 40;

/**
 * Worlds saved before the index existed, and those opened by `?world=`/`?seed=` links
 * (`localWorldName` in worldFiles.ts).
 */
const LEGACY_ID = /^local-g(\d+)-s(\d+)$/;

/** The world type of a generator version (a retired version counts as its type's current one). */
export function typeOfGenerator(generatorVersion: number): WorldType {
  if (generatorVersion === GENERATORS.flat) return 'flat';
  if (generatorVersion === GENERATORS.playground) return 'playground';
  return 'terrain';
}

export function typeLabel(type: WorldType): string {
  return { terrain: 'Terrain', playground: 'Playground', flat: 'Flat' }[type];
}

/** FNV-1a 32-bit hash of a string's UTF-16 code units: text seeds. */
export function hashSeed(text: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

/**
 * The seed a player typed: blank picks a random one, a whole number is used as is (up to 2^53 − 1,
 * what local mode can carry exactly), and any other text is hashed.
 */
export function parseSeed(text: string, random: () => number = Math.random): number {
  const t = text.trim();
  if (t === '') return Math.floor(random() * 2 ** 32);
  if (/^\d+$/.test(t)) {
    const n = Number(t);
    if (Number.isSafeInteger(n)) return n;
  }
  return hashSeed(t);
}

/** A new world's id: `w` and 10 base-36 characters. */
export function newWorldId(random: () => number = Math.random): string {
  let id = 'w';
  for (let i = 0; i < 10; i++) id += Math.floor(random() * 36).toString(36);
  return id;
}

export function cleanName(name: string, fallback: string): string {
  const t = name.replace(/\s+/g, ' ').trim().slice(0, MAX_NAME_LENGTH);
  return t === '' ? fallback : t;
}

/** Whether a world record carries a valid app version (otherwise it predates versioning). */
export function isVersioned(world: WorldMeta): world is WorldMeta & { appVersion: string } {
  return typeof world.appVersion === 'string' && isVersion(world.appVersion);
}

function isMeta(v: unknown): v is WorldMeta {
  if (typeof v !== 'object' || v === null) return false;
  const m = v as Record<string, unknown>;
  return (
    typeof m.id === 'string' &&
    /^[a-z0-9-]{1,64}$/.test(m.id) &&
    typeof m.name === 'string' &&
    typeof m.type === 'string' &&
    (WORLD_TYPES as readonly string[]).includes(m.type) &&
    Number.isSafeInteger(m.seed) &&
    (m.seed as number) >= 0 &&
    Number.isInteger(m.generatorVersion) &&
    typeof m.createdAt === 'number' &&
    typeof m.lastPlayedAt === 'number'
  );
}

/**
 * The index is shared by every app version (RELEASES.md §6): its format is append-only, and a build
 * keeps what it does not understand when it rewrites it — unknown fields of a record, and whole
 * records it cannot read — so an older build never strips what a newer one stored.
 */
export class WorldIndex {
  constructor(private readonly store: KeyValueStore | null) {}

  /** Every stored entry as parsed, readable or not. */
  private entries(): unknown[] {
    try {
      const raw: unknown = JSON.parse(this.store?.getItem(INDEX_KEY) ?? '[]');
      return Array.isArray(raw) ? raw : [];
    } catch {
      return [];
    }
  }

  private records(): WorldMeta[] {
    return this.entries()
      .filter(isMeta)
      .sort((a, b) => b.lastPlayedAt - a.lastPlayedAt);
  }

  /** The worlds, most recently played first: those with an app version. */
  list(): WorldMeta[] {
    return this.records().filter(isVersioned);
  }

  /** Worlds saved before versioned releases: ignored, but kept until the player deletes them. */
  legacy(): WorldMeta[] {
    return this.records().filter((w) => !isVersioned(w));
  }

  get(id: string): WorldMeta | null {
    return this.records().find((w) => w.id === id) ?? null;
  }

  /** Adds or replaces a world, keeping fields of the stored record that this build doesn't know. */
  put(world: WorldMeta): void {
    const entries = this.entries();
    const stored = entries.find((e) => hasId(e, world.id));
    const merged = typeof stored === 'object' && stored !== null ? { ...stored, ...world } : world;
    this.write([merged, ...entries.filter((e) => !hasId(e, world.id))]);
  }

  remove(id: string): void {
    this.write(this.entries().filter((e) => !hasId(e, id)));
  }

  /** Marks a world as played now, by this build's version. */
  touch(id: string, now: number): void {
    const world = this.get(id);
    if (world) this.put({ ...world, lastPlayedAt: now, appVersion: recordedVersion() });
  }

  /** Creates a new world's entry (its file is created when it is first played). */
  create(
    options: { name: string; type: WorldType; seed: number },
    now: number,
    random: () => number = Math.random,
  ): WorldMeta {
    const taken = new Set(this.records().map((w) => w.id));
    let id = newWorldId(random);
    while (taken.has(id)) id = newWorldId(random);
    const world: WorldMeta = {
      id,
      name: cleanName(options.name, 'New world'),
      type: options.type,
      seed: options.seed,
      generatorVersion: GENERATORS[options.type],
      createdAt: now,
      lastPlayedAt: now,
      appVersion: recordedVersion(),
    };
    this.put(world);
    return world;
  }

  /**
   * The entry for a world opened by a `?world=`/`?seed=` link or found as a file without an entry
   * (named `local-g<generator>-s<seed>`), added if missing. Null for other names. `appVersion` is
   * the version to record: this build's for a world the link is creating now, none for a file
   * that was already there (saved before versioned releases, so ignored).
   */
  adopt(id: string, now: number, appVersion?: string): WorldMeta | null {
    const existing = this.get(id);
    if (existing) return existing;
    const m = LEGACY_ID.exec(id);
    if (!m?.[1] || !m[2]) return null;
    const generatorVersion = Number(m[1]);
    const seed = Number(m[2]);
    if (!Number.isSafeInteger(seed)) return null;
    const type = typeOfGenerator(generatorVersion);
    const world: WorldMeta = {
      id,
      name: `${typeLabel(type)} world ${String(seed)}`,
      type,
      seed,
      generatorVersion,
      createdAt: now,
      lastPlayedAt: 0,
      ...(appVersion ? { appVersion } : {}),
    };
    this.put(world);
    return world;
  }

  private write(entries: unknown[]): void {
    try {
      this.store?.setItem(INDEX_KEY, JSON.stringify(entries));
    } catch {
      // Storage full or blocked: the list isn't kept, the world files still are.
    }
  }
}

function hasId(entry: unknown, id: string): boolean {
  return typeof entry === 'object' && entry !== null && (entry as { id?: unknown }).id === id;
}
