// node --test .claude/skills/record/scripts/take.test.mjs — the record skill's manifests and re-record plans, no browser.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { SKILL, VIDEOS, clipPath, fingerprint, loadVideo, metaPath, resolveOnly, selectShots, splicePlan, validate } from './videos.mjs';
import { encodeClip, fetchRetrying } from './take.mjs';

const bifacial = () => loadVideo('bifacial');
const copy = (v) => JSON.parse(JSON.stringify(v));
const fails = (m, re) => assert.throws(() => validate(m), re);

test('every committed video manifest validates', () => {
  for (const name of ['bifacial']) assert.equal(loadVideo(name).name, name);
  assert.ok(existsSync(join(VIDEOS, 'bifacial', 'video.json')));
});

test('unknown keys fail, at every level', () => {
  const m = bifacial();
  fails({ ...copy(m), music: true }, /video: unknown key "music"/);
  let b = copy(m); b.world.seed = 7; fails(b, /video.world: unknown key "seed"/);
  b = copy(m); b.shots[0].speed = 2; fails(b, /shots\[0\] \(s1\): unknown key "speed"/);
  b = copy(m); b.shots[0].setup.time = 0.3; fails(b, /setup: unknown key "time"/);
  b = copy(m); b.shots[2].splice[1].slow = 0.35; fails(b, /splice\[1\]: unknown key "slow"/);
});

test('missing or wrong files fail', () => {
  let b = copy(bifacial()); b.shots[0].path = 'videos/bifacial/nope.json'; fails(b, /no file videos\/bifacial\/nope.json/);
  b = copy(bifacial()); b.shots[2].script = 'videos/bifacial/gone.js'; fails(b, /no file videos\/bifacial\/gone.js/);
  b = copy(bifacial()); b.shots[0].path = 'videos/bifacial/video.json'; fails(b, /is not a theater path/);
  b = copy(bifacial()); b.shots[0].script = 'videos/bifacial/portal_take.js'; fails(b, /script: not used by a "path" shot/);
  b = copy(bifacial()); b.shots[0].kind = 'orbit'; fails(b, /kind: expected one of path, take/);
});

test('bad times, ids and settings fail with a message', () => {
  let b = copy(bifacial()); b.shots[0].splice[0].to = 0; fails(b, /"to" \(0\) must be after "from"/);
  b = copy(bifacial()); b.shots[1].id = 's1'; fails(b, /"s1" is used twice/);
  b = copy(bifacial()); b.shots[3].splice[0].label = 's5'; fails(b, /"s5" is used twice/);
  b = copy(bifacial()); b.shots[0].setup.timeOfDay = 1.5; fails(b, /timeOfDay: expected 0–1/);
  b = copy(bifacial()); b.shots[0].splice[0].cut = true; fails(b, /nothing to cut from/);
  b = copy(bifacial()); b.capture.size = [1080]; fails(b, /capture.size: expected \[width, height\]/);
  b = copy(bifacial()); b.formatVersion = 2; fails(b, /formatVersion: expected 1/);
});

test('a new video by data alone: a fixture manifest validates and plans', () => {
  const m = validate({ formatVersion: 1, name: 'fixture', world: { url: '/dwell/?fresh=1&seed=1' }, capture: { size: [608, 1080], fps: 30 },
    shots: [{ id: 'a', kind: 'path', path: 'videos/bifacial/s1_outline_push.json', splice: [{ to: 3 }] },
      { id: 'b', kind: 'take', script: 'videos/bifacial/portal_take.js', args: { up: 'auto' }, splice: [{ from: 1, to: 2, speed: 4 }] }] });
  assert.deepEqual(splicePlan(m, '/c').clips, [{ src: '/c/fixture/a.mp4', from: 0, to: 3 }, { src: '/c/fixture/b.mp4', from: 1, to: 2, speed: 4 }]);
});

test('--only takes a shot id or a part label; s5 is the portal take (the landing)', () => {
  const m = bifacial();
  assert.deepEqual(resolveOnly(m, ['s5']), ['portal']);
  assert.deepEqual(resolveOnly(m, ['s3', 's5', 's6']), ['portal', 's6']);
  assert.deepEqual(resolveOnly(m, ['s1']), ['s1']);
  assert.throws(() => resolveOnly(m, ['s9']), /--only s9: no shot or part/);
});

test('shot 5 camera and up are manifest args the take script reads', () => {
  const portal = bifacial().shots.find((s) => s.id === 'portal');
  assert.deepEqual(portal.args.camB, [1796.3, -50.5, 20707.3]); // v5 (v4: [1789.5, -58, 20700.5])
  assert.equal(portal.args.up, 'auto'); // v5: upright on face B (v4 held face A's: "a")
  const s6 = JSON.parse(readFileSync(join(SKILL, bifacial().shots.find((s) => s.id === 's6').path), 'utf8'));
  assert.deepEqual(s6.keyframes[0].position, portal.args.camB); // the drone rise starts from shot 5's camera
  const src = readFileSync(join(SKILL, portal.script), 'utf8');
  assert.match(src, /up: A\.up \?\? 'a'/);
  assert.match(src, /A\.camB/);
});

