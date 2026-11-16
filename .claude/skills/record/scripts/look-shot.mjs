// Terrain-look stills, one command per build (owner, 2026-11-16: "make tools to make each screenshot you take super
// quick and easy to setup the world and take it"). Opens a released build (dropkickarcade.com/dwell/v/<version>/) or
// the warm local preview (`--version local`, :4174) on a fresh world, then for each named view in the views file:
// places the camera, sets the time of day, hides the HUD, waits for the view to settle (or `waitS` on builds without
// the settle probe) and saves <out>/<version>-<view>.png.
//
//   node .claude/skills/record/scripts/look-shot.mjs --version 0.65.0 --out DIR [--views FILE] [--only a,b]
//        [--seed 7] [--world test:realistic] [--size 1080x1920]
//
// The views file (default videos/terrain-looks/views.json): {"views": {"name": {"eye": [x, y, z], "yaw", "pitch",
// "time", "waitS"?}}}. Old builds may lack an API; the tool says which and falls back.
import { createRequire } from 'node:module';
import { mkdirSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const require = createRequire(new URL('../../../../client/package.json', import.meta.url));
const { chromium } = require('playwright');

const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const version = opt('version', 'local');
const out = opt('out', join(process.env.TMPDIR ?? '/tmp', 'terrain-looks'));
const viewsFile = opt('views', join(here, '..', 'videos', 'terrain-looks', 'views.json'));
const only = opt('only', '')?.split(',').filter(Boolean);
const seed = opt('seed', '7');
const world = opt('world', 'test:realistic');
const [W, H] = opt('size', '1080x1920').split('x').map(Number);
const views = JSON.parse(readFileSync(viewsFile, 'utf8')).views;
mkdirSync(out, { recursive: true });

const base =
  version === 'local' ? 'http://localhost:4174/dwell/' : `https://dropkickarcade.com/dwell/v/${version}/`;
const url = `${base}?worldgen=${encodeURIComponent(world)}&seed=${seed}&fresh=1`;

const browser = await chromium.launch({
  headless: false,
  channel: 'chrome',
  args: ['--enable-unsafe-webgpu', '--disable-renderer-backgrounding', '--disable-background-timer-throttling'],
});
const page = await (await browser.newContext({ viewport: { width: W, height: H } })).newPage();
await page.addInitScript(() => {
  localStorage.setItem('dwell.music', '"off"');
  localStorage.setItem('dwell.clouds', '"off"');
});
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
const t0 = Date.now();
await page.goto(url);
const ev = (expr) => page.evaluate(`(async () => { const d = globalThis.__dwell; return (${expr}); })()`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// The world joined and its ground drawn.
for (let i = 0; ; i++) {
  const ready = await ev('d && d.state && d.state()?.terrainReady').catch(() => false);
  if (ready) break;
  if (i > 240) throw new Error(`${version}: the world never got ready (${url}); errors: ${errors.slice(-3)}`);
  await sleep(500);
}
console.log(`${version}: ready in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
const has = async (name) => ev(`typeof d.${name} === 'function'`);
const hide = async () => {
  if (await has('theater')) {
    await ev(`d.theater('hud', { shown: false })`).catch(() => {});
  }
  // Every page element but the canvas, re-applied (older builds re-show some).
  await page.addStyleTag({ content: 'body > *:not(canvas) { visibility: hidden !important; }' }).catch(() => {});
  await page.evaluate(() =>
    document.querySelectorAll('body > *:not(canvas)').forEach((e) => (e.style.visibility = 'hidden')),
  );
};
for (const [name, v] of Object.entries(views)) {
  if (only.length && !only.includes(name)) continue;
  if (await has('setTime')) await ev(`d.setTime(${v.time})`);
  else console.log(`${version}: no setTime: the build's own time`);
  if (!(await has('placeCamera'))) throw new Error(`${version}: no placeCamera: too old for this tool`);
  await ev(`d.placeCamera(${JSON.stringify(v.eye)}, ${v.yaw}, ${v.pitch})`);
  await hide();
  // Settled: the bench's view probe where the build has it, else a fixed wait.
  const waitS = v.waitS ?? 25;
  const start = Date.now();
  let settled = false;
  const probe = `(() => { const st = d.state(); const lod = st?.lod; const t = st?.terrain;
    return !!lod && lod.waitingInView === 0 && (lod.generating ?? 0) === 0 && (lod.meshing ?? 0) === 0
      && (!t || t.dirty + t.meshing === 0); })()`;
  if (await ev('!!d.state()?.lod')) {
    let idleFor = 0;
    while ((Date.now() - start) / 1000 < waitS * 3) {
      idleFor = (await ev(probe).catch(() => false)) ? idleFor + 0.5 : 0;
      if (idleFor >= 2) {
        settled = true;
        break;
      }
      await sleep(500);
    }
  } else {
    await sleep(waitS * 1000);
  }
  const file = join(out, `${version}-${name}.png`);
  await page.screenshot({ path: file });
  console.log(`${version} ${name}: ${file} (${settled ? 'settled' : 'waited'} ${((Date.now() - start) / 1000).toFixed(0)} s)`);
}
if (errors.length) console.log(`${version}: page errors: ${errors.slice(-3).join(' | ')}`);
await browser.close();
