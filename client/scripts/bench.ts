// The dev bench (docs/DEV_TOOLS.md): runs scenarios (client/bench/scenarios.json,
// scripts/scenarios.ts) in a real browser and records what the game reports through
// `window.__dwell`: frame-phase timings, job latencies, uploads, the player controller's ticks, the
// browser's long tasks, screenshots and Chrome performance traces.
//   node --experimental-strip-types scripts/bench.ts [scenario…] [options]
// Options:
//   --base URL       a served build (default: serves dist/ with `vite preview` on :4173, or reuses one)
//   --out DIR        where reports go (default bench/out/<timestamp>)
//   --swiftshader    software WebGPU (no GPU: CI, Linux sandboxes); default is Chrome on the GPU
//   --unthrottled    no vsync or frame-rate cap: frame intervals show the frame's real cost
//   --file FILE      another scenarios file
//   --cpu-throttle N slow the page's CPU N× (CDP Emulation.setCPUThrottlingRate): a phone-like CPU
//   --touch          a touch device (coarse pointer, touch events): the phone's touch UI
//   --compare        each scenario once per configuration, compared (scripts/compare.ts)
//   --configs A,B    with --compare: these configurations (and the baseline) only
//   --configs-file F with --compare: another configurations file (default bench/configs.json)
//   --spot NAME      shoot a saved spot (bench/spots.json, e.g. starter:broadleaf) in its own world
//   --spots-file F   another spots file
//   --warm           the warm prebuilt client (scripts/warmClient.ts): built only when its sources
//                    changed, served by a long-lived preview on :4174
//   --stall-s S      a load not playing within S s (60) is retried once in a fresh page
// Build first: `npm run build:wasm && npm run build`. Per scenario, OUT/<name>/ gets report.json
// (each `record` step: telemetry summary, raw frames, ticks, long tasks, state), the screenshots and
// traces; a summary prints as it goes.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium, type Browser, type BrowserContext, type Page } from '@playwright/test';
import { CODEC, compare, decode, type ScenarioReport } from './compare.ts';
import { flickerOf, type Flicker } from './compareReport.ts';
import { formatProfile, profileOf, work } from './profile.ts';
import {
  buildPage,
  parseScenarios,
  scenarioUrl,
  spotScenario,
  scenarioView,
  timelapseTimes,
  clipStripCamera,
  clipStripSheet,
  clipStripTimes,
  swingClip,
  overBudget,
  type Scenario,
  type Step,
  type Vec3,
} from './scenarios.ts';
import {
  eyeOver,
  findSpot,
  groundRows,
  OPEN_HEIGHT_M,
  parseSpots,
  spotParams,
  type Placement,
  type Spot,
} from './spots.ts';
import { viewStorage } from './views.ts';
import { warmServe } from './warmClient.ts';
import { CELL_HEIGHT } from '../src/protocol/constants.gen.ts';
import type { GotoResult } from '../src/debug/find.ts';

interface Options {
  names: string[];
  base: string | null;
  out: string;
  swiftshader: boolean;
  unthrottled: boolean;
  file: string;
  compare: boolean;
  configs: string[];
  configsFile: string;
  cpuThrottle: number;
  touch: boolean;
  /** Saved spots to shoot (`--spot NAME`, bench/spots.json), each in its own world. */
  spots: string[];
  spotsFile: string;
  /** Use the warm prebuilt client (scripts/warmClient.ts) and its long-lived preview server. */
  warm: boolean;
  /** A load not playing within this many seconds is retried once in a fresh page. */
  stallS: number;
}

/** The page emulation every scenario's context gets (`--cpu-throttle`, `--touch`). */
const emulation = { cpuThrottle: 1, touch: false };
/** The run's saved spots and the stall bar (`--spots-file`, `--stall-s`). */
const runWide: { spots: Spot[]; stallS: number } = { spots: [], stallS: 60 };

function options(argv: string[]): Options {
  const o: Options = {
    names: [],
    base: null,
    out: join('bench', 'out', new Date().toISOString().replace(/[:.]/g, '-')),
    swiftshader: false,
    unthrottled: false,
    file: 'bench/scenarios.json',
    compare: false,
    configs: [],
    configsFile: 'bench/configs.json',
    cpuThrottle: 1,
    touch: false,
    spots: [],
    spotsFile: 'bench/spots.json',
    warm: false,
    stallS: 60,
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = (): string => {
      i += 1;
      if (i >= argv.length) throw new Error(`${a} needs a value`);
      return argv[i];
    };
    if (a === '--base') o.base = value();
    else if (a === '--out') o.out = value();
    else if (a === '--file') o.file = value();
    else if (a === '--swiftshader') o.swiftshader = true;
    else if (a === '--unthrottled') o.unthrottled = true;
    else if (a === '--compare') o.compare = true;
    else if (a === '--configs') o.configs = value().split(',').filter(Boolean);
    else if (a === '--configs-file') o.configsFile = value();
    else if (a === '--cpu-throttle') {
      o.cpuThrottle = Number(value());
      if (!(o.cpuThrottle >= 1)) throw new Error('--cpu-throttle needs a number ≥ 1');
    } else if (a === '--touch') o.touch = true;
    else if (a === '--spot') o.spots.push(value());
    else if (a === '--spots-file') o.spotsFile = value();
    else if (a === '--warm') o.warm = true;
    else if (a === '--stall-s') {
      o.stallS = Number(value());
      if (!(o.stallS >= 5)) throw new Error('--stall-s needs a number of seconds ≥ 5');
    } else if (a.startsWith('--')) throw new Error(`unknown option ${a}`);
    else o.names.push(a);
  }
  return o;
}

async function reachable(url: string): Promise<boolean> {
  try {
    return (await fetch(url)).ok;
  } catch {
    return false;
  }
}

/** A served build: `--base`, one already on :4173, or `vite preview` started for the run. */
async function serve(base: string | null): Promise<{ url: string; server: ChildProcess | null }> {
  // The server's root, or a build's path (`…/dwell/`, a release's `…/dwell/v/<version>/`):
  // `buildPage` opens either.
  if (base) return { url: base.replace(/\/+$/, ''), server: null };
  const url = 'http://localhost:4173';
  if (await reachable(`${url}/dwell/`)) return { url, server: null };
  const server = spawn('npx', ['vite', 'preview', '--port', '4173', '--strictPort'], {
    stdio: 'ignore',
  });
  for (let i = 0; i < 100 && !(await reachable(`${url}/dwell/`)); i++) {
    await new Promise((r) => setTimeout(r, 200));
  }
  return { url, server };
}

