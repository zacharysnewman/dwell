import type { Renderer, RendererOptions } from './Renderer';
import { ThreeRenderer } from './three/ThreeRenderer';

export type { PlayerView, Renderer, RendererOptions, RenderStats } from './Renderer';
export { RendererUnavailableError } from './Renderer';

/** Creates the renderer for this platform (Three.js on WebGL2, ADR 0002). */
export function createRenderer(canvas: HTMLCanvasElement, options: RendererOptions = {}): Renderer {
  return new ThreeRenderer(canvas, options);
}
