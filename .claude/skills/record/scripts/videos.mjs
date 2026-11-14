// The record skill's per-video manifests (videos/<name>/video.json), as plain functions with no browser:
// load and validate a manifest, pick which shots to record against the clip cache, fingerprint a shot
// (a cached clip is reused only while its recipe is unchanged), and build the splice plan for splice.py.
// take.mjs does the side effects (the preview, the driver, recording, encoding, splicing); take.test.mjs
// tests this file.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PART_AUDIO, partAudio } from './audio.mjs';

export const SKILL = resolve(dirname(fileURLToPath(import.meta.url)), '..');
export const VIDEOS = join(SKILL, 'videos');

const KEYS = {
  video: ['formatVersion', 'name', 'description', 'world', 'capture', 'aspects', 'title', 'audio', 'splice', 'shots'],
  title: ['text', 'fromEndS', 'fadeS', 'width'],
  world: ['url', 'storage'],
  capture: ['size', 'fps'],
  splice: ['fade', 'fadeIn', 'crf', 'gainDb'],
  shot: ['id', 'note', 'kind', 'path', 'script', 'args', 'setup', 'capture', 'audio', 'splice'],
  setup: ['reload', 'url', 'timeOfDay', 'view', 'hud', 'warmS', 'settleS', 'eval'],
  part: ['label', 'from', 'to', 'speed', 'vf', 'cut', 'audio', 'crop'],
};
const KINDS = { path: 'path', take: 'script' };

