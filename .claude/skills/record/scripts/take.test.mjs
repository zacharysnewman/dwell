// node --test .claude/skills/record/scripts/take.test.mjs — the record skill's manifests and re-record plans, no browser.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, join } from 'node:path';
import { SKILL, VIDEOS, clipPath, fingerprint, loadVideo, metaPath, resolveOnly, selectShots, shotAudio, splicePlan, validate } from './videos.mjs';
import { alignFilter, audioAlignment, audioStartExpr, encodeArgs, partAudio } from './audio.mjs';
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
  assert.deepEqual(splicePlan(m, '/c').clips, [{ src: '/c/fixture/a.mp4', from: 0, to: 3, audio: 'keep' },
    { src: '/c/fixture/b.mp4', from: 1, to: 2, speed: 4, audio: 'mute' }]);
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
  assert.deepEqual(plan.clips.map(({ audio: _, ...c }) => c), v4.clips.map((c) => ({ ...c, src: `/cache/bifacial/${take[basename(c.src)]}.mp4` })));
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

// The showcase sound (WORK_QUEUE item 40: effects and ambience on, never music).

test('the manifest\'s audio: on by default, per shot override, per part mode; bad values fail', () => {
  const m = bifacial();
  assert.equal(m.audio, undefined);
  for (const s of m.shots) assert.equal(shotAudio(m, s), true);
  assert.equal(shotAudio({ ...m, audio: false }, m.shots[0]), false);
  assert.equal(shotAudio({ ...m, audio: false }, { ...m.shots[0], audio: true }), true);
  let b = copy(m); b.audio = 'music'; fails(b, /video.audio: expected true or false/);
  b = copy(m); b.shots[0].audio = 1; fails(b, /shots\[0\] \(s1\).audio: expected true or false/);
  b = copy(m); b.shots[2].splice[1].audio = 'loud'; fails(b, /splice\[1\].audio: expected one of auto, keep, stretch, mute/);
  b = copy(m); b.audio = false; b.shots[1].audio = true; b.shots[2].splice[0].audio = 'mute'; assert.ok(validate(b));
});

test('a part\'s sound: kept at speed 1, stretched for slow motion and gentle speed-ups, muted on a time-lapse', () => {
  assert.equal(partAudio(undefined, 1), 'keep');
  assert.equal(partAudio('auto', 0.35), 'stretch');
  assert.equal(partAudio('auto', 2), 'stretch');
  assert.equal(partAudio('auto', 4), 'mute');
  assert.equal(partAudio('auto', 0.1), 'mute');
  assert.equal(partAudio('stretch', 8), 'stretch');
  assert.equal(partAudio('keep', 1.5), 'stretch'); // kept sound must still follow the picture
  assert.equal(partAudio('mute', 1), 'mute');
  const plan = splicePlan(bifacial(), '/c');
  assert.equal(plan.audio, true);
  assert.deepEqual(plan.clips.map((c) => c.audio), ['keep', 'keep', 'keep', 'stretch', 'keep', 'keep']); // s4 slowed 0.35
  const off = copy(bifacial()); off.shots[1].audio = false;
  assert.equal(splicePlan(validate(off), '/c').clips[1].audio, 'mute');
  const none = copy(bifacial()); none.audio = false;
  const silent = splicePlan(validate(none), '/c');
  assert.equal(silent.audio, false);
  assert.ok(silent.clips.every((c) => c.audio === undefined));
});

test('the sound changes a shot\'s recipe: turning it off re-records', () => {
  const m = bifacial();
  assert.notEqual(fingerprint(m, { ...m.shots[0], audio: false }), fingerprint(m, m.shots[0]));
  assert.equal(fingerprint(m, { ...m.shots[0], audio: true }), fingerprint(m, m.shots[0]));
});

test('the driver taps the mix without music or UI', () => {
  const e = audioStartExpr();
  assert.match(e, /__dwell\?\.audioTap\?\.\(\{"music":false,"ui":false\}\)/);
  assert.match(e, /MediaRecorder/);
  assert.match(e, /performance\.timeOrigin/); // epoch seconds, the screencast frames' clock
});

