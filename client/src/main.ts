import { buildInfo, formatBuildInfo } from './buildInfo';
import { Game, type GameDebugState } from './game/game';
import { BlockInteraction, PALETTE, type EditAction } from './interact/blockInteraction';
import { MeshPool } from './mesh/pool';
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
import type { ChunkCoord } from './protocol/messages';
import { Hotbar, slotForKey } from './ui/hotbar';
import { Hud } from './ui/hud';
import { MAP_SIZE, MAP_STEP, MapOverlay } from './ui/mapOverlay';
import { countChanged } from './world/chunkDiff';
import { formatStatus } from './ui/statusOverlay';
import { ChunkStreamer } from './world/chunkStream';
import { WorldgenPool } from './worldgen/pool';
import { LodSystem } from './lod/lodSystem';
import { Lod } from './protocol/constants.gen';
import { verticalFov } from './render/fov';

/** Hooks for automated tests (Playwright) and debugging from the console. */
interface DwellDebug {
  state(): GameDebugState | null;
  press(code: string, down: boolean): void;
  look(yaw: number, pitch: number): void;
  view(): { yaw: number; pitch: number };
  /** Breaks or places at the crosshair, as a click does (§6.5). */
  edit(action: EditAction): boolean;
  /** Selects a hotbar slot, as a number key does. */
  select(slot: number): void;
  /** Material at a voxel in the client's world. */
  voxel(x: number, y: number, z: number): number;
  /** Turns creative flight on or off, as double-tapping Space does (if the server allows it). */
  fly(on: boolean): void;
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
  touch: TouchControls;
  hud: Hud;
  map: MapOverlay;
  game: Game | null;
  interaction: BlockInteraction | null;
  core: ClientCore | null;
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

  const input = new KeyboardMouseInput(canvas);
  // On-screen controls on touch devices (shown on the first touch too, e.g. a tablet with a mouse).
  const touch = new TouchControls(document.body, input);
  const app: App = {
    canvas,
    renderer,
    input,
    touch,
    hud: new Hud(document.body),
    map: new MapOverlay(document.body),
    game: null,
    interaction: null,
    core: null,
  };
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
    if (key === 'F4') app.map.toggle();
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
    edit: (action) => app.game?.edit(action, performance.now()) ?? false,
    select: (slot) => app.interaction?.select(slot),
    voxel: (x, y, z) => app.core?.voxel(x, y, z) ?? 0,
    fly: (on) => {
      app.input.flight.set(on);
    },
  };
  // Block interaction (§6.5): clicks and taps edit, number keys, the wheel and the hotbar select.
  app.input.onAction = (action) => app.game?.edit(action, performance.now());
  app.input.onDigit = (code) => {
    const slot = slotForKey(code);
    if (slot !== null && slot < PALETTE.length) app.interaction?.select(slot);
  };
  app.input.onScroll = (delta) => app.interaction?.scroll(delta);
  // Creative flight (§8.3): double-tap Space or Jump, or the Fly button.
  touch.onDebug = () => {
    app.hud.toggleDebug();
  };
  touch.onFly = () => {
    input.flight.toggle();
  };
  touch.onJumpPress = (nowMs) => {
    input.flight.jumpPressed(nowMs);
  };
  input.flight.onChange = (flying) => {
    touch.setFlight(input.flight.allowed, flying);
  };
  touch.onTap = () => {
    const action = app.interaction?.touchAction;
    if (action) app.game?.edit(action, performance.now());
  };
  touch.onPlaceMode = (place) => {
    if (app.interaction) app.interaction.touchAction = place ? 'place' : 'break';
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
  // Creative flight is the server's to allow (Welcome, §8.3).
  app.input.flight.allowed = joined.mayFly;
  app.touch.setFlight(joined.mayFly, app.input.flight.flying);
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
    const meshPool = MeshPool.create();
    const terrain = new ChunkStreamer(core, pool, meshPool, app.renderer, (coords) => {
      session.sendControl({ type: MessageType.ChunkResync, coords });
    });
    const interaction = new BlockInteraction(core, (m) => {
      session.sendControl(m);
    });
    const hotbar = new Hotbar(document.body, PALETTE, (slot) => {
      interaction.select(slot);
    });
    interaction.onSelect = (slot) => {
      hotbar.setSelected(slot);
    };
    interaction.select(0);
    app.interaction = interaction;
    app.core = core;
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
      interaction,
      (x, y, z) => core.voxel(x, y, z),
    );
    startDebugTools(app, pool, core, terrain, game);
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

    // The whole-world view (§6.6): generated here unless in full-chunk mode (?lod=0 turns it off).
    const params = new URLSearchParams(location.search);
    if (params.get('lod') !== '0') {
      const mobile = prefersTouch();
      const lod = new LodSystem(
        pool,
        meshPool,
        app.renderer,
        terrain,
        (sections) => {
          session.sendControl({ type: MessageType.LodRequest, sections });
        },
        {
          pixelError: mobile ? Lod.pixelErrorMobile : Lod.pixelErrorDesktop,
          cacheBytes: (mobile ? Lod.cacheMbMobile : Lod.cacheMbDesktop) * 1048576,
          maxGenerationJobs: pool.capacity,
          maxMeshJobs: meshPool.capacity,
          // Comparing ways to draw distant water: ?lodwater=tint recolours the floor instead.
          coarseLiquids: params.get('lodwater') === 'tint' ? 'tint' : 'opaque',
        },
      );
      lod.setFullMode(hash === 0n);
      app.renderer.setChunkVisibility((c) => lod.chunkVisible(c));
      app.renderer.setLodLevelColors(params.get('lodcolors') === '1');
      game.viewport = () => {
        const aspect = app.canvas.clientWidth / Math.max(1, app.canvas.clientHeight);
        return {
          fovYDeg: verticalFov(aspect),
          aspect,
          heightPx: app.canvas.clientHeight * Math.min(window.devicePixelRatio, 2),
        };
      };
      game.lod = lod;
    }
  })();
}

/**
 * Debug tooling (Phase 3e): the F4 terrain map around the player, and in the F3 overlay the
 * player's chunk regenerated and diffed against the one the world holds.
 */
function startDebugTools(
  app: App,
  pool: WorldgenPool,
  core: ClientCore,
  terrain: ChunkStreamer,
  game: Game,
): void {
  let busy = false;
  setInterval(() => {
    const s = game.debugState();
    if (busy || !s.active) return;
    const [x, y, z] = s.feet.map(Math.floor) as [number, number, number];
    const jobs: Promise<void>[] = [];
    if (app.map.visible) {
      const half = (MAP_SIZE / 2) * MAP_STEP;
      jobs.push(
        pool.map({ x0: x - half, z0: z - half, step: MAP_STEP, n: MAP_SIZE }).then((bytes) => {
          app.map.draw(bytes, app.input.yaw);
        }),
      );
    }
    if (app.hud.debugVisible) {
      const c: ChunkCoord = [x >> 5, y >> 5, z >> 5];
      const revision = terrain.revision(c);
      if (revision === null) {
        game.debugNote = '';
      } else {
        jobs.push(
          pool.generate(c).then((generated) => {
            const changed = countChanged(core.paddedChunk(...c), generated.voxels);
            game.debugNote = `chunk (${c.join(', ')}) rev ${String(revision)} · ${String(changed)} voxels differ from generation`;
          }),
        );
      }
    }
    busy = true;
    void Promise.allSettled(jobs).then(() => {
      busy = false;
    });
  }, 1000);
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
