import { buildInfo, channelOf, formatBuildInfo, recordedVersion } from './buildInfo';
import { blockDump, formatBlockDump, type BlockDump, type BlockTarget } from './debug/blockDump';
import {
  Client,
  hotbarSwatch,
  loadWasm,
  pingServer,
  setWorldFileVersion,
  swatchSize,
  worldFileVersion,
} from './engine/wasm';
import {
  connectLocal,
  connectToCode,
  connectToInvite,
  HostVersionError,
  identify,
  type ConnectOptions,
  type LocalSession,
  type TransportPreference,
} from './net/connect';
import { enableHosting } from './hostWorld';
import { formatCode } from './net/joinCode';
import { configuredMasterUrl, MasterClient } from './net/master';
import { parseInvite } from './net/invite';
import { LAUNCHER_PATH } from './launcherPath';
import { PROTOCOL_VERSION } from './protocol/constants.gen';
import { regenerateWorld } from './local/regenerate';
import type { SessionReport } from './net/sessionState';
import { sameLine } from './version/semver';
import { parseLocalWorld, type LocalWorld } from './local/world';
import {
  cleanName,
  parseSeed,
  WORLD_TYPES,
  WorldIndex,
  type CreatableType,
} from './local/worldIndex';
import { worldAccess } from './local/worldAccess';
import { deleteWorldFiles, listWorldFiles, localWorldName } from './local/worldFiles';
import { launchOf, launcherHref, pastedInvite, withRoute, type Launch } from './ui/launch';
import { rememberServer } from './ui/recentServers';
import { prefersTouch, TouchControls } from './ui/touchControls';
import { Hotbar } from './ui/hotbar';
import { Hud } from './ui/hud';
import { MAP_SIZE, MapOverlay } from './ui/mapOverlay';
import { SettingsMenu } from './ui/settingsMenu';
import { FlightSpeedControl } from './ui/flightSpeedControl';
import { loadFlySpeedLevel, saveFlySpeedLevel } from './predict/flightSpeed';
import { formatStatus } from './ui/statusOverlay';
import { startupMessage } from './ui/startupError';
import { FpsMeter } from './ui/fps';
import { START_HINT, TOUCH_START_HINT } from './ui/controls';
import { formatMemory, jsHeapBytes, type MemoryReport } from './ui/memory';
import type { LoopbackTransport } from './net/loopback';
import { audio } from './audio/engine';
import { decodeSounds, EventPlayer } from './audio/events';
import { installPageSounds } from './audio/page';
import { SOUND_DATA } from './audio/sounds.gen';

type Vec3 = [number, number, number];

/** What automated tests read through `window.__dwell.state()` (the Rust client's debugState). */
export interface GameDebugState {
  playerId: number;
  active: boolean;
  feet: Vec3;
  health: number;
  dead: boolean;
  remotes: { playerId: number; feet: Vec3; dead: boolean }[];
  stats: Record<string, number>;
  terrain: Record<string, number>;
  terrainReady: boolean;
  target: BlockTarget | null;
  lod: Record<string, unknown> | null;
  flying: boolean;
}

interface RenderStats {
  calls: number;
  triangles: number;
  meshBytes: number;
  screenBytes: number;
  pixelRatio: number;
}

/** Hooks for automated tests (Playwright) and debugging from the console. */
interface DwellDebug {
  state(): GameDebugState | null;
  press(code: string, down: boolean): void;
  look(yaw: number, pitch: number): void;
  view(): { yaw: number; pitch: number };
  /** Breaks or places at the crosshair, as a click does (§6.5). */
  edit(action: 'break' | 'place'): boolean;
  /** Selects a hotbar slot, as a number key does. */
  select(slot: number): void;
  /** The state the selected slot would place at the crosshair, and where; null with no target. */
  placement(): { cell: [number, number, number]; material: number } | null;
  /** Material at a voxel in the client's world. */
  voxel(x: number, y: number, z: number): number;
  /** Turns creative flight on or off, as double-tapping Space does (if the server allows it). */
  fly(on: boolean): void;
  /** Sets the flight speed level, as the slider does. */
  flySpeed(level: number): void;
  /**
   * Places the camera at `position` looking along yaw/pitch (degrees): the view and the LOD come
   * from there while the player stays put (e2e/views). False before the game has started.
   */
  placeCamera(position: Vec3, yaw: number, pitch: number): boolean;
  /** Back to the player's eyes. */
  releaseCamera(): void;
  /** The last frame's draw calls and triangles. */
  renderStats(): RenderStats;
  /** What the game knows it holds in memory (ui/memory.ts), once playing. */
  memory(): MemoryReport | null;
  /** The block dump around the target (debug/blockDump.ts), or null before playing. */
  blockDump(): BlockDump | null;
  /** Dev telemetry (docs/DEV_TOOLS.md): per-frame series, event samples and counters, summarised. */
  telemetry(): Telemetry;
  /** The kept frames' raw values: `frames[i]` is `[atMs, ...values in names' order]`. */
  telemetryFrames(): { names: string[]; frames: number[][] };
  /** The kept player controller ticks, oldest first. */
  ticks(): TickSample[];
  /** Forgets the telemetry so far: measure from here. */
  telemetryReset(): void;
}

