// The client's side of terrain streaming (ARCHITECTURE.md §6.3) and voxel edits (§6.5): turns the
// server's ChunkData and ChunkUnload messages into chunks in the client sim (Generated ones via the
// worldgen worker pool, Explicit ones decoded from the message), applies VoxelModifications in
// revision order (a gap asks the server to resync the chunk), and keeps render meshes of the loaded
// chunks current through the meshing worker pool.
import type { ChunkMeshes } from '../mesh/mesher';
import type { Mesher } from '../mesh/pool';
import { CHUNK_VOLUME } from '../protocol/chunkVoxels';
import { CHUNK_SIZE, ChunkForm, World } from '../protocol/constants.gen';
import type { ChunkChanges, ChunkCoord, Vec3 } from '../protocol/messages';
import type { ChunkSource } from '../worldgen/pool';

export const MIN_CHUNK_Y = Math.floor(World.worldMinY / CHUNK_SIZE);
export const MAX_CHUNK_Y = World.worldMaxY / CHUNK_SIZE - 1;

/** What the streamer needs from the client sim (ClientCore). */
export interface ChunkStore {
  setChunk(coord: ChunkCoord, revision: number, voxels: Uint16Array): void;
  removeChunk(coord: ChunkCoord): void;
  /** Applies (index, material) pairs; the chunk takes `revision`. */
  editChunk(coord: ChunkCoord, revision: number, changes: Uint16Array): void;
  /** The chunk's voxels with a one-voxel apron (mesh/mesher.ts), for meshing. */
  paddedChunk(cx: number, cy: number, cz: number): Uint16Array<ArrayBuffer>;
}

/** What the streamer needs from the renderer. */
export interface TerrainView {
  setTerrainChunk(key: string, origin: Vec3, meshes: ChunkMeshes | null): void;
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
  /** Chunks with a mesh job in a worker. */
  meshing: number;
  /** Chunks re-requested after a revision gap (§6.3). */
  resyncs: number;
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

/** Mesh jobs in flight at once when the mesher does not say. */
const DEFAULT_MESH_JOBS = 8;

export class ChunkStreamer {
  /** Loaded chunks and their revisions (Air chunks: revision 0). */
  private readonly loaded = new Map<string, { coord: ChunkCoord; revision: number }>();
  /** Loaded chunks the server sent as Air: known empty, never generated or stored. */
  private readonly air = new Set<string>();
  /** Generated chunks announced by the server and not yet generated, by key → request token. */
  private readonly generating = new Map<string, number>();
  /** Modifications of chunks still generating, applied once they arrive. */
  private readonly heldEdits = new Map<string, ChunkChanges[]>();
  private readonly dirty = new Map<string, ChunkCoord>();
  private readonly meshed = new Set<string>();
  /** Mesh jobs in flight, by key → token (a newer job or an unload supersedes). */
  private readonly meshing = new Map<string, number>();
  /** Chunks asked for again after a revision gap, until they arrive. */
  private readonly resyncing = new Set<string>();
  private resyncs = 0;
  private token = 0;
  /** First worldgen failure (the pool is unusable). */
  error: Error | null = null;

  constructor(
    private readonly store: ChunkStore,
    private readonly source: ChunkSource,
    private readonly mesher: Mesher & { capacity?: number },
    private readonly view: TerrainView,
    /** Asks the server to send chunks again (ChunkResync). */
    private readonly resync: (coords: ChunkCoord[]) => void = () => undefined,
  ) {}

