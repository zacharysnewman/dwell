import { buildInfo, formatBuildInfo } from './buildInfo';
import { createRenderer, RendererUnavailableError, type Renderer } from './render';

function element<T extends HTMLElement>(id: string, type: new () => T): T {
  const el = document.getElementById(id);
  if (!(el instanceof type)) throw new Error(`Missing #${id}`);
  return el;
}

function showFatal(message: string): void {
  const fatal = element('fatal', HTMLDivElement);
  fatal.textContent = message;
  fatal.hidden = false;
}

function start(): void {
  element('build-info', HTMLDivElement).textContent = formatBuildInfo(buildInfo);
  const canvas = element('view', HTMLCanvasElement);

  let renderer: Renderer;
  try {
    renderer = createRenderer(canvas);
  } catch (err) {
    showFatal(err instanceof RendererUnavailableError ? err.message : 'Dwell failed to start.');
    throw err;
  }

  const resize = (): void => {
    renderer.resize(canvas.clientWidth, canvas.clientHeight, Math.min(window.devicePixelRatio, 2));
  };
  new ResizeObserver(resize).observe(canvas);
  resize();

  let last = performance.now();
  const frame = (now: number): void => {
    renderer.renderFrame((now - last) / 1000);
    last = now;
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}

start();