/** A distribution's summary (telemetry.rs `Stats`). */
export interface TelemetryStats {
  n: number;
  mean: number;
  p50: number;
  p95: number;
  p99: number;
  max: number;
  sum: number;
}

export interface Telemetry {
  /** Per-frame series (ms, or counts per frame), `interval` the time between frames. */
  frames: Record<string, TelemetryStats>;
  /** Per-event series (e.g. `job.chunk_mesh.run`, ms). */
  samples: Record<string, TelemetryStats>;
  counters: Record<string, number>;
  frameCount: number;
}

/** One controller tick (telemetry.rs `TickSample`). */
export interface TickSample {
  seq: number;
  atMs: number;
  feet: Vec3;
  velocity: Vec3;
  state: string;
  grounded: boolean;
  crouched: boolean;
  touchingWall: boolean;
  ceilingBlocked: boolean;
  /** Movement input: x right, y forward, −1 to 1. */
  move: [number, number];
  jump: boolean;
  crouch: boolean;
}

/** World-file tools for tests and debugging, on every page (the main menu included). */
interface DwellTools {
  /** The app version that last saved a world file (its bytes), or undefined. */
  worldFileVersion(bytes: Uint8Array): string | undefined;
  /** A world file's bytes rewritten to say `version` last saved it (RELEASES.md §6). */
  setWorldFileVersion(bytes: Uint8Array, version: string): Uint8Array;
}

declare global {
  interface Window {
    __dwell?: DwellDebug;
    __dwellTools?: DwellTools;
  }
}

/** Loads the WASM module and offers its world-file tools (window.__dwellTools). */
async function loadTools(): Promise<void> {
  wasmMemory = ((await loadWasm()) as { memory?: WebAssembly.Memory }).memory ?? null;
  window.__dwellTools = {
    worldFileVersion: (bytes) => worldFileVersion(bytes),
    setWorldFileVersion: (bytes, version) => setWorldFileVersion(bytes, version),
  };
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
  client: Client;
  touch: TouchControls | null;
  hud: Hud;
  map: MapOverlay;
  hotbar: Hotbar | null;
  settings: SettingsMenu | null;
  flightSpeed: FlightSpeedControl;
  /** Frames per second, shown in the status line. */
  fps: FpsMeter;
  /** The local world's loopback (its server's memory), while playing one. */
  loopback: LoopbackTransport | null;
  /** The world being played (its seed and generator), once joined: for the block dump. */
  world: { seed: bigint; generatorVersion: number } | null;
  playing: boolean;
}

/** The ?scale= drawing-buffer scale (comparing performance on a device). */
function displayScale(search: string): number {
  const scale = Number(new URLSearchParams(search).get('scale') ?? 1);
  return Number.isFinite(scale) ? Math.min(2, Math.max(0.25, scale)) : 1;
}

/** Workers per pool: generation and meshing each get this many (fewer on phones). */
function defaultWorkerCount(mobile: boolean): number {
  const cores = navigator.hardwareConcurrency || 4;
  return Math.max(1, Math.min(mobile ? 2 : 4, cores - 2));
}

function lockPointer(canvas: HTMLCanvasElement): void {
  if ('requestPointerLock' in canvas) {
    Promise.resolve(canvas.requestPointerLock()).catch(() => undefined);
  }
}

