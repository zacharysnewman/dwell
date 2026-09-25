import type { Renderer } from './Renderer';
import { ThreeRenderer } from './three/ThreeRenderer';

export type { PlayerView, Renderer } from './Renderer';
export { RendererUnavailableError } from './Renderer';

/** Creates the renderer for this platform (Three.js on WebGL2, ADR 0002). */
export function createRenderer(canvas: HTMLCanvasElement): Renderer {
  return new ThreeRenderer(canvas);
}
