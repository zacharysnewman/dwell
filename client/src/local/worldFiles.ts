// Local-mode world files (ARCHITECTURE.md §6.4): the world database (redb) lives in the Origin
// Private File System under `dwell/worlds/`, read and written through an OPFS sync access handle
// (dwell-client's web/local_worker.rs). Sync access handles are synchronous — what redb's storage
// backend needs — but opening one is not, so the worker opens the file before the core starts. A
// handle is exclusive: a second tab on the same world gets no persistence rather than a corrupted
// file.

/** Files a world may have used: the database and, from the SQLite build, its journal and WAL. */
export const WORLD_FILE_SUFFIXES = ['', '-journal', '-wal'] as const;

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
 * Opens (creating) the world's file in OPFS. Null where OPFS or sync handles are unavailable, or
 * when another tab holds the world.
 */
export async function openWorldFile(
  name: string,
): Promise<{ handle: FileSystemSyncAccessHandle; close(): void } | null> {
  type SyncFileHandle = FileSystemFileHandle & {
    createSyncAccessHandle(): Promise<FileSystemSyncAccessHandle>;
  };
  try {
    const dir = await worldsDirectory();
    const file = (await dir.getFileHandle(`${name}.dwellworld`, {
      create: true,
    })) as SyncFileHandle;
    const handle = await file.createSyncAccessHandle();
    return {
      handle,
      close: () => {
        handle.close();
      },
    };
  } catch {
    return null;
  }
}