function displayName(): string {
  const fromUrl = new URLSearchParams(location.search).get('name');
  return fromUrl?.trim() ?? 'Player';
}

/** The F3 debug overlay and the client's own drawing of it go on and off together. */
function toggleDebug(app: App): void {
  app.hud.toggleDebug();
  app.client.setDebugVisible(app.hud.debugVisible);
  audio().action(app.hud.debugVisible ? 'overlay_on' : 'overlay_off');
}

/** The F4 map on or off, with its sound. */
function toggleMap(app: App): void {
  app.map.toggle();
  audio().action(app.map.visible ? 'map_open' : 'map_close');
}

/** The F4 map's zoom (− and =), with its sound while the map shows. */
function zoomMap(app: App, delta: number): void {
  if (app.map.zoomBy(delta) && app.map.visible) audio().action('map_zoom');
}

/** The game's sound events since the last frame, and the listener at the camera (docs/AUDIO.md). */
function playSounds(app: App, player: EventPlayer): void {
  player.playAll(decodeSounds(app.client.takeSounds()));
  const l = app.client.listener();
  if (l.length === 9) {
    const v = (i: number): [number, number, number] => [l[i] ?? 0, l[i + 1] ?? 0, l[i + 2] ?? 0];
    audio().setListener(v(0), v(3), v(6));
  }
}

/** Keyboard and mouse: the DOM's events, forwarded to the client's input (dwell-game input.rs). */
function forwardInput(app: App): void {
  const { canvas, client } = app;
  window.addEventListener('keydown', (e) => {
    if (client.keyDown(e.code, e.repeat, e.ctrlKey, e.metaKey, e.timeStamp)) e.preventDefault();
  });
  window.addEventListener('keyup', (e) => {
    client.keyUp(e.code);
  });
  window.addEventListener('blur', () => {
    client.blur();
  });
  // Not every browser has pointer lock (iOS Safari doesn't); touch looks by dragging instead.
  canvas.addEventListener('click', () => {
    if (document.pointerLockElement !== canvas) lockPointer(canvas);
  });
  document.addEventListener('pointerlockchange', () => {
    client.setPointerLocked(document.pointerLockElement === canvas);
  });
  canvas.addEventListener('mousedown', (e) => {
    client.mouseDown(e.button, performance.now());
  });
  canvas.addEventListener('contextmenu', (e) => {
    e.preventDefault();
  });
  document.addEventListener('mousemove', (e) => {
    client.mouseMove(e.movementX, e.movementY);
  });
  window.addEventListener(
    'wheel',
    (e) => {
      client.wheel(e.deltaY);
    },
    { passive: true },
  );
  // Flight speed (§6.7): the − and = keys while flying (not while typing in a text field).
  window.addEventListener('keydown', (e) => {
    const typing = e.target instanceof HTMLInputElement && e.target.type !== 'range';
    if (!client.flying() || typing) return;
    if (e.code === 'Minus' || e.code === 'NumpadSubtract')
      client.flySpeed(client.flySpeedLevel() - 1);
    if (e.code === 'Equal' || e.code === 'NumpadAdd') client.flySpeed(client.flySpeedLevel() + 1);
  });
}

/** What the client's input asked of the shell since the last frame. */
function handleEvents(app: App): void {
  for (const e of app.client.takeEvents() as { t: string; value: unknown }[]) {
    if (e.t === 'toggle') {
      if (e.value === 'F3') toggleDebug(app);
      if (e.value === 'F6') copyBlockDump(app);
      if (e.value === 'F4') toggleMap(app);
      if (e.value === 'Minus') zoomMap(app, 1);
      if (e.value === 'Equal') zoomMap(app, -1);
    } else if (e.t === 'slot') {
      app.hotbar?.setSelected(e.value as number);
      audio().action('hotbar');
    } else if (e.t === 'flight' || e.t === 'flightAllowed') {
      const flying = app.client.flying();
      app.touch?.setFlight(app.client.flightAllowed(), flying);
      app.flightSpeed.visible = flying;
      if (e.t === 'flight') audio().action(flying ? 'fly_on' : 'fly_off');
    } else if (e.t === 'flySpeed') {
      app.flightSpeed.show(e.value as number);
      saveFlySpeedLevel(e.value as number);
      audio().action('fly_speed');
    }
  }
}

