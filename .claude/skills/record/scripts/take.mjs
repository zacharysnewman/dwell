// One-command re-records (the record skill, WORK_QUEUE item 50).
//
//   node take.mjs <video> [--only s5,s6] [--splice-only] [--aspect 9x16] [--dry-run] [--keep] [--cache DIR] [--base URL]
//   node take.mjs --clean <video> [--cache DIR]      delete a video's clip cache once the owner approves it
//   node take.mjs <path.json> <clipName> [warmS]     (the old single-path take into a running driver)
//
// <video> is videos/<video>/video.json (or a manifest's path). Records the shots `--only` names (a shot id,
// or a splice part's label: `--only s5` records the take that holds part s5) and every shot with no cached
// clip or a stale one, reuses the rest, then splices the whole video with splice.py and prints its path and
// length (one video per aspect when the manifest has `aspects`: <video>-9x16.mp4, …; `--aspect` splices one). Each shot's frames are encoded to <cache>/<video>/<shot>.mp4 right after capture and deleted.
// The cache is $DWELL_RECORD_CACHE, else <tmp>/dwell-record (an agent passes its scratchpad with --cache).
// Starts the warm preview (`npm run bench:warm`, :4174) and the driver (:4999) when they aren't up.
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, openSync, readFileSync, renameSync, rmSync, statSync, writeFileSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { SKILL, aspectsOf, clipPath, fingerprint, loadVideo, metaPath, selectShots, shotAudio, splicePlan } from './videos.mjs';
import { renderTitle } from './title.mjs';
import { audioAlignment, encodeArgs } from './audio.mjs';

const DRIVER = `http://localhost:${process.env.RECORD_PORT || 4999}`;
const REPO = resolve(SKILL, '../../..');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
/** A fetch that retries once when it went out on a keep-alive socket the server had already closed (Node's http
 * server drops idle ones after 5 s, so the first request after a long encode failed with "fetch failed"): such a
 * request never reached the server, so a retry can't run it twice. */
export async function fetchRetrying(url, init, f = fetch) {
  try { return await f(url, init); } catch (e) {
    const code = e?.cause?.code;
    if (code !== 'UND_ERR_SOCKET' && code !== 'ECONNRESET') throw e;
    return f(url, init);
  }
}
const post = async (p, body = '') => {
  const r = await fetchRetrying(DRIVER + p, { method: 'POST', body });
  const text = await r.text();
  if (!r.ok) throw new Error(`driver ${p}: ${text}`);
  return text;
};
const up = async (url) => { try { await fetch(url, { signal: AbortSignal.timeout(2000) }); return true; } catch { return false; } };

function args(argv) {
  const o = { only: [], spliceOnly: false, dryRun: false, keep: false, cache: process.env.DWELL_RECORD_CACHE || join(tmpdir(), 'dwell-record'), base: 'http://localhost:4174', rest: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--only') o.only.push(...argv[++i].split(',').filter(Boolean));
    else if (a === '--splice-only') o.spliceOnly = true;
    else if (a === '--aspect') o.aspect = argv[++i];
    else if (a === '--dry-run') o.dryRun = true;
    else if (a === '--keep') o.keep = true;
    else if (a === '--clean') o.clean = true;
    else if (a === '--cache') o.cache = resolve(argv[++i]);
    else if (a === '--base') o.base = argv[++i].replace(/\/$/, '');
    else if (a.startsWith('--')) throw new Error(`unknown option ${a}`);
    else o.rest.push(a);
  }
  return o;
}

/**
 * A driver frame folder (f00000.jpg … + times.json) → an h264 mp4 at the measured rate (splice.py's timeline),
 * with the shot's sound (the driver's `<dir>.webm` beside the frames) muxed in when it was recorded: aligned
 * to the first frame, cut to the clip, 48 kHz AAC.
 */
