import { buildInfo, formatBuildInfo } from './buildInfo';
import { connectLocal, connectToInvite, type ConnectOptions } from './net/connect';
import { parseInvite } from './net/invite';
import { createRenderer, RendererUnavailableError, type Renderer } from './render';
import { formatStatus } from './ui/statusOverlay';

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

function displayName(): string {
  const fromUrl = new URLSearchParams(location.search).get('name');
  return fromUrl?.trim() ?? 'Player';
}

async function connect(): Promise<void> {
  const status = element('net-status', HTMLDivElement);
  const params = new URLSearchParams(location.search);
  const invite = params.get('local') === '1' ? null : parseInvite(location.search);
  const target = invite ? `${invite.host}:${String(invite.port)}` : 'Local world';
  const forced = params.get('transport');
  const options: ConnectOptions = {
    displayName: displayName(),
    clientVersion: buildInfo.sha.slice(0, 12),
    transport: forced === 'webrtc' || forced === 'webtransport' ? forced : 'auto',
  };
  status.textContent = invite ? `Connecting to ${target}…` : 'Starting local world…';
  try {
    const session = invite ? await connectToInvite(invite, options) : await connectLocal(options);
    session.subscribe((state, stats) => {
      status.textContent = formatStatus(target, session.transportKind, state, stats);
    });
  } catch (err) {
    status.textContent = `Could not connect to ${target}: ${err instanceof Error ? err.message : String(err)}`;
  }
}

start();
void connect();