/** The HUD from the client's latest frame. */
function showHud(app: App, now: number): void {
  const h = app.client.hud() as {
    message?: string;
    status?: string;
    health?: number;
    debug?: string | null;
    damaged?: boolean;
  };
  if (h.message === undefined) return;
  if (h.damaged) app.hud.flashDamage(now);
  app.hud.setHealth(h.health ?? 0, now);
  app.hud.setMessage(h.message);
  app.hud.setStatus(h.status ?? '');
  if (h.debug) app.hud.setDebug(h.debug);
}

async function start(): Promise<App | null> {
  element('build-info', HTMLDivElement).textContent = formatBuildInfo(buildInfo);
  const canvas = element('view', HTMLCanvasElement);
  const params = new URLSearchParams(location.search);
  const scale = displayScale(location.search);
  const mobile = prefersTouch();
  let client: Client;
  try {
    await loadTools();
    client = await Client.create(canvas, {
      displayName: displayName(),
      clientVersion: recordedVersion(),
      makeWorker: () =>
        new Worker(new URL('./engine/jobWorker.ts', import.meta.url), { type: 'module' }),
      workers: defaultWorkerCount(mobile),
      lod: params.get('lod') !== '0',
      lodColors: params.get('lodcolors') === '1',
      // ?chunks=full asks the server to send every chunk explicitly (full-chunk mode).
      fullChunks: params.get('chunks') === 'full',
      netsim: params.get('netsim') ?? undefined,
      search: location.search,
    });
  } catch (err) {
    showFatal(startupMessage(err, 'gpu' in navigator));
    console.error(err);
    return null;
  }

  const resize = (): void => {
    client.resize(
      canvas.clientWidth,
      canvas.clientHeight,
      Math.min(window.devicePixelRatio, 2) * scale,
    );
  };
  new ResizeObserver(resize).observe(canvas);
  resize();

  const app: App = {
    canvas,
    client,
    touch: null,
    hud: new Hud(document.body),
    map: new MapOverlay(document.body),
    hotbar: null,
    settings: null,
    flightSpeed: new FlightSpeedControl(document.body, (level) => {
      client.flySpeed(level);
    }),
    fps: new FpsMeter(),
    loopback: null,
    world: null,
    playing: false,
  };
  // On-screen controls on touch devices (shown on the first touch too, e.g. a tablet with a mouse).
  const touch = new TouchControls(document.body, client);
  app.touch = touch;
  let touchWanted = mobile;
  touch.visible = mobile;
  window.addEventListener(
    'touchstart',
    () => {
      touchWanted = true;
      touch.visible = !(app.settings?.isOpen ?? false);
    },
    { once: true, passive: true },
  );
  forwardInput(app);
  // Sound (docs/AUDIO.md): started by the first gesture; menus sound by delegation.
  const sfx = installPageSounds();
  const sounds = new EventPlayer(SOUND_DATA, (sound, o) => {
    sfx.play(sound, {
      bus: 'effects',
      intensity: o.intensity,
      at: o.at,
      gain: o.gain,
      ...(o.pan !== undefined ? { pan: o.pan } : {}),
    });
  });
  // The game menu (top left, or Esc): settings applied as the sliders move.
  const settings = new SettingsMenu(document.body, mobile, (s) => {
    sfx.setVolumes(s.audio);
    client.setFog(s.fog.distanceM, s.fog.density, s.fog.heightM);
    client.setExposure(s.exposure);
    client.setSkyRotation(s.skyRotation);
    client.setDetail(s.detail.distanceM, s.detail.pixelError, s.detail.memoryMb);
  });
  app.settings = settings;
  // While the menu is open the touch controls step aside: they would take its taps.
  settings.onOpenChange = (open) => {
    touch.visible = touchWanted && !open;
    sfx.action(open ? 'menu_open' : 'menu_close');
  };
  settings.setFootnote(formatBuildInfo(buildInfo));
  settings.addDebugTools([
    {
      key: 'F3',
      run: () => {
        toggleDebug(app);
      },
      on: () => app.hud.debugVisible,
    },
    {
      key: 'F4',
      run: () => {
        toggleMap(app);
      },
      on: () => app.map.visible,
    },
    {
      key: 'F6',
      run: () => {
        copyBlockDump(app);
      },
    },
  ]);
  app.hud.onToggleMap = () => {
    toggleMap(app);
  };
  app.hud.onCloseDebug = () => {
    toggleDebug(app);
  };
  // No F3 on a phone: ?debug=1 opens the debug overlay.
  if (params.get('debug') === '1') toggleDebug(app);
  app.hud.onCopyBlocks = () => {
    copyBlockDump(app);
  };
  client.flySpeed(loadFlySpeedLevel());
  app.flightSpeed.show(client.flySpeedLevel());
  window.__dwell = {
    state: () => client.debugState() as GameDebugState | null,
    press: (code, down) => {
      client.press(code, down);
    },
    look: (yaw, pitch) => {
      client.setLook(yaw, pitch);
    },
    view: () => {
      const [yaw = 0, pitch = 0] = client.look();
      return { yaw, pitch };
    },
    edit: (action) => client.edit(action, performance.now()),
    select: (slot) => {
      client.select(slot);
    },
    placement: () =>
      client.placement() as { cell: [number, number, number]; material: number } | null,
    voxel: (x, y, z) => client.voxel(x, y, z),
    fly: (on) => {
      client.fly(on);
    },
    flySpeed: (level) => {
      client.flySpeed(level);
    },
    placeCamera: ([x, y, z], yaw, pitch) => client.placeCamera(x, y, z, yaw, pitch),
    releaseCamera: () => {
      client.releaseCamera();
    },
    renderStats: () => client.renderStats() as RenderStats,
    memory: () => memoryReport(app),
    blockDump: () => blockDumpOf(app),
    telemetry: () => client.telemetry() as Telemetry,
    telemetryFrames: () => client.telemetryFrames() as { names: string[]; frames: number[][] },
    ticks: () => client.telemetryTicks() as TickSample[],
    telemetryReset: () => {
      client.telemetryReset();
    },
  };

  const frame = (now: number): void => {
    app.fps.frame(now);
    client.frame(now);
    handleEvents(app);
    playSounds(app, sounds);
    showHud(app, now);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  return app;
}

/** The WebAssembly memories and GPU estimates (F3's memory line), once playing. */
function memoryReport(app: App): MemoryReport | null {
  if (!app.playing) return null;
  const render = app.client.renderStats() as RenderStats;
  return {
    coreBytes: wasmMemoryBytes(),
    serverBytes: app.loopback ? app.loopback.heapBytes : null,
    worldgenBytes: [...app.client.workerMemory()],
    meshBytes: render.meshBytes,
    screenBytes: render.screenBytes,
    jsHeapBytes: jsHeapBytes(),
  };
}

let wasmMemory: WebAssembly.Memory | null = null;
function wasmMemoryBytes(): number {
  return wasmMemory?.buffer.byteLength ?? 0;
}

/** Once joined: the hotbar, the block dump's world, and the debug tools. */
function play(app: App, joined: Extract<SessionReport, { phase: 'joined' }>): void {
  app.playing = true;
  app.world = { seed: joined.worldSeed, generatorVersion: joined.generatorVersion };
  const size = swatchSize();
  const slots = app.client.palette().map((name, i) => ({ name, swatch: hotbarSwatch(i), size }));
  app.hotbar = new Hotbar(document.body, slots, (slot) => {
    app.client.select(slot);
  });
  app.client.select(0);
  app.touch?.setFlight(joined.mayFly, app.client.flying());
  startDebugTools(app);
  document.body.classList.add('in-game');
  app.hud.showHint(prefersTouch() ? TOUCH_START_HINT : START_HINT);
}

/**
 * Debug tooling (Phase 3e): the F4 terrain map around the player, and in the F3 overlay the
 * player's chunk regenerated and diffed against the one the world holds, and the memory line.
 */
function startDebugTools(app: App): void {
  let mapKey = '';
  let mapBytes: Uint8Array | null = null;
  let mapWanted = '';
  setInterval(() => {
    const client = app.client;
    if (app.hud.debugVisible) {
      const report = memoryReport(app);
      if (report) client.setMemoryNote(formatMemory(report));
      client.requestDiff();
    }
    const s = client.debugState() as GameDebugState | null;
    if (!s?.active || !app.map.visible) return;
    const [x, , z] = s.feet.map(Math.floor) as Vec3;
    const yaw = client.look()[0] ?? 0;
    const view = app.map.view(x, z);
    const key = `${String(view.x0)},${String(view.z0)},${String(view.step)}`;
    const arrived = client.takeMap();
    if (arrived) {
      mapKey = mapWanted;
      mapBytes = arrived.length > 0 ? arrived : null;
    }
    if (key === mapKey) {
      // The same part of the world (the whole disc never changes): only the marker moves.
      app.map.draw(mapBytes, yaw, view, x, z);
    } else if (key !== mapWanted) {
      mapWanted = key;
      client.requestMap(view.x0, view.z0, view.step, MAP_SIZE, 0);
    }
  }, 1000);
}

/** The block dump (F6, or the debug overlay's button) of the client's world, once playing. */
function blockDumpOf(app: App): BlockDump | null {
  const s = app.client.debugState() as GameDebugState | null;
  if (!s) return null;
  const [yaw = 0, pitch = 0] = app.client.look();
  return blockDump({
    voxel: (x, y, z) => app.client.voxel(x, y, z),
    target: s.target,
    feet: s.feet,
    view: { yaw, pitch },
    world: app.world,
    build: { version: buildInfo.version, sha: buildInfo.sha },
  });
}

/** Copies the block dump to the clipboard, or shows it to copy by hand where that is refused. */
function copyBlockDump(app: App): void {
  const dump = blockDumpOf(app);
  if (!dump) {
    app.hud.flash('No world to dump yet');
    audio().ui('error');
    return;
  }
  const text = formatBlockDump(dump);
  const shown = () => {
    app.hud.showDump(text);
  };
  try {
    navigator.clipboard.writeText(text).then(() => {
      app.hud.flash('Block info copied');
      audio().action('copy');
    }, shown);
  } catch {
    shown(); // no clipboard (an insecure context)
  }
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
  // Always through the launcher, which opens the build the choice needs (RELEASES.md §5).
  location.assign(launcherHref(location.search, route));
}

/** Carries out the menu's New world or Regenerate: the world's id, or why not. */
async function makeWorld(
  index: WorldIndex,
  launch: Extract<Launch, { kind: 'new' | 'regenerate' }>,
): Promise<string | { message: string }> {
  if (launch.kind === 'new') {
    const type = (WORLD_TYPES as readonly string[]).includes(launch.type)
      ? (launch.type as CreatableType)
      : 'terrain';
    const world = index.create(
      { name: cleanName(launch.name, 'New world'), type, seed: parseSeed(launch.seed) },
      Date.now(),
    );
    return world.id;
  }
  const world = index.get(launch.id);
  if (!world) return { message: 'That world is not in this browser any more.' };
  if (!(await regenerateWorld(index, world, deleteWorldFiles))) {
    return { message: `"${world.name}" is open in another tab. Close it there first.` };
  }
  return world.id;
}

/** Whether this build is served at the launcher's own path: a local or preview build, no launcher. */
function servesLauncherPath(): boolean {
  return location.pathname === LAUNCHER_PATH;
}

/**
 * The main menu is the launcher's (RELEASES.md §5): a build hands over to it, with `message` to say
 * why. A build served at the launcher's path (a local or preview build, the e2e suite) has no
 * launcher, and shows the same menu itself, loaded only then.
 */
function openMainMenu(app: App | null, message?: string): void {
  if (app?.touch) app.touch.visible = false;
  if (!servesLauncherPath()) {
    location.replace(launcherHref(location.search, message ? { say: message } : {}));
    return;
  }
  void import('./ui/menuPage').then(({ showMenu }) => {
    showMenu({
      go,
      ...(message ? { message } : {}),
      version: recordedVersion(),
      channel: channelOf(buildInfo.version),
      protocol: PROTOCOL_VERSION,
      detail: `Build ${buildInfo.sha.slice(0, 7)}, ${buildInfo.time.slice(0, 10)}.`,
      versionsHref: `${LAUNCHER_PATH}?versions`,
      noticesHref: `${import.meta.env.BASE_URL}THIRD_PARTY_NOTICES.txt`,
      ping: async () => {
        await loadWasm();
        return pingServer;
      },
    });
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
      lockPointer(app.canvas);
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
  let launch = launchOf(location.search);
  if (launch.kind === 'menu') {
    openMainMenu(app, params.get('say') ?? undefined);
    return;
  }

  // A local world: from the menu (?play=<id>) or by link (?world=, ?seed=, ?local=1).
  const index = new WorldIndex(storage());
  // The menu's new and regenerated worlds are this build's to make (RELEASES.md §5); then they are
  // played as any world, the address saying so (a reload continues it rather than making another).
  if (launch.kind === 'new' || launch.kind === 'regenerate') {
    const id = await makeWorld(index, launch);
    if (typeof id !== 'string') {
      openMainMenu(app, id.message);
      return;
    }
    history.replaceState(null, '', withRoute(location.search, { play: id }));
    launch = { kind: 'play', id };
  }
  let localWorld: LocalWorld | null = null;
  let worldName = 'Local world';
  if (launch.kind === 'play') {
    const world = index.get(launch.id);
    if (!world) {
      openMainMenu(app, 'That world is not in this browser any more.');
      return;
    }
    const access = worldAccess(world, buildInfo.version);
    if (!access.ok) {
      openMainMenu(app, access.message);
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
    // A link to a world file that is already there opens a world saved before versioned releases
    // (ignored); a new one is this build's.
    const name = localWorldName(localWorld.generatorVersion, localWorld.worldSeed);
    const existed = (await listWorldFiles()).includes(name);
    const world = index.adopt(name, Date.now(), existed ? undefined : recordedVersion());
    const access = world ? worldAccess(world, buildInfo.version) : null;
    if (access && !access.ok) {
      openMainMenu(app, access.message);
      return;
    }
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
  const preference: TransportPreference =
    forced === 'webrtc' || forced === 'webtransport' ? forced : 'auto';
  const options: ConnectOptions = {
    clientVersion: recordedVersion(),
    transport: preference,
    ...(localWorld ? { localWorld } : {}),
  };
  status.textContent = localWorld ? `Starting ${target}…` : `Connecting to ${target}…`;
  const client = app.client;
  try {
    const key = await identify(client);
    let local: LocalSession | null = null;
    let onJoined: (() => void) | undefined;
    if (code) {
      const base = configuredMasterUrl();
      if (!base) throw new Error('this build has no master server configured');
      ({ onJoined } = await connectToCode(client, new MasterClient(base, key), code, options));
    } else if (invite) {
      await connectToInvite(client, invite, preference);
    } else {
      local = await connectLocal(client, key, options);
      app.loopback = local.loopback;
    }
    enableGameMenu(app, local?.save ?? null);
    app.settings?.setSubtitle(localWorld ? `${target} · this device` : target);
    // A local world can be opened to friends (Host…, Phase 5c).
    if (local && app.settings) enableHosting(app.settings, local, prefersTouch(), worldName);
    let started = false;
    const showStatus = () => {
      const s = client.sessionState() as SessionReport;
      const stats = s.stats ?? {
        rttMs: null,
        datagramRttMs: null,
        serverTick: 0,
        hostPaused: false,
      };
      status.textContent = formatStatus(target, s.transport, s, stats, app.fps.fps);
      if (s.phase === 'joined' && !started) {
        started = true;
        const joinedInvite = invite ? pastedInvite(location.search) : null;
        if (joinedInvite) rememberServer(storage(), joinedInvite, Date.now());
        onJoined?.();
        play(app, s);
      }
    };
    // The session's state is polled: twice a second is quick enough for the join and the line.
    setInterval(showStatus, 500);
    showStatus();
  } catch (err) {
    if (err instanceof HostVersionError && code) {
      // The host runs another version line: the launcher opens a build of it (RELEASES.md §5). If
      // it already chose for this host's line, don't loop: say why.
      const routed = params.get('v');
      if (!routed || !sameLine(routed, err.hostVersion)) {
        location.replace(launcherHref(location.search, { code, v: err.hostVersion }));
        return;
      }
    }
    status.textContent = `Could not connect to ${target}: ${err instanceof Error ? err.message : String(err)}`;
  }
}

// The main menu needs no game client (nor WebGPU): it opens at once. Anything else starts the
// client, and shows the unsupported screen where it cannot.
if (launchOf(location.search).kind === 'menu') {
  element('build-info', HTMLDivElement).textContent = formatBuildInfo(buildInfo);
  openMainMenu(null, new URLSearchParams(location.search).get('say') ?? undefined);
  void loadTools();
} else {
  void start().then((app) => {
    if (app) void connect(app);
  });
}