function launch(o: Options): Promise<Browser> {
  const common = [
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    '--no-proxy-server',
    // Full-precision WebGPU timestamps for the `gpu.<pass>` samples (otherwise Chrome rounds them
    // to 65.536 µs; `--enable-features=WebGPUDeveloperFeatures` does not lift that, checked
    // 2026-10-14).
    '--enable-webgpu-developer-features',
    ...(o.unthrottled ? ['--disable-gpu-vsync', '--disable-frame-rate-limit'] : []),
  ];
  if (o.swiftshader) {
    return chromium.launch({
      headless: false,
      ...(process.env.DWELL_CHROMIUM ? { executablePath: process.env.DWELL_CHROMIUM } : {}),
      args: [
        '--enable-unsafe-swiftshader',
        '--enable-unsafe-webgpu',
        '--enable-features=Vulkan',
        '--use-vulkan=swiftshader',
        '--use-webgpu-adapter=swiftshader',
        ...common,
      ],
    });
  }
  // The installed Chrome on the machine's GPU (Metal on macOS).
  return chromium.launch({
    headless: false,
    ...(process.env.DWELL_CHROMIUM
      ? { executablePath: process.env.DWELL_CHROMIUM }
      : { channel: 'chrome' }),
    args: common,
  });
}

/** Evaluates `expression` with `d` bound to `__dwell` (string evaluation: the page's CSP). */
function call<T>(page: Page, expression: string): Promise<T> {
  return page.evaluate<T>(
    `(() => { const d = globalThis.__dwell; return d ? (${expression}) : null; })()`,
  );
}

async function until(page: Page, expression: string, timeoutMs: number): Promise<boolean> {
  const deadline = Date.now() + timeoutMs;
  while ((await call<boolean | null>(page, expression)) !== true) {
    if (Date.now() > deadline) return false;
    await page.waitForTimeout(100);
  }
  return true;
}

const sleep = (page: Page, s: number): Promise<void> => page.waitForTimeout(s * 1000);

/** Long tasks (> 50 ms on the main thread), collected from the page's start. */
const LONG_TASKS = `
  globalThis.__benchLongTasks = [];
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) globalThis.__benchLongTasks.push([e.startTime, e.duration]);
    }).observe({ type: 'longtask', buffered: true });
  } catch {}
  // The page's frame gaps over 100 ms in its first 30 s ([startMs, ms]): a screen that did not
  // repaint (the main thread held, or the browser's compositor waiting on the GPU process).
  globalThis.__benchFrameGaps = [];
  try {
    let last = null;
    const tick = (t) => {
      if (last !== null && t - last > 100) globalThis.__benchFrameGaps.push([last, t - last]);
      last = t;
      if (t < 30000) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  } catch {}
`;

interface Telemetry {
  frames: Record<string, Stats | undefined>;
  samples: Record<string, Stats | undefined>;
  counters: Record<string, number>;
  frameCount: number;
}
interface Stats {
  n: number;
  mean: number;
  p50: number;
  p95: number;
  p99: number;
  max: number;
  sum: number;
}

interface Run {
  page: Page;
  browser: Browser;
  dir: string;
  markMs: number;
  records: Record<string, unknown>;
  /** Each `settle` step's seconds, in order. */
  settles: number[];
  /** When the page was opened (`Date.now()`): the join's timeline is measured from it. */
  openedAt: number;
  /** Each `until` step's seconds since the page was opened (null: not within its maxS). */
  untils: { what: string; s: number | null }[];
  /** Each `die` step's respawn → playing seconds, and the loading screen's state then. */
  respawns: { s: number | null; loading: Record<string, unknown> | null }[];
  /** Each `burst` step's flicker, by name. */
  bursts: Record<string, Flicker>;
  /** Each `server` step's samples of the local server's figures, by name. */
  servers: Record<string, unknown[]>;
  /** The scenario's face's up along y (1 face A, −1 face B): `aboveFeet` heights are along it. */
  up: number;
  /** Where the last `camera` step placed the camera (a drifting burst moves on from it). */
  camera?: { position: [number, number, number]; yaw: number; pitch: number };
  /** Each `goto` step's result (`__dwell.goto`): what it found and where it put the camera. */
  gotos: GotoResult[];
  /** The saved spots (bench/spots.json) a `spot` step names. */
  spots: Spot[];
  /** Each placed `shot`'s camera and how long its view took to settle. */
  shots: ShotResult[];
}

interface ShotResult {
  name: string;
  eye: Vec3;
  yaw: number;
  pitch: number;
  face: 'a' | 'b';
  /** The ground's top under an `above` shot (m), or null. */
  groundY: number | null;
  /** Seconds from the step's start to the shot; `settled` false: shot at `maxS` regardless. */
  s: number;
  settled: boolean;
}

/** The top of the ground's column at x, z on `face` among the loaded chunks (m), or null. */
async function groundAt(page: Page, x: number, z: number, face: 'a' | 'b'): Promise<number | null> {
  const rows = groundRows(face, GROUND_SCAN_M, CELL_HEIGHT);
  const row = await call<number | null>(
    page,
    `(() => { const rows = ${JSON.stringify(rows)}; for (const r of rows) if (d.voxel(${String(Math.floor(x))}, r, ${String(Math.floor(z))}) !== 0) return r; return null; })()`,
  );
  return row === null ? null : eyeOver(face, row, CELL_HEIGHT, 0);
}
/** How far from the midplane an `above` shot looks for the ground (m). */
const GROUND_SCAN_M = 1200;

/**
 * A placed shot: flies the player to the place (so its full-detail chunks stream), finds the
 * ground under an `above` placement (descending until its chunks hold it), places the camera
 * exactly, waits for the view to settle — the LOD with nothing to generate, mesh or ask for,
 * nothing waiting in view and the chunks meshed, for 1 s, then `quietFrames` frames — and shoots.
 */
