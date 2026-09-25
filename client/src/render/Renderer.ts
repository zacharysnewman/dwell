/**
 * Dwell's render interface (ARCHITECTURE.md §5, ADR 0002). Game code talks only to this; the
 * Three.js implementation lives in ./three and is the only code allowed to import three.
 *
 * Phase 0 covers the frame lifecycle. Chunk meshes, dynamic body meshes, player views, the
 * camera rig, and debug draw are added here as later phases need them.
 */
export interface Renderer {
  /** Resize the drawing buffer to CSS pixels × device pixel ratio. */
  resize(width: number, height: number, pixelRatio: number): void;
  /** Draw one frame. `dtSeconds` is the time since the previous frame. */
  renderFrame(dtSeconds: number): void;
  /** Release GPU resources. */
  dispose(): void;
}

export class RendererUnavailableError extends Error {
  override name = 'RendererUnavailableError';
}