export function encodeClip(dir, out, { fps, crf = 16, preset = 'veryfast' } = {}) {
  // an intermediate (splice.py re-encodes it): veryfast at crf 16 is ~3x quicker than medium at 18 and as clean
  const times = JSON.parse(readFileSync(join(dir, 'times.json'), 'utf8'));
  const rate = times.length > 1 ? (times.length - 1) / Math.max(times[times.length - 1] - times[0], 1e-6) : 30;
  const tmp = out.replace(/\.mp4$/, '.part.mp4');
  const base = dir.replace(/\/$/, '');
  const audio = existsSync(base + '.webm') && existsSync(base + '.audio.json')
    ? { file: base + '.webm', ...audioAlignment(times, JSON.parse(readFileSync(base + '.audio.json', 'utf8')), rate) } : null;
  const r = spawnSync('ffmpeg', encodeArgs({ frames: join(dir, 'f%05d.jpg'), rate, out: tmp, fps, crf, preset, audio }), { stdio: 'inherit' });
  if (r.status !== 0) throw new Error(`ffmpeg failed encoding ${dir}`);
  renameSync(tmp, out);
  return { frames: times.length, rate, audio: audio ? +audio.offsetS.toFixed(3) : null };
}

async function ensurePreview(base) {
  if (await up(base + '/dwell/')) return;
  if (!base.includes('localhost:4174')) throw new Error(`nothing serves ${base}`);
  console.log('starting the warm preview (npm run bench:warm)…');
  const r = spawnSync('npm', ['run', 'bench:warm'], { cwd: join(REPO, 'client'), stdio: 'inherit' });
  if (r.status !== 0) throw new Error('npm run bench:warm failed');
  for (let i = 0; i < 60 && !(await up(base + '/dwell/')); i++) await sleep(1000);
}

async function ensureDriver(m, url, raw, log) {
  if (await up(DRIVER + '/errors')) return false;
  console.log('starting the driver…');
  const [w, h] = m.capture.size;
  const fd = openSync(log, 'a');
  spawn(process.execPath, [join(SKILL, 'scripts/driver.mjs'), url, raw, String(w), String(h)], {
    detached: true, stdio: ['ignore', fd, fd], env: { ...process.env, RECORD_STORAGE: JSON.stringify(m.world.storage ?? {}) },
  }).unref();
  for (let i = 0; i < 90; i++) { if (await up(DRIVER + '/errors')) return true; await sleep(1000); }
  throw new Error(`the driver didn't come up (log: ${log})`);
}

async function waitReady(url) {
  const t0 = Date.now();
  while (Date.now() - t0 < 180_000) {
    if ((await post('/eval', 'd ? d.state().terrainReady : false').catch(() => 'false')) === 'true') { await sleep(1500); return; }
    await sleep(1000);
  }
  throw new Error(`the world didn't load in 3 minutes: ${url}`);
}

async function load(m, url) {
  // localStorage first: an attached driver may have been started without this video's storage
  const kv = JSON.stringify(m.world.storage ?? {});
  await post('/eval', `(Object.entries(${kv}).forEach(([k, v]) => localStorage.setItem(k, v)), 1)`);
  await post('/goto', url);
  await waitReady(url);
}

const scriptSource = (file) => readFileSync(file, 'utf8').replace(/^\s*\/\/.*$/gm, '');