test('selectShots: named shots record, cached ones are reused, missing or stale ones record', () => {
  const m = bifacial();
  const fp = (s) => fingerprint(m, s);
  const portal = m.shots.find((s) => s.id === 'portal');
  const metas = { s1: { fingerprint: fp(m.shots[0]) }, s2: { fingerprint: 'old' }, portal: { fingerprint: fp(portal) } };
  const plan = selectShots(m, { only: ['s5'], cached: (id) => metas[id] ?? null, fingerprintOf: fp });
  assert.deepEqual(plan.map((p) => [p.id, p.action, p.why]), [
    ['s1', 'reuse', 'cached'], ['s2', 'record', 'recipe changed since the cached clip'], ['portal', 'record', 'named'], ['s6', 'record', 'no cached clip']]);
  const all = Object.fromEntries(m.shots.map((s) => [s.id, { fingerprint: fp(s) }]));
  assert.deepEqual(selectShots(m, { cached: (id) => all[id], fingerprintOf: fp }).map((p) => p.action), ['reuse', 'reuse', 'reuse', 'reuse']);
  assert.throws(() => selectShots(m, { spliceOnly: true, cached: (id) => metas[id] ?? null, fingerprintOf: fp }), /no cached clip for s6/);
  const stale = selectShots(m, { spliceOnly: true, cached: (id) => ({ ...all, s2: { fingerprint: 'old' } })[id], fingerprintOf: fp });
  assert.deepEqual(stale.map((p) => p.action), ['reuse', 'reuse', 'reuse', 'reuse']);
});

test('the fingerprint changes with the footage recipe, not with the splice cut', () => {
  const m = bifacial();
  const s = m.shots.find((x) => x.id === 'portal');
  const before = fingerprint(m, s);
  assert.equal(fingerprint(m, { ...s, splice: [{ from: 0, to: 1 }] }), before);
  assert.notEqual(fingerprint(m, { ...s, args: { ...s.args, up: 'b' } }), before);
  assert.notEqual(fingerprint(m, { ...s, setup: { reload: false } }), before);
  assert.notEqual(fingerprint({ ...m, world: { url: '/dwell/?seed=8' } }, s), before);
});

test('the generated splice plan is the hand-written v4 plan, from the cached clips', () => {
  const v4 = JSON.parse(readFileSync(join(SKILL, 'paths/bifacial-v4/plan.json'), 'utf8'));
  const take = { s1: 's1', s2: 's2', portal4: 'portal', s5: 's6' }; // v4's frame folders → the manifest's shots
  const plan = splicePlan(bifacial(), '/cache');
  assert.equal(plan.out, '/cache/bifacial/bifacial.mp4');
  assert.deepEqual(plan.size, v4.size);
  assert.equal(plan.fps, v4.fps);
  assert.deepEqual(plan.clips, v4.clips.map((c) => ({ ...c, src: `/cache/bifacial/${take[basename(c.src)]}.mp4` })));
});

test('take.mjs --dry-run reuses a cached clip whose recipe matches and records the rest', () => {
  const cache = mkdtempSync(join(tmpdir(), 'take-test-'));
  const m = bifacial();
  mkdirSync(join(cache, 'bifacial'), { recursive: true });
  for (const s of m.shots.filter((x) => x.id !== 's2')) {
    writeFileSync(clipPath(cache, m, s.id), '');
    writeFileSync(metaPath(cache, m, s.id), JSON.stringify({ fingerprint: fingerprint(m, s) }));
  }
  const r = spawnSync(process.execPath, [join(SKILL, 'scripts/take.mjs'), 'bifacial', '--only', 's6', '--dry-run', '--cache', cache], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /reuse  s1 {2}\(cached\)\nrecord s2 {2}\(no cached clip\)\nreuse  portal {2}\(cached\)\nrecord s6 {2}\(named\)/);
  assert.equal(JSON.parse(r.stdout.slice(r.stdout.indexOf('{'))).clips.length, 6);
  const bad = spawnSync(process.execPath, [join(SKILL, 'scripts/take.mjs'), 'bifacial', '--splice-only', '--cache', cache], { encoding: 'utf8' });
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /--splice-only: no cached clip for s2/);
});

const ffmpeg = spawnSync('ffmpeg', ['-version']).status === 0;
test('encodeClip turns a driver frame folder into an mp4 at the measured rate', { skip: !ffmpeg && 'no ffmpeg' }, () => {
  const dir = mkdtempSync(join(tmpdir(), 'take-frames-'));
  const n = 30;
  assert.equal(spawnSync('ffmpeg', ['-v', 'error', '-f', 'lavfi', '-i', `testsrc=size=64x112:rate=60`, '-frames:v', String(n), join(dir, 'f%05d.jpg')]).status, 0);
  writeFileSync(join(dir, 'times.json'), JSON.stringify(Array.from({ length: n }, (_, i) => 100 + i / 60)));
  const out = join(dir, 'clip.mp4');
  const enc = encodeClip(dir, out);
  assert.equal(enc.frames, n);
  assert.ok(Math.abs(enc.rate - 60) < 0.01);
  const dur = +spawnSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', out], { encoding: 'utf8' }).stdout;
  assert.ok(Math.abs(dur - n / 60) < 0.05, `duration ${dur}`);
});

test('a post on a socket the driver closed while idle is retried once; other errors are not', async () => {
  const sock = Object.assign(new TypeError('fetch failed'), { cause: { code: 'UND_ERR_SOCKET' } });
  let n = 0;
  const flaky = async () => { if (n++ === 0) throw sock; return 'ok'; };
  assert.equal(await fetchRetrying('u', {}, flaky), 'ok');
  assert.equal(n, 2);
  const refused = Object.assign(new TypeError('fetch failed'), { cause: { code: 'ECONNREFUSED' } });
  n = 0;
  await assert.rejects(fetchRetrying('u', {}, async () => { n++; throw refused; }), /fetch failed/);
  assert.equal(n, 1);
});
