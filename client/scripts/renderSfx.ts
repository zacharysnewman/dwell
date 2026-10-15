// Renders every sound effect offline in Chromium and writes WAVs to listen to (docs/AUDIO.md §7).
//
//   npm run sfx:render -- --out <dir> [--only <substring>]
//
// Builds src/audio/render.ts as a page, serves it with `vite preview` on port 4178, renders each
// job of the plan (src/audio/renderPlan.ts) through the real synth and mixer in an
// OfflineAudioContext, checks it (non-silent, peak < 1.0, a sensible length; src/audio/wav.ts) and
// writes `<dir>/<name>.wav` and `<dir>/summary.txt`. Exits non-zero if a render fails its check.
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';
import { build } from 'vite';
import { renderPlan } from '../src/audio/renderPlan.ts';
import { SOUND_DATA } from '../src/audio/sounds.gen.ts';
import { checkRender, type RenderStats } from '../src/audio/wav.ts';

const PORT = 4178;
const client = resolve(dirname(fileURLToPath(import.meta.url)), '..');

function arg(name: string): string | undefined {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

async function waitFor(url: string, ms: number): Promise<void> {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`${url} did not come up`);
}

async function main(): Promise<void> {
  const out = resolve(arg('--out') ?? join(client, '..', 'target', 'sfx'));
  const only = arg('--only');
  const site = join(out, '.site');
  mkdirSync(out, { recursive: true });
  await build({
    configFile: false,
    root: client,
    base: '/dwell/',
    logLevel: 'warn',
    build: {
      outDir: site,
      emptyOutDir: true,
      target: 'es2022',
      lib: {
        entry: join(client, 'src/audio/render.ts'),
        formats: ['es'],
        fileName: () => 'render.js',
      },
    },
  });
  writeFileSync(
    join(site, 'index.html'),
    '<!doctype html><meta charset="utf-8"><title>Dwell sound renders</title><script type="module" src="./render.js"></script>\n',
  );
  const preview = spawn(
    'npx',
    ['vite', 'preview', '--port', String(PORT), '--strictPort', '--outDir', site],
    {
      cwd: client,
      stdio: 'ignore',
    },
  );
  const url = `http://localhost:${String(PORT)}/dwell/`;
  const problems: string[] = [];
  const rows: string[] = [];
  try {
    await waitFor(url, 20_000);
    const browser = await chromium.launch({ headless: true });
    const page = await browser.newPage();
    await page.goto(url);
    await page.waitForFunction(() => typeof window.renderJob === 'function');
    const jobs = renderPlan(SOUND_DATA).filter((j) => !only || j.name.includes(only));
    for (const job of jobs) {
      const r = await page.evaluate(async (j) => {
        const render = window.renderJob;
        if (!render) throw new Error('no renderJob');
        return render(j);
      }, job);
      writeFileSync(join(out, `${job.name}.wav`), Buffer.from(r.wav, 'base64'));
      const s: RenderStats = r.stats;
      const bad = checkRender(s, job);
      rows.push(
        `${job.name.padEnd(36)} peak ${s.peak.toFixed(3)}  rms ${s.rms.toFixed(4)}  audible ${s.audible.toFixed(3)} s${bad.length ? `  FAIL: ${bad.join('; ')}` : ''}`,
      );
      for (const b of bad) problems.push(`${job.name}: ${b}`);
    }
    await browser.close();
  } finally {
    preview.kill();
  }
  writeFileSync(join(out, 'summary.txt'), `${rows.join('\n')}\n`);
  console.log(rows.join('\n'));
  console.log(`\n${String(rows.length)} renders in ${out}`);
  if (problems.length) {
    console.error(`\n${String(problems.length)} problems:\n${problems.join('\n')}`);
    process.exit(1);
  }
}

await main();
