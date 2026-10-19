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
//   --compare        each scenario once per renderer configuration, compared (scripts/compare.ts)
//   --configs A,B    with --compare: these configurations (and the baseline) only
//   --configs-file F with --compare: another configurations file (default bench/configs.json)
// Build first: `npm run build:wasm && npm run build`. Per scenario, OUT/<name>/ gets report.json
// (each `record` step: telemetry summary, raw frames, ticks, long tasks, state), the screenshots and
// traces; a summary prints as it goes.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium, type Browser, type Page } from '@playwright/test';
import { compare, type ScenarioReport } from './compare.ts';
import { supportOf } from './configs.ts';
import { formatProfile, profileOf, work } from './profile.ts';
import {
  parseScenarios,
  scenarioUrl,
  scenarioView,
  timelapseTimes,
  type Scenario,
  type Step,
} from './scenarios.ts';
import { viewStorage } from './views.ts';

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
}

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
    else if (a.startsWith('--')) throw new Error(`unknown option ${a}`);
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
  // The server's root; a base given with the build's path (`…/dwell/`) works too.
  if (base) return { url: base.replace(/\/$/, '').replace(/\/dwell$/, ''), server: null };
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
}

async function record(run: Run, name: string): Promise<void> {
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
}

const f = (v: number): string => v.toFixed(v < 10 ? 2 : 1);

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
    if (s && k.startsWith('gpu.')) {
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

async function step(run: Run, s: Step): Promise<void> {
  const { page } = run;
  switch (s.do) {
    case 'wait':
      return sleep(page, s.s);
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
    case 'camera': {
      // aroundPlayer: x and z from the player's feet, y absolute.
      const [x, y, z] = s.position;
      const at = s.aroundPlayer
        ? `(() => { const f = d.state().feet; return [f[0] + ${String(x)}, ${String(y)}, f[2] + ${String(z)}]; })()`
        : JSON.stringify(s.position);
      await until(page, `d.placeCamera(${at}, ${String(s.yaw)}, ${String(s.pitch)})`, 30_000);
      return;
    }
    case 'releaseCamera':
      await call(page, 'd.releaseCamera()');
      return;
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
    case 'mark':
      await call(page, 'd.telemetryReset()');
      run.markMs = await page.evaluate<number>('performance.now()');
      return;
    case 'record':
      return record(run, s.name);
    case 'shot':
      await page.screenshot({ path: join(run.dir, `${s.name}.png`) });
      return;
    case 'time':
      await call(
        page,
        `d.setTime(${String(s.t)}${s.moon === undefined ? '' : `, ${String(s.moon)}`})`,
      );
      return;
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
async function runScenario(
  browser: Browser,
  base: string,
  dir: string,
  sc: Scenario,
  params: Record<string, string> = {},
): Promise<ScenarioReport> {
  mkdirSync(dir, { recursive: true });
  const context = await browser.newContext({ viewport: sc.window });
  const page = await context.newPage();
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
    viewStorage(scenarioView(sc)),
  );
  const url = scenarioUrl(`${base}/dwell/`, sc, params);
  await page.goto(url);
  const gpu = await page.evaluate<unknown>(`(async () => {
    const a = await navigator.gpu?.requestAdapter();
    return a ? { ...a.info, vendor: a.info.vendor, architecture: a.info.architecture,
      device: a.info.device, description: a.info.description } : null;
  })()`);
  if (!(await until(page, 'd.state()?.terrainReady === true', 120_000))) {
    throw new Error(`${sc.name}: the game did not start (${errors.join('; ')})`);
  }
  // The renderer seam's hooks (docs/DEV_TOOLS.md); a build without them has only `raster`.
  const support = supportOf(
    await call(page, `{ renderers: d.renderers?.() ?? null, upscalers: d.upscalers?.() ?? null }`),
  );
  const run: Run = { page, browser, dir, markMs: 0, records: {}, settles: [] };
  for (const s of sc.steps) await step(run, s);
  const report: ScenarioReport = {
    scenario: sc,
    gpu,
    errors,
    records: run.records,
    settles: run.settles,
    url,
    support,
  };
  writeFileSync(join(dir, 'report.json'), `${JSON.stringify(report, null, 2)}\n`);
  if (errors.length) console.log(`  page errors: ${errors.slice(0, 5).join(' | ')}`);
  await context.close();
  return report;
}

async function main(): Promise<void> {
  const o = options(process.argv.slice(2));
  const all = parseScenarios(JSON.parse(readFileSync(o.file, 'utf8')) as unknown);
  const unknown = o.names.filter((n) => !all.some((s) => s.name === n));
  if (unknown.length) {
    throw new Error(
      `no scenario ${unknown.join(', ')} (have ${all.map((s) => s.name).join(', ')})`,
    );
  }
  const chosen = o.names.length ? all.filter((s) => o.names.includes(s.name)) : all;
  const { url, server } = await serve(o.base);
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
    }
  } finally {
    await browser.close();
    server?.kill();
  }
}

await main();