/** Throws with every problem in the manifest (unknown keys, missing files, bad times), or returns it. */
export function validate(m, base = SKILL) {
  const errs = [];
  const obj = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
  const keys = (v, kind, where) => {
    if (!obj(v)) { errs.push(`${where}: expected an object`); return false; }
    for (const k of Object.keys(v)) if (!KEYS[kind].includes(k)) errs.push(`${where}: unknown key "${k}" (known: ${KEYS[kind].join(', ')})`);
    return true;
  };
  const num = (v, where, min = 0) => { if (typeof v !== 'number' || !(v >= min)) errs.push(`${where}: expected a number ≥ ${min}, got ${JSON.stringify(v)}`); };
  const size = (v, where) => { if (!Array.isArray(v) || v.length !== 2 || !v.every((x) => Number.isInteger(x) && x > 0)) errs.push(`${where}: expected [width, height] in pixels`); };
  if (!keys(m, 'video', 'video')) throw new Error(errs.join('\n'));
  if (m.formatVersion !== 1) errs.push(`video.formatVersion: expected 1, got ${JSON.stringify(m.formatVersion)}`);
  if (typeof m.name !== 'string' || !/^[a-z0-9][a-z0-9-]*$/.test(m.name)) errs.push('video.name: expected a lowercase slug');
  if (keys(m.world, 'world', 'video.world')) {
    if (typeof m.world.url !== 'string' || !m.world.url.startsWith('/')) errs.push('video.world.url: expected a link path from the preview\'s root ("/dwell/?fresh=1&seed=…")');
    if (m.world.storage !== undefined && (!obj(m.world.storage) || !Object.values(m.world.storage).every((s) => typeof s === 'string')))
      errs.push('video.world.storage: expected {localStorage key: string value}');
  }
  if (keys(m.capture, 'capture', 'video.capture')) { size(m.capture.size, 'video.capture.size'); num(m.capture.fps, 'video.capture.fps', 1); }
  if (m.audio !== undefined && typeof m.audio !== 'boolean') errs.push('video.audio: expected true or false (the game\'s effects and ambience; never music)');
  if (m.splice !== undefined && keys(m.splice, 'splice', 'video.splice')) {
    if (m.splice.fade !== undefined) num(m.splice.fade, 'video.splice.fade');
    if (m.splice.fadeIn !== undefined) num(m.splice.fadeIn, 'video.splice.fadeIn');
    if (m.splice.crf !== undefined) num(m.splice.crf, 'video.splice.crf');
    if (m.splice.gainDb !== undefined && !(typeof m.splice.gainDb === 'number' && m.splice.gainDb >= 0 && m.splice.gainDb <= 40)) errs.push('video.splice.gainDb: expected a number of dB from 0 to 40 (the whole sound lifted, limited)');
  }
  // aspects: one shot list cut to several framings ({"9x16": [1080, 1920], "16x9": [1920, 1080]}), each a crop of the
  // capture (record a square, crop each aspect out of it); without it the video has one framing, capture.size
  const aspects = m.aspects === undefined ? [] : obj(m.aspects) ? Object.keys(m.aspects) : (errs.push('video.aspects: expected {name: [width, height]}'), []);
  for (const a of aspects) {
    if (!/^[a-z0-9][a-z0-9-]*$/.test(a)) errs.push(`video.aspects: "${a}" is not a lowercase name (e.g. 9x16)`);
    size(m.aspects[a], `video.aspects.${a}`);
  }
  if (m.title !== undefined && keys(m.title, 'title', 'video.title')) {
    if (typeof m.title.text !== 'string' || !m.title.text.trim()) errs.push('video.title.text: expected the title\'s text');
    num(m.title.fromEndS, 'video.title.fromEndS');
    if (m.title.fadeS !== undefined) num(m.title.fadeS, 'video.title.fadeS');
    const w = m.title.width;
    const frac = (v, where) => { if (!(typeof v === 'number' && v > 0 && v <= 1)) errs.push(`${where}: expected a share of the frame's width, 0–1`); };
    if (typeof w === 'number') frac(w, 'video.title.width');
    else if (obj(w)) for (const [a, v] of Object.entries(w)) { if (!aspects.includes(a)) errs.push(`video.title.width: no aspect "${a}"`); frac(v, `video.title.width.${a}`); }
    else errs.push('video.title.width: expected a share of the frame\'s width, or one per aspect');
  }
  if (!Array.isArray(m.shots) || m.shots.length === 0) errs.push('video.shots: expected a non-empty list');
  const ids = new Set(), labels = new Set();
  for (const [i, s] of (Array.isArray(m.shots) ? m.shots : []).entries()) {
    const at = `shots[${i}]${s?.id ? ` (${s.id})` : ''}`;
    if (!keys(s, 'shot', at)) continue;
    if (typeof s.id !== 'string' || !/^[a-z0-9][a-z0-9_-]*$/.test(s.id)) errs.push(`${at}.id: expected a lowercase id`);
    else if (ids.has(s.id) || labels.has(s.id)) errs.push(`${at}.id: "${s.id}" is used twice`);
    ids.add(s.id);
    const fileKey = KINDS[s.kind];
    if (!fileKey) errs.push(`${at}.kind: expected one of ${Object.keys(KINDS).join(', ')}, got ${JSON.stringify(s.kind)}`);
    else {
      for (const other of Object.values(KINDS)) if (other !== fileKey && s[other] !== undefined) errs.push(`${at}.${other}: not used by a "${s.kind}" shot`);
      if (typeof s[fileKey] !== 'string') errs.push(`${at}.${fileKey}: expected a file path (relative to the skill)`);
      else if (!existsSync(resolve(base, s[fileKey]))) errs.push(`${at}.${fileKey}: no file ${s[fileKey]}`);
      else if (s.kind === 'path') {
        try {
          const p = JSON.parse(readFileSync(resolve(base, s.path), 'utf8'));
          if (p.formatVersion !== 1 || !Array.isArray(p.keyframes) || p.keyframes.length < 2) errs.push(`${at}.path: ${s.path} is not a theater path (formatVersion 1, two keyframes or more)`);
        } catch (e) { errs.push(`${at}.path: ${s.path}: ${e.message}`); }
      }
    }
    if (s.audio !== undefined && typeof s.audio !== 'boolean') errs.push(`${at}.audio: expected true or false`);
    if (s.args !== undefined && (s.kind !== 'take' || !obj(s.args))) errs.push(`${at}.args: an object, on a "take" shot only`);
    if (s.setup !== undefined && keys(s.setup, 'setup', `${at}.setup`)) {
      const u = s.setup;
      if (u.reload !== undefined && typeof u.reload !== 'boolean') errs.push(`${at}.setup.reload: expected true or false`);
      if (u.url !== undefined && (typeof u.url !== 'string' || !u.url.startsWith('/'))) errs.push(`${at}.setup.url: expected a link path`);
      if (u.timeOfDay !== undefined && (typeof u.timeOfDay !== 'number' || u.timeOfDay < 0 || u.timeOfDay > 1)) errs.push(`${at}.setup.timeOfDay: expected 0–1`);
      if (u.view !== undefined && !obj(u.view)) errs.push(`${at}.setup.view: expected d.setView's object`);
      if (u.hud !== undefined && typeof u.hud !== 'boolean') errs.push(`${at}.setup.hud: expected true or false`);
      if (u.warmS !== undefined) num(u.warmS, `${at}.setup.warmS`);
      if (u.settleS !== undefined) num(u.settleS, `${at}.setup.settleS`);
      if (u.eval !== undefined && typeof u.eval !== 'string') errs.push(`${at}.setup.eval: expected a JS expression`);
    }
    if (s.capture !== undefined && keys(s.capture, 'capture', `${at}.capture`)) {
      if (s.capture.size !== undefined) size(s.capture.size, `${at}.capture.size`);
      if (s.capture.fps !== undefined) num(s.capture.fps, `${at}.capture.fps`, 1);
    }
    if (!Array.isArray(s.splice) || s.splice.length === 0) errs.push(`${at}.splice: expected a list of parts ({from, to, …})`);
    for (const [j, p] of (Array.isArray(s.splice) ? s.splice : []).entries()) {
      const pat = `${at}.splice[${j}]`;
      if (!keys(p, 'part', pat)) continue;
      if (p.from !== undefined) num(p.from, `${pat}.from`);
      num(p.to, `${pat}.to`);
      if (typeof p.to === 'number' && p.to <= (p.from ?? 0)) errs.push(`${pat}: "to" (${p.to}) must be after "from" (${p.from ?? 0})`);
      if (p.speed !== undefined && !(typeof p.speed === 'number' && p.speed > 0)) errs.push(`${pat}.speed: expected a number > 0`);
      if (p.vf !== undefined && typeof p.vf !== 'string') errs.push(`${pat}.vf: expected ffmpeg filters`);
      if (p.cut !== undefined && typeof p.cut !== 'boolean') errs.push(`${pat}.cut: expected true or false`);
      if (p.audio !== undefined && !PART_AUDIO.includes(p.audio)) errs.push(`${pat}.audio: expected one of ${PART_AUDIO.join(', ')}, got ${JSON.stringify(p.audio)}`);
      if (p.crop !== undefined) {
        // where each aspect's crop sits in the capture: [x, y], 0 = left/top edge, 0.5 the centre, 1 = right/bottom
        if (!obj(p.crop)) errs.push(`${pat}.crop: expected {aspect: [x, y]} (0–1, 0.5 the centre)`);
        else for (const [a, v] of Object.entries(p.crop)) {
          if (!aspects.includes(a)) errs.push(`${pat}.crop: no aspect "${a}" (video.aspects: ${aspects.join(', ') || 'none'})`);
          if (!Array.isArray(v) || v.length !== 2 || !v.every((x) => typeof x === 'number' && x >= 0 && x <= 1)) errs.push(`${pat}.crop.${a}: expected [x, y], each 0–1`);
        }
      }
      if (p.cut && i === 0 && j === 0) errs.push(`${pat}.cut: the first part has nothing to cut from`);
      if (p.label !== undefined) {
        if (typeof p.label !== 'string' || !/^[a-z0-9][a-z0-9_-]*$/.test(p.label)) errs.push(`${pat}.label: expected a lowercase id`);
        else if (labels.has(p.label) || (ids.has(p.label) && p.label !== s.id)) errs.push(`${pat}.label: "${p.label}" is used twice`);
        labels.add(p.label);
      }
    }
  }
  if (errs.length) throw new Error(`invalid video manifest:\n  ${errs.join('\n  ')}`);
  return m;
}

