import { buildInfo, formatBuildInfo } from './buildInfo';
import { Game, type GameDebugState } from './game/game';
import { BlockInteraction, PALETTE, type EditAction } from './interact/blockInteraction';
import { MeshPool } from './mesh/pool';
import {
  connectLocal,
  connectToInvite,
  connectToCode,
  openTransport,
  type ConnectOptions,
  type LocalSession,
} from './net/connect';
import { pingServer } from './net/statusPing';
import { enableHosting } from './hostWorld';
import { formatCode } from './net/joinCode';
import { configuredMasterUrl, MasterClient, serverInvite } from './net/master';
import { IndexedDbKeyStore, loadOrCreateDeviceKey } from './identity/deviceKey';
import { parseInvite } from './net/invite';
import { parseLocalWorld, type LocalWorld } from './local/world';
import { WorldIndex } from './local/worldIndex';
import { deleteWorldFiles, listWorldFiles, localWorldName } from './local/worldFiles';
import { launchOf, pastedInvite, withRoute } from './ui/launch';
import { MainMenu, type MainMenuDeps } from './ui/mainMenu';
import { loadRecent, rememberServer } from './ui/recentServers';
import { parseNetConditions } from './net/netsim';
import type { ClientSession, SessionState, SessionStats } from './net/session';
import { KeyboardMouseInput } from './predict/input';
import { prefersTouch, TouchControls } from './predict/touch';
import {
  createRenderer,
  RendererUnavailableError,
  type Renderer,
  type RenderStats,
} from './render';
import { displayOptions } from './render/display';
import { ClientCore } from './sim/clientCore';
import { importDwellCore } from './sim/module';
import { MessageType } from './protocol/constants.gen';
import type { ChunkCoord } from './protocol/messages';
import { Hotbar, slotForKey } from './ui/hotbar';
import { Hud } from './ui/hud';
import { MAP_SIZE, MAP_STEP, MapOverlay } from './ui/mapOverlay';
import { SettingsMenu } from './ui/settingsMenu';
import { FlightSpeedControl } from './ui/flightSpeedControl';
import { loadFlySpeedLevel, saveFlySpeedLevel } from './predict/flightSpeed';
import { countChanged } from './world/chunkDiff';
import { formatStatus } from './ui/statusOverlay';
import { FpsMeter } from './ui/fps';
import { formatMemory, jsHeapBytes, type MemoryReport } from './ui/memory';
import type { LoopbackTransport } from './net/loopback';
import { ChunkStreamer } from './world/chunkStream';
import { defaultWorkerCount, WorldgenPool } from './worldgen/pool';
import { LodSystem } from './lod/lodSystem';
import { defaultDetail } from './lod/detail';
import { lodViewport } from './lod/frustum';

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
  /** Sets the flight speed level, as the slider does. */
  flySpeed(level: number): void;
  /** The last frame's draw calls and triangles. */
  renderStats(): RenderStats;
  /** What the game knows it holds in memory (ui/memory.ts), once playing. */
  memory(): MemoryReport | null;
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
  settings: SettingsMenu | null;
  /** Frames per second, shown in the status line. */
  fps: FpsMeter;
  /** What the game knows it holds in memory (F3), once playing. */
  memory: (() => MemoryReport) | null;
}

