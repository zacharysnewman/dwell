import { buildInfo, formatBuildInfo } from './buildInfo';
import { Game, type GameDebugState } from './game/game';
import { connectLocal, connectToInvite, type ConnectOptions } from './net/connect';
import { parseInvite } from './net/invite';
import { parseLocalWorld } from './local/world';
import { parseNetConditions } from './net/netsim';
import type { ClientSession, SessionState, SessionStats } from './net/session';
import { KeyboardMouseInput } from './predict/input';
import { prefersTouch, TouchControls } from './predict/touch';
import { createRenderer, RendererUnavailableError, type Renderer } from './render';
import { ClientCore } from './sim/clientCore';
import { importDwellCore } from './sim/module';
import { MessageType } from './protocol/constants.gen';
import { Hud } from './ui/hud';
import { formatStatus } from './ui/statusOverlay';
import { ChunkStreamer } from './world/chunkStream';
import { WorldgenPool } from './worldgen/pool';

/** Hooks for automated tests (Playwright) and debugging from the console. */
interface DwellDebug {
  state(): GameDebugState | null;
  press(code: string, down: boolean): void;
  look(yaw: number, pitch: number): void;
  view(): { yaw: number; pitch: number };
}

declare global {
  interface Window {
    __dwell?: DwellDebug;
  }
}

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

interface App {
  canvas: HTMLCanvasElement;
  renderer: Renderer;
  input: KeyboardMouseInput;
  hud: Hud;
  game: Game | null;
}

function start(): App {
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

  const app: App = {
    canvas,
    renderer,
    input: new KeyboardMouseInput(canvas),
    hud: new Hud(document.body),
    game: null,
  };
  // On-screen controls on touch devices (shown on the first touch too, e.g. a tablet with a mouse).
  const touch = new TouchControls(document.body, app.input);
  touch.visible = prefersTouch();
  app.input.touch = touch.state;
  window.addEventListener('touchstart', () => (touch.visible = true), {
    once: true,
    passive: true,
  });
  // No F3 on a phone: ?debug=1 opens the debug overlay.
  if (new URLSearchParams(location.search).get('debug') === '1') app.hud.toggleDebug();
  app.input.onToggle = (key) => {
    if (key === 'F3') app.hud.toggleDebug();
  };
  window.__dwell = {
    state: () => app.game?.debugState() ?? null,
    press: (code, down) => {
      app.input.setKey(code, down);
    },
    look: (yaw, pitch) => {
      app.input.yaw = yaw;
      app.input.pitch = pitch;
    },
    view: () => ({ yaw: app.input.yaw, pitch: app.input.pitch }),
  };

  let last = performance.now();
  const frame = (now: number): void => {
    app.game?.frame(now);
    renderer.renderFrame((now - last) / 1000);
    last = now;
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  return app;
}

function displayName(): string {
  const fromUrl = new URLSearchParams(location.search).get('name');
  return fromUrl?.trim() ?? 'Player';
}

/**
 * Starts the game once the session has joined: the client's own sim core runs prediction, and the
 * worldgen workers generate the chunks the server streams as Generated (§6.3).
 */
function play(
  app: App,
  session: ClientSession,
  joined: Extract<SessionState, { phase: 'joined' }>,
) {
  void (async () => {
    // ?chunks=full asks the server to send every chunk explicitly (full-chunk mode).
    const fullChunks = new URLSearchParams(location.search).get('chunks') === 'full';
    const pool = WorldgenPool.create(joined.generatorVersion, joined.worldSeed);
    let core: ClientCore;
    try {
      core = await ClientCore.load(await importDwellCore());
    } catch (err) {
      app.hud.setMessage(
        `Simulation unavailable: ${err instanceof Error ? err.message : String(err)}`,
      );
      return;
    }
    let stats: SessionStats | null = null;
    session.subscribe((_state, s) => {
      stats = s;
    });
    const terrain = new ChunkStreamer(core, pool, app.renderer);
    const game = new Game(
      joined.playerId,
      core,
      app.renderer,
      app.input,
      app.hud,
      {
        send: (m) => {
          session.sendGameDatagram(m);
        },
        rttMs: () => stats?.datagramRttMs ?? stats?.rttMs ?? null,
      },
      terrain,
    );
    session.onGame((m, bytes) => {
      game.onGameMessage(m, bytes, performance.now());
    });
    app.game = game;

    // Verification (§6.3): the server streams Generated chunks only if our generator reproduces
    // its verification chunk bit for bit; a hash of 0 (or a failed worker) selects full-chunk mode.
    let hash = 0n;
    if (!fullChunks) {
      try {
        hash = (await pool.generate(joined.verificationChunk)).hash;
      } catch {
        hash = 0n;
      }
    }
    session.sendControl({ type: MessageType.WorldgenCheck, hash });
  })();
}

async function connect(app: App): Promise<void> {
  const status = element('net-status', HTMLDivElement);
  const params = new URLSearchParams(location.search);
  const invite = params.get('local') === '1' ? null : parseInvite(location.search);
  const target = invite ? `${invite.host}:${String(invite.port)}` : 'Local world';
  const forced = params.get('transport');
  const options: ConnectOptions = {
    displayName: displayName(),
    clientVersion: buildInfo.sha.slice(0, 12),
    transport: forced === 'webrtc' || forced === 'webtransport' ? forced : 'auto',
    netsim: parseNetConditions(params.get('netsim')),
    localWorld: parseLocalWorld(location.search),
  };
  status.textContent = invite ? `Connecting to ${target}…` : 'Starting local world…';
  try {
    const session = invite ? await connectToInvite(invite, options) : await connectLocal(options);
    let started = false;
    session.subscribe((state, stats) => {
      status.textContent = formatStatus(target, session.transportKind, state, stats);
      if (state.phase === 'joined' && !started) {
        started = true;
        play(app, session, state);
      }
    });
  } catch (err) {
    status.textContent = `Could not connect to ${target}: ${err instanceof Error ? err.message : String(err)}`;
  }
}

void connect(start());
