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
import { rememberTrust, showRegistryPrompt, trustedHash } from './ui/registryPrompt';
import { sameLine } from './version/semver';
import { parseLocalWorld, type LocalWorld } from './local/world';
import { cleanName, creatableType, parseSeed, WorldIndex } from './local/worldIndex';
import { worldAccess } from './local/worldAccess';
import { deleteWorldFiles, listWorldFiles, localWorldName } from './local/worldFiles';
import { launchOf, launcherHref, pastedInvite, withRoute, type Launch } from './ui/launch';
import { rememberServer } from './ui/recentServers';
import { prefersTouch, TouchControls } from './ui/touchControls';
import { Hotbar } from './ui/hotbar';
import type { BrushPanelState } from './ui/brushPanel';
import { prefersReducedMotion } from './ui/motionSettings';
import type { SpyglassHud } from './ui/spyglass';
import { Hud } from './ui/hud';
import { MAP_SIZE, MapOverlay } from './ui/mapOverlay';
import { SettingsMenu } from './ui/settingsMenu';
import { StructuresPanel } from './ui/structuresPanel';
import { opfsStructureDir, StructureStore } from './local/structureStore';
import { clipboardOf, type Vec3 as Cell } from './local/structures';
import { waterQualityLevel, type WaterQuality } from './ui/waterSettings';
import { shadowLevel, shadowsFromQuery, type ShadowQuality } from './ui/shadowSettings';
import { displayScale } from './ui/displayScale';
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
  /** The day/night cycle as this client shows it: 0 midnight, 0.5 noon; the day; 0 new, 0.5 full. */
  timeOfDay: number;
  day: number;
  moonPhase: number;
  /** The frame's vertical FOV (degrees) and the spyglass's magnification (1: unzoomed). */
  fov: number;
  spyglass: number;
}

interface RenderStats {
  calls: number;
  triangles: number;
  meshBytes: number;
  screenBytes: number;
  pixelRatio: number;
  /** Bodies drawn, whether the arms were, the characters' mesh bytes and instance upload. */
  characters: number;
  arms: boolean;
  characterMeshBytes: number;
  characterUploadBytes: number;
}