async function placedShot(
  run: Run,
  name: string,
  p: Placement,
  quietFrames: number,
  maxS: number,
): Promise<void> {
  const { page } = run;
  const start = Date.now();
  const up = p.face === 'b' ? -1 : 1;
  const [x, z] = p.at ? [p.at[0], p.at[2]] : [p.above[0], p.above[1]];
  await call(page, 'd.fly(true)');
  let eyeY: number;
  let groundY: number | null = null;
  if (p.at) {
    eyeY = p.at[1];
    await call(page, `d.teleport(${JSON.stringify([x, eyeY - 1.6 * up, z])})`);
  } else {
    const feet = (await call<Vec3 | null>(page, 'd.state()?.feet ?? null')) ?? [
      x,
      OPEN_HEIGHT_M * up,
      z,
    ];
    // Over the spot at the feet's height (or the open height if far below it), then down a
    // chunk's worth at a time until the column's ground has streamed in.
    let y = Math.abs(feet[0] - x) + Math.abs(feet[2] - z) < 2 ? feet[1] : OPEN_HEIGHT_M * up;
    await call(page, `d.teleport(${JSON.stringify([x, y, z])})`);
    while (Date.now() - start < maxS * 1000) {
      await page.waitForTimeout(1000);
      groundY = await groundAt(page, x, z, p.face);
      if (groundY !== null) break;
      if (y * up > 32) {
        y -= 48 * up;
        await call(page, `d.teleport(${JSON.stringify([x, y, z])})`);
      }
    }
    if (groundY === null)
      throw new Error(`shot ${name}: no ground under ${f(x)}, ${f(z)} on face ${p.face}`);
    eyeY = groundY + p.above[2] * up;
    const feetY = up > 0 ? Math.max(eyeY - 1.6, groundY) : Math.min(eyeY + 1.6, groundY);
    await call(page, `d.teleport(${JSON.stringify([x, feetY, z])})`);
  }
  const eye: Vec3 = [x, eyeY, z];
  await until(
    page,
    `d.placeCamera(${JSON.stringify(eye)}, ${String(p.yaw)}, ${String(p.pitch)})`,
    30_000,
  );
  run.camera = { position: eye, yaw: p.yaw, pitch: p.pitch };
  let quiet: number | null = null;
  let settled = false;
  while (Date.now() - start < maxS * 1000) {
    const st = await call<{
      lod?: { generating: number; meshing: number; asking: number; waitingInView: number };
      terrain?: { dirty: number; meshing: number };
    } | null>(page, 'd.state()');
    const lod = st?.lod;
    const t = st?.terrain;
    const idle =
      !!lod &&
      lod.generating === 0 &&
      lod.meshing === 0 &&
      lod.asking === 0 &&
      lod.waitingInView === 0 &&
      (!t || t.dirty + t.meshing === 0);
    const now = Date.now();
    quiet = idle ? (quiet ?? now) : null;
    if (quiet !== null && now - quiet >= 1000) {
      settled = true;
      break;
    }
    await page.waitForTimeout(100);
  }
  await page.evaluate(
    `new Promise((done) => { let n = ${String(quietFrames)}; const tick = () => (n-- <= 0 ? done(null) : requestAnimationFrame(tick)); tick(); })`,
  );
  await page.screenshot({ path: join(run.dir, `${name}.png`) });
  const s = (Date.now() - start) / 1000;
  run.shots.push({ name, eye, yaw: p.yaw, pitch: p.pitch, face: p.face, groundY, s, settled });
  console.log(
    `  shot ${name}: eye [${eye.map((c) => f(c)).join(', ')}] yaw ${f(p.yaw)} pitch ${f(p.pitch)}${groundY === null ? '' : ` (${f(p.above?.[2] ?? 0)} m over the ground at ${f(groundY)})`}, ${settled ? 'settled' : 'NOT settled'} in ${f(s)} s`,
  );
}

/** The local server's figures (`__dwell.serverStats`, a second each) for `seconds`, saved as `name`. */
async function serverSamples(run: Run, name: string, seconds: number): Promise<void> {
  type Sample = {
    ticks: number;
    tickMeanMs: number;
    advanceMaxMs: number;
    water: string;
    waterStats: Record<string, number>;
  };
  const samples: Sample[] = [];
  for (let i = 0; i < seconds; i++) {
    await sleep(run.page, 1);
    const s = await call<Sample | null>(run.page, 'd.serverStats()');
    if (s) samples.push(s);
  }
  run.servers[name] = samples;
  if (!samples.length) {
    console.log(`  [${name}] no server figures (not a local world?)`);
    return;
  }
  const w = (k: string) => samples.map((s) => s.waterStats[k] ?? 0);
  const max = (v: number[]) => Math.max(...v);
  const first = samples[0];
  const last = samples[samples.length - 1];
  console.log(
    `  [${name}] water ${last.water}: ${String(samples.reduce((a, s) => a + s.ticks, 0))} ticks in ${String(samples.length)} s, ` +
      `tick mean ${f(samples.reduce((a, s) => a + s.tickMeanMs, 0) / samples.length)} ms, worst advance ${f(max(samples.map((s) => s.advanceMaxMs)))} ms; ` +
      `water clock ${f(last.waterStats.waterTimeS - first.waterStats.waterTimeS)} s, owed max ${f(max(w('owedS')))} s, ` +
      `dropped ${f(last.waterStats.droppedS)} s, longest run ${f(max(w('maxBusyMs')))} ms, queue max ${String(max(w('queue')))}, awake max ${String(max(w('awakeChunks')))}, copy ${f(max(w('cellBytes')) / 1e6)} MB`,
  );
}

async function record(run: Run, name: string): Promise<Telemetry> {
  const { page } = run;
  const data = await call<{
    telemetry: Telemetry;
    frames: { names: string[]; frames: number[][] };
    ticks: unknown[];
    state: unknown;
    render: unknown;
    memory: unknown;
    now: number;
  } | null>(
    page,
    `{ telemetry: d.telemetry(), frames: d.telemetryFrames(), ticks: d.ticks(), state: d.state(),
       render: d.renderStats(), memory: d.memory(), now: performance.now() }`,
  );
  if (!data) throw new Error('the game is not running (no __dwell)');
  const longTasks = (await page.evaluate<number[][]>('globalThis.__benchLongTasks ?? []')).filter(
    ([start]) => start >= run.markMs,
  );
  run.records[name] = { ...data, longTasks, seconds: (data.now - run.markMs) / 1000 };
  summarise(name, data.telemetry, longTasks, (data.now - run.markMs) / 1000);
  return data.telemetry;
}

const f = (v: number, digits = 2): string => v.toFixed(v < 10 ? digits : 1);