test('the audio is aligned to the first frame and cut to the clip, 48 kHz AAC', () => {
  const times = [100, 100.5, 101]; // 3 frames
  assert.deepEqual(audioAlignment(times, { startS: 100.25 }, 2), { offsetS: 0.25, durationS: 1.5 });
  assert.equal(alignFilter({ offsetS: 0.25, durationS: 1.5 }),
    '[1:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,adelay=delays=250:all=1,apad,atrim=0:1.500[a]');
  assert.equal(alignFilter({ offsetS: -0.125, durationS: 2 }),
    '[1:a]aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,atrim=start=0.125,asetpts=PTS-STARTPTS,apad,atrim=0:2.000[a]');
  const withSound = encodeArgs({ frames: 'f%05d.jpg', rate: 60, out: 'o.mp4', audio: { file: 's.webm', offsetS: 0, durationS: 1 } });
  assert.deepEqual(withSound.slice(withSound.indexOf('-i', 6), withSound.indexOf('-i', 6) + 2), ['-i', 's.webm']);
  assert.ok(withSound.includes('[a]') && withSound.includes('aac') && withSound.includes('48000'));
  assert.ok(!withSound.join(' ').includes('adelay')); // a zero offset needs no delay
  const silent = encodeArgs({ frames: 'f%05d.jpg', rate: 60, out: 'o.mp4' });
  assert.ok(silent.includes('-an') && !silent.includes('aac'));
});

