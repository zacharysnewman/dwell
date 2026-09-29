// The local-mode world file (§6.4) through the browser's VFS path: the WASM core's SQLite over
// `dwellFiles`, here backed by in-memory stand-ins for OPFS sync access handles. The core part is
// skipped until `npm run build:wasm` has been run, unless DWELL_REQUIRE_WASM is set (CI).
import { existsSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { Channel, TransportKind } from '../protocol/constants.gen';
import { LocalCore, type DwellCoreFactory } from './wasmCore';
import {
  handleFiles,
  localWorldName,
  WORLD_FILE,
  WORLD_FILE_SUFFIXES,
  type SyncHandle,
} from './worldFiles';

/** An in-memory FileSystemSyncAccessHandle. */
class MemoryHandle implements SyncHandle {
  data = new Uint8Array(0);
  read(buffer: Uint8Array, { at }: { at: number }): number {
    const n = Math.max(0, Math.min(buffer.length, this.data.length - at));
    buffer.set(this.data.subarray(at, at + n));
    return n;
  }
  write(buffer: Uint8Array, { at }: { at: number }): number {
    if (at + buffer.length > this.data.length) {
      const grown = new Uint8Array(at + buffer.length);
      grown.set(this.data);
      this.data = grown;
    }
    this.data.set(buffer, at);
    return buffer.length;
  }
  getSize(): number {
    return this.data.length;
  }
  truncate(size: number): void {
    const next = new Uint8Array(size);
    next.set(this.data.subarray(0, size));
    this.data = next;
  }
  flush(): void {}
  close(): void {}
}

function memoryWorld(): Map<string, MemoryHandle> {
  return new Map(WORLD_FILE_SUFFIXES.map((s) => [WORLD_FILE + s, new MemoryHandle()]));
}

describe('world files over sync handles', () => {
  it('open only the world files, treat empty ones as absent, and truncate on remove', () => {
    const handles = memoryWorld();
    const files = handleFiles(handles);
    expect(files.open(WORLD_FILE, true)).toBe(0);
    expect(files.open(`${WORLD_FILE}-journal`, true)).toBe(1);
    expect(files.open('other.db', true)).toBe(-1);
    expect(files.exists(`${WORLD_FILE}-journal`)).toBe(false);
    files.write(1, Uint8Array.of(1, 2, 3), 10);
    expect(files.size(1)).toBe(13);
    expect(files.exists(`${WORLD_FILE}-journal`)).toBe(true);
    const out = new Uint8Array(4);
    expect(files.read(1, out, 11)).toBe(2);
    expect([...out]).toEqual([2, 3, 0, 0]);
    files.remove(`${WORLD_FILE}-journal`);
    expect(files.exists(`${WORLD_FILE}-journal`)).toBe(false);
  });

  it('name one world file per generator and seed', () => {
    expect(localWorldName(3, 0)).toBe('local-g3-s0');
    expect(localWorldName(1, 42)).not.toBe(localWorldName(3, 42));
  });
});

const wasmJs = new URL('../../public/wasm/dwell_core.js', import.meta.url);
const skip = !existsSync(wasmJs) && !process.env.DWELL_REQUIRE_WASM;

describe.skipIf(skip)('local world persistence (WASM)', () => {
  async function factory(): Promise<DwellCoreFactory> {
    return ((await import(/* @vite-ignore */ wasmJs.href)) as { default: DwellCoreFactory })
      .default;
  }

  it('saves the world into its file and loads it again in a new core', async () => {
    const handles = memoryWorld();
    const first = await LocalCore.load(await factory(), 99, 0, handleFiles(handles));
    expect(first.persisted).toBe(true);
    expect(first.storageError).toBe('');
    first.connected(1, TransportKind.Loopback, new Uint8Array(32));
    first.advance(0.1);
    expect(first.save()).toBe(true);
    const size = handles.get(WORLD_FILE)?.getSize() ?? 0;
    expect(size).toBeGreaterThan(4096);
    // SQLite's header, and a rollback journal left empty after the commit.
    const header = new TextDecoder().decode(handles.get(WORLD_FILE)?.data.subarray(0, 15));
    expect(header).toBe('SQLite format 3');
    expect(handles.get(`${WORLD_FILE}-journal`)?.getSize()).toBe(0);

    // A new core opens the same file (the saved world wins over the seed it is asked for).
    const second = await LocalCore.load(await factory(), 5, 0, handleFiles(handles));
    expect(second.persisted).toBe(true);
    second.connected(1, TransportKind.Loopback, new Uint8Array(32));
    second.reliable(1, Channel.control, Uint8Array.of(0x40)); // StatusRequest
    expect(second.takeOutbox().length).toBe(1);
  });

  it('runs without persistence when the file cannot be opened', async () => {
    const core = await LocalCore.load(await factory(), 1, 0, handleFiles(new Map()));
    expect(core.persisted).toBe(false);
    expect(core.storageError).not.toBe('');
    expect(core.save()).toBe(false);
  });
});