/** Hooks for automated tests (Playwright) and debugging from the console. */
interface DwellDebug {
  state(): GameDebugState | null;
  press(code: string, down: boolean): void;
  look(yaw: number, pitch: number): void;
  view(): { yaw: number; pitch: number };
  /** Breaks or places at the crosshair, as a click does (§6.5). */
  edit(action: 'break' | 'place'): boolean;
  /** Runs a region tool by name (`fill`, `copy`, `undo`, … — `Client.tool`), as its key does. */
  tool(name: string): boolean;
  /**
   * Drives the brush tool (`toggle`, `preset:<name>`, `size:<n>`, `nextBrush`, `nextPerformer`,
   * `apply`, `gunpowder`, `pick` — `Client.brush`, BRUSHES.md), as its keys and clicks do.
   */
  brush(command: string): boolean;
  /** Selects a hotbar slot, as a number key does. */
  select(slot: number): void;
  /** The state the selected slot would place at the crosshair, and where; null with no target. */
  placement(): { cell: [number, number, number]; material: number } | null;
  /** Material at a voxel in the client's world. */
  voxel(x: number, y: number, z: number): number;
  /** Turns creative flight on or off by pressing F, the fly key, if it isn't already (and the
   * server allows it). */
  fly(on: boolean): void;
  /** Sets the flight speed level, as the slider does. */
  flySpeed(level: number): void;
  /**
   * Zooms the spyglass to `step` (0 the least zoom; docs/SPYGLASS.md) or out, as its key and the
   * wheel do. False before the game has started.
   */
  spyglass(on: boolean, step?: number): boolean;
  /**
   * Places the camera at `position` looking along yaw/pitch (degrees): the view and the LOD come
   * from there while the player stays put (e2e/views). False before the game has started.
   */
  placeCamera(position: Vec3, yaw: number, pitch: number): boolean;
  /**
   * Stand-in players (docs/DEV_TOOLS.md): `count` bodies ahead of `feet` on `face` (1 or −1), each
   * playing a clip of its own and holding block `held` (0: none); 0 removes them.
   */
  puppets(count: number, feet: Vec3, face?: number, held?: number): boolean;
  /** Shows or hides your first-person arms (the setting, for this page only). */
  showArms(on: boolean): void;
  /** Back to the player's eyes. */
  releaseCamera(): void;
  /** Plants' wind scale (0 still, 1 the data's) and whether cutout meshes draw (cost A/B). */
  plants(wind: number, shown: boolean): void;
  /** The last frame's draw calls and triangles. */
  renderStats(): RenderStats;
  /** What the game knows it holds in memory (ui/memory.ts), once playing. */
  memory(): MemoryReport | null;
  /** The block dump around the target (debug/blockDump.ts), or null before playing. */
  blockDump(): BlockDump | null;
  /**
   * The water quality (as the settings menu sets it, not stored) and/or its cost-isolation
   * switches (`?waterdbg=` names, comma-separated; "" for none); null leaves either as it is.
   */
  setWater(quality: WaterQuality | null, debug: string | null): void;
  /** The Shadows setting (as the settings menu sets it, not stored; docs/DAY_NIGHT.md §10). */
  setShadows(quality: ShadowQuality): void;
  /** The ink outline's mode (`shared/look/outline.json`, as `?outline=` names it), or `off`. */
  outline(): string;
  /** Sets the ink outline's mode by name (`off` or an unknown name: off); true when one is on. */
  setOutline(name: string): boolean;
  /** Dev telemetry (docs/DEV_TOOLS.md): per-frame series, event samples and counters, summarised. */
  telemetry(): Telemetry;
  /** The kept frames' raw values: `frames[i]` is `[atMs, ...values in names' order]`. */
  telemetryFrames(): { names: string[]; frames: number[][] };
  /** The kept player controller ticks, oldest first. */
  ticks(): TickSample[];
  /** The latest sounds played (ids, newest last; docs/AUDIO.md). */
  sounds(): string[];
  /** Forgets the telemetry so far: measure from here. */
  telemetryReset(): void;
  /**
   * Shows the time of day `t` (0–1, 0 midnight) and optionally the moon's phase (0 new, 0.5 full)
   * on this client only, as `?time=` and `?moon=` do; null returns to the world's.
   */
  setTime(t: number | null, moon?: number | null): void;
  /** The water's cost-isolation switches (`?waterdbg=` names, comma-separated; "" for none). */
  waterDebug(names: string): void;
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
  /** The first-person eye height (world y) after the tick: the step view's smoothing. */
  eye: number;
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
    // Typing in a field of the menu (a structure's name, pasted JSON) is not playing.
    const t = e.target;
    if (t instanceof HTMLTextAreaElement || (t instanceof HTMLInputElement && t.type !== 'range'))
      return;
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
    client.mouseDown(e.button, performance.now(), e.altKey);
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
      client.wheel(e.deltaY, e.altKey);
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

/** The hotbar slot last shown (null before the game selects its first). */
let shownSlot: number | null = null;

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
      const slot = e.value as number;
      app.hotbar?.setSelected(slot);
      // A change of slot sounds; the first selection, when the game starts, does not.
      if (shownSlot !== null && shownSlot !== slot) audio().action('hotbar');
      shownSlot = slot;
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
    region?: string;
    brush?: BrushPanelState | null;
    spyglass?: SpyglassHud | null;
  };
  if (h.message === undefined) return;
  app.hud.setSpyglass(h.spyglass ?? null);
  if (h.damaged) app.hud.flashDamage(now);
  app.hud.setHealth(h.health ?? 0, now);
  app.hud.setMessage(h.message);
  app.hud.setStatus(h.status ?? '');
  app.hotbar?.setRegion(h.region ?? '');
  app.hotbar?.setBrush(h.brush ?? null);
  if (h.debug) app.hud.setDebug(h.debug);
}

