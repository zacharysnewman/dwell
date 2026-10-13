// A headless in-game screenshot in one command (debug tooling):
//   npm run shot -- --at 15557,30,-12546 --look 135,-12 --standins --out coast.png
// Serves dist/ with `vite preview` (or uses --url), opens a local world in headless Chromium,
// moves the player there (`?at=`, local worlds only), waits until the view has finished loading
// (`__dwell.settled()`, up to --settle seconds; the summary says if it had not), and writes the
// PNG plus a JSON summary beside it: position, view, LOD and terrain stats, render stats, timings.
// Needs a build (`npm run build`, with the WASM in public/wasm: scripts/setup.sh). Options:
// scripts/shotArgs.ts. Software rendering is slow and bound by pixels: --scale 0.5 is the default.
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { chromium, type Page } from '@playwright/test';
import { parseShotArgs, SHOT_USAGE, shotQuery, type ShotView } from './shotArgs.ts';

/** A free local TCP port. */
function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => {
      const a = s.address();
      s.close(() => {
        if (a && typeof a === 'object') resolve(a.port);
        else reject(new Error('no port'));
      });
    });
  });
}

async function startPreview(): Promise<{ url: string; server: ChildProcess }> {
  if (!existsSync('dist/index.html')) throw new Error('no dist/: run `npm run build` first');
  const port = await freePort();
  const server = spawn('npx', ['vite', 'preview', '--port', String(port), '--strictPort'], {
    stdio: 'ignore',
    detached: true,
  });
  const url = `http://localhost:${String(port)}/dwell/`;
  const deadline = Date.now() + 30_000;
  for (;;) {
    try {
      if ((await fetch(url)).ok) return { url, server };
    } catch {
      // not up yet
    }
    if (Date.now() > deadline) throw new Error('vite preview did not start');
    await new Promise((r) => setTimeout(r, 200));
  }
}

/** The preinstalled Chromium (cloud sessions), DWELL_CHROMIUM, or Playwright's own. */
function chromiumPath(): string | undefined {
  if (process.env.DWELL_CHROMIUM) return process.env.DWELL_CHROMIUM;
  return existsSync('/opt/pw-browsers/chromium') ? '/opt/pw-browsers/chromium' : undefined;
}

const call = <T>(page: Page, expression: string): Promise<T> =>
  page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expression} : null; })()`);

/** Waits until `check` holds, polling; false on timeout. */
async function until(check: () => Promise<boolean>, ms: number, every = 250): Promise<boolean> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await check().catch(() => false)) return true;
    await new Promise((r) => setTimeout(r, every));
  }
  return false;
}

async function shoot(page: Page, view: ShotView, first: boolean, settleMs: number, t0: number) {
  const lap = (): number => Math.round((Date.now() - t0) / 100) / 10;
  if (!first) {
    if (view.at) await call(page, `d.teleport(${view.at.join(',')})`);
    await call(page, `d.look(${view.look.join(',')})`);
  }
  if (view.at) {
    const [x, , z] = view.at;
    const arrived = await until(async () => {
      const f = await call<number[] | null>(page, 'd.state()?.feet ?? null');
      return !!f && Math.abs((f[0] ?? 0) - x) < 2 && Math.abs((f[2] ?? 0) - z) < 2;
    }, 60_000);
    if (!arrived) throw new Error(`the player did not reach ${view.at.join(',')} (a local world?)`);
  }
  const ready = await until(() => call<boolean>(page, 'd.state()?.terrainReady ?? false'), 60_000);
  const readyAt = lap();
  const settled = await until(() => call<boolean>(page, 'd.settled()'), settleMs, 500);
  const settledAt = lap();
  await page.screenshot({ path: view.out });
  const state = await call<Record<string, unknown>>(page, 'd.state()');
  const render = await call<unknown>(page, 'd.renderStats()');
  const selection = await call<unknown>(page, 'd.lodSelection(2000)');
  const summary = {
    view,
    terrainReady: ready,
    settled,
    seconds: { ready: readyAt, shot: settledAt },
    feet: state.feet,
    flying: state.flying,
    lod: state.lod,
    terrain: state.terrain,
    render,
    /** The LOD's leaves within 2 km: drawn sections (with their skirts), chunks, stand-ins. */
    selection,
  };
  writeFileSync(view.out.replace(/\.png$/i, '') + '.json', JSON.stringify(summary, null, 2));
  console.log(
    `${view.out}: ${settled ? 'settled' : `NOT settled after ${String(settleMs / 1000)} s`} at ${String(settledAt)} s`,
  );
}

async function main(): Promise<void> {
  let options;
  try {
    options = parseShotArgs(process.argv.slice(2), (p) => readFileSync(p, 'utf8'));
  } catch (err) {
    console.error(err instanceof Error ? err.message : String(err));
    console.error(SHOT_USAGE);
    process.exit(2);
  }
  const t0 = Date.now();
  const preview = options.url ? null : await startPreview();
  const base = options.url ?? preview?.url ?? '';
  const browser = await chromium.launch({
    executablePath: chromiumPath(),
    args: ['--enable-unsafe-swiftshader', '--no-proxy-server'],
  });
  try {
    const page = await browser.newPage({
      viewport: { width: options.width, height: options.height },
    });
    page.on('pageerror', (e) => {
      console.error(`page error: ${e.message}`);
    });
    await page.goto(base + shotQuery(options));
    await until(() => call<boolean>(page, 'd.state()?.active ?? false'), 120_000);
    for (const [i, view] of options.views.entries()) {
      await shoot(page, view, i === 0, options.settleSeconds * 1000, t0);
    }
  } finally {
    await browser.close();
    if (preview?.server.pid) process.kill(-preview.server.pid);
  }
}

await main();