function summarise(name: string, t: Telemetry, longTasks: number[][], seconds: number): void {
  const iv = t.frames.interval;
  console.log(`  [${name}] ${String(t.frameCount)} frames in ${f(seconds)} s`);
  if (iv) {
    console.log(
      `    frame interval ms  p50 ${f(iv.p50)}  p95 ${f(iv.p95)}  p99 ${f(iv.p99)}  max ${f(iv.max)}` +
        `  (${f(1000 / iv.mean)} fps)`,
    );
  }
  // Times only: counts per frame (`ticks.count`, `upload.chunks`, …) are totalled below.
  const COUNTS = ['jobs.done', 'upload.chunks', 'upload.sections', 'upload.bytes', 'ticks.count'];
  const phases = Object.entries(t.frames)
    .filter(
      (e): e is [string, Stats] =>
        e[1] !== undefined && e[0] !== 'interval' && !COUNTS.includes(e[0]),
    )
    .sort((a, b) => b[1].p95 - a[1].p95);
  console.log('    per-frame ms        mean    p95    p99    max');
  for (const [k, s] of phases) {
    console.log(
      `      ${k.padEnd(16)} ${f(s.mean).padStart(6)} ${f(s.p95).padStart(6)} ${f(s.p99).padStart(6)} ${f(s.max).padStart(6)}`,
    );
  }
  const totals = COUNTS.flatMap((k) => {
    const sum = t.frames[k]?.sum;
    if (sum === undefined) return [];
    return [`${k} ${k === 'upload.bytes' ? `${f(sum / 2 ** 20)} MiB` : String(sum)}`];
  });
  if (totals.length) console.log(`    totals: ${totals.join(', ')}`);
  const jobs = Object.entries(t.samples).filter(
    (e): e is [string, Stats] => e[1] !== undefined && e[0].endsWith('.run'),
  );
  for (const [k, s] of jobs) {
    const wait = t.samples[k.replace(/\.run$/, '.wait')];
    console.log(
      `    ${k.padEnd(22)} n ${String(s.n).padStart(5)}  run p50 ${f(s.p50)} p95 ${f(s.p95)} ms` +
        (wait ? `  queued p50 ${f(wait.p50)} p95 ${f(wait.p95)} ms` : ''),
    );
  }
  for (const [k, s] of Object.entries(t.samples)) {
    if (s && (k.startsWith('gpu.') || k === 'pacing.done')) {
      console.log(
        `    ${k.padEnd(22)} n ${String(s.n).padStart(5)}  ms mean ${f(s.mean)} p50 ${f(s.p50)} p95 ${f(s.p95)} p99 ${f(s.p99)}`,
      );
    }
  }
  if (longTasks.length) {
    const total = longTasks.reduce((a, [, d]) => a + d, 0);
    const max = Math.max(...longTasks.map(([, d]) => d));
    console.log(
      `    long tasks: ${String(longTasks.length)}, ${f(total)} ms total, longest ${f(max)} ms`,
    );
  }
}

/** The world clock's day (s): daylight and night (shared/look/daynight.json). */
const DAY_LENGTH_S = ((c: { dayS: number; nightS: number }) => c.dayS + c.nightS)(
  (
    JSON.parse(
      readFileSync(new URL('../../shared/look/daynight.json', import.meta.url), 'utf8'),
    ) as { clock: { dayS: number; nightS: number } }
  ).clock,
);

/**
 * A still camera's burst of screenshots and their flicker (`Step` 'burst'). With `clock`, the time
 * of day runs on from it at the world clock's rate, set every animation frame (the bench holds it
 * still otherwise), so the light moves between the frames as it does in play.
 */
/** One swing frame by frame, from each view, and a contact sheet (docs/DEV_TOOLS.md "Clip strip"). */
async function clipStrip(run: Run, s: Extract<Step, { do: 'clip-strip' }>): Promise<void> {
  const { page } = run;
  const characters = JSON.parse(
    readFileSync(new URL('../../shared/characters/characters.json', import.meta.url), 'utf8'),
  ) as unknown;
  const { clip, duration } = swingClip(characters, s.vertical);
  const times = clipStripTimes(duration, s.frames);
  const [ax, ay, az] = s.at;
  // A stand-in holding your tool, 4 m ahead of `at` (puppet 0 stands at x − 10, z + 4).
  const feet = await call<Vec3>(page, 'd.state().feet');
  const origin: Vec3 = [feet[0] + ax + 10, feet[1] + ay, feet[2] + az];
  const standIn: Vec3 = [origin[0] - 10, origin[1], origin[2] + 4];
  const third = s.views.some((v) => v !== 'fp');
  if (third) {
    await call(page, `d.puppets(1, ${JSON.stringify(origin)}, ${String(run.up)}, 65535)`);
  }
  const frame = (): Promise<unknown> =>
    page.evaluate('new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))');
  const rows: { view: string; frames: { t: number; src: string }[] }[] = s.views.map((view) => ({
    view,
    frames: [],
  }));
  const width = Math.max(2, String(times.length - 1).length);
  try {
    for (const [i, t] of times.entries()) {
      await call(page, `d.holdSwing(${String(s.vertical)}, ${String(t)})`);
      for (const [k, view] of s.views.entries()) {
        if (view === 'fp') await call(page, 'd.releaseCamera()');
        else {
          const c = clipStripCamera(view, standIn, run.up);
          await call(
            page,
            `d.placeCamera(${JSON.stringify(c.position)}, ${String(c.yaw)}, ${String(c.pitch)})`,
          );
        }
        await frame();
        await frame();
        const path = join(run.dir, `${s.name}-${view}-${String(i).padStart(width, '0')}.png`);
        const png = await page.screenshot({ path });
        rows[k]?.frames.push({ t, src: `data:image/png;base64,${png.toString('base64')}` });
      }
    }
  } finally {
    await call(page, 'd.holdSwing(false, -1)');
    if (third) await call(page, 'd.puppets(0, [0, 0, 0], 1, 0)');
    await call(page, 'd.releaseCamera()');
    run.camera = undefined;
  }
  const sheet = await run.browser.newPage({ viewport: { width: 1600, height: 900 } });
  try {
    await sheet.setContent(clipStripSheet(`${s.name}: ${clip}, ${String(s.frames)} frames`, rows));
    await sheet.screenshot({ path: join(run.dir, `${s.name}-sheet.png`), fullPage: true });
  } finally {
    await sheet.close();
  }
  console.log(`  clip-strip ${s.name}: ${clip} at ${times.map((t) => t.toFixed(2)).join(', ')} s`);
}

