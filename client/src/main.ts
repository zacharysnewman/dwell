import { buildInfo, channelOf, formatBuildInfo, recordedVersion } from './buildInfo';
import { isGameMode } from './ui/modes';
import { blockDump, formatBlockDump, type BlockDump, type BlockTarget } from './debug/blockDump';
import {
  deviceOf,
  formatStreamingReport,
  gpuName,
  streamingReport,
  type ReportSettings,
  type StreamingReport,
} from './debug/streamingReport';
import {
  Client,
  hotbarSwatch,
  itemSwatch,
  loadWasm,
  pingServer,
  setWorldFileVersion,
  swatchSize,
  type WasmBytes,
  worldFileVersion,
  worldMapKeys,
} from './engine/wasm';
import { IndexedDbMapStore, type MapDeps, workerBuilder, worldMaps } from './world/worldMaps';
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
import { showUpgradePrompt } from './ui/upgradePrompt';
import { compatibilityLine, sameLine } from './version/semver';
import { GENERATORS, isSpawn, parseLocalWorld, spawnOptions, type LocalWorld } from './local/world';
import { cleanName, creatableType, parseSeed, testWorldId, WorldIndex } from './local/worldIndex';
import { worldAccess } from './local/worldAccess';
import {
  copyWorldFile,
  deleteWorldFiles,
  listWorldFiles,
  localWorldName,
} from './local/worldFiles';
import { launchOf, launcherHref, pastedInvite, withRoute, type Launch } from './ui/launch';
import { rememberServer } from './ui/recentServers';
import { prefersTouch, TouchControls } from './ui/touchControls';
import { Hotbar, type HotbarSlot, type InventoryHud } from './ui/hotbar';
import { Backpack, backpackKey } from './ui/backpack';
import { CreativeInventory, loadPicks, savePicks } from './ui/creativeInventory';
import type { BrushPanelState } from './ui/brushPanel';
import { prefersReducedMotion } from './ui/motionSettings';
import type { SpyglassHud } from './ui/spyglass';
import { Hud, type VitalsHud } from './ui/hud';
import { MAP_SIZE, MapOverlay } from './ui/mapOverlay';
import { SettingsMenu, loadSettings } from './ui/settingsMenu';
import { antialiasFromQuery, antialiasMode } from './ui/antialiasSettings';
import { StructuresPanel } from './ui/structuresPanel';
import { opfsStructureDir, StructureStore } from './local/structureStore';
import { clipboardOf, type Vec3 as Cell } from './local/structures';
import { waterQualityLevel, type WaterQuality } from './ui/waterSettings';
import {
  shadowLevel,
  shadowsFromQuery,
  torchShadowsFromQuery,
  type ShadowQuality,
} from './ui/shadowSettings';
import { displayScale } from './ui/displayScale';
import type { CloudQuality } from './ui/viewSettings';
import { UPSCALE, renderScaleFromQuery } from './ui/renderScaleSettings';
import { presetFromQuery, presetOf } from './ui/graphicsPresets';
import { FlightSpeedControl } from './ui/flightSpeedControl';
import { loadFlySpeedLevel, saveFlySpeedLevel } from './predict/flightSpeed';
import { formatStatus } from './ui/statusOverlay';
import { formatPerf, TickRate } from './ui/perfLine';
import { applyHud } from './ui/hudSettings';
import { LayoutRunner } from './ui/hudLayout';
import { HUD_LAYOUT } from './ui/layout.gen';
import { MapUi, indexedDbExplored, indexedDbTiles } from './ui/minimap';
import { hasWasmSimd } from './engine/simd';
import { NEEDS_SIMD, startupMessage } from './ui/startupError';
import { FpsMeter } from './ui/fps';
import { readDevice } from './ui/loadingEstimate';
import { type LoadingHud, LoadingScreen, loadingScreenEnabled } from './ui/loadingScreen';
import type { LoadingStageId } from './ui/loading.gen';
import { LAUNCH_AT_KEY, pageStartMs, type StageRun } from './ui/loadingStages';
import { START_HINT, TOUCH_START_HINT } from './ui/controls';
import { formatMemory, jsHeapBytes, type MemoryReport } from './ui/memory';
import type { LoopbackTransport } from './net/loopback';
import {
  findPlace,
  gotoPlace,
  type Camera,
  type FindHost,
  type FindOptions,
  type FoundPlace,
  type GotoOptions,
  type GotoResult,
} from './debug/find';
import type { ServerStats } from './local/messages';
import { audio } from './audio/engine';
import { decodeSounds, EventPlayer } from './audio/events';
import { AmbiencePlayer, decodeSurroundings } from './audio/ambience';
import { installPageSounds } from './audio/page';
import { SOUND_DATA } from './audio/sounds.gen';
import { GamepadInput } from './input/gamepad';
import { PLAY_ACTIONS } from './input/gamepad.gen';
import {
  loadGamepadSettings,
  saveGamepadSettings,
  type GamepadSettings,
} from './input/gamepadSettings';
import { ControllerPanel } from './ui/controllerPanel';
import { workerCount } from './workerCount';
import { startThumbnailCapture, WorldThumbs } from './local/worldThumbs';
import { pauseFromQuery, togglePause } from './debug/pause';

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
  /**
   * The loading screen (ARCHITECTURE.md §6.3): on for this page, shown now with the near ring's
   * chunks drawn and needed, and how long its last wait took (s) and whether it timed out.
   */
  loading: {
    enabled: boolean;
    shown: boolean;
    ready?: number;
    needed?: number;
    openedS?: number;
    timedOut?: boolean;
  };
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
  /**
   * Presses (`true`) or lets go of Place as a held right button does: pressed, it places and keeps
   * building as the aim moves, in the build mode (docs/ITEMS.md §5). True if it placed.
   */
  holdPlace(on: boolean): boolean;
  /** The next build mode, as its key does: its label (`Line`, `Flat`). */
  buildMode(): string;
  /** Selects a hotbar slot, as a number key does. */
  select(slot: number): void;
  /**
   * Creative: puts the item named `item` (shared/items/items.json) in the selected hotbar slot, as
   * a click in the inventory view does; false if it is not a creative item.
   */
  pick(item: string): boolean;
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
  /**
   * The placed camera (`placeCamera`, `goto`), or null while the view is the player's eyes
   * (`view()` is the player's look, not a placed camera's).
   */
  camera(): Camera | null;
  /**
   * The nearest loaded place matching `query` (docs/DEV_TOOLS.md "Finding places"): a block or
   * fluid by name (`dwell:sand`, `water`) or `fluid`, from the player's feet (or `from`) within
   * `radius` m, with filters. Null when there is none; throws for a name no block has.
   */
  find(query: string, options?: FindOptions): FoundPlace | null;
  /**
   * Finds `query` (the world map's water first with `scope: 'world'`), moves the player there
   * with `move`, and places the camera `height` m over it aimed `downstream`, at a `yaw`, or
   * `toward` a point. Rejects with what it looked for when nothing is found.
   */
  goto(query: string, options?: GotoOptions): Promise<GotoResult>;
  /** Moves the local player's feet there (a local world, on its face); false otherwise. */
  teleport(feet: Vec3): boolean;
  /** Plants' wind scale (0 still, 1 the data's) and whether cutout meshes draw (cost A/B). */
  plants(wind: number, shown: boolean): void;
  /** The last frame's draw calls and triangles. */
  renderStats(): RenderStats;
  /** What the game knows it holds in memory (ui/memory.ts), once playing. */
  memory(): MemoryReport | null;
  /** The block dump around the target (debug/blockDump.ts), or null before playing. */
  blockDump(): BlockDump | null;
  /**
   * The streaming report (F8; debug/streamingReport.ts, docs/DEV_TOOLS.md): the near ring's
   * chunks and what each waits on, the job pool, the streamer and the LOD; copied too.
   */
  streamingReport(): Promise<StreamingReport>;
  /**
   * The local server's figures over the last second (its ticks' time; the water's thread: run
   * time, owed time, queue; docs/DEV_TOOLS.md); null outside a local world or before the first.
   */
  serverStats(): ServerStats | null;
  /** The local server's water around a cell (JSON, `Water::report`); null outside a local world. */
  waterReport(x: number, y: number, z: number, radius?: number): Promise<string | null>;
  /**
   * The water quality (as the settings menu sets it, not stored) and/or its cost-isolation
   * switches (`?waterdbg=` names, comma-separated; "" for none); null leaves either as it is.
   * `phone` draws the phone's reflection tiers (or a desktop's) whatever the device is.
   */
  setWater(quality: WaterQuality | null, debug: string | null, phone?: boolean): void;
  /** The Shadows setting (as the settings menu sets it, not stored; docs/DAY_NIGHT.md §10). */
  setShadows(quality: ShadowQuality): void;
  /**
   * Other settings mid-run, for the bench's `view` step (not stored; docs/DEV_TOOLS.md): the
   * clouds, the speed effects, the full-detail distance and the LOD's pixel error, the blasts'
   * smoke, the water's flecks, flow ripples, flow foam and waterfalls. Each one given replaces the menu's value; the rest stay.
   */
  setView(view: {
    clouds?: CloudQuality;
    speedEffects?: boolean;
    distanceM?: number;
    pixelError?: number;
    smoke?: boolean;
    flowParticles?: boolean;
    flowRipples?: boolean;
    flowFoam?: boolean;
    waterfalls?: boolean;
  }): void;
  /** The ink outline's mode (`shared/look/outline.json`, as `?outline=` names it), or `off`. */
  outline(): string;
  /** Sets the ink outline's mode by name (`off` or an unknown name: off); true when one is on. */
  setOutline(name: string): boolean;
  /** The pixel-art preview's preset (`shared/look/pixelart.json`, as `?pixelart=` names it), or `off`. */
  pixelArt(): string;
  /** Sets the pixel-art preview's preset by name (`off` or an unknown name: off); true when one is on. */
  setPixelArt(name: string): boolean;
  /** The LOD colour map preview on or off (`?lodmap=on` starts it on; off on a CPU adapter); true when on. */
  setLodMap(on: boolean): boolean;
  /**
   * The big map's tiles (ADR 0068): the open map's zoom level (null: shut), tiles kept, built this
   * session, in flight and still to build in the background.
   */
  mapTiles(): {
    level: number | null;
    tiles: number;
    built: number;
    inFlight: number;
    backgroundLeft: number;
  } | null;
  /** Draws the distant terrain (LOD) or not, live (a debug switch); true in a world. */
  setLod(on: boolean): boolean;
  /** Fixed points (x, y, z triples) the streaming probe reports (`probe.target`, `probe.control`). */
  setProbe(points: number[]): boolean;
  /** How long the camera rolls upright after crossing the midplane (s; `?crossingroll=`). */
  crossingRoll(): number;
  /** Sets the crossing roll's duration (s, 0..5; 0 snaps upright at once); false for a value it refuses. */
  setCrossingRoll(seconds: number): boolean;
  /** Dev telemetry (docs/DEV_TOOLS.md): per-frame series, event samples and counters, summarised. */
  telemetry(): Telemetry;
  /** The kept frames' raw values: `frames[i]` is `[atMs, ...values in names' order]`. */
  telemetryFrames(): { names: string[]; frames: number[][] };
  /** The client's water current at a world point (m/s x, z) and the flecks near the eye. */
  waterFlowAt(x: number, y: number, z: number): number[];
  /**
   * The waterfalls near the eye (docs/WATER_SURFACE_GRID.md §2e): each one's high column x, z, its
   * way (0 +x, 1 −x, 2 +z, 3 −z), width, lip and landing (world y), flow per metre and lip speed.
   */
  waterFalls(): {
    x: number;
    z: number;
    dir: number;
    width: number;
    lip: number;
    land: number;
    flow: number;
    speed: number;
  }[];
  /** The blasts whose effects are still going (docs/EXPLOSIVES.md): each one's place and age (s). */
  blasts(): { x: number; y: number; z: number; age: number }[];
  /** The creatures drawn (docs/adr/0073-creatures.md): each one's feet, heading (deg) and speed (m/s). */
  creatures(): { x: number; y: number; z: number; yaw: number; speed: number }[];
  /** The kept player controller ticks, oldest first. */
  ticks(): TickSample[];
  /** The latest sounds played (ids, newest last; docs/AUDIO.md). */
  sounds(): string[];
  /** Kills the local player (a local world only; false otherwise): the bench's `die` step. */
  die(): boolean;
  /** Gives the local player `count` of `item` (a local world only): the bench's `give` step. */
  give(item: string, count: number): boolean;
  /**
   * Carries the local player on a raft moved at `speed` m/s toward `yaw` (degrees, 0 = +Z) for
   * `seconds`, `lift` m above its feet (a local world only; false otherwise): the bench's `carry`.
   */
  carry(speed: number, yaw: number, seconds: number, lift?: number): boolean;
  /** Forgets the telemetry so far: measure from here. */
  telemetryReset(): void;
  /**
   * Shows the time of day `t` (0–1, 0 midnight) and optionally the moon's phase (0 new, 0.5 full)
   * on this client only, as `?time=` and `?moon=` do; null returns to the world's.
   */
  setTime(t: number | null, moon?: number | null): void;
  /** The water's cost-isolation switches (`?waterdbg=` names, comma-separated; "" for none). */
  waterDebug(names: string): void;
  /**
   * Pauses (true) or resumes (false) the look: every drawn animation stands still at one instant
   * while the game runs on (`?pause=1`, F7, Settings → Debug; debug/pause.ts). Returns the state;
   * no argument only reads it.
   */
  pause(on?: boolean): boolean;
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
    /** The loading screen's stages so far (§6.3; docs/DEV_TOOLS.md): when each began and ended. */
    __dwellStages?: () => StageRun[];
    /** The loading bar's estimate: its fill, the "About N s" label, and whether it holds (§6.3). */
    __dwellLoadingBar?: () => { fill: number; label: string; holding: boolean };
  }
}