/** Reads and validates videos/<name>/video.json (or a manifest file path). */
export function loadVideo(nameOrFile, videos = VIDEOS) {
  const file = nameOrFile.endsWith('.json') ? resolve(nameOrFile) : join(videos, nameOrFile, 'video.json');
  if (!existsSync(file)) throw new Error(`no video manifest ${file}`);
  let m;
  try { m = JSON.parse(readFileSync(file, 'utf8')); } catch (e) { throw new Error(`${file}: ${e.message}`); }
  return validate(m);
}

/** The shot ids that `--only` names: a shot's id, or a splice part's label (which selects its whole take). */
export function resolveOnly(m, only) {
  const out = [];
  for (const name of only) {
    const shot = m.shots.find((s) => s.id === name || s.splice.some((p) => p.label === name));
    if (!shot) throw new Error(`--only ${name}: no shot or part of that name (shots: ${m.shots.map(label).join(', ')})`);
    if (!out.includes(shot.id)) out.push(shot.id);
  }
  return out;
}
const label = (s) => s.splice.some((p) => p.label) ? `${s.id} [${s.splice.map((p) => p.label).filter(Boolean).join(' ')}]` : s.id;

/** Whether a shot records the game's sound: the shot's `audio`, else the video's, else on (effects and ambience; never music). */
export const shotAudio = (m, shot) => shot.audio ?? m.audio ?? true;

