// The ray marcher's dev page (`?marchtest=1`, local builds only; docs/RAY_MARCHED_RENDERER.md,
// Phase 25 A3b): a generated world drawn by the marcher alone, before the renderer seam wires it
// into the game. `window.__march` places the camera and reads the marcher's numbers (the bench and
// screenshots use it). Query: `seed`, `chunks` (chunk radius), `sections` (section radius),
// `levels` (top LOD level), `x,y,z,yaw,pitch` (the camera; default above the spawn).
import { MarchTest } from '../engine/wasm';

export interface MarchHooks {
  setCamera(x: number, y: number, z: number, yaw: number, pitch: number): void;
  configure(stepCap: number, lodPixels: number, columns: boolean, distances: boolean): void;
  spawn(): number[];
  pending(): number;
  stats(): unknown;
  telemetryReset(): void;
}

declare global {
  interface Window {
    __march?: MarchHooks;
  }
}

export async function startMarchTest(canvas: HTMLCanvasElement, search: string): Promise<void> {
  const params = new URLSearchParams(search);
  const num = (name: string, fallback: number) => {
    const v = Number(params.get(name));
    return params.has(name) && Number.isFinite(v) ? v : fallback;
  };
  const size = () => {
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.round(canvas.clientWidth * ratio));
    canvas.height = Math.max(1, Math.round(canvas.clientHeight * ratio));
  };
  size();
  const test = await MarchTest.create(
    canvas,
    () => new Worker(new URL('../engine/jobWorker.ts', import.meta.url), { type: 'module' }),
    BigInt(num('seed', 7)),
    num('workers', 4),
    num('chunks', 6),
    num('sections', 6),
    num('levels', 12),
  );
  const [sx = 0, sy = 0, sz = 0] = Array.from(test.spawn());
  const spawn = [sx, sy, sz] as const;
  test.setCamera(
    num('x', spawn[0]),
    num('y', spawn[1] + 30),
    num('z', spawn[2]),
    num('yaw', 0),
    num('pitch', -20),
  );
  window.__march = {
    setCamera: (x, y, z, yaw, pitch) => {
      test.setCamera(x, y, z, yaw, pitch);
    },
    configure: (cap, px, columns, distances) => {
      test.configure(cap, px, columns, distances);
    },
    spawn: () => Array.from(test.spawn()),
    pending: () => test.pending(),
    stats: () => JSON.parse(test.stats()) as unknown,
    telemetryReset: () => {
      test.telemetryReset();
    },
  };
  const status = document.createElement('div');
  status.style.cssText =
    'position:fixed;left:8px;top:8px;font:12px monospace;color:#fff;background:#0008;padding:4px 6px;white-space:pre';
  if (params.get('hud') !== '0') document.body.append(status);
  let last = 0;
  const loop = (now: number) => {
    size();
    test.resize(canvas.width, canvas.height);
    test.frame(now);
    if (now - last > 500) {
      last = now;
      status.textContent = `march test  ${test.stats()}`;
    }
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}