async function burst(run: Run, s: Extract<Step, { do: 'burst' }>): Promise<void> {
  const { page } = run;
  if (s.clock !== undefined) {
    await page.evaluate(`(() => {
      const d = globalThis.__dwell, t0 = performance.now();
      globalThis.__burstClock = true;
      const tick = () => {
        if (!globalThis.__burstClock) return;
        d.setTime((${String(s.clock)} + (performance.now() - t0) / 1000 / ${String(DAY_LENGTH_S)}) % 1);
        requestAnimationFrame(tick);
      };
      tick();
    })()`);
    // The kept maps catch up with the moving light first.
    await sleep(page, 1);
  }
  // The scene only: the page's overlays (the HUD's fps and tick counters change every frame).
  // (Set from script: the page's CSP allows no inline style sheet.)
  await page.evaluate(`(() => {
    const els = [...document.querySelectorAll('body *')];
    globalThis.__burstShown = els.map((e) => [e, e.style.visibility]);
    for (const e of els) e.style.visibility = e.tagName === 'CANVAS' ? 'visible' : 'hidden';
  })()`);
  const width = Math.max(2, String(s.frames - 1).length);
  const paths: string[] = [];
  const from = run.camera;
  if (s.drift && !from) throw new Error(`burst ${s.name}: a drift needs a camera step before it`);
  for (let i = 0; i < s.frames; i++) {
    const path = join(run.dir, `${s.name}-${String(i).padStart(width, '0')}.png`);
    if (s.drift && from) {
      const p = from.position.map((c, k) => c + (s.drift?.position[k] ?? 0) * i);
      const [yaw, pitch] = [from.yaw + s.drift.yaw * i, from.pitch + s.drift.pitch * i];
      await call(page, `d.placeCamera(${JSON.stringify(p)}, ${String(yaw)}, ${String(pitch)})`);
      // Two animation frames: the placed camera drawn.
      await page.evaluate(
        'new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))',
      );
    }
    await page.screenshot({ path });
    paths.push(path);
  }
  await page.evaluate(
    'for (const [e, v] of globalThis.__burstShown) e.style.visibility = v; delete globalThis.__burstShown',
  );
  if (s.clock !== undefined) await page.evaluate('globalThis.__burstClock = false');
  const codec = await page.context().newPage();
  await codec.evaluate(CODEC);
  const frames = [];
  for (const p of paths) frames.push(await decode(codec, p));
  await codec.close();
  const fl = flickerOf(frames);
  run.bursts[s.name] = fl;
  console.log(
    `  burst ${s.name}: ${String(s.frames)} frames, flicker ${f(fl.changed * 100)} % of pixels, mean Δ ${f(fl.meanDelta)}, hue flips ${f(fl.hueFlips * 100, 3)} %`,
  );
  if (s.maxHueFlips !== undefined && fl.hueFlips > s.maxHueFlips) {
    throw new Error(
      `burst ${s.name}: ${f(fl.hueFlips * 100, 3)} % of pixels flip hue, over ${f(s.maxHueFlips * 100, 3)} %`,
    );
  }
  if (s.maxFlicker !== undefined && fl.changed > s.maxFlicker) {
    throw new Error(
      `burst ${s.name}: ${f(fl.changed * 100)} % of pixels flicker, over ${f(s.maxFlicker * 100)} %`,
    );
  }
}