const dry = (plan) => {
  const dir = mkdtempSync(join(tmpdir(), 'splice-plan-'));
  const file = join(dir, 'plan.json');
  writeFileSync(file, JSON.stringify(plan));
  const r = spawnSync('python3', [join(SKILL, 'scripts/splice.py'), file, '--dry-run'], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  return JSON.parse(r.stdout);
};
const python = spawnSync('python3', ['--version']).status === 0;

test('the splice\'s audio graph: cuts concatenate the sound, speed stretches or mutes it, joins cross-fade it', { skip: !python && 'no python3' }, () => {
  const plan = { out: '/w/v.mp4', size: [108, 192], fps: 30, fade: 0.8, audio: true, clips: [
    { src: '/c/a.mp4', from: 0, to: 4, audio: 'keep' },
    { src: '/c/b.mp4', from: 0, to: 2, speed: 0.35, cut: true, audio: 'stretch' },
    { src: '/c/b.mp4', from: 2, to: 6, cut: true, audio: 'keep' },
    { src: '/c/c.mp4', from: 0, to: 8, speed: 4, audio: 'mute' },
  ] };
  const { commands, seconds } = dry(plan);
  const graph = (c) => c[c.indexOf('-filter_complex') + 1];
  const [keep, slow, , lapse, cut, final] = commands;
  assert.match(graph(keep), /\[0:a\]aresample=48000,[^;]*,apad,atrim=0:4\.000\[a\]$/);
  assert.ok(!graph(keep).includes('atempo'));
  assert.match(graph(slow), /atempo=0\.5,atempo=0\.7,apad,atrim=0:5\.714\[a\]/); // 0.35 = 0.5 × 0.7, pitch kept
  assert.match(graph(slow), /\[0:v\]setpts=PTS\/0\.35,/);
  assert.ok(lapse.includes('anullsrc=r=48000:cl=stereo')); // the 4× time-lapse is silent
  assert.match(graph(lapse), /\[1:a\]aresample=48000,[^;]*,atrim=0:2\.000\[a\]/);
  for (const c of [keep, slow, lapse]) assert.ok(c.includes('aac') && c.includes('-map'));
  assert.match(graph(cut), /^\[0:v\]\[0:a\]\[1:v\]\[1:a\]\[2:v\]\[2:a\]concat=n=3:v=1:a=1\[v\]\[a\]$/);
  // the groups (a 4 + 5.714 + 4 s cut group, then the 2 s time-lapse) cross-fade picture and sound together
  assert.match(graph(final), /xfade=transition=fade:duration=0\.8:offset=12\.914/);
  assert.match(graph(final), /\[0:a\]\[1:a\]acrossfade=d=0\.8:c1=tri:c2=tri\[ax1\]/);
  assert.match(graph(final), /\[ax1\]afade=t=out:st=13\.914:d=1,atrim=0:14\.914\[aout\]$/);
  assert.ok(final.includes('[aout]') && final.includes('aac'));
  assert.equal(seconds, 14.914);
});

test('a plan without audio splices silent, as before', { skip: !python && 'no python3' }, () => {
  const { commands } = dry({ out: '/w/v.mp4', size: [108, 192], fps: 30, clips: [
    { src: '/c/a.mp4', from: 0, to: 4 }, { src: '/c/b.mp4', from: 0, to: 2, cut: true }, { src: '/c/c.mp4', from: 0, to: 3 }] });
  for (const c of commands) assert.ok(!c.join(' ').match(/acrossfade|anullsrc|aac|\[0:a\]/), c.join(' '));
  assert.ok(commands[0].includes('-an'));
  assert.ok(commands.at(-1).includes('-an'));
});

test('end to end: a frame folder and its sound → a clip with aligned audio → a spliced video whose sound lasts as long as its picture',
  { skip: (!ffmpeg || !python) && 'no ffmpeg or python3' }, () => {
    const dir = mkdtempSync(join(tmpdir(), 'take-audio-'));
    const frames = join(dir, 'shot');
    mkdirSync(frames);
    const n = 60;
    assert.equal(spawnSync('ffmpeg', ['-v', 'error', '-f', 'lavfi', '-i', 'testsrc=size=64x112:rate=30', '-frames:v', String(n), join(frames, 'f%05d.jpg')]).status, 0);
    writeFileSync(join(frames, 'times.json'), JSON.stringify(Array.from({ length: n }, (_, i) => 1000 + i / 30)));
    // the sound began 0.3 s after the first frame, 44.1 kHz mono (resampled to 48 kHz stereo)
    assert.equal(spawnSync('ffmpeg', ['-v', 'error', '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=44100:duration=3', '-c:a', 'libopus', frames + '.webm']).status, 0);
    writeFileSync(frames + '.audio.json', JSON.stringify({ startS: 1000.3, stopS: 1003.3 }));
    const clip = join(dir, 'shot.mp4');
    const enc = encodeClip(frames, clip);
    assert.equal(enc.audio, 0.3);
    const probe = (f, sel) => spawnSync('ffprobe', ['-v', 'error', '-select_streams', sel, '-show_entries', 'stream=duration,sample_rate,channels', '-of', 'json', f], { encoding: 'utf8' });
    const a = JSON.parse(probe(clip, 'a').stdout).streams[0];
    assert.equal(a.sample_rate, '48000');
    assert.equal(a.channels, 2);
    assert.ok(Math.abs(+a.duration - 2) < 0.05, `audio ${a.duration}`);
    const plan = join(dir, 'plan.json');
    writeFileSync(plan, JSON.stringify({ out: join(dir, 'v.mp4'), size: [64, 112], fps: 30, fade: 0.4, audio: true, clips: [
      { src: clip, from: 0, to: 1, audio: 'keep' }, { src: clip, from: 1, to: 1.5, speed: 0.5, cut: true, audio: 'stretch' },
      { src: clip, from: 0, to: 2, speed: 4, audio: 'mute' }, { src: clip, from: 0.5, to: 2, audio: 'keep' }] }));
    const r = spawnSync('python3', [join(SKILL, 'scripts/splice.py'), plan], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stderr);
    const v = +JSON.parse(probe(join(dir, 'v.mp4'), 'v').stdout).streams[0].duration;
    const s = +JSON.parse(probe(join(dir, 'v.mp4'), 'a').stdout).streams[0].duration;
    assert.ok(Math.abs(v - 3.2) < 0.1, `video ${v}`); // (1 + 1 cut) + 0.5 + 1.5 − 2 × 0.4
    assert.ok(Math.abs(s - v) < 0.08, `audio ${s} vs video ${v}`);
  });
