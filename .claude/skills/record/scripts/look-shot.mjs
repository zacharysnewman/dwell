// Terrain-look stills, one command per build (owner, 2026-11-16: "make tools to make each screenshot you take super
// quick and easy to setup the world and take it"). Opens a released build (dropkickarcade.com/dwell/v/<version>/) or
// the warm local preview (`--version local`, :4174) on a fresh world, then for each named view in the views file:
// places the camera, sets the time of day, hides the HUD, waits for the view to settle (or `waitS` on builds without
// the settle probe) and saves <out>/<version>-<view>.png.
//
//   node .claude/skills/record/scripts/look-shot.mjs --version 0.65.0 --out DIR [--views FILE] [--only a,b]
//        [--seed 7] [--world test:realistic] [--size 1080x1920] [--source auto|github] [--cache DIR]
// A build the site no longer hosts is downloaded from its GitHub release (`gh`) and served on :4180.
//
// The views file (default videos/terrain-looks/views.json): {"views": {"name": {"eye": [x, y, z], "yaw", "pitch",
// "time", "waitS"?, "fromSpawn"?}}} (`fromSpawn`: `eye` is an offset from the spawn's feet). Old builds may lack an API; the tool says which and falls back.
import { createRequire } from 'node:module';
import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import http from 'node:http';
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

// A build: the local preview, a hosted one, or (`--source github`, or one the site retired) its
// release's built site downloaded from GitHub into `--cache` and served here at its own path.
const source = opt('source', 'auto');
const cache = opt('cache', join(process.env.TMPDIR ?? '/tmp', 'dwell-builds'));
let server = null;
async function servedBase() {
  if (version === 'local') return 'http://localhost:4174/dwell/';
  const hosted = `https://dropkickarcade.com/dwell/v/${version}/`;
  if (source !== 'github') {
    const ok = await fetch(hosted, { method: 'HEAD' }).then((r) => r.ok).catch(() => false);
    if (ok) return hosted;
  }
  const dir = join(cache, version);
  if (!existsSync(join(dir, 'index.html'))) {
    mkdirSync(dir, { recursive: true });
    const tar = `dwell-${version}.tar.gz`;
    execFileSync('gh', ['release', 'download', `v${version}`, '-R', 'zacharysnewman/dwell', '-p', tar, '-D', cache, '--clobber']);
    execFileSync('tar', ['xzf', join(cache, tar), '-C', dir]);
  }
  const prefix = `/dwell/v/${version}/`;
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.m4a': 'audio/mp4', '.woff2': 'font/woff2' };
  server = http.createServer((req, res) => {
    const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    const rel = path.startsWith(prefix) ? path.slice(prefix.length) || 'index.html' : null;
    const file = rel && join(dir, rel);
    if (!file || !file.startsWith(dir) || !existsSync(file)) {
      res.writeHead(404).end();
      return;
    }
    const ext = file.slice(file.lastIndexOf('.'));
    res.writeHead(200, {
      'Content-Type': types[ext] ?? 'application/octet-stream',
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    });
    res.end(readFileSync(file));
  });
  await new Promise((r) => server.listen(4180, r));
  return `http://localhost:4180${prefix}`;
}
const base = await servedBase();
// `--world default`: the build's own default terrain (no worldgen parameter: the builds before Realistic).
const url =
  world === 'default'
    ? `${base}?seed=${seed}&fresh=1`
    : `${base}?worldgen=${encodeURIComponent(world)}&seed=${seed}&fresh=1`;

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
  // Everything but the canvas, at any depth (an old build's hotbar sits in the canvas's container).
  await page.evaluate(() => {
    document.querySelectorAll('body *').forEach((e) => {
      e.style.setProperty('visibility', e.tagName === 'CANVAS' ? 'visible' : 'hidden', 'important');
    });
  });
};
for (const [name, v] of Object.entries(views)) {
  if (only.length && !only.includes(name)) continue;
  if (await has('setTime')) await ev(`d.setTime(${v.time})`);
  else console.log(`${version}: no setTime: the build's own time`);
  if (!(await has('placeCamera'))) throw new Error(`${version}: no placeCamera: too old for this tool`);
  // `fromSpawn`: the eye offset from the player's feet at the spawn (a coast differs per version).
  let eye = v.eye;
  if (v.fromSpawn) {
    const feet = await ev('d.state().feet');
    eye = [feet[0] + v.eye[0], feet[1] + v.eye[1], feet[2] + v.eye[2]];
  }
  await ev(`d.placeCamera(${JSON.stringify(eye)}, ${v.yaw}, ${v.pitch})`);
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
  await hide(); // again: an old build re-shows its hotbar
  await sleep(200);
  await page.screenshot({ path: file });
  console.log(`${version} ${name}: ${file} (${settled ? 'settled' : 'waited'} ${((Date.now() - start) / 1000).toFixed(0)} s)`);
}
if (errors.length) console.log(`${version}: page errors: ${errors.slice(-3).join(' | ')}`);
await browser.close();
server?.close();