async function step(run: Run, s: Step): Promise<void> {
  const { page } = run;
  switch (s.do) {
    case 'wait':
      return sleep(page, s.s);
    case 'server':
      return serverSamples(run, s.name, s.s);
    case 'key':
      await call(page, `d.press(${JSON.stringify(s.code)}, ${String(s.down)})`);
      return;
    case 'hold':
      await call(page, `d.press(${JSON.stringify(s.code)}, true)`);
      await sleep(page, s.s);
      await call(page, `d.press(${JSON.stringify(s.code)}, false)`);
      return;
    case 'look':
      await call(page, `d.look(${String(s.yaw)}, ${String(s.pitch)})`);
      return;
    case 'fly':
      await call(page, `d.fly(${String(s.on)})`);
      return;
    case 'flySpeed':
      await call(page, `d.flySpeed(${String(s.level)})`);
      return;
    case 'spyglass':
      await call(page, `d.spyglass(${String(s.on)}, ${String(s.step ?? 0)})`);
      return;
    case 'camera': {
      // aroundPlayer: x and z from the player's feet, y absolute (or with aboveFeet a height over
      // the feet along the scenario's face's up); aroundFound: from the last goto's place, y a
      // height over its surface along its face's up.
      const [x, y, z] = s.position;
      const found = run.gotos.at(-1)?.found;
      if (s.aroundFound && !found)
        throw new Error('camera: aroundFound needs a goto step before it');
      const at =
        s.aroundFound && found
          ? JSON.stringify([found.at[0] + x, found.at[1] + y * found.face, found.at[2] + z])
          : s.aroundPlayer
            ? `(() => { const f = d.state().feet; return [f[0] + ${String(x)}, ${s.aboveFeet ? `f[1] + ${String(y * run.up)}` : String(y)}, f[2] + ${String(z)}]; })()`
            : JSON.stringify(s.position);
      await until(page, `d.placeCamera(${at}, ${String(s.yaw)}, ${String(s.pitch)})`, 30_000);
      run.camera = {
        position: await call<[number, number, number]>(page, at),
        yaw: s.yaw,
        pitch: s.pitch,
      };
      return;
    }
    case 'releaseCamera':
      await call(page, 'd.releaseCamera()');
      return;
    case 'creatureCamera': {
      // The nearest creature to the feet, seen from 6 m off its side, 2.5 m up, looking at it.
      const at = await call<number[] | null>(
        page,
        `(() => { const f = d.state().feet; const cs = d.creatures(); if (!cs.length) return null; cs.sort((a, b) => Math.hypot(a.x - f[0], a.z - f[2]) - Math.hypot(b.x - f[0], b.z - f[2])); const c = cs[0]; return [c.x, c.y, c.z, cs.length]; })()`,
      );
      if (!at) throw new Error('creatureCamera: no creature drawn');
      const [cx, cy, cz, n] = at as [number, number, number, number];
      const up = run.up;
      const position: [number, number, number] = [cx - 4, cy + 2.5 * up, cz - 4.5];
      const yaw = (Math.atan2(cx - position[0], cz - position[2]) * 180) / Math.PI;
      const pitch = -20;
      await until(
        page,
        `d.placeCamera(${JSON.stringify(position)}, ${String(yaw)}, ${String(pitch)})`,
        30_000,
      );
      run.camera = { position, yaw, pitch };
      console.log(
        `  creatureCamera: ${String(n)} creatures, nearest at ${[cx, cy, cz].map((v) => f(v)).join(', ')}`,
      );
      return;
    }
    case 'goto': {
      const find = s.find;
      const options = Object.fromEntries(
        Object.entries(s).filter(([k]) => k !== 'do' && k !== 'find'),
      );
      const r = await call<{ ok?: GotoResult; error?: string } | null>(
        page,
        `d.goto(${JSON.stringify(find)}, ${JSON.stringify(options)}).then((ok) => ({ ok }), (e) => ({ error: String(e?.message ?? e) }))`,
      );
      if (!r?.ok) throw new Error(`goto ${find}: ${r?.error ?? 'not playing'}`);
      const { found, camera, map } = r.ok;
      run.gotos.push(r.ok);
      run.camera = camera;
      const v = (p: number[]): string => `[${p.map((c) => f(c)).join(', ')}]`;
      console.log(
        `  goto ${find}: ${found.block} at ${v(found.at)} (${f(found.distance)} m off, ${f(found.depth)} m deep, ${String(found.area)} cells, current ${v(found.current)} m/s${map ? `; the map's ${map.kind} ${f(map.distance)} m away` : ''}), camera ${v(camera.position)} yaw ${f(camera.yaw)} pitch ${f(camera.pitch)}`,
      );
      return;
    }
    case 'shadows':
      await call(page, `d.setShadows(${JSON.stringify(s.quality)})`);
      return;
    case 'view': {
      await call(page, `d.setView(${JSON.stringify({ ...s, do: undefined })})`);
      return;
    }
    case 'water':
      await call(
        page,
        `d.setWater(${JSON.stringify(s.quality ?? null)}, ${JSON.stringify(s.debug ?? null)})`,
      );
      return;
    case 'edit':
      await call(page, `d.edit(${JSON.stringify(s.action)})`);
      return;
    case 'select':
      await call(page, `d.select(${String(s.slot)})`);
      return;
    case 'pick':
      if ((await call<boolean | null>(page, `d.pick(${JSON.stringify(s.item)})`)) !== true) {
        throw new Error(`pick: ${s.item} is not a creative item here`);
      }
      return;
    case 'brush':
      await call(page, `d.brush(${JSON.stringify(s.command)})`);
      return;
    case 'region': {
      // Corners as world cells from the feet (x, z in metres, y in ½ m rows; face B's up is −y).
      const up = s.up ?? 1;
      const cell = (o: Vec3): string =>
        `[Math.floor(f[0] + ${String(o[0])}), Math.floor((f[1] + ${String(up * o[1])}) / 0.5), Math.floor(f[2] + ${String(o[2])})]`;
      const ok = await until(
        page,
        `(() => { const f = d.state().feet; d.tool('cornerA@' + ${cell(s.from)}.join(',')); d.tool('cornerB@' + ${cell(s.to)}.join(',')); return d.tool(${JSON.stringify(s.op)}); })()`,
        30_000,
      );
      if (!ok) throw new Error(`region ${s.op}: not sent`);
      return;
    }
    case 'arms':
      await call(page, `d.showArms(${String(s.on)})`);
      return;
    case 'lod':
      await call(page, `d.setLod(${String(s.on)})`);
      return;
    case 'puppets': {
      const [x, y, z] = s.at ?? [0, 0, 0];
      const held = s.held ? '65535' : '0';
      await call(
        page,
        `(() => { const f = d.state().feet; return d.puppets(${String(s.count)}, [f[0] + ${String(x)}, f[1] + ${String(y)}, f[2] + ${String(z)}], ${String(s.face ?? 1)}, ${held}); })()`,
      );
      return;
    }
    case 'settle': {
      const start = Date.now();
      let quiet: number | null = null;
      while (Date.now() - start < s.maxS * 1000) {
        const lod = await call<{ generating: number; meshing: number; asking: number } | null>(
          page,
          'd.state()?.lod ?? null',
        );
        const now = Date.now();
        const idle = !lod || (lod.generating === 0 && lod.meshing === 0 && lod.asking === 0);
        quiet = idle ? (quiet ?? now) : null;
        if (quiet !== null && now - quiet >= 1000) break;
        await page.waitForTimeout(100);
      }
      run.settles.push((Date.now() - start) / 1000);
      console.log(`  settled in ${f((Date.now() - start) / 1000)} s`);
      return;
    }
    case 'until': {
      const key = {
        behind: 'waitingOffView',
        nearBehind: 'waitingNearOffView',
        ahead: 'waitingInView',
      }[s.what];
      const start = Date.now();
      let at: number | null = null;
      while (Date.now() - start < s.maxS * 1000) {
        const n = await call<number | null>(page, `d.state()?.lod?.${key} ?? null`);
        if (n === 0) {
          at = (Date.now() - run.openedAt) / 1000;
          break;
        }
        await page.waitForTimeout(50);
      }
      run.untils.push({ what: s.what, s: at });
      console.log(
        at === null
          ? `  until ${s.what}: still waiting after ${f(s.maxS)} s`
          : `  until ${s.what}: ${f(at)} s after opening the page`,
      );
      return;
    }
    case 'probe': {
      const feet = await call<[number, number, number]>(page, 'd.state().feet');
      const points = s.at.flatMap((p) => [feet[0] + p[0], feet[1] + p[1], feet[2] + p[2]]);
      await call(page, `d.setProbe(${JSON.stringify(points)})`);
      console.log(`  probe: ${JSON.stringify(points)}`);
      return;
    }
    case 'carry': {
      const ok = await call<boolean | null>(
        page,
        `d.carry(${String(s.speed)}, ${String(s.yaw)}, ${String(s.s)}, ${String(s.lift ?? 1)})`,
      );
      if (ok !== true) throw new Error('carry: not a local world');
      await page.waitForTimeout(s.s * 1000);
      return;
    }
    case 'lava': {
      const r = await call<string | null>(
        page,
        `d.lava(${JSON.stringify(s.op)}, ${JSON.stringify(s.args)})`,
      );
      if (r === null) throw new Error('lava: not a local world');
      console.log(`  lava ${s.op}: ${r}`);
      if (r.includes('"error"')) throw new Error(`lava ${s.op}: ${r}`);
      return;
    }
    case 'give': {
      const ok = await call<boolean | null>(
        page,
        `d.give(${JSON.stringify(s.item)}, ${String(s.count)})`,
      );
      if (ok !== true) throw new Error('give: not a local world');
      await page.waitForTimeout(300);
      return;
    }
    case 'die': {
      // The respawn's timeline (ARCHITECTURE.md §6.3): killed, the death screen (the respawn
      // preload streams the spawn meanwhile), the respawn, then playing behind the loading screen.
      if ((await call<boolean | null>(page, 'd.die()')) !== true) {
        throw new Error('die: not a local world');
      }
      if (!(await until(page, 'd.state()?.dead === true', 10_000)))
        throw new Error('die: no death');
      if (!(await until(page, 'd.state()?.dead === false && d.state()?.active === true', 30_000))) {
        throw new Error('die: no respawn');
      }
      const respawned = Date.now();
      const playing = await until(page, 'd.state()?.terrainReady === true', s.maxS * 1000);
      const at = playing ? (Date.now() - respawned) / 1000 : null;
      const loading = await call<Record<string, unknown> | null>(
        page,
        'd.state()?.loading ?? null',
      );
      run.respawns.push({ s: at, loading });
      console.log(
        at === null
          ? `  respawn: still not playing after ${f(s.maxS)} s`
          : `  respawn → playing ${f(at)} s${typeof loading?.openedS === 'number' ? ` (loading screen ${f(loading.openedS)} s${loading.timedOut ? ', timed out' : ''})` : ''}`,
      );
      return;
    }
    case 'mark':
      await call(page, 'd.telemetryReset()');
      run.markMs = await page.evaluate<number>('performance.now()');
      return;
    case 'record': {
      const t = await record(run, s.name);
      const over = s.budget ? overBudget(t.frames, s.budget, t.samples) : [];
      if (over.length > 0) {
        const tag = s.reportOnly ? 'OVER BUDGET (report only)' : 'OVER BUDGET';
        for (const o of over) console.log(`    ${tag} [${s.name}] ${o}`);
        if (!s.reportOnly) throw new Error(`record ${s.name} over its budget: ${over.join('; ')}`);
      } else if (s.budget) {
        console.log(`    within budget [${s.name}]`);
      }
      return;
    }
    case 'shot':
      if (s.place) await placedShot(run, s.name, s.place, s.quietFrames, s.maxS);
      else await page.screenshot({ path: join(run.dir, `${s.name}.png`) });
      return;
    case 'spot': {
      const spot = findSpot(run.spots, s.spot);
      await placedShot(run, s.name, spot.place, 30, 90);
      return;
    }
    case 'time':
      await call(
        page,
        `d.setTime(${String(s.t)}${s.moon === undefined ? '' : `, ${String(s.moon)}`})`,
      );
      return;
    case 'theater': {
      // Load (a path, or a preset expanded around its subject) and play; wait for the end.
      const path =
        s.path ??
        (await call<unknown>(
          page,
          `d.theater('preset', ${JSON.stringify({ name: s.preset, params: s.params ?? {}, subject: s.subject ?? 'player' })})`,
        ));
      await call(page, `d.theater('hud', ${JSON.stringify({ shown: s.hud })})`);
      await call(page, `d.theater('play', ${JSON.stringify({ path })})`);
      if (s.wait) {
        await until(page, `d.theater('state', {})?.finished === true`, 3_600_000);
      }
      return;
    }
    case 'waterDebug':
      await call(page, `d.waterDebug(${JSON.stringify(s.names)})`);
      return;
    case 'timelapse': {
      const times = timelapseTimes(s);
      const width = String(times.length - 1).length;
      for (const [i, t] of times.entries()) {
        await call(page, `d.setTime(${String(t)})`);
        await sleep(page, s.waitS);
        const n = String(i).padStart(Math.max(2, width), '0');
        await page.screenshot({ path: join(run.dir, `${s.name}-${n}.png`) });
      }
      return;
    }
    case 'burst':
      return burst(run, s);
    case 'clip-strip':
      return clipStrip(run, s);
    case 'traceStart':
      await run.browser.startTracing(page, {
        screenshots: true,
        categories: [
          'devtools.timeline',
          'disabled-by-default-devtools.timeline',
          'gpu',
          'v8',
          // The sampling profiler: `<name>.profile.txt` (scripts/profile.ts).
          'disabled-by-default-v8.cpu_profiler',
        ],
      });
      return;
    case 'repeat':
      for (let i = 0; i < s.times; i++) for (const inner of s.steps) await step(run, inner);
      return;
    case 'traceStop': {
      const trace = await run.browser.stopTracing();
      writeFileSync(join(run.dir, `${s.name}.trace.json`), trace);
      const threads = profileOf(JSON.parse(trace.toString('utf8')) as unknown);
      writeFileSync(join(run.dir, `${s.name}.profile.txt`), formatProfile(threads));
      for (const t of threads.slice(0, 3)) {
        const top = work(t)
          .slice(0, 6)
          .map((x) => `${x.name} ${f(x.selfMs)}`)
          .join(', ');
        console.log(`  profile ${t.thread}: ${f(t.totalMs)} ms busy — ${top}`);
      }
      return;
    }
  }
}