async function start(): Promise<App | null> {
  element('build-info', HTMLDivElement).textContent = formatBuildInfo(buildInfo);
  const canvas = element('view', HTMLCanvasElement);
  const params = new URLSearchParams(location.search);
  // The ?scale= drawing-buffer scale.
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
  const sounds = new EventPlayer(
    SOUND_DATA,
    (sound, o) => {
      sfx.play(sound, {
        bus: 'effects',
        intensity: o.intensity,
        at: o.at,
        gain: o.gain,
        ...(o.pan !== undefined ? { pan: o.pan } : {}),
      });
    },
    {
      start: (voice, sound, o) =>
        sfx.startLoop(voice, sound, {
          bus: 'effects',
          intensity: o.intensity,
          at: o.at,
          gain: o.gain,
        }),
      update: (voice, intensity, at) => {
        sfx.updateLoop(voice, intensity, at);
      },
      stop: (voice) => {
        sfx.stopLoop(voice);
      },
    },
  );
  // The game menu (top left, or Esc): settings applied as the sliders move.
  const settings = new SettingsMenu(document.body, mobile, (s) => {
    sfx.setVolumes(s.audio);
    client.setExposure(s.exposure);
    client.setWaterQuality(waterQualityLevel(s.water.quality));
    // ?waterdbg=caustics,foam,… turns pieces of the water's shading off (a dev tool, DEV_TOOLS.md).
    const waterDebug = new URLSearchParams(location.search).get('waterdbg');
    if (waterDebug) client.setWaterDebug(waterDebug);
    client.setClouds(s.clouds);
    // ?shadows=off|low|high overrides the stored setting (the bench's configurations).
    client.setShadows(shadowLevel(shadowsFromQuery(location.search) ?? s.shadows.quality));
    client.setMotion(s.motion.speedEffects, s.motion.fovBoost);
    client.setReducedMotion(prefersReducedMotion());
    client.setPlants(s.motion.plantSway ? 1 : 0, true);
    client.setFov(s.fovDeg);
    client.setShowArms(s.showArms);
    client.setDetail(s.detail.distanceM, s.detail.pixelError, s.detail.memoryMb);
  });
  app.settings = settings;
  // While the menu is open the touch controls step aside: they would take its taps.
  settings.onOpenChange = (open) => {
    touch.visible = touchWanted && !open;
    sfx.action(open ? 'menu_open' : 'menu_close');
    // The speed effects hold still while the menu is open.
    client.setMenuOpen(open);
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
  // The Structures page (STRUCTURES.md §4): the browser's structures store, saved from and
  // loaded into this world through the region tools.
  const structures = new StructuresPanel({
    store: new StructureStore(opfsStructureDir()),
    selection: () => {
      const s = client.selection();
      return s.length === 6
        ? { min: [s[0], s[1], s[2]] as Cell, size: [s[3], s[4], s[5]] as Cell }
        : null;
    },
    read: (min, size) => client.voxels(...min, ...size),
    feet: () => (client.debugState() as GameDebugState | null)?.feet ?? null,
    target: () => (client.debugState() as GameDebugState | null)?.target?.cell ?? null,
    load: (s) => {
      const { cells, keep } = clipboardOf(s);
      return client.loadStructure(s.size[0], s.size[1], s.size[2], cells, keep, s.name);
    },
    close: () => {
      settings.setOpen(false);
    },
  });
  settings.addStructuresPanel(structures.element, () => {
    void structures.refresh();
  });
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
    tool: (name) => client.tool(name, performance.now()),
    brush: (command) => client.brush(command, performance.now()),
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
    spyglass: (on, step) => client.spyglass(on, step ?? 0),
    placeCamera: ([x, y, z], yaw, pitch) => client.placeCamera(x, y, z, yaw, pitch),
    puppets: (count, [x, y, z], face, held) => client.puppets(count, x, y, z, face ?? 1, held ?? 0),
    showArms: (on) => {
      client.setShowArms(on);
    },
    releaseCamera: () => {
      client.releaseCamera();
    },
    plants: (wind, shown) => {
      client.setPlants(wind, shown);
    },
    renderStats: () => client.renderStats() as RenderStats,
    setWater: (quality, debug) => {
      if (quality !== null) client.setWaterQuality(waterQualityLevel(quality));
      if (debug !== null) client.setWaterDebug(debug);
    },
    setShadows: (quality) => {
      client.setShadows(shadowLevel(quality));
    },
    outline: () => client.outline(),
    setOutline: (name) => client.setOutline(name),
    memory: () => memoryReport(app),
    blockDump: () => blockDumpOf(app),
    telemetry: () => client.telemetry() as Telemetry,
    telemetryFrames: () => client.telemetryFrames() as { names: string[]; frames: number[][] },
    ticks: () => client.telemetryTicks() as TickSample[],
    telemetryReset: () => {
      client.telemetryReset();
    },
    sounds: () => [...sfx.recent],
    waterDebug: (names) => {
      client.setWaterDebug(names);
    },
    setTime: (t, moon) => {
      client.setTimeOfDay(t ?? undefined);
      if (moon !== undefined) client.setMoonPhase(moon ?? undefined);
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
    const type = creatableType(launch.type);
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
  const launch = launchOf(location.search);
  if (launch.kind === 'menu') {
    openMainMenu(app, params.get('say') ?? undefined);
    return;
  }

  // A local world: from the menu (?play=<id>) or by link (?world=, ?seed=, ?local=1). The menu's
  // new and regenerated worlds were made before the client started (routeMadeWorld).
  const index = new WorldIndex(storage());
  if (launch.kind === 'new' || launch.kind === 'regenerate') {
    openMainMenu(app);
    return;
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
    const name =
      localWorld.file ?? localWorldName(localWorld.generatorVersion, localWorld.worldSeed);
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
    // "Don't ask again" for this server's blocks (ADR 0038), keyed by how it was reached.
    const serverKey = code ? `code:${code}` : invite ? `invite:${target}` : `local:${target}`;
    client.trustRegistry(trustedHash(storage(), serverKey) ?? undefined);
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
    let asking = false;
    const showStatus = () => {
      const s = client.sessionState() as SessionReport;
      const stats = s.stats ?? {
        rttMs: null,
        datagramRttMs: null,
        serverTick: 0,
        hostPaused: false,
      };
      status.textContent = formatStatus(target, s.transport, s, stats, app.fps.fps);
      if (s.phase === 'confirming' && !asking) {
        asking = true;
        console.info('Dwell: this server’s blocks differ', s.registry);
        void showRegistryPrompt(document, target, s.registry).then((answer) => {
          if (answer.dontAsk) rememberTrust(storage(), serverKey, s.registry.hash, Date.now());
          client.answerRegistry(answer.proceed);
          if (!answer.proceed) go({});
        });
      }
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
/**
 * The menu's new and regenerated worlds are this build's to make (RELEASES.md §5), before the
 * client starts (they need no WASM or WebGPU): the address then says `?play=<id>` at once, so a
 * reload while the game loads continues the world rather than making another. False: it could not
 * be made, and the menu says why.
 */
async function routeMadeWorld(): Promise<boolean> {
  const launch = launchOf(location.search);
  if (launch.kind !== 'new' && launch.kind !== 'regenerate') return true;
  const id = await makeWorld(new WorldIndex(storage()), launch);
  if (typeof id !== 'string') {
    openMainMenu(null, id.message);
    return false;
  }
  history.replaceState(null, '', withRoute(location.search, { play: id }));
  return true;
}

if (launchOf(location.search).kind === 'menu') {
  element('build-info', HTMLDivElement).textContent = formatBuildInfo(buildInfo);
  openMainMenu(null, new URLSearchParams(location.search).get('say') ?? undefined);
  void loadTools();
} else {
  void routeMadeWorld().then(async (ok) => {
    if (!ok) return;
    const app = await start();
    if (app) void connect(app);
  });
}
