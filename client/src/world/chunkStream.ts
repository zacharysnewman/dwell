// The client's side of terrain streaming (ARCHITECTURE.md §6.3): turns the server's ChunkData and
// ChunkUnload messages into chunks in the client sim (Generated ones via the worldgen worker pool,
// Explicit ones decoded from the message), and keeps render meshes of the loaded chunks current.
import { CHUNK_SIZE, ChunkForm, World } from '../protocol/constants.gen';
import type { ChunkCoord, Vec3 } from '../protocol/messages';
import type { ChunkSource } from '../worldgen/pool';

export const MIN_CHUNK_Y = Math.floor(World.worldMinY / CHUNK_SIZE);
export const MAX_CHUNK_Y = World.worldMaxY / CHUNK_SIZE - 1;

/** What the streamer needs from the client sim (ClientCore). */
export interface ChunkStore {
  setChunk(coord: ChunkCoord, revision: number, voxels: Uint16Array): void;
  removeChunk(coord: ChunkCoord): void;
  chunkFaces(cx: number, cy: number, cz: number): Uint8Array;
}

/** What the streamer needs from the renderer. */
export interface TerrainView {
  setTerrainChunk(key: string, origin: Vec3, faces: Uint8Array | null): void;
}

export interface ChunkMessage {
  form: ChunkForm;
  coord: ChunkCoord;
  revision: number;
  voxels: Uint16Array | null;
}

export interface StreamStats {
  loaded: number;
  generating: number;
  meshed: number;
  dirty: number;
}

export const chunkKey = (c: ChunkCoord): string =>
  `${String(c[0])},${String(c[1])},${String(c[2])}`;

const NEIGHBOURS: ChunkCoord[] = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
];

export class ChunkStreamer {
  private readonly loaded = new Map<string, ChunkCoord>();
  /** Generated chunks announced by the server and not yet generated, by key → request token. */
  private readonly generating = new Map<string, number>();
  private readonly dirty = new Map<string, ChunkCoord>();
  private readonly meshed = new Set<string>();
  private token = 0;
  /** First worldgen failure (the pool is unusable). */
  error: Error | null = null;

  constructor(
    private readonly store: ChunkStore,
    private readonly source: ChunkSource,
    private readonly view: TerrainView,
  ) {}

  onChunkData(m: ChunkMessage): void {
    const key = chunkKey(m.coord);
    this.forget(key, m.coord);
    if (m.form === ChunkForm.Explicit) {
      if (m.voxels) this.put(key, m.coord, m.revision, m.voxels);
      return;
    }
    const token = ++this.token;
    this.generating.set(key, token);
    this.source.generate(m.coord).then(
      (c) => {
        if (this.generating.get(key) !== token) return; // unloaded or replaced meanwhile
        this.generating.delete(key);
        this.put(key, m.coord, m.revision, c.voxels);
      },
      (err: unknown) => {
        this.generating.delete(key);
        this.error ??= err instanceof Error ? err : new Error(String(err));
      },
    );
  }

  onChunkUnload(coords: ChunkCoord[]): void {
    for (const coord of coords) {
      const key = chunkKey(coord);
      this.forget(key, coord);
      if (this.loaded.delete(key)) this.store.removeChunk(coord);
      this.dirty.delete(key);
      if (this.meshed.delete(key)) this.view.setTerrainChunk(key, [0, 0, 0], null);
    }
  }

  isLoaded(coord: ChunkCoord): boolean {
    return this.loaded.has(chunkKey(coord));
  }

  /** True when the chunks around `position` (±1 chunk, within the world's rows) are loaded. */
  readyAround(position: Vec3): boolean {
    const [cx, cy, cz] = position.map((v) => Math.floor(v / CHUNK_SIZE)) as ChunkCoord;
    for (let y = Math.max(cy - 1, MIN_CHUNK_Y); y <= Math.min(cy + 1, MAX_CHUNK_Y); y++)
      for (let z = cz - 1; z <= cz + 1; z++)
        for (let x = cx - 1; x <= cx + 1; x++)
          if (!this.loaded.has(chunkKey([x, y, z]))) return false;
    return true;
  }

  /**
   * (Re)builds render meshes of changed chunks, nearest to `center` first, for up to `budgetMs`
   * (at least one). A chunk waits while a neighbour is still being generated, so it is meshed once
   * with its borders known rather than again when each neighbour arrives.
   */
  meshDirty(center: Vec3, budgetMs: number, now: () => number = () => performance.now()): number {
    if (this.dirty.size === 0) return 0;
    const deadline = now() + budgetMs;
    const distance = (c: ChunkCoord): number =>
      ((c[0] + 0.5) * CHUNK_SIZE - center[0]) ** 2 +
      ((c[1] + 0.5) * CHUNK_SIZE - center[1]) ** 2 +
      ((c[2] + 0.5) * CHUNK_SIZE - center[2]) ** 2;
    const ready = [...this.dirty.entries()]
      .filter(([, c]) => !NEIGHBOURS.some((d) => this.generating.has(chunkKey(add(c, d)))))
      .sort((a, b) => distance(a[1]) - distance(b[1]));
    let count = 0;
    for (const [key, c] of ready) {
      this.dirty.delete(key);
      this.view.setTerrainChunk(
        key,
        [c[0] * CHUNK_SIZE, c[1] * CHUNK_SIZE, c[2] * CHUNK_SIZE],
        this.store.chunkFaces(c[0], c[1], c[2]),
      );
      this.meshed.add(key);
      count++;
      if (now() >= deadline) break;
    }
    return count;
  }

  stats(): StreamStats {
    return {
      loaded: this.loaded.size,
      generating: this.generating.size,
      meshed: this.meshed.size,
      dirty: this.dirty.size,
    };
  }

  private put(key: string, coord: ChunkCoord, revision: number, voxels: Uint16Array): void {
    this.store.setChunk(coord, revision, voxels);
    this.loaded.set(key, coord);
    this.dirty.set(key, coord);
    // Neighbours' border faces may now be hidden by this chunk.
    for (const d of NEIGHBOURS) {
      const n = add(coord, d);
      const nk = chunkKey(n);
      if (this.loaded.has(nk)) this.dirty.set(nk, n);
    }
  }

  /** Drops a pending generation of the chunk (the new message or an unload supersedes it). */
  private forget(key: string, coord: ChunkCoord): void {
    if (this.generating.delete(key)) this.source.cancel(coord);
  }
}

function add(a: ChunkCoord, b: ChunkCoord): ChunkCoord {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}