/** Runs `sc` in a fresh context into `dir`; `params` add to its page address (`--compare`). */
/** A loading stage's times (ms on the page's clock; `firstMs`: its first count). */
interface StageTime {
  id: string;
  beganMs: number;
  endedMs: number | null;
  firstMs: number | null;
}

/** "game 0.03–0.08 s, worldMap 0.16–2.2 s (first count 1.8 s), …": each stage's span from opening. */
function formatStageTimes(stages: StageTime[]): string {
  const s = (ms: number | null): string => (ms === null ? '…' : f(ms / 1000));
  return stages
    .map(
      (t) =>
        `${t.id} ${s(t.beganMs)}–${s(t.endedMs)} s${t.firstMs !== null ? ` (first count ${s(t.firstMs)} s)` : ''}`,
    )
    .join(', ');
}

/** A scenario's page, opened and playing; or, not playing within `stallS`, closed as stalled. */
async function openPage(
  browser: Browser,
  sc: Scenario,
  url: string,
  stallS: number,
): Promise<
  | { context: BrowserContext; page: Page; errors: string[]; openedAt: number; gpu: unknown }
  | { stalled: { stalledS: number; errors: string[] } }
> {
  const { pixelRatio, ...viewport } = sc.window;
  const context = await browser.newContext({
    viewport,
    ...(pixelRatio === undefined ? {} : { deviceScaleFactor: pixelRatio }),
    ...(emulation.touch ? { hasTouch: true, isMobile: true } : {}),
  });
  const page = await context.newPage();
  if (emulation.cpuThrottle > 1) {
    const cdp = await context.newCDPSession(page);
    await cdp.send('Emulation.setCPUThrottlingRate', { rate: emulation.cpuThrottle });
  }
  const errors: string[] = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  await page.addInitScript(LONG_TASKS);
  await page.addInitScript(
    (entries: Record<string, string>) => {
      for (const [k, v] of Object.entries(entries)) localStorage.setItem(k, v);
    },
    // No music (docs/AUDIO.md §6a): its streaming and decoding are not what the bench measures.
    { ...viewStorage(scenarioView(sc)), 'dwell.music': '"off"' },
  );
  const openedAt = Date.now();
  await page.goto(url);
  const gpu = await page.evaluate<unknown>(`(async () => {
    const a = await navigator.gpu?.requestAdapter();
    return a ? { ...a.info, vendor: a.info.vendor, architecture: a.info.architecture,
      device: a.info.device, description: a.info.description } : null;
  })()`);
  if (!(await until(page, 'd.state()?.terrainReady === true', stallS * 1000))) {
    await context.close();
    return { stalled: { stalledS: (Date.now() - openedAt) / 1000, errors } };
  }
  return { context, page, errors, openedAt, gpu };
}

