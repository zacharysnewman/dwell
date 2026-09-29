// The LOD sections' water in one BatchedMesh (ARCHITECTURE.md §6.6): a see-through surface drawn
// per section would cost a draw call each (a quarter more calls in a coastal view); batched, the
// whole world's LOD water is one draw per render pass. Space grows as needed, and deleted
// sections' space is reclaimed by repacking before growing.
import { BatchedMesh, type BufferGeometry, type Material, Matrix4 } from 'three';

export interface WaterHandle {
  geometry: number;
  instance: number;
  vertices: number;
  indices: number;
}

const matrix = new Matrix4();

export class WaterBatch {
  readonly mesh: BatchedMesh;
  private instances = 64;
  private vertices = 1 << 17;
  private indices = 1 << 18;
  /** Space of live geometries, and the end of the space used (deleted space included). */
  private usedVertices = 0;
  private usedIndices = 0;
  private endVertices = 0;
  private endIndices = 0;
  private live = 0;

  constructor(material: Material) {
    this.mesh = new BatchedMesh(this.instances, this.vertices, this.indices, material);
    // Sorted back to front per pass, and culled per section. Not as a whole: three.js caches a
    // BatchedMesh's bounds on its first check, and sections come and go.
    this.mesh.sortObjects = true;
    this.mesh.perObjectFrustumCulled = true;
    this.mesh.frustumCulled = false;
  }

  /** Adds a section's water at `origin`, scaled by `cellSize` (hidden until shown). */
  add(
    geometry: BufferGeometry,
    origin: readonly [number, number, number],
    cellSize: number,
  ): WaterHandle {
    const vertices = geometry.getAttribute('position').count;
    const indices = geometry.getIndex()?.count ?? 0;
    if (this.live >= this.instances) {
      this.instances *= 2;
      this.mesh.setInstanceCount(this.instances);
    }
    if (this.endVertices + vertices > this.vertices || this.endIndices + indices > this.indices) {
      // Reclaim deleted sections' space first; grow only if it is still short.
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
    matrix.makeScale(cellSize, cellSize, cellSize).setPosition(origin[0], origin[1], origin[2]);
    this.mesh.setMatrixAt(instance, matrix);
    this.mesh.setVisibleAt(instance, false);
    this.usedVertices += vertices;
    this.usedIndices += indices;
    this.endVertices += vertices;
    this.endIndices += indices;
    this.live++;
    return { geometry: g, instance, vertices, indices };
  }

  remove(h: WaterHandle): void {
    this.mesh.deleteGeometry(h.geometry); // and its instance
    this.usedVertices -= h.vertices;
    this.usedIndices -= h.indices;
    this.live--;
  }

  setVisible(h: WaterHandle, visible: boolean): void {
    this.mesh.setVisibleAt(h.instance, visible);
  }
}
