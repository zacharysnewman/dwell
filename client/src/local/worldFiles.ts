// Local-mode world files (ARCHITECTURE.md §6.4): the SQLite world database lives in the Origin
// Private File System under `dwell/worlds/`, reached from the WASM core's VFS
// (server/core/src/storage/opfs_vfs.cpp) through the `dwellFiles` interface below. OPFS sync access
// handles are synchronous — what SQLite needs — but opening one is not, so the worker opens every
// file the world can use (the database, its rollback journal, and a WAL for opening a file saved
// by a dedicated server) before the core starts. A handle is exclusive: a second tab on the same
// world gets no persistence rather than a corrupted file.

/** The part of FileSystemSyncAccessHandle the VFS uses. */
export interface SyncHandle {
  read(buffer: Uint8Array, options: { at: number }): number;
  write(buffer: Uint8Array, options: { at: number }): number;
  getSize(): number;
  truncate(size: number): void;
  flush(): void;
  close(): void;
}

/** What the WASM core's VFS calls (`Module.dwellFiles`). */
export interface DwellFiles {
  open(name: string, create: boolean): number;
  read(id: number, bytes: Uint8Array, offset: number): number;
  write(id: number, bytes: Uint8Array, offset: number): void;
  size(id: number): number;
  truncate(id: number, size: number): void;
  flush(id: number): void;
  close(id: number): void;
  exists(name: string): boolean;
  remove(name: string): void;
}

/** The name the core opens its world file by (wasm_api.cpp kLocalWorldFile). */
export const WORLD_FILE = 'world.dwellworld';
/** Files a world may use besides the database. */
export const WORLD_FILE_SUFFIXES = ['', '-journal', '-wal'] as const;

/**
 * `dwellFiles` over already-open handles, keyed by the name SQLite uses. Unknown names cannot be
 * opened; an empty file counts as absent (journals are truncated, never deleted).
 */
export function handleFiles(handles: ReadonlyMap<string, SyncHandle>): DwellFiles {
  const byId = [...handles.values()];
  const names = [...handles.keys()];
  const handle = (id: number): SyncHandle => {
    const h = byId[id];
    if (!h) throw new RangeError(`no world file ${String(id)}`);
    return h;
  };
  return {
    open: (name) => names.indexOf(name),
    read: (id, bytes, offset) => handle(id).read(bytes, { at: offset }),
    write: (id, bytes, offset) => {
      let done = 0;
      while (done < bytes.length) {
        const n = handle(id).write(bytes.subarray(done), { at: offset + done });
        if (n <= 0) throw new Error('world file write failed');
        done += n;
      }
    },
    size: (id) => handle(id).getSize(),
    truncate: (id, size) => {
      handle(id).truncate(size);
    },
    flush: (id) => {
      handle(id).flush();
    },
    close: () => undefined, // the handles stay open for the worker's lifetime
    exists: (name) => (handles.get(name)?.getSize() ?? 0) > 0,
    remove: (name) => {
      const h = handles.get(name);
      if (h) {
        h.truncate(0);
        h.flush();
      }
    },
  };
}

/** File name (in `dwell/worlds/`) of the local world for a generator and seed. */
export function localWorldName(generatorVersion: number, worldSeed: number): string {
  return `local-g${String(generatorVersion)}-s${String(worldSeed)}`;
}

/** `dwell/worlds/` in OPFS (created if missing). Throws where OPFS is unavailable. */
async function worldsDirectory(): Promise<FileSystemDirectoryHandle> {
  const root = await navigator.storage.getDirectory();
  const dwell = await root.getDirectoryHandle('dwell', { create: true });
  return dwell.getDirectoryHandle('worlds', { create: true });
}

/** The worlds saved in OPFS, by name (without `.dwellworld`); empty where OPFS is unavailable. */
export async function listWorldFiles(): Promise<string[]> {
  try {
    const dir = (await worldsDirectory()) as FileSystemDirectoryHandle & {
      keys(): AsyncIterable<string>;
    };
    const names: string[] = [];
    for await (const file of dir.keys()) {
      if (file.endsWith('.dwellworld')) names.push(file.slice(0, -'.dwellworld'.length));
    }
    return names;
  } catch {
    return [];
  }
}

/**
 * Deletes a world's files. True once none is left (including a world never saved); false if one
 * could not be removed — typically because the world is open in another tab.
 */
export async function deleteWorldFiles(name: string): Promise<boolean> {
  let dir: FileSystemDirectoryHandle;
  try {
    dir = await worldsDirectory();
  } catch {
    return true; // no OPFS: nothing was saved
  }
  for (const suffix of WORLD_FILE_SUFFIXES) {
    try {
      await dir.removeEntry(`${name}.dwellworld${suffix}`);
    } catch (err) {
      if (!(err instanceof DOMException && err.name === 'NotFoundError')) return false;
    }
  }
  return true;
}

/**
 * Opens (creating) the world's files in OPFS. Null where OPFS or sync handles are unavailable, or
 * when another tab holds the world.
 */
export async function openWorldFiles(
  name: string,
): Promise<{ files: DwellFiles; close(): void } | null> {
  type SyncFileHandle = FileSystemFileHandle & {
    createSyncAccessHandle(): Promise<SyncHandle>;
  };
  const opened: SyncHandle[] = [];
  try {
    const dir = await worldsDirectory();
    const handles = new Map<string, SyncHandle>();
    for (const suffix of WORLD_FILE_SUFFIXES) {
      const file = (await dir.getFileHandle(`${name}.dwellworld${suffix}`, {
        create: true,
      })) as SyncFileHandle;
      const h = await file.createSyncAccessHandle();
      opened.push(h);
      handles.set(WORLD_FILE + suffix, h);
    }
    return {
      files: handleFiles(handles),
      close: () => {
        for (const h of opened) h.close();
      },
    };
  } catch {
    for (const h of opened) h.close();
    return null;
  }
}