async function runScenario(
  browser: Browser,
  base: string,
  dir: string,
  sc: Scenario,
  params: Record<string, string> = {},
): Promise<ScenarioReport> {
  mkdirSync(dir, { recursive: true });
  const url = scenarioUrl(buildPage(base), sc, params);
  // A load that isn't playing within the stall bar (`--stall-s`) is retried once in a fresh page
  // (fresh loads at far spots have stalled now and then); the report lists it in `stalls`.
  const stalls: { stalledS: number; errors: string[] }[] = [];
  let opened = await openPage(browser, sc, url, runWide.stallS);
  if ('stalled' in opened) {
    console.log(`  not playing after ${f(runWide.stallS)} s: retrying once in a fresh page`);
    stalls.push(opened.stalled);
    opened = await openPage(browser, sc, url, 120);
    if ('stalled' in opened) {
      throw new Error(
        `${sc.name}: the game did not start, twice (${opened.stalled.errors.join('; ')})`,
      );
    }
  }
  const { context, page, errors, openedAt, gpu } = opened;
  // The join's timeline (ARCHITECTURE.md §6.3): page opened → playing, and the loading screen's
  // wait (its near ring drawn, or its timeout).
  const joined = {
    playingS: (Date.now() - openedAt) / 1000,
    loading: await call<Record<string, unknown> | null>(page, 'd.state()?.loading ?? null'),
    // The loading screen's stages (§6.3, `__dwellStages`): when each began and ended (ms).
    stages: await page.evaluate<StageTime[]>(
      '(globalThis.__dwellStages?.() ?? []).map((s) => ({ id: s.id, beganMs: s.beganMs, endedMs: s.endedMs, firstMs: s.firstMs }))',
    ),
    // The main thread's long tasks (> 50 ms) from the page's start to playing: [startMs, ms].
    longTasks: await page.evaluate<number[][]>('globalThis.__benchLongTasks ?? []'),
    frameGaps: await page.evaluate<number[][]>('globalThis.__benchFrameGaps ?? []'),
    // The renderer's start, group by group (`dwell:create:<group>` marks, each at its group's end).
    createMarks: await page.evaluate<[string, number][]>(
      "performance.getEntriesByType('mark').filter((m) => m.name.startsWith('dwell:create:')).map((m) => [m.name.slice(13), m.startTime])",
    ),
  };
  if (joined.stages.length) console.log(`  stages: ${formatStageTimes(joined.stages)}`);
  if (joined.createMarks.length) {
    // Each group's time since the one before (the first: since the graphics stage began).
    const began = joined.stages.find((s) => s.id === 'graphics')?.beganMs ?? 0;
    const steps = joined.createMarks.map(
      ([n, t], i) => `${n} ${f(t - (i ? joined.createMarks[i - 1][1] : began))}`,
    );
    console.log(`  create (ms): ${steps.join(', ')}`);
  }
  if (joined.frameGaps.length) {
    const gaps = joined.frameGaps.map(([t, ms]) => `${f(ms)} ms at ${f(t / 1000)} s`);
    console.log(`  start's frame gaps > 100 ms: ${gaps.join(', ')}`);
  }
  if (joined.longTasks.length) {
    const longest = joined.longTasks.reduce((a, b) => (b[1] > a[1] ? b : a));
    console.log(
      `  start's long tasks: ${String(joined.longTasks.length)}, longest ${f(longest[1])} ms at ${f(longest[0] / 1000)} s`,
    );
  }
  console.log(
    `  playing ${f(joined.playingS)} s after opening the page${typeof joined.loading?.openedS === 'number' ? ` (loading screen ${f(joined.loading.openedS)} s${joined.loading.timedOut ? ', timed out' : ''})` : ''}`,
  );
  const run: Run = {
    page,
    browser,
    dir,
    markMs: 0,
    records: {},
    settles: [],
    openedAt,
    untils: [],
    respawns: [],
    bursts: {},
    servers: {},
    up: sc.face === 'b' ? -1 : 1,
    gotos: [],
    spots: runWide.spots,
    shots: [],
  };
  for (const s of sc.steps) await step(run, s);
  const report: ScenarioReport = {
    scenario: sc,
    gpu,
    errors,
    records: run.records,
    settles: run.settles,
    join: joined,
    untils: run.untils,
    respawns: run.respawns,
    bursts: run.bursts,
    gotos: run.gotos,
    servers: run.servers,
    shots: run.shots,
    stalls,
    url,
  };
  writeFileSync(join(dir, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  if (errors.length) console.log(`  page errors: ${errors.slice(0, 5).join(' | ')}`);
  await context.close();
  return report;
}

async function main(): Promise<void> {
  const o = options(process.argv.slice(2));
  emulation.cpuThrottle = o.cpuThrottle;
  emulation.touch = o.touch;
  const all = parseScenarios(JSON.parse(readFileSync(o.file, 'utf8')) as unknown);
  const unknown = o.names.filter((n) => !all.some((s) => s.name === n));
  if (unknown.length) {
    throw new Error(
      `no scenario ${unknown.join(', ')} (have ${all.map((s) => s.name).join(', ')})`,
    );
  }
  runWide.stallS = o.stallS;
  runWide.spots = parseSpots(JSON.parse(readFileSync(o.spotsFile, 'utf8')) as unknown);
  const spotRuns = o.spots.map((n) => {
    const spot = findSpot(runWide.spots, n);
    return { sc: spotScenario(spot), params: spotParams(spot) };
  });
  const chosen = o.names.length
    ? all.filter((s) => o.names.includes(s.name))
    : spotRuns.length
      ? []
      : all;
  const { url, server } = o.warm ? await warmServe() : await serve(o.base);
  const browser = await launch(o);
  console.log(
    `bench: ${url} → ${o.out} (${o.swiftshader ? 'SwiftShader' : 'GPU'}${o.unthrottled ? ', unthrottled' : ''}${o.compare ? ', compare' : ''})`,
  );
  try {
    if (o.compare) {
      await compare({
        browser,
        out: o.out,
        mode: `${o.swiftshader ? 'SwiftShader' : 'GPU'}, ${o.unthrottled ? 'unthrottled' : 'vsync'}`,
        scenarios: chosen,
        configsFile: o.configsFile,
        configNames: o.configs,
        run: (sc, dir, params) => runScenario(browser, url, dir, sc, params),
      });
    } else {
      for (const sc of chosen) {
        console.log(`${sc.name}: ${sc.notes}`);
        await runScenario(browser, url, join(o.out, sc.name), sc);
      }
      for (const { sc, params } of spotRuns) {
        console.log(`${sc.name}: ${sc.notes}`);
        await runScenario(browser, url, join(o.out, sc.name), sc, params);
      }
    }
  } finally {
    await browser.close();
    server?.kill();
  }
}

await main();