async function recordShot(m, shot, o, state) {
  const size = shot.capture?.size ?? m.capture.size;
  if (size.join('x') !== state.size) { console.log(await post(`/viewport?w=${size[0]}&h=${size[1]}`)); state.size = size.join('x'); }
  const s = shot.setup ?? {};
  const url = o.base + (s.url ?? m.world.url);
  if (!state.loaded && state.fresh && !s.url) await waitReady(url); // a driver just started on the world's link
  else if (!state.loaded || s.reload || s.url) await load(m, url);
  state.loaded = true;
  await post('/eval', `(async () => { ${s.view ? `d.setView(${JSON.stringify(s.view)});` : ''}
    ${s.timeOfDay !== undefined ? `d.setTime(${s.timeOfDay});` : ''}
    ${s.hud !== undefined ? `d.theater('hud', { shown: ${s.hud} });` : ''}
    ${s.eval ? `await (${s.eval});` : ''} return 'setup'; })()`);
  let log = '', started = '';
  const sound = shotAudio(m, shot) ? '&audio=1' : '';
  if (shot.kind === 'path') {
    const path = JSON.parse(readFileSync(resolve(SKILL, shot.path), 'utf8'));
    const hold = { formatVersion: 1, keyframes: [{ ...path.keyframes[0], t: 0 }, { t: 1000 }], segments: [{ move: 'cut' }] };
    await post('/eval', `(async () => { d.theater('hud', { shown: false });
      d.theater('play', { path: ${JSON.stringify(hold)} }); await new Promise(r => setTimeout(r, ${(s.warmS ?? 10) * 1000})); return 1; })()`);
    started = await post(`/rec/start?name=${shot.id}${sound}`); console.log(started);
    log = await post('/eval', `(async () => { d.theater('play', { path: ${JSON.stringify(path)} });
      while (!d.theater('state', {}).finished) await new Promise(r => setTimeout(r, 100)); await new Promise(r => setTimeout(r, 300)); return 'played'; })()`);
  } else {
    const src = scriptSource(resolve(SKILL, shot.script));
    const a = shot.args ?? {};
    console.log('setup', await post('/eval', `(globalThis.__args=${JSON.stringify({ ...a, phase: 'setup' })}, ${src})`));
    started = await post(`/rec/start?name=${shot.id}${sound}`); console.log(started);
    log = await post('/eval', `(globalThis.__args=${JSON.stringify(a)}, ${src})`);
  }
  const stats = JSON.parse(await post('/rec/stop'));
  console.log(`  ${shot.id}: ${stats.frames} frames, ${stats.seconds.toFixed(1)} s, ${stats.fps.toFixed(1)} fps — ${log}`);
  // an attached driver (started by hand) writes its frames under its own folder: the reply names it
  return { stats, log, frames: /^"?recording (\S+)/.exec(started)?.[1] ?? null };
}

async function main() {
  const o = args(process.argv.slice(2));
  if (o.rest[0]?.endsWith('.json') && o.rest[1] && !o.rest[0].endsWith('video.json')) return legacy(o.rest);
  if (!o.rest[0]) throw new Error('usage: take.mjs <video> [--only s5,s6] [--splice-only] [--dry-run] [--cache DIR]  |  take.mjs --clean <video>');
  const m = loadVideo(o.rest[0]);
  const dir = join(o.cache, m.name);
  if (o.clean) {
    if (!existsSync(dir)) return console.log(`no cache at ${dir}`);
    const mb = readdirSync(dir, { recursive: true }).map((f) => join(dir, f)).filter((f) => statSync(f).isFile()).reduce((n, f) => n + statSync(f).size, 0) / 1048576;
    rmSync(dir, { recursive: true, force: true });
    return console.log(`deleted ${dir} (${mb.toFixed(1)} MB)`);
  }
  const cached = (id) => existsSync(clipPath(o.cache, m, id)) && existsSync(metaPath(o.cache, m, id))
    ? JSON.parse(readFileSync(metaPath(o.cache, m, id), 'utf8')) : null;
  const plan = selectShots(m, { only: o.only, spliceOnly: o.spliceOnly, cached, fingerprintOf: (s) => fingerprint(m, s) });
  for (const p of plan) console.log(`${p.action === 'record' ? 'record' : 'reuse '} ${p.id}  (${p.why})`);
  const aspects = o.aspect ? [o.aspect] : aspectsOf(m);
  const splices = aspects.map((a) => splicePlan(m, o.cache, a));
  if (o.dryRun) return console.log(JSON.stringify(splices.length === 1 ? splices[0] : splices, null, 1));

  mkdirSync(dir, { recursive: true });
  const todo = plan.filter((p) => p.action === 'record');
  let started = false;
  if (todo.length) {
    const raw = join(dir, 'raw');
    await ensurePreview(o.base);
    started = await ensureDriver(m, o.base + m.world.url, raw, join(dir, 'driver.log'));
    const state = { loaded: false, fresh: started, size: started ? m.capture.size.join('x') : '' };
    try {
      for (const p of todo) {
        const shot = m.shots.find((s) => s.id === p.id);
        const t0 = Date.now();
        console.log(`— ${shot.id}: ${shot.note ?? shot.kind}`);
        const { stats, log, frames: dirFromDriver } = await recordShot(m, shot, o, state);
        const frames = dirFromDriver ?? join(raw, shot.id);
        const enc = encodeClip(frames, clipPath(o.cache, m, shot.id), { fps: shot.capture?.fps });
        for (const f of [frames, frames + '.webm', frames + '.audio.json']) rmSync(f, { recursive: true, force: true });
        writeFileSync(metaPath(o.cache, m, shot.id), JSON.stringify({ fingerprint: fingerprint(m, shot), frames: enc.frames,
          fps: enc.rate, seconds: stats.seconds, audioOffsetS: enc.audio, log, recordedAt: new Date().toISOString(),
          mb: +(statSync(clipPath(o.cache, m, shot.id)).size / 1048576).toFixed(2) }, null, 1));
        console.log(`  → ${clipPath(o.cache, m, shot.id)} (${((Date.now() - t0) / 1000).toFixed(0)} s)`);
      }
    } finally {
      if (started && !o.keep) await post('/quit').catch(() => {});
      rmSync(raw, { recursive: true, force: true });
    }
  }
  for (const [n, splice] of splices.entries()) {
    const aspect = aspects[n];
    if (splice.titleCard) console.log('title', JSON.stringify(await renderTitle(m.title.text, splice.titleCard.image, splice.titleCard.widthPx)));
    const planFile = join(dir, aspect ? `plan-${aspect}.json` : 'plan.json');
    writeFileSync(planFile, JSON.stringify(splice, null, 1));
    const r = spawnSync('python3', [join(SKILL, 'scripts/splice.py'), planFile], { stdio: 'inherit' });
    if (r.status !== 0) throw new Error('splice.py failed');
    const len = spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', splice.out], { encoding: 'utf8' }).stdout.trim();
    console.log(`\n${splice.out}\n${(+len).toFixed(1)} s, ${(statSync(splice.out).size / 1048576).toFixed(1)} MB`);
  }
  console.log(`clips kept in ${dir} until \`take.mjs --clean ${m.name}\``);
}

// The old single-path take: hold the path's first keyframe for warmS (the LOD streams), record, play, stop.
async function legacy([pathFile, name, warmS = '10']) {
  const path = JSON.parse(readFileSync(pathFile, 'utf8'));
  const hold = { formatVersion: 1, keyframes: [{ ...path.keyframes[0], t: 0 }, { t: 1000 }], segments: [{ move: 'cut' }] };
  console.log(await post('/eval', `(async () => { d.theater('hud', { shown: false }); d.setView({ clouds: 'off' });
    d.theater('play', { path: ${JSON.stringify(hold)} }); await new Promise(r => setTimeout(r, ${+warmS * 1000})); return JSON.stringify(d.state().lod.drawn); })()`));
  console.log(await post(`/rec/start?name=${name}`));
  console.log(await post('/eval', `(async () => { d.theater('play', { path: ${JSON.stringify(path)} });
    while (!d.theater('state', {}).finished) await new Promise(r => setTimeout(r, 100)); await new Promise(r => setTimeout(r, 300)); return 'played'; })()`));
  console.log(await post('/rec/stop'));
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) main().catch((e) => { console.error(e.message); process.exit(1); });
