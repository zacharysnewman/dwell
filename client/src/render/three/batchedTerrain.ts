// The terrain in three batches (ARCHITECTURE.md §6.6, `?batch=1`): every chunk's blocks, every
// chunk's water, and every LOD section's surface and skirts, each a MeshBatch drawn with one call
// per pass, instead of a mesh (and draw call) per chunk and per section. A section's surface and
// its six skirts are separate members, so showing a side's skirt is a visibility flag. Members
// change visibility only when the chunks hidden by the LOD or the sections shown change.
import { BufferAttribute, BufferGeometry, Color, type Material, type Object3D } from 'three';
import type { FlatMesh, SectionMeshes } from '../../mesh/lodMesher';
import type { ChunkMeshes, MeshArrays } from '../../mesh/mesher';
import type { ChunkCoord, Vec3 } from '../../protocol/messages';
import { type BatchHandle, MeshBatch } from './meshBatch';

function chunkGeometry(arrays: MeshArrays): BufferGeometry | null {
  if (arrays.indices.length === 0) return null;
  const g = new BufferGeometry();
  g.setAttribute('position', new BufferAttribute(arrays.positions, 3));
  g.setAttribute('normal', new BufferAttribute(arrays.normals, 3));
  g.setAttribute('color', new BufferAttribute(arrays.colors, 3));
  g.setAttribute('uv', new BufferAttribute(arrays.uvs, 2));
  g.setAttribute('tile', new BufferAttribute(arrays.tiles, 4));
  g.setIndex(new BufferAttribute(arrays.indices, 1));
  return g;
}

function flatGeometry(m: FlatMesh): BufferGeometry | null {
  if (m.indices.length === 0) return null;
  const g = new BufferGeometry();
  g.setAttribute('position', new BufferAttribute(m.positions, 3));
  g.setAttribute('normal', new BufferAttribute(m.normals, 3));
  g.setAttribute('color', new BufferAttribute(m.colors, 3));
  g.setIndex(new BufferAttribute(m.indices, 1));
  return g;
}

interface ChunkEntry {
  coord: ChunkCoord;
  opaque: BatchHandle | null;
  water: BatchHandle | null;
  visible: boolean;
}

interface SectionEntry {
  level: number;
  /** The surface (0) and the skirt of each side (1 + face index); null where empty. */
  parts: (BatchHandle | null)[];
  /** The sides whose skirts show (bit per face), or -1 while the section is hidden. */
  mask: number;
}

const WHITE = new Color(0xffffff);

export class BatchedTerrain {
  private readonly chunkOpaque: MeshBatch;
  private readonly chunkWater: MeshBatch;
  private readonly lodOpaque: MeshBatch;
  private readonly chunks = new Map<string, ChunkEntry>();
  private readonly sections = new Map<number, SectionEntry>();
  private levelTints: Color[] | null = null;

  constructor(materials: { chunk: Material; chunkWater: Material; lod: Material }) {
    this.chunkOpaque = new MeshBatch(materials.chunk, { instances: 512 });
    this.chunkWater = new MeshBatch(materials.chunkWater);
    this.chunkWater.mesh.renderOrder = 1;
    this.lodOpaque = new MeshBatch(materials.lod, { instances: 2048 });
  }

  /** The batches, to add to the scene. */
  get meshes(): Object3D[] {
    return [this.chunkOpaque.mesh, this.chunkWater.mesh, this.lodOpaque.mesh];
  }

  /** Bytes reserved by the three batches (on the GPU, and copied in the page). */
  get bytes(): number {
    return this.chunkOpaque.bytes + this.chunkWater.bytes + this.lodOpaque.bytes;
  }

  /** Members per batch (tests and the debug overlay). */
  get counts(): { chunks: number; chunkWater: number; lod: number } {
    return {
      chunks: this.chunkOpaque.count,
      chunkWater: this.chunkWater.count,
      lod: this.lodOpaque.count,
    };
  }

