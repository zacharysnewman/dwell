import { buildInfo, formatBuildInfo, recordedVersion } from './buildInfo';
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
import { configuredMasterUrl, MasterClient, serverInvite } from './net/master';
import { IndexedDbKeyStore, loadOrCreateDeviceKey } from './identity/deviceKey';
import { parseInvite } from './net/invite';
import type { SessionReport } from './net/sessionState';
import { sameLine } from './version/semver';
import { parseLocalWorld, type LocalWorld } from './local/world';
import { WorldIndex } from './local/worldIndex';
import { worldAccess } from './local/worldAccess';
import { deleteWorldFiles, listWorldFiles, localWorldName } from './local/worldFiles';
import { launchOf, launcherHref, pastedInvite } from './ui/launch';
import { MainMenu, type MainMenuDeps } from './ui/mainMenu';
import { loadRecent, rememberServer } from './ui/recentServers';
import { prefersTouch, TouchControls } from './ui/touchControls';
import { Hotbar } from './ui/hotbar';
import { Hud } from './ui/hud';
import { MAP_SIZE, MapOverlay } from './ui/mapOverlay';
import { SettingsMenu } from './ui/settingsMenu';
import { FlightSpeedControl } from './ui/flightSpeedControl';
import { loadFlySpeedLevel, saveFlySpeedLevel } from './predict/flightSpeed';
import { formatStatus } from './ui/statusOverlay';
import { FpsMeter } from './ui/fps';
import { formatMemory, jsHeapBytes, type MemoryReport } from './ui/memory';
import type { LoopbackTransport } from './net/loopback';

/** Most time a lobby ping may take, connecting included (§8.3). */
const PING_TIMEOUT_MS = 5000;

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
  /** The last frame's draw calls and triangles. */
  renderStats(): RenderStats;
  /** What the game knows it holds in memory (ui/memory.ts), once playing. */
  memory(): MemoryReport | null;
  /** The block dump around the target (debug/blockDump.ts), or null before playing. */
  blockDump(): BlockDump | null;
  /** The app version that last saved a world file (its bytes), or undefined. */
  worldFileVersion(bytes: Uint8Array): string | undefined;
  /** A world file's bytes rewritten to say `version` last saved it (RELEASES.md §6). */
  setWorldFileVersion(bytes: Uint8Array, version: string): Uint8Array;
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
      if (e.value === 'F4') app.map.toggle();
      if (e.value === 'Minus') app.map.zoomBy(1);
      if (e.value === 'Equal') app.map.zoomBy(-1);
    } else if (e.t === 'slot') {
      app.hotbar?.setSelected(e.value as number);
    } else if (e.t === 'flight' || e.t === 'flightAllowed') {
      const flying = app.client.flying();
      app.touch?.setFlight(app.client.flightAllowed(), flying);
      app.flightSpeed.visible = flying;
    } else if (e.t === 'flySpeed') {
      app.flightSpeed.show(e.value as number);
      saveFlySpeedLevel(e.value as number);
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
    wasmMemory = ((await loadWasm()) as { memory?: WebAssembly.Memory }).memory ?? null;
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
    showFatal(
      'navigator' in globalThis && !('gpu' in navigator)
        ? 'Dwell needs WebGPU, which this browser does not have (or has turned off).'
        : `Dwell failed to start: ${err instanceof Error ? err.message : String(err)}`,
    );
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
  touch.visible = mobile;
  window.addEventListener('touchstart', () => (touch.visible = true), {
    once: true,
    passive: true,
  });
  forwardInput(app);
  // Settings (top left): fog, detail, exposure and the sky, applied as the sliders move.
  app.settings = new SettingsMenu(document.body, mobile, (s) => {
    client.setFog(s.fog.distanceM, s.fog.density, s.fog.heightM);
    client.setExposure(s.exposure);
    client.setSkyRotation(s.skyRotation);
    client.setDetail(s.detail.distanceM, s.detail.pixelError, s.detail.memoryMb);
  });
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
    renderStats: () => client.renderStats() as RenderStats,
    memory: () => memoryReport(app),
    blockDump: () => blockDumpOf(app),
    worldFileVersion: (bytes) => worldFileVersion(bytes),
    setWorldFileVersion: (bytes, version) => setWorldFileVersion(bytes, version),
  };

  const frame = (now: number): void => {
    app.fps.frame(now);
    client.frame(now);
    handleEvents(app);
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
    return;
  }
  const text = formatBlockDump(dump);
  const shown = () => {
    app.hud.showDump(text);
  };
  try {
    navigator.clipboard.writeText(text).then(() => {
      app.hud.flash('Block info copied');
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
      const w = invite.webrtc;
      const rtc = w ? [w.ip, w.port, w.ufrag, w.pwd] : undefined;
      await loadWasm();
      return pingServer(invite.url, invite.certHash, rtc, PING_TIMEOUT_MS);
    },
  };
}

function openMainMenu(app: App, message?: string): void {
  if (app.touch) app.touch.visible = false;
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

void start().then((app) => {
  if (app) void connect(app);
});