/** Loads the WASM module and offers its world-file tools (window.__dwellTools). */
async function loadTools(onBytes?: WasmBytes): Promise<void> {
  wasmMemory = ((await loadWasm(onBytes)) as { memory?: WebAssembly.Memory }).memory ?? null;
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
  /** The loading screen over the view on joining and respawning (`Client.hud().loading`). */
  loading: LoadingScreen;
  map: MapOverlay;
  hotbar: Hotbar | null;
  /** The backpack screen (ADR 0066), in a world that uses items. */
  backpack: Backpack | null;
  /** The creative inventory view (docs/ITEMS.md §6a), in a world that doesn't use items. */
  creative: CreativeInventory | null;
  settings: SettingsMenu | null;
  flightSpeed: FlightSpeedControl;
  /** Frames per second, shown in the status line. */
  fps: FpsMeter;
  /** The local world's loopback (its server's memory), while playing one. */
  loopback: LoopbackTransport | null;
  /** Reads the player's view into the local world's picture (set while a local world plays). */
  captureThumb: ((timeoutMs?: number) => Promise<void>) | null;
  /** The world being played (its seed and generator), once joined: for the block dump. */
  world: { seed: bigint; generatorVersion: number } | null;
  playing: boolean;
  /** The minimap and the explored map (ADR 0068), once joined. */
  minimap: MapUi | null;
  /** The minimap's height shading (Settings, On screen). */
  mapShading: boolean;
}

