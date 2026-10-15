// The structures store (docs/STRUCTURES.md §4, ARCHITECTURE.md §6.4): the sandbox's library of
// saved structures, one file each in OPFS at `dwell/structures/<name>.json`, in exactly the
// format of `shared/structures/` (structures.ts). It outlives any world and every app version
// reads it, so it is append-only like the world index: new fields are optional, a rewrite keeps
// the fields this build doesn't know, and files it can't read (broken JSON, a newer format, a
// rule this build doesn't share) are listed as such and left alone — never rewritten or deleted.
import {
  checkStructure,
  parseStructure,
  structureFile,
  structureText,
  unknownFields,
  knownFields,
  type StructureJson,
  type ValidStructure,
} from './structures';

/** The part of a directory the store uses (OPFS in the game, a map in tests). */
export interface StructureDir {
  /** File names in the directory. */
  names(): Promise<string[]>;
  /** A file's text, or null if there is none. */
  read(file: string): Promise<string | null>;
  write(file: string, text: string): Promise<void>;
  remove(file: string): Promise<void>;
}

export type StoredStructure =
  | { file: string; status: 'ok'; json: StructureJson; structure: ValidStructure }
  /** Kept as it is: `reason` says why this build can't use it; `text` is exported unchanged. */
  | { file: string; status: 'unreadable'; reason: string; text: string };

/** A file name the store holds: `<name>.json`, the name's part after the namespace. */
const FILE = /^[a-z0-9_]+\.json$/;

export class StructureStore {
  constructor(private readonly dir: StructureDir) {}

  /** Every structure file, readable or not, by name. */
  async list(): Promise<StoredStructure[]> {
    const files = (await this.dir.names()).filter((f) => FILE.test(f)).sort();
    const out: StoredStructure[] = [];
    for (const file of files) {
      const entry = await this.get(file);
      if (entry) out.push(entry);
    }
    return out;
  }

  /** One file, or null if it doesn't exist. */
  async get(file: string): Promise<StoredStructure | null> {
    const text = await this.dir.read(file);
    if (text === null) return null;
    try {
      const { json, structure } = parseStructure(text, file);
      if (structureFile(structure.name) !== file) {
        throw new Error(`${file}: name: "${structure.name}" must match the file name`);
      }
      return { file, status: 'ok', json, structure };
    } catch (e) {
      return { file, status: 'unreadable', reason: e instanceof Error ? e.message : String(e), text };
    }
  }

  /**
   * Saves a structure (checked first) as `<name>.json`. Over an existing file only with
   * `overwrite`, and never over one this build can't read; the fields this build doesn't know are
   * kept from the stored file, unless `json` itself carries them (an import).
   */
  async save(json: StructureJson, options: { overwrite?: boolean } = {}): Promise<string> {
    const structure = checkStructure(json, 'structure');
    const file = structureFile(structure.name);
    const stored = await this.get(file);
    if (stored?.status === 'unreadable') {
      throw new Error(`${file} is kept as it is: this version can't read it (${stored.reason}).`);
    }
    if (stored && !options.overwrite) throw new Error(`A structure named ${file} exists.`);
    const merged = stored ? { ...unknownFields(stored.json), ...json } : json;
    await this.dir.write(file, structureText(merged));
    return file;
  }

  /** Renames a readable structure (its name and file); every other field is kept as it is. */
  async rename(file: string, name: string): Promise<string> {
    const stored = await this.get(file);
    if (stored?.status !== 'ok') throw new Error(`${file} can't be renamed by this version.`);
    const json = { ...stored.json, name };
    const structure = checkStructure(json, 'structure');
    const to = structureFile(structure.name);
    if (to === file) {
      await this.dir.write(file, structureText(json));
      return file;
    }
    if ((await this.dir.read(to)) !== null) throw new Error(`A structure named ${to} exists.`);
    await this.dir.write(to, structureText(json));
    await this.dir.remove(file);
    return to;
  }

  /** Deletes a readable structure; files this build can't read are never deleted. */
  async remove(file: string): Promise<void> {
    const stored = await this.get(file);
    if (stored?.status !== 'ok') throw new Error(`${file} is kept: this version can't read it.`);
    await this.dir.remove(file);
  }

  /** The text to export: the file as stored (every field, readable or not). */
  async exportText(file: string): Promise<string | null> {
    return this.dir.read(file);
  }
}

/** The known fields of a stored file, for code that shouldn't see a newer build's (stamping). */
export function knownJson(entry: Extract<StoredStructure, { status: 'ok' }>): StructureJson {
  return knownFields(entry.json);
}

/** `dwell/structures/` in OPFS (created if missing). Throws where OPFS is unavailable. */
async function structuresDirectory(): Promise<FileSystemDirectoryHandle> {
  const root = await navigator.storage.getDirectory();
  const dwell = await root.getDirectoryHandle('dwell', { create: true });
  return dwell.getDirectoryHandle('structures', { create: true });
}

/** The store's directory in OPFS. Writing needs `createWritable` on the page (not a worker). */
export function opfsStructureDir(): StructureDir {
  type Writable = { write(data: string): Promise<void>; close(): Promise<void> };
  type WritableFile = FileSystemFileHandle & { createWritable(): Promise<Writable> };
  return {
    names: async () => {
      const dir = (await structuresDirectory()) as FileSystemDirectoryHandle & {
        keys(): AsyncIterable<string>;
      };
      const names: string[] = [];
      for await (const name of dir.keys()) names.push(name);
      return names;
    },
    read: async (file) => {
      const dir = await structuresDirectory();
      try {
        return await (await dir.getFileHandle(file)).getFile().then((f) => f.text());
      } catch (err) {
        if (err instanceof DOMException && err.name === 'NotFoundError') return null;
        throw err;
      }
    },
    write: async (file, text) => {
      const dir = await structuresDirectory();
      const handle = (await dir.getFileHandle(file, { create: true })) as WritableFile;
      const w = await handle.createWritable();
      await w.write(text);
      await w.close();
    },
    remove: async (file) => {
      const dir = await structuresDirectory();
      await dir.removeEntry(file);
    },
  };
}