function start(): App {
  element('build-info', HTMLDivElement).textContent = formatBuildInfo(buildInfo);
  const canvas = element('view', HTMLCanvasElement);

  // Comparing performance on a device: ?batch=1 and ?scale= (render/display.ts).
  const display = displayOptions(location.search);
  let renderer: Renderer;
  try {
    renderer = createRenderer(canvas, { batched: display.batched });
  } catch (err) {
    showFatal(err instanceof RendererUnavailableError ? err.message : 'Dwell failed to start.');
    throw err;
  }

  const resize = (): void => {
    renderer.resize(
      canvas.clientWidth,
      canvas.clientHeight,
      Math.min(window.devicePixelRatio, 2) * display.scale,
    );
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
    settings: null,
    fps: new FpsMeter(),
    memory: null,
  };
  touch.visible = prefersTouch();
  app.input.touch = touch.state;
  // Settings (top left): fog and full-detail distance, applied as the sliders move.
  app.settings = new SettingsMenu(document.body, prefersTouch(), (s) => {
    renderer.setFog(s.fog);
    app.game?.lod?.setDetailDistance(s.detail.distanceM);
    app.game?.lod?.setQuality(s.detail.pixelError, s.detail.memoryMb * 1048576);
  });
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
    flySpeed: (level) => {
      app.input.flight.speedLevel = level;
    },
    renderStats: () => renderer.stats(),
    memory: () => app.memory?.() ?? null,
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
  // Flight speed (§6.7): a slider while flying, the − and = keys, kept in this browser.
  const flightSpeed = new FlightSpeedControl(document.body, (level) => {
    input.flight.speedLevel = level;
  });
  input.flight.speedLevel = loadFlySpeedLevel();
  flightSpeed.show(input.flight.speedLevel);
  input.flight.onSpeedChange = (level) => {
    flightSpeed.show(level);
    saveFlySpeedLevel(level);
  };
  window.addEventListener('keydown', (e) => {
    // Not while typing in a text field (the slider itself still takes the keys).
    const typing = e.target instanceof HTMLInputElement && e.target.type !== 'range';
    if (!input.flight.flying || typing) return;
    if (e.code === 'Minus' || e.code === 'NumpadSubtract') input.flight.speedLevel -= 1;
    if (e.code === 'Equal' || e.code === 'NumpadAdd') input.flight.speedLevel += 1;
  });
  input.flight.onChange = (flying) => {
    touch.setFlight(input.flight.allowed, flying);
    flightSpeed.visible = flying;
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
    app.fps.frame(now);
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
  /** A local world's worker (its server core's memory), or null on a server. */
  loopback: LoopbackTransport | null,
) {
  // Creative flight is the server's to allow (Welcome, §8.3).
  app.input.flight.allowed = joined.mayFly;
  app.touch.setFlight(joined.mayFly, app.input.flight.flying);
  void (async () => {
    // ?chunks=full asks the server to send every chunk explicitly (full-chunk mode).
    const fullChunks = new URLSearchParams(location.search).get('chunks') === 'full';
    const workers = defaultWorkerCount(prefersTouch());
    const pool = WorldgenPool.create(joined.generatorVersion, joined.worldSeed, workers);
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
    const meshPool = MeshPool.create(workers);
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
    app.memory = () => {
      const render = app.renderer.stats();
      return {
        coreBytes: core.heapBytes(),
        serverBytes: loopback ? loopback.heapBytes : null,
        worldgenBytes: pool.heapBytes(),
        meshBytes: render.meshBytes,
        screenBytes: render.screenBytes,
        jsHeapBytes: jsHeapBytes(),
      };
    };
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
      const detail = app.settings?.current.detail ?? defaultDetail(prefersTouch());
      const lod = new LodSystem(
        pool,
        meshPool,
        app.renderer,
        terrain,
        (sections) => {
          session.sendControl({ type: MessageType.LodRequest, sections });
        },
        {
          pixelError: detail.pixelError,
          cacheBytes: detail.memoryMb * 1048576,
          maxGenerationJobs: pool.capacity,
          maxMeshJobs: meshPool.capacity,
          // Full detail beyond the streamed view: chunks asked for by the LOD (§6.6).
          requestChunks: (coords) => {
            session.sendControl({ type: MessageType.ChunkRequest, coords });
          },
        },
      );
      lod.setFullMode(hash === 0n);
      lod.setDetailDistance(detail.distanceM);
      app.renderer.setChunkVisibility((c) => lod.chunkVisible(c));
      app.renderer.setLodLevelColors(params.get('lodcolors') === '1');
      game.viewport = () => lodViewport(app.canvas.clientWidth, app.canvas.clientHeight);
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
    if (app.hud.debugVisible && app.memory) game.memoryNote = formatMemory(app.memory());
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

/** Local storage, or null where it is blocked. */
function storage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

/** Opens a route (a world or server, or `{}` for the main menu), keeping debug parameters. */
function go(route: Record<string, string>): void {
  location.assign(`${location.pathname}${withRoute(location.search, route)}`);
}

/** The main menu (Phase 5a): shown when the address names no world or server. */
/**
 * The master as the menu uses it: server addresses and games on this network (Phase 5d), and the
 * lobby list with its pings (Phase 5e).
 */
function menuMaster(): MainMenuDeps['master'] {
  const base = configuredMasterUrl();
  if (!base) return undefined;
  const client = loadOrCreateDeviceKey(new IndexedDbKeyStore()).then(
    (key) => new MasterClient(base, key),
  );
  return {
    resolveAddress: async (address) => {
      const found = await (await client).resolve({ address });
      if (found.kind !== 'server') throw new Error('That is not a server address.');
      return serverInvite(found.server);
    },
    nearby: async () => (await client).nearby(),
    lobby: async (query) => (await client).lobby(query),
    ping: async (server) => {
      const invite = parseInvite(`?${new URLSearchParams(serverInvite(server)).toString()}`);
      if (!invite) throw new Error('unusable server address');
      return (await pingServer(() => openTransport(invite))).rttMs;
    },
  };
}

function openMainMenu(app: App, message?: string): void {
  app.touch.visible = false;
  const master = menuMaster();
  new MainMenu(document.body, {
    index: new WorldIndex(storage()),
    recent: loadRecent(storage()),
    listFiles: listWorldFiles,
    deleteFiles: deleteWorldFiles,
    go,
    now: () => Date.now(),
    ...(message ? { message } : {}),
    ...(master ? { master } : {}),
  });
}

/**
 * The game menu (the ☰ panel, Phase 5a): opens when the pointer is released (Esc) or on Esc,
 * Resume locks the pointer again, and Quit to main menu saves a local world before leaving.
 */
function enableGameMenu(app: App, save: (() => Promise<boolean>) | null): void {
  const settings = app.settings;
  if (!settings) return;
  settings.addGameActions(
    () => {
      if ('requestPointerLock' in app.canvas) {
        Promise.resolve(app.canvas.requestPointerLock()).catch(() => undefined);
      }
    },
    () => {
      const saved = save ? save() : Promise.resolve(true);
      // Leave even if the save hangs; the world also saved every few seconds.
      const timeout = new Promise((resolve) => setTimeout(resolve, 5000));
      void Promise.race([saved, timeout]).then(() => {
        go({});
      });
    },
  );
  let locked = false;
  let releasedAt = -Infinity;
  document.addEventListener('pointerlockchange', () => {
    const now = document.pointerLockElement === app.canvas;
    if (locked && !now) {
      settings.setOpen(true);
      releasedAt = performance.now();
    }
    locked = now;
  });
  window.addEventListener('keydown', (e) => {
    // The Esc that released the pointer (if a browser delivers it at all) doesn't close the menu.
    if (e.code !== 'Escape' || locked || performance.now() - releasedAt < 300) return;
    settings.setOpen(!settings.isOpen);
  });
}

async function connect(app: App): Promise<void> {
  const status = element('net-status', HTMLDivElement);
  const params = new URLSearchParams(location.search);
  const launch = launchOf(location.search);
  if (launch.kind === 'menu') {
    openMainMenu(app);
    return;
  }

  // A local world: from the menu (?play=<id>) or by link (?world=, ?seed=, ?local=1).
  const index = new WorldIndex(storage());
  let localWorld: LocalWorld | null = null;
  let worldName = 'Local world';
  if (launch.kind === 'play') {
    const world = index.get(launch.id);
    if (!world) {
      openMainMenu(app, 'That world is not in this browser any more.');
      return;
    }
    localWorld = {
      worldSeed: world.seed,
      generatorVersion: world.generatorVersion,
      file: world.id,
    };
    worldName = world.name;
    index.touch(world.id, Date.now());
  } else if (launch.kind === 'link') {
    localWorld = parseLocalWorld(location.search);
    const world = index.adopt(
      localWorldName(localWorld.generatorVersion, localWorld.worldSeed),
      Date.now(),
    );
    if (world) {
      worldName = world.name;
      index.touch(world.id, Date.now());
    }
  }

  const code = launch.kind === 'code' ? launch.code : null;
  const invite = localWorld || code ? null : parseInvite(location.search);
  if (!localWorld && !invite && !code) {
    openMainMenu(app, 'That invite link is incomplete or damaged.');
    return;
  }
  const target = code
    ? formatCode(code)
    : invite
      ? `${invite.host}:${String(invite.port)}`
      : worldName;
  const forced = params.get('transport');
  const options: ConnectOptions = {
    displayName: displayName(),
    clientVersion: buildInfo.sha.slice(0, 12),
    transport: forced === 'webrtc' || forced === 'webtransport' ? forced : 'auto',
    netsim: parseNetConditions(params.get('netsim')),
    ...(localWorld ? { localWorld } : {}),
  };
  status.textContent = localWorld ? `Starting ${target}…` : `Connecting to ${target}…`;
  try {
    let session: ClientSession;
    let local: LocalSession | null = null;
    if (code) {
      const base = configuredMasterUrl();
      if (!base) throw new Error('this build has no master server configured');
      const key = await loadOrCreateDeviceKey(new IndexedDbKeyStore());
      session = await connectToCode(new MasterClient(base, key), code, options);
    } else if (invite) {
      session = await connectToInvite(invite, options);
    } else {
      local = await connectLocal(options);
      session = local.session;
    }
    enableGameMenu(app, local?.save ?? null);
    // A local world can be opened to friends (Host…, Phase 5c).
    if (local && app.settings) enableHosting(app.settings, local, prefersTouch(), worldName);
    let started = false;
    let latest: [SessionState, SessionStats] | null = null;
    const showStatus = () => {
      if (latest) {
        status.textContent = formatStatus(target, session.transportKind, ...latest, app.fps.fps);
      }
    };
    // The frame rate changes without session events: refresh the line every second too.
    setInterval(showStatus, 1000);
    session.subscribe((state, stats) => {
      latest = [state, stats];
      showStatus();
      if (state.phase === 'joined' && !started) {
        started = true;
        const joinedInvite = invite ? pastedInvite(location.search) : null;
        if (joinedInvite) rememberServer(storage(), joinedInvite, Date.now());
        play(app, session, state, local?.loopback ?? null);
      }
    });
  } catch (err) {
    status.textContent = `Could not connect to ${target}: ${err instanceof Error ? err.message : String(err)}`;
  }
}

void connect(start());
