import type { SectionMeshes } from '../mesh/lodMesher';
import type { ChunkMeshes } from '../mesh/mesher';
import type { ChunkCoord, Vec3 } from '../protocol/messages';
import type { FogSettings } from './fog';

/** A player drawn by the renderer (remote players, and the local one when dead). */
export interface PlayerView {
  /** Feet position. */
  feet: Vec3;
  /** The face the player stands on (default +1): −1 draws it upside down, head toward −y. */
  face?: 1 | -1;
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
 * first-person camera, and debug lines; Phase 3d the targeted-block outline; Phase 4c LOD sections
 * and a two-pass depth split (far pass for LOD beyond LOD_NEAR_SPLIT_M, then a near pass). Dynamic body meshes arrive with Tier 1 bodies (Phase 14).
 */
/** How the renderer draws (main.ts, from the address: ?batch=1). */
export interface RendererOptions {
  /** Chunks and LOD sections in a few batches (one draw call each per pass), not a mesh each. */
  batched?: boolean;
}

/** The last frame's work (the debug overlay). */
export interface RenderStats {
  /** Draw calls and triangles over both passes. */
  calls: number;
  triangles: number;
  batched: boolean;
  /** Drawing-buffer pixels per CSS pixel (device pixel ratio × ?scale). */
  pixelRatio: number;
  /** GPU memory (bytes): terrain and LOD geometry (batches: their reserved space), and an
   *  estimate of the drawing buffers (multisampled colour and depth, and the resolved image). */
  meshBytes: number;
  screenBytes: number;
}

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
  /**
   * LOD sections (§6.6): adds, replaces or (null) removes a section's meshes (lodMesher.ts, in
   * cells from `origin`, scaled by `cellSize`). Nothing shows until `showLodSections` lists it.
   */
  setLodSection(id: number, origin: Vec3, cellSize: number, meshes: SectionMeshes | null): void;
  /** The LOD sections to draw from now on, each with the sides (bit per face) whose skirt shows. */
  showLodSections(visible: ReadonlyMap<number, number>): void;
  /**
   * Ancestor sections drawn clipped to boxes (world metres) where the traversal has nothing ready
   * (§6.6); `id` is the source section's. [] for none.
   */
  showLodStandIns(standIns: readonly { id: number; lo: Vec3; hi: Vec3 }[]): void;
  /** Which terrain chunks are drawn (LOD draws instead of the others); null: all of them. */
  setChunkVisibility(visible: ((coord: ChunkCoord) => boolean) | null): void;
  /** Debug: tint LOD sections by level. */
  setLodLevelColors(on: boolean): void;
  /** Height fog (§6.6): the haze's distance, density and scale height (render/fog.ts). */
  setFog(fog: FogSettings): void;
  /** Tone mapping exposure (render/look.ts): 1 leaves the lights as set. */
  setExposure(exposure: number): void;
  /** Outlines the targeted block (§6.5): its min corner and box height (slabs 0.5), or none. */
  setBlockOutline(cell: Vec3 | null, height?: number): void;
  /**
   * Outlines the shape about to be placed (SLOPE_BLOCKS.md §6): the cell's min corner and the
   * shape's edges (render/shapeEdges.ts, six floats per segment, cell coordinates), or none.
   */
  setPlacementPreview(cell: Vec3 | null, edges?: Float32Array): void;
  /** Adds, updates, or (null) removes a player. */
  setPlayer(id: number, view: PlayerView | null): void;
  /**
   * Places the camera at `eye`, looking along yaw/pitch (degrees; yaw 0 = +Z, pitch toward the
   * player's up > 0). `face` is the player's: +1 face A (up is +y), −1 face B (up is −y, the view
   * is upside down); the view and the sky, light and haze it sees follow it. `flip` (radians, 0 at
   * rest) pitches the view over about its right axis, nose first: the camera turning over when the
   * face changes (the heading has already turned by 180°, so a flip of π is the view before).
   */
  setCamera(eye: Vec3, yawDeg: number, pitchDeg: number, face?: 1 | -1, flip?: number): void;
  /** Debug line segments (pairs of points) with one colour each, or null to clear. */
  setDebugLines(segments: readonly { from: Vec3; to: Vec3; color: number }[] | null): void;
  /** The last frame's draw calls and triangles. */
  stats(): RenderStats;
  /** Release GPU resources. */
  dispose(): void;
}

export class RendererUnavailableError extends Error {
  override name = 'RendererUnavailableError';
}
