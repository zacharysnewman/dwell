// The player's local worlds (ARCHITECTURE.md §2.1, Phase 5a): a small index kept in local storage
// listing each world's name, type, seed and when it was last played. The world file in OPFS
// (`dwell/worlds/<id>.dwellworld`, §6.4) stays the source of truth for the seed and generator;
// the index is what the main menu shows.
import { recordedVersion } from '../buildInfo';
import { isVersion } from '../version/semver';
import { GENERATORS, RETIRED_PLAYGROUND, sizeOptions, type Spawn } from './world';

/** The types a new world can have. */
export type CreatableType = keyof typeof GENERATORS;
/** Types in the index: `playground` only on worlds of versions before 0.9 (append-only store). */
export type WorldType = CreatableType | 'playground';
export const WORLD_TYPES: readonly CreatableType[] = ['terrain', 'flat', 'sandbox'];
const STORED_TYPES: readonly WorldType[] = [...WORLD_TYPES, 'playground'];

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
  /**
   * Where the world's first spawn is (the New world form's Spawn: `b`, `portal`, `portal-b`):
   * read only when its file is created. Absent: face A's usual spawn.
   */
  spawn?: Spawn;
  /**
   * The world's game mode (ADR 0065, by name): the New world form's Mode, read only when its
   * file is created (the world file keeps the mode after that), then updated when the host
   * switches it (the Host page's Mode) so the world list's badge shows it. Absent: creative
   * (`DEFAULT_MODE`).
   */
  mode?: string;
  /**
   * A test world's worldgen config (docs/DEV_TOOLS.md "Play a test world"; dev links and Chisel's
   * Play in game only): a fixture world's name (`test:tall_peak`) or a Chisel edit's JSON, whose
   * height field its maps are built from. Kept so the world reopens on the same terrain. Absent:
   * the build's terrain (every world the New world form makes).
   */
  worldgen?: string;
  /**
   * A bigger world's size as its radius scale (docs/proposals/BIG_MAP.md M2b: the New world
   * form's World size; `shared/protocol/constants.json` `worldSizes`). The world file's settings
   * are its truth once created; kept here so the page builds its maps at that size before the
   * world starts. Absent: today's (1×).
   */
  radiusScale?: number;
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

/**
 * The id of a `?worldgen=` link's test world: `test-<name>-s<seed>` (the name's other characters
 * as `-`), never a `local-g<generator>-s<seed>` world's.
 */
export function testWorldId(worldgen: string, seed: number): string {
  const name = worldgen
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 32);
  return `test-${name}-s${String(seed)}`.slice(0, 64);
}

/** The world type of a generator version (a retired version counts as its type's current one). */
export function typeOfGenerator(generatorVersion: number): WorldType {
  if (generatorVersion === GENERATORS.flat) return 'flat';
  if (generatorVersion === GENERATORS.sandbox) return 'sandbox';
  if (generatorVersion === RETIRED_PLAYGROUND) return 'playground';
  return 'terrain';
}

export function typeLabel(type: WorldType): string {
  return {
    terrain: 'Terrain',
    playground: 'Playground',
    flat: 'Flat',
    sandbox: 'Sandbox',
  }[type];
}

/** The type a new world asked for by name (`?new=1&type=…`): one this build makes, else terrain. */
export function creatableType(name: string): CreatableType {
  return (WORLD_TYPES as readonly string[]).includes(name) ? (name as CreatableType) : 'terrain';
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
    (STORED_TYPES as readonly string[]).includes(m.type) &&
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

  /** The host switched a world's mode (ADR 0065): its entry says so, other fields kept. */
  setMode(id: string, mode: string): void {
    const world = this.get(id);
    if (world) this.put({ ...world, mode });
  }

  remove(id: string): void {
    this.write(this.entries().filter((e) => !hasId(e, id)));
  }

  /**
   * Marks a world as played now, by this build's version, on this build's generator of its type:
   * a world from an older line opens on this build's terrain (owner, 2026-11-06;
   * `dwell_world::current_generator`), and its file then records that too.
   */
  touch(id: string, now: number): void {
    const world = this.get(id);
    if (!world) return;
    const generatorVersion =
      world.type === 'playground' ? world.generatorVersion : GENERATORS[world.type];
    this.put({ ...world, lastPlayedAt: now, appVersion: recordedVersion(), generatorVersion });
  }

  /**
   * A copy's entry, to play a world from an older line in this build and keep the original
   * (owner, 2026-11-06): a new id and the name marked with the version it came from; the same
   * seed, generator and spawn. Its file is the original's, copied by the caller
   * (`copyWorldFile`), so it records the original's version until it is played.
   */
  copy(world: WorldMeta, now: number, random: () => number = Math.random): WorldMeta {
    const taken = new Set(this.records().map((w) => w.id));
    let id = newWorldId(random);
    while (taken.has(id)) id = newWorldId(random);
    const from = world.appVersion ? ` (from ${world.appVersion})` : ' (copy)';
    const copy: WorldMeta = {
      id,
      name: cleanName(`${world.name.slice(0, MAX_NAME_LENGTH - from.length)}${from}`, world.name),
      type: world.type,
      seed: world.seed,
      generatorVersion: world.generatorVersion,
      createdAt: now,
      lastPlayedAt: now,
      ...(world.appVersion ? { appVersion: world.appVersion } : {}),
      ...(world.spawn ? { spawn: world.spawn } : {}),
      ...(world.mode ? { mode: world.mode } : {}),
      ...(world.worldgen ? { worldgen: world.worldgen } : {}),
      ...(world.radiusScale ? { radiusScale: world.radiusScale } : {}),
    };
    this.put(copy);
    return copy;
  }

  /** Creates a new world's entry (its file is created when it is first played). */
  create(
    options: {
      name: string;
      type: CreatableType;
      seed: number;
      spawn?: Spawn;
      mode?: string;
      /** A test world's worldgen config (dev links and Chisel only; `WorldMeta.worldgen`). */
      worldgen?: string;
      /** A bigger terrain world's radius scale (`WorldMeta.radiusScale`); 1 or absent: today's. */
      radiusScale?: number;
    },
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
      ...(options.spawn ? { spawn: options.spawn } : {}),
      ...(options.mode ? { mode: options.mode } : {}),
      ...(options.worldgen ? { worldgen: options.worldgen } : {}),
      ...(options.type === 'terrain' ? sizeOptions(options.radiusScale) : {}),
    };
    this.put(world);
    return world;
  }

  /**
   * The entry of a test world opened by a `?worldgen=<fixture>` link (docs/DEV_TOOLS.md), added
   * if missing: a terrain world of `seed` with the link's worldgen config, under its own id
   * (`testWorldId`), so it never shares a file with the seed's own world.
   */
  adoptTest(worldgen: string, seed: number, now: number): WorldMeta {
    const id = testWorldId(worldgen, seed);
    const existing = this.get(id);
    if (existing) return existing;
    const world: WorldMeta = {
      id,
      name: cleanName(`${worldgen.replace(/^test:/, '')} ${String(seed)}`, 'Test world'),
      type: 'terrain',
      seed,
      generatorVersion: GENERATORS.terrain,
      createdAt: now,
      lastPlayedAt: 0,
      appVersion: recordedVersion(),
      worldgen,
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
