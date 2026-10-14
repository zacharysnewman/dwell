// Screenshots of the named views (client/views/views.json, scripts/views.ts), for comparing builds
// and for looking at a rendering problem from a fixed place:
//   node --experimental-strip-types scripts/shots.ts BASE_URL OUT_DIR [view…] [--views FILE]
// BASE_URL is a served build (`npm run build && npx vite preview`, e.g. http://localhost:4173).
// OUT_DIR gets `<view>.png` and `<view>.json` (whether the LOD settled, how long it took, its
// stats and the frame's draw calls) per view, and `frame.json`, the average frame time at the
// `spawn` view when it is shot. Run it against the build before and after a change and compare.
// The camera is placed at each view (`__dwell.placeCamera`), not flown: the player stays at the
// spawn, so away from it the view is the LOD's. WebGPU without a GPU draws with SwiftShader's
// Vulkan, headed (CLAUDE.md): under Xvfb, `xvfb-run -a node … scripts/shots.ts …`.
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';
import { loadViews, openView } from '../e2e/views.ts';
import { selectViews } from './views.ts';

function args(argv: string[]): { base: string; out: string; names: string[]; file?: string } {
  const rest = [...argv];
  let file: string | undefined;
  const at = rest.indexOf('--views');
  if (at >= 0) {
    file = rest[at + 1];
    rest.splice(at, 2);
  }
  const [base, out, ...names] = rest;
  if (!base || !out) {
    console.error('usage: shots.ts BASE_URL OUT_DIR [view…] [--views FILE]');
    process.exit(2);
  }
  return { base, out, names, file };
}

async function main(): Promise<void> {
  const { base, out, names, file } = args(process.argv.slice(2));
  const views = selectViews(
    loadViews(file ? new URL(file, `file://${process.cwd()}/`) : undefined),
    names,
  );
  mkdirSync(out, { recursive: true });
  const browser = await chromium.launch({
    headless: false,
    ...(process.env.DWELL_CHROMIUM ? { executablePath: process.env.DWELL_CHROMIUM } : {}),
    args: [
      '--enable-unsafe-swiftshader',
      '--enable-unsafe-webgpu',
      '--enable-features=Vulkan',
      '--use-vulkan=swiftshader',
      '--use-webgpu-adapter=swiftshader',
      '--no-proxy-server',
    ],
  });
  const client = `${base.replace(/\/$/, '')}/dwell/`;
  for (const view of views) {
    // A fresh context per view: its own settings and world.
    const context = await browser.newContext();
    const page = await context.newPage();
    page.on('pageerror', (e) => {
      console.log(`  page error: ${e.message}`);
    });
    const result = await openView(page, view, client);
    console.log(
      `${view.name}: ${result.settled ? 'settled' : 'still loading'} after ${result.seconds.toFixed(1)} s`,
    );
    await page.screenshot({ path: join(out, `${view.name}.png`) });
    writeFileSync(
      join(out, `${view.name}.json`),
      `${JSON.stringify({ view, ...result }, null, 2)}\n`,
    );
    if (view.name === 'spawn') {
      // Frame time at a standing view, for comparing builds (software rendering: relative only).
      const ms = await page.evaluate<number>(`new Promise((resolve) => {
        let frames = 0; const t0 = performance.now();
        const tick = () => { frames++; if (performance.now() - t0 > 8000) resolve((performance.now() - t0) / frames); else requestAnimationFrame(tick); };
        requestAnimationFrame(tick);
      })`);
      writeFileSync(join(out, 'frame.json'), `${JSON.stringify({ frameMs: ms }, null, 2)}\n`);
      console.log(`  frame time ${ms.toFixed(1)} ms`);
    }
    await context.close();
  }
  await browser.close();
}

await main();