/** A hash of everything that changes a shot's footage (not its splice parts): reuse a clip only while it matches. */
export function fingerprint(m, shot, base = SKILL) {
  const file = shot[KINDS[shot.kind]];
  const recipe = {
    world: m.world, capture: { ...m.capture, ...shot.capture }, kind: shot.kind, args: shot.args ?? null,
    setup: shot.setup ?? null, file: readFileSync(resolve(base, file), 'utf8'), audio: shotAudio(m, shot),
  };
  return createHash('sha256').update(JSON.stringify(recipe)).digest('hex').slice(0, 16);
}

/** Where a video's clips live: <cache>/<video>/<shot>.mp4 with <shot>.json (the fingerprint and capture stats). */
export const clipPath = (cache, m, id) => join(cache, m.name, `${id}.mp4`);
export const metaPath = (cache, m, id) => join(cache, m.name, `${id}.json`);
/** The title card's image for an aspect (title.mjs renders it: the game's wordmark, transparent). */
export const titlePath = (cache, m, aspect) => join(cache, m.name, `title${aspect ? `-${aspect}` : ''}.png`);

/**
 * Which shots to record and why: the ones `--only` names, and every other shot with no cached clip or
 * a stale one (its recipe changed). `cached(id)` returns the clip's sidecar JSON or null.
 * With `spliceOnly`, nothing records and a missing clip is an error.
 */
export function selectShots(m, { only = [], spliceOnly = false, cached, fingerprintOf }) {
  const named = resolveOnly(m, only);
  const plan = m.shots.map((s) => {
    const meta = cached(s.id);
    if (named.includes(s.id)) return { id: s.id, action: 'record', why: 'named' };
    if (!meta) return { id: s.id, action: 'record', why: 'no cached clip' };
    if (meta.fingerprint !== fingerprintOf(s)) return { id: s.id, action: 'record', why: 'recipe changed since the cached clip' };
    return { id: s.id, action: 'reuse', why: 'cached' };
  });
  if (spliceOnly) {
    if (named.length) throw new Error('--splice-only records nothing: drop --only');
    const missing = plan.filter((p) => p.why === 'no cached clip').map((p) => p.id);
    if (missing.length) throw new Error(`--splice-only: no cached clip for ${missing.join(', ')}`);
    return plan.map((p) => p.action === 'record' ? { ...p, action: 'reuse', why: `${p.why} (kept: --splice-only)` } : p);
  }
  return plan;
}

/** The video's framings: its aspects' names, or [null] (one framing, the capture's size). */
export const aspectsOf = (m) => m.aspects ? Object.keys(m.aspects) : [null];

/** The splice.py plan for the whole video from the cached clips, in one aspect (a name from video.aspects) or the capture's framing. */
export function splicePlan(m, cache, aspect = null) {
  if (aspect !== null && !m.aspects?.[aspect]) throw new Error(`no aspect "${aspect}" (video.aspects: ${aspectsOf(m).join(', ')})`);
  const size = aspect ? m.aspects[aspect] : m.capture.size;
  const plan = { out: join(cache, m.name, `${m.name}${aspect ? `-${aspect}` : ''}.mp4`), size, fps: m.capture.fps };
  if (m.splice?.fade !== undefined) plan.fade = m.splice.fade;
  if (m.splice?.fadeIn !== undefined) plan.fadeIn = m.splice.fadeIn;
  if (m.splice?.crf !== undefined) plan.crf = m.splice.crf;
  if (m.splice?.gainDb !== undefined) plan.gainDb = m.splice.gainDb;
  plan.audio = m.shots.some((s) => shotAudio(m, s));
  plan.clips = m.shots.flatMap((s) => s.splice.map(({ label: _, audio, crop, ...p }) => ({
    src: clipPath(cache, m, s.id), from: p.from ?? 0, ...p,
    ...(crop?.[aspect] ? { crop: crop[aspect] } : {}),
    ...(plan.audio ? { audio: shotAudio(m, s) ? partAudio(audio, p.speed ?? 1) : 'mute' } : {}),
  })));
  if (m.title) {
    const w = typeof m.title.width === 'number' ? m.title.width : m.title.width[aspect] ?? 0.5;
    plan.titleCard = { image: titlePath(cache, m, aspect), fromEndS: m.title.fromEndS, fadeS: m.title.fadeS ?? 1 };
    plan.titleCard.widthPx = Math.round(size[0] * w);
  }
  return plan;
}
