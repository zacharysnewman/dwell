import type { ChunkMeshes } from '../mesh/mesher';
import type { Vec3 } from '../protocol/messages';

/** A player drawn by the renderer (remote players, and the local one when dead). */
export interface PlayerView {
  /** Feet position. */
  feet: Vec3;
  /** Degrees; 0 = +Z. */
  yaw: number;
  crouched: boolean;
  /** Lying on the ground (cosmetic death pose). */
  dead: boolean;
  color: number;
  radius: number;
  height: number;
}

/**
 * Dwell's render interface (ARCHITECTURE.md §5, ADR 0002). Game code talks only to this; the
 * Three.js implementation lives in ./three and is the only code allowed to import three.
 *
 * Phase 2 adds terrain chunk meshes (greedy-meshed in workers since Phase 3d), player capsules, the
 * first-person camera, and debug lines; Phase 3d the targeted-block outline. Dynamic body meshes arrive with Tier 1 bodies (Phase 5).
 */
export interface Renderer {
  /** Resize the drawing buffer to CSS pixels × device pixel ratio. */
  resize(width: number, height: number, pixelRatio: number): void;
  /** Draw one frame. `dtSeconds` is the time since the previous frame. */
  renderFrame(dtSeconds: number): void;
  /**
   * Replaces (or, with null, removes) a terrain chunk: `meshes` from the meshing workers
   * (mesh/mesher.ts), chunk-local; `origin` is the chunk's min corner.
   */
  setTerrainChunk(key: string, origin: Vec3, meshes: ChunkMeshes | null): void;
  /** Outlines the targeted block (§6.5): its min corner and box height (slabs 0.5), or none. */
  setBlockOutline(cell: Vec3 | null, height?: number): void;
  /** Adds, updates, or (null) removes a player. */
  setPlayer(id: number, view: PlayerView | null): void;
  /** Places the camera at `eye`, looking along yaw/pitch (degrees; yaw 0 = +Z, pitch up > 0). */
  setCamera(eye: Vec3, yawDeg: number, pitchDeg: number): void;
  /** Debug line segments (pairs of points) with one colour each, or null to clear. */
  setDebugLines(segments: readonly { from: Vec3; to: Vec3; color: number }[] | null): void;
  /** Release GPU resources. */
  dispose(): void;
}

export class RendererUnavailableError extends Error {
  override name = 'RendererUnavailableError';
}
