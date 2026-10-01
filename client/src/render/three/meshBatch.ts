// Many static meshes of one material in one three.js BatchedMesh (ARCHITECTURE.md §6.6): drawn
// with one draw call per render pass (WEBGL_multi_draw, which three.js uses where the browser has
// it) instead of one per mesh, culled per member against each pass's camera and sorted (back to
// front when see-through). Used for the LOD sections' water always, and for the chunks and the LOD
// sections when batching is on (?batch=1). Space grows as needed, and deleted members' space is
// reclaimed by repacking before growing. A batch keeps a CPU copy of its geometry (for partial
// uploads), unlike the separate meshes, whose vertex data leaves the page once uploaded.
import { BatchedMesh, type BufferGeometry, type Color, type Material, Matrix4 } from 'three';

export interface BatchHandle {
  geometry: number;
  instance: number;
  vertices: number;
  indices: number;
}

export interface MeshBatchOptions {
  /** Initial space (instances, vertices, indices); it grows as needed. */
  instances?: number;
  vertices?: number;
  indices?: number;
}

const matrix = new Matrix4();

export class MeshBatch {
  readonly mesh: BatchedMesh;
  private instances: number;
  private vertices: number;
  private indices: number;
  /** Space of live geometries, and the end of the space used (deleted space included). */
  private usedVertices = 0;
  private usedIndices = 0;
  private endVertices = 0;
  private endIndices = 0;
  private live = 0;

  constructor(material: Material, options: MeshBatchOptions = {}) {
    this.instances = options.instances ?? 64;
    this.vertices = options.vertices ?? 1 << 17;
    this.indices = options.indices ?? 1 << 18;
    this.mesh = new BatchedMesh(this.instances, this.vertices, this.indices, material);
    // Sorted per pass, and culled per member. Not as a whole: three.js caches a BatchedMesh's
    // bounds on its first check, and members come and go.
    this.mesh.sortObjects = true;
    this.mesh.perObjectFrustumCulled = true;
    this.mesh.frustumCulled = false;
  }

  /** Members in the batch. */
  get count(): number {
    return this.live;
  }

  /** Adds a mesh at `origin`, scaled by `scale` (hidden until shown). `geometry` is copied. */
  add(geometry: BufferGeometry, origin: readonly [number, number, number], scale = 1): BatchHandle {
    const vertices = geometry.getAttribute('position').count;
    const indices = geometry.getIndex()?.count ?? 0;
    if (this.live >= this.instances) {
      this.instances *= 2;
      this.mesh.setInstanceCount(this.instances);
    }
    if (this.endVertices + vertices > this.vertices || this.endIndices + indices > this.indices) {
      // Reclaim deleted members' space first; grow only if it is still short.
      this.mesh.optimize();
      this.endVertices = this.usedVertices;
      this.endIndices = this.usedIndices;
      if (this.endVertices + vertices > this.vertices || this.endIndices + indices > this.indices) {
        while (this.endVertices + vertices > this.vertices) this.vertices *= 2;
        while (this.endIndices + indices > this.indices) this.indices *= 2;
        this.mesh.setGeometrySize(this.vertices, this.indices);
      }
    }
    const g = this.mesh.addGeometry(geometry);
    const instance = this.mesh.addInstance(g);
    matrix.makeScale(scale, scale, scale).setPosition(origin[0], origin[1], origin[2]);
    this.mesh.setMatrixAt(instance, matrix);
    this.mesh.setVisibleAt(instance, false);
    this.usedVertices += vertices;
    this.usedIndices += indices;
    this.endVertices += vertices;
    this.endIndices += indices;
    this.live++;
    return { geometry: g, instance, vertices, indices };
  }

  remove(h: BatchHandle): void {
    this.mesh.deleteGeometry(h.geometry); // and its instance
    this.usedVertices -= h.vertices;
    this.usedIndices -= h.indices;
    this.live--;
  }

  setVisible(h: BatchHandle, visible: boolean): void {
    this.mesh.setVisibleAt(h.instance, visible);
  }

  isVisible(h: BatchHandle): boolean {
    return this.mesh.getVisibleAt(h.instance);
  }

  /** Multiplies the member's vertex colours (the debug per-level tints). */
  setColor(h: BatchHandle, color: Color): void {
    this.mesh.setColorAt(h.instance, color);
  }
}