  /** Adds, replaces or (null) removes a chunk's meshes; shown until update() hides it. */
  setChunk(key: string, origin: Vec3, coord: ChunkCoord, meshes: ChunkMeshes | null): void {
    const old = this.chunks.get(key);
    if (old) {
      if (old.opaque) this.chunkOpaque.remove(old.opaque);
      if (old.water) this.chunkWater.remove(old.water);
      this.chunks.delete(key);
    }
    if (!meshes) return;
    const add = (batch: MeshBatch, arrays: MeshArrays): BatchHandle | null => {
      const g = chunkGeometry(arrays);
      if (!g) return null;
      const h = batch.add(g, origin);
      batch.setVisible(h, true);
      g.dispose();
      return h;
    };
    const opaque = add(this.chunkOpaque, meshes.opaque);
    const water = add(this.chunkWater, meshes.transparent);
    if (!opaque && !water) return;
    this.chunks.set(key, { coord, opaque, water, visible: true });
  }

  /** Adds, replaces or (null) removes a section's surface and skirts (hidden until shown). */
  setLodSection(id: number, origin: Vec3, cellSize: number, meshes: SectionMeshes | null): void {
    const old = this.sections.get(id);
    if (old) {
      for (const h of old.parts) if (h) this.lodOpaque.remove(h);
      this.sections.delete(id);
    }
    if (!meshes) return;
    const level = Math.round(Math.log2(cellSize));
    const parts = [meshes.opaque, ...meshes.skirts].map((m) => {
      const g = flatGeometry(m);
      if (!g) return null;
      const h = this.lodOpaque.add(g, origin, cellSize);
      g.dispose();
      const tint = this.levelTints?.[level];
      if (tint) this.lodOpaque.setColor(h, tint);
      return h;
    });
    if (parts.every((h) => h === null)) return;
    this.sections.set(id, { level, parts, mask: -1 });
  }

  /**
   * Shows the chunks `chunkVisible` allows (all if null) and the sections in `shown` with the
   * skirts of the sides in their masks; everything else is hidden.
   */
  update(
    chunkVisible: ((coord: ChunkCoord) => boolean) | null,
    shown: ReadonlyMap<number, number>,
  ): void {
    for (const c of this.chunks.values()) {
      const visible = !chunkVisible || chunkVisible(c.coord);
      if (visible === c.visible) continue;
      c.visible = visible;
      if (c.opaque) this.chunkOpaque.setVisible(c.opaque, visible);
      if (c.water) this.chunkWater.setVisible(c.water, visible);
    }
    for (const [id, s] of this.sections) {
      const mask = shown.get(id) ?? -1;
      if (mask === s.mask) continue;
      s.mask = mask;
      s.parts.forEach((h, i) => {
        if (!h) return;
        const on = mask >= 0 && (i === 0 || (mask & (1 << (i - 1))) !== 0);
        this.lodOpaque.setVisible(h, on);
      });
    }
  }

  /** Debug: tints sections by level (null: no tints). */
  setLevelTints(tints: readonly number[] | null): void {
    this.levelTints = tints ? tints.map((c) => new Color(c)) : null;
    for (const s of this.sections.values()) {
      const tint = this.levelTints?.[s.level] ?? WHITE;
      for (const h of s.parts) if (h) this.lodOpaque.setColor(h, tint);
    }
  }

  /** Members the batches draw (tests): chunks, section surfaces, and skirts. */
  visible(): { chunks: number; sections: number; skirts: number } {
    let chunks = 0;
    let sections = 0;
    let skirts = 0;
    for (const c of this.chunks.values()) {
      if (c.opaque && this.chunkOpaque.isVisible(c.opaque)) chunks++;
    }
    for (const s of this.sections.values()) {
      s.parts.forEach((h, i) => {
        if (!h || !this.lodOpaque.isVisible(h)) return;
        if (i === 0) sections++;
        else skirts++;
      });
    }
    return { chunks, sections, skirts };
  }
}