/**
 * The minimap and the explored map (ADR 0068): M or a tap on the minimap opens the big map, M or
 * Esc closes it; the explored cells are kept per world (`world`) in this browser.
 */
function startMinimap(app: App, world: string): void {
  if (app.minimap) return;
  const ui = new MapUi(
    document,
    app.client,
    world,
    indexedDbExplored(),
    indexedDbTiles(),
    recordedVersion(),
  );
  app.minimap = ui;
  // While the big map is open the game holds still, as under the game menu (the keys and the
  // controller drive the map).
  ui.onOpenChange = (open) => {
    app.client.setMenuOpen(open || (app.settings?.isOpen ?? false));
    audio().action(open ? 'map_open' : 'map_close');
  };
  ui.onZoom = () => {
    audio().action('map_zoom');
  };
  window.addEventListener('keydown', (e) => {
    const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
    if (typing || e.repeat) return;
    if (e.code === 'KeyM') ui.toggleBig();
    else if (e.code === 'Escape' && ui.bigOpen) ui.toggleBig();
    else ui.key(e.code);
  });
  window.addEventListener('pagehide', () => {
    ui.save(performance.now() + 1e9, true);
  });
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

/** The F7 debug pause (debug/pause.ts): the look stands still, with its sound. */
function togglePauseLook(app: App): void {
  audio().action(togglePause(app.client) ? 'overlay_on' : 'overlay_off');
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
function playSounds(app: App, player: EventPlayer, beds: AmbiencePlayer): void {
  player.playAll(decodeSounds(app.client.takeSounds()));
  beds.update(decodeSurroundings(app.client.ambience()));
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
  // Letting go of Place stops building on (docs/ITEMS.md §5); on the document, so a release off
  // the canvas counts too.
  document.addEventListener('mouseup', (e) => {
    client.mouseUp(e.button);
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
      // Line or page deltas (Firefox's mouse wheel) as pixels, for the notch accumulator.
      const px = e.deltaMode === 1 ? 40 : e.deltaMode === 2 ? 800 : 1;
      client.wheel(e.deltaX * px, e.deltaY * px, e.altKey, e.timeStamp);
    },
    { passive: true },
  );
  // Flight speed (§6.7): the − and = keys while flying (not while typing in a text field).
  window.addEventListener('keydown', (e) => {
    const typing = e.target instanceof HTMLInputElement && e.target.type !== 'range';
    // On the open big map − and = zoom it instead (MapUi.key).
    if (!client.flying() || typing || app.minimap?.bigOpen) return;
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
      if (e.value === 'F8') copyStreamingReport(app);
      if (e.value === 'F7') togglePauseLook(app);
      if (e.value === 'F4') toggleMap(app);
      if (e.value === 'Minus') zoomMap(app, 1);
      if (e.value === 'Equal') zoomMap(app, -1);
    } else if (e.t === 'slot') {
      const slot = e.value as number;
      app.hotbar?.setSelected(slot);
      app.creative?.set([...app.client.creativeHotbar()], slot);
      // A change of slot sounds; the first selection, when the game starts, does not.
      if (shownSlot !== null && shownSlot !== slot) audio().action('hotbar');
      shownSlot = slot;
    } else if (e.t === 'flight' || e.t === 'flightAllowed') {
      const flying = app.client.flying();
      app.touch?.setFlight(app.client.flightAllowed(), flying);
      app.flightSpeed.visible = flying;
      if (e.t === 'flight') audio().action(flying ? 'fly_on' : 'fly_off');
    } else if (e.t === 'buildMode') {
      app.hotbar?.setBuildMode(e.value as string);
      app.touch?.setBuildMode(e.value as string);
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
    vitals?: VitalsHud | null;
    debug?: string | null;
    damaged?: boolean;
    region?: string;
    /** The held axe's swing hint (ADR 0067), else null. */
    chop?: string | null;
    swing?: 'level' | 'vertical' | null;
    brush?: BrushPanelState | null;
    spyglass?: SpyglassHud | null;
    loading?: LoadingHud | null;
    loadingOpens?: number;
    inventory?: InventoryHud | null;
  };
  app.loading.frame(h, now);
  if (h.message === undefined) return;
  app.hud.setSpyglass(h.spyglass ?? null);
  if (h.damaged) app.hud.flashDamage(now);
  app.hud.setHealth(h.health ?? 0, now);
  app.hud.setVitals(h.vitals ?? null);
  app.hud.setMessage(h.message);
  app.hud.setStatus(h.status ?? '');
  app.hotbar?.setRegion(h.chop || h.region || '');
  app.hud.setSwing(h.swing ?? null);
  app.hotbar?.setBrush(h.brush ?? null);
  app.hotbar?.setInventory(h.inventory ?? null, itemPicture);
  app.backpack?.set(h.inventory ?? null);
  if (h.debug) app.hud.setDebug(h.debug);
}

/** A job worker (ARCHITECTURE.md §5.1): generation, meshing, or one world map's build. */
function makeJobWorker(): Worker {
  return new Worker(new URL('./engine/jobWorker.ts', import.meta.url), { type: 'module' });
}

let maps: MapDeps | null = null;

/**
 * The local world's worldgen config, when it is a test world (docs/DEV_TOOLS.md "Play a test
 * world": a `?worldgen=` link or Chisel's Play in game, kept in its record): its maps are built
 * from it. Empty: the build's terrain.
 */
let worldgenConfig = '';

/** How this page gets a terrain world's maps (ADR 0049; `world/worldMaps.ts`). */
function mapDeps(): MapDeps {
  maps ??= {
    keys: (seed, worldgen) => worldMapKeys(seed, worldgen || undefined),
    build: workerBuilder(makeJobWorker),
    store: typeof indexedDB === 'undefined' ? null : new IndexedDbMapStore(),
    onProgress: (ready, needed) => {
      const stages = bootLoading?.stages;
      const now = performance.now();
      stages?.progress('worldMap', ready, needed, now);
      if (ready >= needed) stages?.end('worldMap', now);
    },
  };
  return maps;
}

async function start(): Promise<App | null> {
  const loading = (bootLoading ??= new LoadingScreen(document.body, false, false));
  element('build-info', HTMLDivElement).textContent = formatBuildInfo(buildInfo);
  const canvas = element('view', HTMLCanvasElement);
  const params = new URLSearchParams(location.search);
  // The ?scale= drawing-buffer scale.
  const scale = displayScale(location.search);
  const mobile = prefersTouch();
  // The module is built with WebAssembly SIMD: a browser without it is told it is too old, before
  // the download (engine/simd.ts).
  if (!hasWasmSimd()) {
    loading.abort();
    showFatal(NEEDS_SIMD);
    return null;
  }
  let client: Client;
  try {
    // The stages the loading screen lists (§6.3): the module's download, then the graphics.
    const stages = loading.stages;
    stages.begin('game', performance.now());
    await loadTools((done, total) => {
      stages.progress('game', done, total, performance.now());
    });
    stages.end('game', performance.now());
    stages.begin('graphics', performance.now());
    client = await Client.create(canvas, {
      displayName: displayName(),
      clientVersion: recordedVersion(),
      makeWorker: makeJobWorker,
      // A terrain world's maps (ADR 0049): cached, or built in workers, never on this thread;
      // under the worldgen config the world's server sent (ADR 0074 B7: a guest of a test world).
      prepareWorld: (seed: bigint, version: number, worldgen?: string) =>
        worldMaps(seed, version, mapDeps(), worldgen ?? worldgenConfig) ?? Promise.resolve(null),
      // Per pool (generation, meshing); ?workers=N overrides it (docs/DEV_TOOLS.md).
      workers: workerCount(location.search, navigator.hardwareConcurrency, mobile),
      // ?lod=0 or ?lod=off: no distant terrain at all (a debug switch; __dwell.setLod hides it live).
      lod: params.get('lod') !== '0' && params.get('lod') !== 'off',
      lodColors: params.get('lodcolors') === '1',
      // ?chunks=full asks the server to send every chunk explicitly (full-chunk mode).
      fullChunks: params.get('chunks') === 'full',
      netsim: params.get('netsim') ?? undefined,
      search: location.search,
      // The loading screen on joining; ?loading=off or the dwell.loading store "off" skip it.
      loadingScreen: loadingScreenEnabled(location.search, storage()),
      // The scene's samples per pixel (Settings → Anti-aliasing; fixed when the renderer is made,
      // so a change applies on the next load; ?aa= overrides it).
      samples: antialiasMode(
        antialiasFromQuery(location.search, loadSettings(mobile).antialias).mode,
      ).samples,
    });
    stages.end('graphics', performance.now());
  } catch (err) {
    loading.abort();
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
  loading.setCpuAdapter(client.cpuAdapter());

  const app: App = {
    canvas,
    client,
    touch: null,
    hud: new Hud(document.body),
    loading,
    map: new MapOverlay(document.body),
    hotbar: null,
    backpack: null,
    creative: null,
    settings: null,
    flightSpeed: new FlightSpeedControl(document.body, (level) => {
      client.flySpeed(level);
    }),
    fps: new FpsMeter(),
    loopback: null,
    captureThumb: null,
    minimap: null,
    mapShading: true,
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
  const beds = new AmbiencePlayer(SOUND_DATA.ambience, sfx.bedOutput);
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
      update: (voice, intensity, at) => sfx.updateLoop(voice, intensity, at),
      stop: (voice) => {
        sfx.stopLoop(voice);
      },
    },
  );
  // The game menu (top left, or Esc): settings applied as the sliders move.
  const settings = new SettingsMenu(document.body, mobile, (stored) => {
    // ?preset=<id> draws the visit at a graphics preset (the bench; ui/graphicsPresets.ts).
    const s = presetFromQuery(location.search, stored, mobile);
    sfx.setVolumes(s.audio);
    client.setExposure(s.exposure);
    // What the screen shows (Settings, On screen; shared/ui/hud.json).
    applyHud(document, s.hud, mobile);
    app.mapShading = s.hud.mapShading;
    client.setWaterQuality(waterQualityLevel(s.water.quality));
    // A phone draws the water's cheaper reflection tiers (shared/look/reflections.json `phone`).
    client.setPhone(mobile);
    client.setWaterFlow(
      s.water.flowRipples,
      s.water.flowFoam,
      s.water.flowParticles,
      s.water.waterfalls,
    );
    client.setExplosions(s.explosions.flash, s.explosions.smoke, s.explosions.shake);
    // ?waterdbg=caustics,foam,… turns pieces of the water's shading off (a dev tool, DEV_TOOLS.md).
    const waterDebug = new URLSearchParams(location.search).get('waterdbg');
    if (waterDebug) client.setWaterDebug(waterDebug);
    // ?heightfield=1|0 forces the near water's GPU heightfield on or off (a dev tool, DEV_TOOLS.md).
    const heightfield = new URLSearchParams(location.search).get('heightfield');
    if (heightfield === '1' || heightfield === '0') client.setHeightfield(Number(heightfield));
    client.setClouds(s.clouds);
    // FXAA and the atlas's anisotropic filtering apply live (?aa= and ?aniso= override them).
    const aa = antialiasFromQuery(location.search, s.antialias);
    client.setAntialias(antialiasMode(aa.mode).fxaa, aa.anisotropy);
    // The skinned models' detail level (ADR 0070 §10); ?modeldetail=<level id> overrides it.
    client.setModelDetail(new URLSearchParams(location.search).get('modeldetail') ?? s.modelDetail);
    // ?chop=v2: the chopping prototype's crosshair aim (docs/WORK_QUEUE.md item 7; the local
    // server's half rides the world's start, net/connect.ts).
    client.setChopAim(/[?&]chop=v2(&|$)/.test(location.search));
    // The light shafts (ADR 0064); ?shafts=off|low|high overrides them (the bench).
    client.setShafts(new URLSearchParams(location.search).get('shafts') ?? s.shafts);
    // Render scale and its upscaler; ?renderscale= and ?upscaler= override them (the bench).
    const rs = renderScaleFromQuery(location.search, s.renderScale);
    client.setRenderScale(rs.scale, rs.upscaler, UPSCALE.sharpenStops);
    // ?shadows=off|low|high overrides the stored setting (the bench's configurations).
    client.setShadows(shadowLevel(shadowsFromQuery(location.search) ?? s.shadows.quality));
    // ?torchshadows=off|low|high likewise (docs/LIGHTS.md §4).
    client.setTorchShadows(shadowLevel(torchShadowsFromQuery(location.search) ?? s.shadows.torch));
    client.setMotion(s.motion.speedEffects, s.motion.fovBoost);
    client.setReducedMotion(prefersReducedMotion());
    client.setPlants(s.motion.plantSway ? 1 : 0, true);
    client.setFov(s.fovDeg);
    client.setShowArms(s.showArms);
    client.setDetail(s.detail.distanceM, s.detail.pixelError, s.detail.memoryMb);
    reportSettings = {
      renderScale: rs.scale,
      upscaler: rs.upscaler,
      fullDetailM: s.detail.distanceM,
      distantPixelError: s.detail.pixelError,
      distantMemoryMb: s.detail.memoryMb,
      preset: presetOf(s, mobile)?.id ?? 'custom',
    };
  });
  app.settings = settings;
  // While the menu is open the touch controls step aside: they would take its taps.
  settings.onOpenChange = (open) => {
    touch.visible = touchWanted && !open;
    sfx.action(open ? 'menu_open' : 'menu_close');
    // The speed effects hold still while the menu is open (or the big map).
    client.setMenuOpen(open || (app.minimap?.bigOpen ?? false));
  };
  // The controller (docs/GAMEPAD.md): polled every frame; Menu opens the game menu, and in it the
  // D-pad moves, A presses, B and Menu close it. Settings → Controller rebinds it.
  const pad = new GamepadInput({
    client,
    menuOpen: () => settings.isOpen || inventoryOpen(app),
    openMenu: () => {
      settings.setOpen(true);
    },
    closeMenu: () => {
      if (inventoryOpen(app)) setInventoryOpen(app, false);
      else settings.setOpen(false);
    },
    back: () => {
      if (inventoryOpen(app)) setInventoryOpen(app, false);
      else settings.setOpen(false);
    },
    inventory: () => {
      setInventoryOpen(app, !inventoryOpen(app));
    },
    map: {
      open: () => app.minimap?.bigOpen ?? false,
      toggle: () => app.minimap?.toggleBig(),
      pad: (values, axes, now) => app.minimap?.padFrame(values, axes, now),
    },
  });
  const applyPad = (s: GamepadSettings): void => {
    client.resetGamepadBindings();
    for (const a of PLAY_ACTIONS) client.setGamepadBinding(a, s.bindings[a]);
    client.setGamepadLook(s.lookSpeed, s.invertY);
  };
  const padSettings = loadGamepadSettings();
  applyPad(padSettings);
  const controller = new ControllerPanel({
    settings: padSettings,
    apply: (s) => {
      applyPad(s);
      saveGamepadSettings(s);
    },
    capture: (done) => {
      pad.capture = done;
    },
    connected: () => pad.active,
  });
  settings.addControllerPanel(
    controller.element,
    () => {
      controller.refresh();
    },
    () => {
      controller.cancel();
    },
  );
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
    {
      key: 'F8',
      run: () => {
        copyStreamingReport(app);
      },
    },
    {
      key: 'F7',
      run: () => {
        togglePauseLook(app);
      },
      on: () => client.lookPaused(),
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
  // ?pause=1 starts with the look paused (debug/pause.ts; the panoramas pause from script).
  if (pauseFromQuery(location.search)) client.setLookPaused(true);
  app.hud.onCopyBlocks = () => {
    copyBlockDump(app);
  };
  app.hud.onCopyStreaming = () => {
    copyStreamingReport(app);
  };
  client.flySpeed(loadFlySpeedLevel());
  app.flightSpeed.show(client.flySpeedLevel());
  window.__dwell = {
    state: () => client.debugState() as GameDebugState | null,
    press: (code, down) => {
      client.press(code, down);
      // The map's key is the shell's (ADR 0068), as a real key press is.
      if (down && code === 'KeyM') app.minimap?.toggleBig();
      // The backpack's key is the shell's too (ADR 0066 §10).
      else if (down && code === 'KeyE' && app.backpack?.usable)
        setInventoryOpen(app, !inventoryOpen(app));
      else if (down) app.minimap?.key(code);
    },
    look: (yaw, pitch) => {
      client.setLook(yaw, pitch);
    },
    view: () => {
      const [yaw = 0, pitch = 0] = client.look();
      return { yaw, pitch };
    },
    edit: (action) => client.edit(action, performance.now()),
    holdPlace: (on) => client.holdPlace(on, performance.now()),
    buildMode: () => client.cycleBuildMode(),
    tool: (name) => client.tool(name, performance.now()),
    brush: (command) => client.brush(command, performance.now()),
    select: (slot) => {
      client.select(slot);
      // The creative view follows at once (its `slot` event comes a frame later): a `pick` right
      // after a `select` (the bench's steps) fills the slot just selected, not the one before
      // (the `axe` scenario's trunk was put in slot 1 and filled with the pickaxe: nothing).
      app.creative?.set([...client.creativeHotbar()], slot);
    },
    pick: (item) => app.creative?.pickItem(item) ?? false,
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
    camera: () => client.camera() as Camera | null,
    find: (query, options) => findPlace(findHost(app), query, options),
    goto: (query, options) => gotoPlace(findHost(app), query, options),
    teleport: (feet) => teleport(app, feet),
    plants: (wind, shown) => {
      client.setPlants(wind, shown);
    },
    renderStats: () => client.renderStats() as RenderStats,
    setWater: (quality, debug, phone) => {
      if (quality !== null) client.setWaterQuality(waterQualityLevel(quality));
      if (debug !== null) client.setWaterDebug(debug);
      if (phone !== undefined) client.setPhone(phone);
    },
    setShadows: (quality) => {
      client.setShadows(shadowLevel(quality));
    },
    setView: (v) => {
      const s = app.settings?.current;
      if (!s) return;
      if (v.clouds !== undefined) client.setClouds(v.clouds);
      if (v.speedEffects !== undefined) client.setMotion(v.speedEffects, s.motion.fovBoost);
      if (v.distanceM !== undefined || v.pixelError !== undefined) {
        client.setDetail(
          v.distanceM ?? s.detail.distanceM,
          v.pixelError ?? s.detail.pixelError,
          s.detail.memoryMb,
        );
      }
      if (v.smoke !== undefined) {
        client.setExplosions(s.explosions.flash, v.smoke, s.explosions.shake);
      }
      if (
        v.flowParticles !== undefined ||
        v.flowRipples !== undefined ||
        v.flowFoam !== undefined ||
        v.waterfalls !== undefined
      ) {
        client.setWaterFlow(
          v.flowRipples ?? s.water.flowRipples,
          v.flowFoam ?? s.water.flowFoam,
          v.flowParticles ?? s.water.flowParticles,
          v.waterfalls ?? s.water.waterfalls,
        );
      }
    },
    outline: () => client.outline(),
    setOutline: (name) => client.setOutline(name),
    pixelArt: () => client.pixelArt(),
    setPixelArt: (name) => client.setPixelArt(name),
    setLodMap: (on) => client.setLodMap(on),
    mapTiles: () => {
      const m = app.minimap;
      if (!m) return null;
      const r = m.reveal;
      return {
        level: m.level,
        tiles: r.tiles.size,
        built: r.built,
        inFlight: r.inFlight,
        backgroundLeft: r.backgroundLeft,
      };
    },
    setLod: (on) => client.setLod(on),
    setProbe: (points) => client.setProbe(new Float64Array(points)),
    crossingRoll: () => client.crossingRoll(),
    setCrossingRoll: (seconds) => client.setCrossingRoll(seconds),
    memory: () => memoryReport(app),
    blockDump: () => blockDumpOf(app),
    streamingReport: async () => {
      const r = await streamingReportOf(app);
      copyText(app, Promise.resolve(formatStreamingReport(r)), 'Streaming report copied');
      return r;
    },
    telemetry: () => client.telemetry() as Telemetry,
    telemetryFrames: () => client.telemetryFrames() as { names: string[]; frames: number[][] },
    waterFlowAt: (x: number, y: number, z: number) => Array.from(client.waterFlowAt(x, y, z)),
    waterFalls: () => {
      const v = Array.from(client.waterFalls());
      const at = (k: number) => v[k] ?? 0;
      const out = [];
      for (let i = 0; i + 8 <= v.length; i += 8) {
        out.push({
          x: at(i),
          z: at(i + 1),
          dir: at(i + 2),
          width: at(i + 3),
          lip: at(i + 4),
          land: at(i + 5),
          flow: at(i + 6),
          speed: at(i + 7),
        });
      }
      return out;
    },
    blasts: () => {
      const v = Array.from(client.blasts());
      const at = (k: number) => v[k] ?? 0;
      const out = [];
      for (let i = 0; i + 4 <= v.length; i += 4) {
        out.push({ x: at(i), y: at(i + 1), z: at(i + 2), age: at(i + 3) });
      }
      return out;
    },
    creatures: () => {
      const v = Array.from(client.creatures());
      const at = (k: number) => v[k] ?? 0;
      const out = [];
      for (let i = 0; i + 5 <= v.length; i += 5) {
        out.push({ x: at(i), y: at(i + 1), z: at(i + 2), yaw: at(i + 3), speed: at(i + 4) });
      }
      return out;
    },
    ticks: () => client.telemetryTicks() as TickSample[],
    telemetryReset: () => {
      client.telemetryReset();
    },
    sounds: () => [...sfx.recent],
    serverStats: () => app.loopback?.serverStats ?? null,
    waterReport: (x, y, z, radius) =>
      app.loopback?.waterReport([x, y, z], radius ?? WATER_REPORT_RADIUS) ?? Promise.resolve(null),
    die: () => {
      app.loopback?.postToWorker({ t: 'kill' });
      return app.loopback !== null;
    },
    give: (item, count) => {
      app.loopback?.postToWorker({ t: 'give', item, count });
      return app.loopback !== null;
    },
    carry: (speed, yaw, seconds, lift) => {
      app.loopback?.postToWorker({ t: 'carry', speed, yaw, seconds, lift: lift ?? 1 });
      return app.loopback !== null;
    },
    waterDebug: (names) => {
      client.setWaterDebug(names);
    },
    setTime: (t, moon) => {
      client.setTimeOfDay(t ?? undefined);
      if (moon !== undefined) client.setMoonPhase(moon ?? undefined);
    },
    pause: (on) => {
      if (on !== undefined) client.setLookPaused(on);
      return client.lookPaused();
    },
  };

  // The HUD's layout (shared/ui/layout.json): placed again when the screen or a part's size changes.
  const layout = new LayoutRunner(document, HUD_LAYOUT, mobile, LAYOUT_EVERY_MS);
  const frame = (now: number): void => {
    pad.poll(now);
    // Only the frames drawn count (one is skipped while the GPU is behind).
    app.fps.frame(now, client.frame(now));
    handleEvents(app);
    playSounds(app, sounds, beds);
    showHud(app, now);
    if (app.minimap) {
      app.minimap.shading = app.mapShading;
      app.minimap.frame(now);
    }
    layout.frame(now);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
  return app;
}

/** Moves the local player's feet (a local world; the dev tools' `goto`). */
function teleport(app: App, feet: Vec3): boolean {
  app.loopback?.postToWorker({ t: 'teleport', at: feet });
  return app.loopback !== null;
}

/** The page's side of the dev tools' place search (debug/find.ts). */
function findHost(app: App): FindHost {
  const { client } = app;
  return {
    feet: () => (client.debugState() as GameDebugState | null)?.feet ?? null,
    find: (query, [x, y, z], radius, surface, minDepth, minArea, minCurrent, face, keep) =>
      client.find(
        query,
        x,
        y,
        z,
        radius,
        surface,
        minDepth,
        minArea,
        minCurrent,
        face,
        keep,
      ) as FoundPlace | null,
    viewFound: (height, aim, yaw, [x, y, z], pitch, turn) =>
      client.viewFound(height, aim, yaw, x, y, z, pitch, turn) as Camera,
    fly: (on) => {
      client.fly(on);
    },
    teleport: (feet) => teleport(app, feet),
    findOnMap: (kind, at, radius, face, minDepth) =>
      app.loopback?.findOnMap(kind, at, radius, face, minDepth) ?? Promise.resolve(null),
    wait: (ms) => new Promise((r) => setTimeout(r, ms)),
  };
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

const itemPictures = new Map<string, HotbarSlot>();

/** An inventory item's picture (ADR 0066), drawn once per item. */
function itemPicture(item: string): HotbarSlot {
  let p = itemPictures.get(item);
  if (!p) {
    p = { name: item, swatch: itemSwatch(item), size: swatchSize() };
    itemPictures.set(item, p);
  }
  return p;
}

/** Whether the inventory view is open: the backpack (a world that uses items) or the creative one. */
function inventoryOpen(app: App): boolean {
  return (app.backpack?.open ?? false) || (app.creative?.open ?? false);
}

/**
 * Opens or closes the inventory view: the backpack in a world that uses items, else the creative
 * catalogue. While it is open the game holds still and the controller moves its focus.
 */
function setInventoryOpen(app: App, open: boolean): void {
  const usesItems = app.backpack?.usable ?? false;
  if (app.backpack) app.backpack.open = open && usesItems;
  if (app.creative) app.creative.open = open && !usesItems;
  if (open) document.exitPointerLock();
  // The touch controls step aside but for the bag button, which closes it again.
  document.body.classList.toggle('inventory-open', open);
  app.client.setMenuOpen(
    open || (app.settings?.isOpen ?? false) || (app.minimap?.bigOpen ?? false),
  );
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
  // The creative hotbar's picks (docs/ITEMS.md §6a), saved per world in this browser.
  const items = app.client.paletteItems();
  const worldKey = `${String(joined.generatorVersion)}:${String(joined.worldSeed)}`;
  const store = (() => {
    try {
      return window.localStorage;
    } catch {
      return null;
    }
  })();
  const saved = store ? loadPicks(store, worldKey, items) : null;
  saved?.forEach((index, slot) => app.client.setCreativeSlot(slot, index));
  app.hotbar.setPicks([...app.client.creativeHotbar()]);
  app.creative = new CreativeInventory(
    document.body,
    items,
    (i) => slots[i] ?? { name: '', swatch: new Uint8Array(), size: 0 },
    (slot, index) => {
      app.client.setCreativeSlot(slot, index);
      const picks = [...app.client.creativeHotbar()];
      app.hotbar?.setPicks(picks);
      if (store) savePicks(store, worldKey, picks, items);
      audio().action('hotbar');
      return picks;
    },
    (slot) => {
      app.client.select(slot);
    },
  );
  app.touch?.setOnBag(() => {
    setInventoryOpen(app, !inventoryOpen(app));
  });
  app.client.select(0);
  // The backpack screen (ADR 0066): E opens and closes it in a world that uses items.
  app.backpack = new Backpack(
    document.body,
    itemPicture,
    (from, to, count) => {
      app.client.moveItems(from, to, count);
    },
    (recipe) => {
      app.client.combine(recipe);
    },
  );
  window.addEventListener(
    'keydown',
    (e) => {
      if (!app.playing || (app.settings?.isOpen ?? false)) return;
      // Typing in the inventory's search: only Escape is the view's.
      if (e.target instanceof HTMLInputElement && e.code !== 'Escape') return;
      // E is the inventory only: a ground item is knocked loose by the break action (ADR 0071).
      const open = backpackKey(e.code, e.repeat, inventoryOpen(app), true);
      if (open === null) return;
      setInventoryOpen(app, open);
      e.preventDefault();
      e.stopPropagation();
    },
    true,
  );
  // The build mode as the game has it: a `buildMode` event may have come before the hotbar.
  app.hotbar.setBuildMode(app.client.buildMode());
  app.touch?.setBuildMode(app.client.buildMode());
  app.touch?.setFlight(joined.mayFly, app.client.flying());
  startDebugTools(app);
  document.body.classList.add('in-game');
  // The start hint once the view is uncovered (at once without the loading screen).
  const hint = prefersTouch() ? TOUCH_START_HINT : START_HINT;
  if (app.loading.phase === 'waiting') {
    app.loading.onOpen = () => {
      app.loading.onOpen = null;
      app.hud.showHint(hint);
    };
  } else {
    app.hud.showHint(hint);
  }
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
function blockDumpOf(app: App, water?: unknown): BlockDump | null {
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
    ...(water === undefined ? {} : { water }),
  });
}

/** How far around the player (cells) the block dump reports the local server's water. */
/** How often the HUD's layout looks for parts made since its last run (ms). */
const LAYOUT_EVERY_MS = 250;

const WATER_REPORT_RADIUS = 48;

/**
 * The local server's water around the player (a local world only; docs/DEV_TOOLS.md): parsed,
 * or undefined where there is none or it doesn't answer within a second.
 */
async function waterReportOf(app: App): Promise<unknown> {
  const s = app.client.debugState() as GameDebugState | null;
  if (!app.loopback || !s) return undefined;
  const [x, y, z] = s.feet;
  const at: [number, number, number] = [Math.floor(x), Math.floor(y / 0.5), Math.floor(z)];
  const report = await Promise.race([
    app.loopback.waterReport(at, WATER_REPORT_RADIUS),
    new Promise<null>((resolve) => {
      setTimeout(() => {
        resolve(null);
      }, 1000);
    }),
  ]);
  if (report === null) return undefined;
  try {
    return JSON.parse(report) as unknown;
  } catch {
    return report;
  }
}

/** The settings last applied, for the streaming report. */
let reportSettings: ReportSettings | null = null;

/** The streaming report (debug/streamingReport.ts): the game's part and the shell's. */
async function streamingReportOf(app: App): Promise<StreamingReport> {
  const gpu = await gpuName();
  let game: Record<string, unknown> | null;
  try {
    game = JSON.parse(app.client.streamingReport()) as Record<string, unknown> | null;
  } catch {
    game = null;
  }
  const q = new URLSearchParams(location.search);
  return streamingReport({
    build: {
      version: buildInfo.version,
      sha: buildInfo.sha,
      channel: channelOf(buildInfo.version),
    },
    game,
    device: deviceOf(gpu, app.client.cpuAdapter()),
    settings: reportSettings,
    world: {
      local: app.loopback !== null,
      seed: app.world ? app.world.seed.toString() : null,
      generatorVersion: app.world?.generatorVersion ?? null,
      type: q.get('world'),
      mode: q.get('mode'),
      compatibilityLine: compatibilityLine(buildInfo.version),
    },
    ...(app.client.flying() ? { flySpeedLevel: app.client.flySpeedLevel() } : {}),
  });
}

/** Copies the streaming report (F8, or the Debug page's button), or shows it to copy by hand. */
function copyStreamingReport(app: App): void {
  if (!app.client.debugState()) {
    app.hud.flash('No world to report yet');
    audio().ui('error');
    return;
  }
  copyText(
    app,
    streamingReportOf(app).then((r) => formatStreamingReport(r)),
    'Streaming report copied',
  );
}

/**
 * Copies `text` to the clipboard (given as a promise within the tap that asked: Safari's rule),
 * or shows it in the HUD's text box to copy by hand where that is refused.
 */
function copyText(app: App, text: Promise<string>, done: string): void {
  const shown = () => {
    void text.then((t) => {
      app.hud.showDump(t);
    });
  };
  const copied = () => {
    app.hud.flash(done);
    audio().action('copy');
  };
  try {
    if (typeof ClipboardItem !== 'undefined') {
      const blob = text.then((t) => new Blob([t], { type: 'text/plain' }));
      navigator.clipboard.write([new ClipboardItem({ 'text/plain': blob })]).then(copied, shown);
    } else {
      void text.then((t) => navigator.clipboard.writeText(t)).then(copied, shown);
    }
  } catch {
    shown(); // no clipboard (an insecure context)
  }
}

/** Copies the block dump to the clipboard, or shows it to copy by hand where that is refused. */
function copyBlockDump(app: App): void {
  if (!blockDumpOf(app)) {
    app.hud.flash('No world to dump yet');
    audio().ui('error');
    return;
  }
  // In a local world the dump carries the server's water near the player, asked of the worker:
  // the clipboard is given a promise of the text, within the tap that asked (Safari's rule).
  const text = waterReportOf(app).then((water) => {
    const dump = blockDumpOf(app, water);
    return dump ? formatBlockDump(dump) : '';
  });
  copyText(app, text, 'Block info copied');
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
      {
        name: cleanName(launch.name, 'New world'),
        type,
        seed: parseSeed(launch.seed),
        ...(launch.spawn !== '' && isSpawn(launch.spawn) ? { spawn: launch.spawn } : {}),
        ...(isGameMode(launch.mode) ? { mode: launch.mode } : {}),
      },
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
  bootLoading?.abort();
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
      // The world's picture is the player's view as they quit (worldThumbs.ts captureNow).
      const pictured = app.captureThumb ? app.captureThumb() : Promise.resolve();
      const saved = save ? save() : Promise.resolve(true);
      // Leave even if the save hangs; the world also saved every few seconds.
      const timeout = new Promise((resolve) => setTimeout(resolve, 5000));
      void Promise.all([pictured, Promise.race([saved, timeout])]).then(() => {
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
  /** The local world's id in the index, for its thumbnail (local/worldThumbs.ts). */
  let thumbId: string | null = null;
  if (launch.kind === 'play') {
    const world = index.get(launch.id);
    if (!world) {
      openMainMenu(app, 'That world is not in this browser any more.');
      return;
    }
    const access = worldAccess(world, buildInfo.version);
    let playing = world;
    if (!access.ok && access.upgrade) {
      // An older line's world, from the version page's choice of this build (owner,
      // 2026-11-06): a warning to confirm, then the world itself or a copy of it.
      status.textContent = '';
      const answer = await showUpgradePrompt(
        document,
        world.name,
        access.upgrade.from,
        buildInfo.version,
      );
      if (answer === 'cancel') {
        openMainMenu(app);
        return;
      }
      if (answer === 'copy') {
        const copy = index.copy(world, Date.now());
        if (!(await copyWorldFile(world.id, copy.id))) {
          index.remove(copy.id);
          openMainMenu(
            app,
            `"${world.name}" couldn't be copied. If it is open in another tab, close it there first.`,
          );
          return;
        }
        playing = copy;
        // A reload goes on with the copy.
        history.replaceState(null, '', withRoute(location.search, { play: copy.id }));
      }
    } else if (!access.ok) {
      openMainMenu(app, access.message);
      return;
    }
    // The world file is opened from here to the local world's worker (which ends the stage).
    app.loading.stages.begin('worldFile', performance.now());
    // The world's chosen spawn is its first one: read only when its file is created.
    localWorld = {
      worldSeed: playing.seed,
      generatorVersion: playing.generatorVersion,
      file: playing.id,
      ...(isSpawn(playing.spawn) ? spawnOptions(playing.spawn) : {}),
      ...(!access.ok && access.upgrade ? { upgrade: true } : {}),
      ...(playing.mode && isGameMode(playing.mode) ? { mode: playing.mode } : {}),
    };
    worldName = playing.name;
    thumbId = playing.id;
    // A test world (Chisel's Play in game): its maps from its recorded worldgen config.
    worldgenConfig = playing.worldgen ?? '';
    index.touch(playing.id, Date.now());
  } else if (launch.kind === 'link') {
    app.loading.stages.begin('worldFile', performance.now());
    localWorld = parseLocalWorld(location.search);
    // Dev link `?worldgen=test:tall_peak` (docs/DEV_TOOLS.md "Play a test world"): a terrain world
    // of the fixture's ground, its own file and record (which keeps the config).
    const worldgen = params.get('worldgen') ?? '';
    if (worldgen) {
      localWorld = {
        ...localWorld,
        generatorVersion: GENERATORS.terrain,
        file: testWorldId(worldgen, localWorld.worldSeed),
      };
      worldgenConfig = worldgen;
    }
    // A link to a world file that is already there opens a world saved before versioned releases
    // (ignored); a new one is this build's.
    const name =
      localWorld.file ?? localWorldName(localWorld.generatorVersion, localWorld.worldSeed);
    // Dev link `?fresh=1`: the link's world starts again from its seed (docs/DEV_TOOLS.md), whatever
    // build saved it.
    if (params.get('fresh') === '1') {
      if (!(await deleteWorldFiles(name))) {
        openMainMenu(app, 'That world is open in another tab. Close it there first.');
        return;
      }
      index.remove(name);
    }
    const existed = (await listWorldFiles()).includes(name);
    const world = worldgen
      ? index.adoptTest(worldgen, localWorld.worldSeed, Date.now())
      : index.adopt(name, Date.now(), existed ? undefined : recordedVersion());
    const access = world ? worldAccess(world, buildInfo.version) : null;
    if (access && !access.ok) {
      openMainMenu(app, access.message);
      return;
    }
    if (world) {
      worldName = world.name;
      thumbId = world.id;
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
    worldMaps: (seed, version) => worldMaps(seed, version, mapDeps(), worldgenConfig),
    // The local world's worker's stages (the world file, its server, the spawn), on this clock.
    onStage: (id, phase, atMs) => {
      // By id from the worker: an unknown one is ignored (the tracker checks it against the data).
      const stage = id as LoadingStageId;
      if (phase === 'begin') app.loading.stages.begin(stage, atMs);
      else app.loading.stages.end(stage, atMs);
    },
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
    const stages = app.loading.stages;
    if (code || invite) stages.begin('connect', performance.now());
    if (code) {
      const base = configuredMasterUrl();
      if (!base) throw new Error('this build has no master server configured');
      ({ onJoined } = await connectToCode(client, new MasterClient(base, key), code, options));
    } else if (invite) {
      await connectToInvite(client, invite, preference);
    } else {
      local = await connectLocal(client, key, options);
      app.loopback = local.loopback;
      // The world card's picture: the world's view now and then, and when the page is left.
      if (thumbId) {
        const thumbs = startThumbnailCapture(app.canvas, new WorldThumbs(storage()), thumbId);
        app.captureThumb = thumbs.captureNow;
      }
      // The respawn preload (§6.3) off for the bench's comparison: the server's part.
      if (/[?&]respawnPreload=off(&|$)/.test(location.search)) {
        local.loopback.postToWorker({ t: 'respawnPreload', on: false });
      }
    }
    // Connected: the join's handshake until the client places the player (its readout then says).
    stages.end('connect', performance.now());
    stages.begin('join', performance.now());
    enableGameMenu(app, local?.save ?? null);
    app.settings?.setSubtitle(localWorld ? `${target} · this device` : target);
    // A local world can be opened to friends (Host…, Phase 5c).
    if (local && app.settings) {
      const id = thumbId;
      enableHosting(app.settings, local, prefersTouch(), worldName, (mode) => {
        if (id) index.setMode(id, mode);
      });
    }
    let started = false;
    let asking = false;
    const perf = element('perf-status', HTMLDivElement);
    const tickRate = new TickRate();
    const showStatus = () => {
      const s = client.sessionState() as SessionReport;
      const stats = s.stats ?? {
        rttMs: null,
        datagramRttMs: null,
        serverTick: 0,
        hostPaused: false,
      };
      status.textContent = formatStatus(target, s.transport, s, stats);
      // What holds the frame back (owner, 2026-11-06): the frames drawn, the CPU's and the GPU's
      // time, the server's ticks (and a local server's time per tick).
      if (s.phase === 'joined') {
        tickRate.sample(performance.now(), stats.serverTick);
        const [cpuMs = null, gpuMs = null, pacedShare = null, noSurfaceShare = null] =
          client.perf();
        perf.textContent = formatPerf({
          fps: app.fps.fps,
          animationFps: app.fps.animationFps,
          pacedShare,
          noSurfaceShare,
          cpuMs,
          gpuMs,
          serverTicks: app.loopback?.serverTicks ?? tickRate.rate,
          serverTickMs: app.loopback?.serverTickMs ?? null,
        });
      } else {
        perf.textContent = '';
      }
      // Turned away or cut off: the status line says why, not a loading screen that never opens.
      if (s.phase === 'rejected' || s.phase === 'closed') app.loading.abort();
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
        startMinimap(app, localWorld ? `local:${localWorld.file ?? target}` : serverKey);
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
    app.loading.abort();
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
/** The game page's loading screen: up from the page's start (none on the menu page). */
let bootLoading: LoadingScreen | undefined;

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
  // The tools need the module, which needs WebAssembly SIMD (engine/simd.ts); the menu works without.
  if (hasWasmSimd()) void loadTools();
} else {
  // The loading screen first, before the client starts or anything of the game shows (§6.3).
  bootLoading = new LoadingScreen(
    document.body,
    loadingScreenEnabled(location.search, storage()),
    prefersReducedMotion(),
  );
  // The page itself, from opening it to this script running (the HTML and the script's download).
  // The bar's opening guess (§6.3): the device as the browser reads it, and the route (joining a
  // server runs no local world).
  bootLoading.device = readDevice(
    navigator,
    prefersTouch(),
    workerCount(location.search, navigator.hardwareConcurrency, prefersTouch()),
  );
  const route = launchOf(location.search).kind;
  bootLoading.remote = route === 'code' || route === 'join';
  // From the launcher's opening when it sent this page here (its stamp, read once).
  let launchAt: string | null = null;
  try {
    launchAt = sessionStorage.getItem(LAUNCH_AT_KEY);
    sessionStorage.removeItem(LAUNCH_AT_KEY);
  } catch {
    // no session storage: from this page's opening
  }
  bootLoading.stages.begin(
    'page',
    pageStartMs(launchAt, performance.timeOrigin, performance.timeOrigin + performance.now()),
  );
  bootLoading.stages.end('page', performance.now());
  const bootStages = bootLoading.stages;
  window.__dwellStages = () => bootStages.timeline();
  const bootScreen = bootLoading;
  window.__dwellLoadingBar = () => bootScreen.barState;
  void routeMadeWorld().then(async (ok) => {
    if (!ok) return;
    const app = await start();
    if (app) void connect(app);
  });
}
