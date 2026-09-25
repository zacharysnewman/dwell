import { GridHelper, PerspectiveCamera, Scene, WebGLRenderer } from 'three';
import { RendererUnavailableError, type Renderer } from '../Renderer';

/** Three.js / WebGL2 implementation of the render interface (ADR 0002). */
export class ThreeRenderer implements Renderer {
  private readonly renderer: WebGLRenderer;
  private readonly scene = new Scene();
  private readonly camera = new PerspectiveCamera(70, 1, 0.1, 1000);
  private elapsed = 0;

  constructor(canvas: HTMLCanvasElement) {
    if (!canvas.getContext('webgl2')) {
      throw new RendererUnavailableError('WebGL2 is not available on this device.');
    }
    this.renderer = new WebGLRenderer({ canvas, antialias: true });
    this.renderer.setClearColor(0x87b5e0);

    // Phase 0 placeholder scene: a ground grid so a working renderer is visible.
    const grid = new GridHelper(64, 64, 0x3a4a5c, 0x5d7188);
    this.scene.add(grid);
    this.camera.position.set(0, 6, 14);
    this.camera.lookAt(0, 0, 0);
  }

  resize(width: number, height: number, pixelRatio: number): void {
    this.renderer.setPixelRatio(pixelRatio);
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / Math.max(1, height);
    this.camera.updateProjectionMatrix();
  }

  renderFrame(dtSeconds: number): void {
    // Slow orbit so the overlay shows frames are being produced.
    this.elapsed += dtSeconds;
    const angle = this.elapsed * 0.1;
    this.camera.position.set(Math.sin(angle) * 14, 6, Math.cos(angle) * 14);
    this.camera.lookAt(0, 0, 0);
    this.renderer.render(this.scene, this.camera);
  }

  dispose(): void {
    this.renderer.dispose();
  }
}
