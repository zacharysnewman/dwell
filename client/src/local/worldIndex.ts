// The player's local worlds (ARCHITECTURE.md §2.1, Phase 5a): a small index kept in local storage
// listing each world's name, type, seed and when it was last played. The world file in OPFS
// (`dwell/worlds/<id>.dwellworld`, §6.4) stays the source of truth for the seed and generator;
// the index is what the main menu shows.
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

export class WorldIndex {
  constructor(private readonly store: KeyValueStore | null) {}

  /** The worlds, most recently played first. Entries that don't parse are dropped. */
  list(): WorldMeta[] {
    let raw: unknown;
    try {
      raw = JSON.parse(this.store?.getItem(INDEX_KEY) ?? '[]');
    } catch {
      raw = null;
    }
    const worlds = Array.isArray(raw) ? raw.filter(isMeta) : [];
    return worlds.sort((a, b) => b.lastPlayedAt - a.lastPlayedAt);
  }

  get(id: string): WorldMeta | null {
    return this.list().find((w) => w.id === id) ?? null;
  }

  /** Adds or replaces a world. */
  put(world: WorldMeta): void {
    this.write([world, ...this.list().filter((w) => w.id !== world.id)]);
  }

  remove(id: string): void {
    this.write(this.list().filter((w) => w.id !== id));
  }

  /** Marks a world as played now. */
  touch(id: string, now: number): void {
    const world = this.get(id);
    if (world) this.put({ ...world, lastPlayedAt: now });
  }

  /** Creates a new world's entry (its file is created when it is first played). */
  create(
    options: { name: string; type: WorldType; seed: number },
    now: number,
    random: () => number = Math.random,
  ): WorldMeta {
    const taken = new Set(this.list().map((w) => w.id));
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
    };
    this.put(world);
    return world;
  }

  /**
   * The entry for a world opened by a `?world=`/`?seed=` link or saved before the index existed
   * (named `local-g<generator>-s<seed>`), added if missing. Null for other names.
   */
  adopt(id: string, now: number): WorldMeta | null {
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
    };
    this.put(world);
    return world;
  }

  private write(worlds: WorldMeta[]): void {
    try {
      this.store?.setItem(INDEX_KEY, JSON.stringify(worlds));
    } catch {
      // Storage full or blocked: the list isn't kept, the world files still are.
    }
  }
}