  onChunkData(m: ChunkMessage): void {
    const key = chunkKey(m.coord);
    this.forget(key, m.coord);
    this.resyncing.delete(key);
    if (m.form === ChunkForm.Air) {
      this.putAir(key, m.coord);
      return;
    }
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
        for (const changes of this.heldEdits.get(key) ?? []) this.apply(key, changes);
        this.heldEdits.delete(key);
      },
      (err: unknown) => {
        this.generating.delete(key);
        this.heldEdits.delete(key);
        this.error ??= err instanceof Error ? err : new Error(String(err));
      },
    );
  }

  onChunkUnload(coords: ChunkCoord[]): void {
    for (const coord of coords) {
      const key = chunkKey(coord);
      this.forget(key, coord);
      this.resyncing.delete(key);
      if (this.loaded.delete(key) && !this.air.delete(key)) this.store.removeChunk(coord);
      this.dirty.delete(key);
      this.meshing.delete(key);
      if (this.meshed.delete(key)) this.view.setTerrainChunk(key, [0, 0, 0], null);
    }
  }

  /** A VoxelModification's chunks (§6.5): applied in revision order. */
  onVoxelModification(chunks: ChunkChanges[]): void {
    for (const changes of chunks) {
      const key = chunkKey(changes.coord);
      if (this.generating.has(key)) {
        const held = this.heldEdits.get(key) ?? [];
        held.push(changes);
        this.heldEdits.set(key, held);
      } else if (this.loaded.has(key)) {
        this.apply(key, changes);
      }
      // Otherwise the chunk is not ours (unloaded meanwhile): the server streams it anew.
    }
  }

  isLoaded(coord: ChunkCoord): boolean {
    return this.loaded.has(chunkKey(coord));
  }

  /** Revision of a loaded chunk (Air: 0), or null. */
  revision(coord: ChunkCoord): number | null {
    return this.loaded.get(chunkKey(coord))?.revision ?? null;
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
   * Starts mesh jobs for changed chunks, nearest to `center` first, while the mesher has room and
   * for up to `budgetMs` (at least one). A chunk waits while a neighbour is still being generated,
   * so it is meshed once with its borders known rather than again when each neighbour arrives.
   * Returns the number of jobs started.
   */
  meshDirty(center: Vec3, budgetMs: number, now: () => number = () => performance.now()): number {
    const room = (this.mesher.capacity ?? DEFAULT_MESH_JOBS) - this.meshing.size;
    if (this.dirty.size === 0 || room <= 0) return 0;
    const deadline = now() + budgetMs;
    const distance = (c: ChunkCoord): number =>
      ((c[0] + 0.5) * CHUNK_SIZE - center[0]) ** 2 +
      ((c[1] + 0.5) * CHUNK_SIZE - center[1]) ** 2 +
      ((c[2] + 0.5) * CHUNK_SIZE - center[2]) ** 2;
    const ready = [...this.dirty.entries()]
      .filter(
        ([key, c]) =>
          !this.meshing.has(key) &&
          !NEIGHBOURS.some((d) => this.generating.has(chunkKey(add(c, d)))),
      )
      .sort((a, b) => distance(a[1]) - distance(b[1]));
    let count = 0;
    for (const [key, c] of ready) {
      if (count >= room) break;
      this.dirty.delete(key);
      const token = ++this.token;
      this.meshing.set(key, token);
      void this.mesher.mesh(this.store.paddedChunk(c[0], c[1], c[2])).then((meshes) => {
        if (this.meshing.get(key) !== token) return; // unloaded meanwhile
        this.meshing.delete(key);
        if (!this.loaded.has(key) || this.air.has(key)) return;
        this.view.setTerrainChunk(
          key,
          [c[0] * CHUNK_SIZE, c[1] * CHUNK_SIZE, c[2] * CHUNK_SIZE],
          meshes,
        );
        this.meshed.add(key);
      });
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
      meshing: this.meshing.size,
      resyncs: this.resyncs,
    };
  }

  private put(key: string, coord: ChunkCoord, revision: number, voxels: Uint16Array): void {
    this.store.setChunk(coord, revision, voxels);
    this.air.delete(key);
    this.loaded.set(key, { coord, revision });
    this.dirty.set(key, coord);
    this.neighboursChanged(coord);
  }

  /** An all-air chunk: the client sim reads a missing chunk as air, so nothing is stored. */
  private putAir(key: string, coord: ChunkCoord): void {
    const replaced = this.loaded.has(key) && !this.air.has(key);
    if (replaced) this.store.removeChunk(coord);
    this.loaded.set(key, { coord, revision: 0 });
    this.air.add(key);
    this.dirty.delete(key);
    this.meshing.delete(key);
    if (this.meshed.delete(key)) this.view.setTerrainChunk(key, [0, 0, 0], null);
    if (replaced) this.neighboursChanged(coord);
  }

  /** Applies one chunk's changes if they are the next revision; a gap asks for a resync. */
  private apply(key: string, changes: ChunkChanges): void {
    const entry = this.loaded.get(key);
    if (!entry || this.resyncing.has(key)) return; // a resync brings the current state
    if (changes.revision <= entry.revision) return; // stale
    if (changes.revision !== entry.revision + 1) {
      this.resyncing.add(key);
      this.resyncs++;
      this.resync([changes.coord]);
      return;
    }
    if (this.air.delete(key)) this.store.setChunk(changes.coord, 0, new Uint16Array(CHUNK_VOLUME));
    this.store.editChunk(changes.coord, changes.revision, changes.changes);
    entry.revision = changes.revision;
    this.dirty.set(key, changes.coord);
    // Neighbours whose border faces the changed voxels touch.
    const touched = new Set<number>();
    for (let i = 0; i < changes.changes.length; i += 2) {
      const index = changes.changes[i] ?? 0;
      const local = [index & 31, (index >> 5) & 31, (index >> 10) & 31];
      for (let axis = 0; axis < 3; axis++) {
        if (local[axis] === 0) touched.add(axis * 2 + 1);
        if (local[axis] === CHUNK_SIZE - 1) touched.add(axis * 2);
      }
    }
    for (const n of touched) {
      const d = NEIGHBOURS[n];
      if (!d) continue;
      const c = add(changes.coord, d);
      const nk = chunkKey(c);
      if (this.loaded.has(nk) && !this.air.has(nk)) this.dirty.set(nk, c);
    }
  }

  /** Neighbours' border faces may now be hidden (or exposed) by this chunk. */
  private neighboursChanged(coord: ChunkCoord): void {
    for (const d of NEIGHBOURS) {
      const n = add(coord, d);
      const nk = chunkKey(n);
      if (this.loaded.has(nk) && !this.air.has(nk)) this.dirty.set(nk, n);
    }
  }

  /** Drops a pending generation of the chunk (the new message or an unload supersedes it). */
  private forget(key: string, coord: ChunkCoord): void {
    this.heldEdits.delete(key);
    if (this.generating.delete(key)) this.source.cancel(coord);
  }
}

function add(a: ChunkCoord, b: ChunkCoord): ChunkCoord {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}
