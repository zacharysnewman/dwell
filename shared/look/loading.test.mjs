// node --test shared/look/loading.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { VIEW_RADIUS_CHUNKS, loadingRust, loadingTs, preloadServerRust, validateLoading } from './loading.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const data = () => JSON.parse(readFileSync(join(here, 'loading.json'), 'utf8'));
const checkedIn = (p) => readFileSync(join(here, '../..', p), 'utf8');

test('the checked-in tunables validate and generate the checked-in Rust', () => {
  assert.equal(loadingRust(validateLoading(data())), checkedIn('crates/dwell-game/src/loading.gen.rs'));
  assert.equal(preloadServerRust(validateLoading(data())), checkedIn('crates/dwell-sim/src/respawn_preload.gen.rs'));
  assert.equal(loadingTs(validateLoading(data())), checkedIn('client/src/ui/loading.gen.ts'));
});

test('the fade is data: a change by data alone is in the shell', () => {
  const d = data();
  d.fade.color = '#FFEEDD';
  d.fade.durationS = 1.5;
  d.fade.easePower = 2;
  d.fade.cpuDurationS = 0;
  const ts = loadingTs(validateLoading(d));
  assert.match(ts, /color: '#ffeedd',/);
  assert.match(ts, /durationS: 1\.5,/);
  assert.match(ts, /easePower: 2,/);
  assert.match(ts, /cpuDurationS: 0,/);
});

test('a change by data alone is in the output', () => {
  const d = data();
  d.radiusChunks = 1;
  d.timeoutS = 7.5;
  const rs = loadingRust(validateLoading(d));
  assert.match(rs, /radius_chunks: 1,/);
  assert.match(rs, /timeout_s: 7\.5,/);
});

test('the near ring stays within the server-streamed view', () => {
  assert.equal(VIEW_RADIUS_CHUNKS, 3);
  const d = data();
  d.radiusChunks = 4;
  assert.throws(() => validateLoading(d, 4 - 1), /radiusChunks must be a whole number in \[0, 3\]/);
  assert.doesNotThrow(() => validateLoading(d, 5));
});

test('bad entries fail with their path named', () => {
  const cases = [
    [(d) => (d.radiusChunks = 1.5), /radiusChunks must be a whole number/],
    [(d) => (d.radiusChunks = -1), /radiusChunks must be a whole number/],
    [(d) => (d.timeoutS = 0), /timeoutS must be a number in \[1, 600\]/],
    [(d) => (d.timeoutS = '20'), /timeoutS must be a number/],
    [(d) => (d.radius = 2), /unknown entry radius/],
    [(d) => delete d.timeoutS, /missing entry timeoutS/],
    [(d) => (d.respawnPreload.enabled = 'yes'), /respawnPreload.enabled must be true or false/],
    [(d) => (d.respawnPreload.radiusChunks = 4), /respawnPreload.radiusChunks must be a whole number in \[0, 3\]/],
    [(d) => (d.respawnPreload.radius = 2), /unknown entry respawnPreload.radius/],
    [(d) => delete d.respawnPreload.enabled, /missing entry respawnPreload.enabled/],
    [(d) => delete d.fade, /missing entry fade/],
    [(d) => (d.fade.color = 'white'), /fade\.color must be a #rrggbb colour/],
    [(d) => (d.fade.durationS = -1), /fade\.durationS must be a number in \[0, 5\]/],
    [(d) => (d.fade.easePower = 0.5), /fade\.easePower must be a number in \[1, 8\]/],
    [(d) => (d.fade.cpuDurationS = 2), /must be at most fade\.durationS/],
    [(d) => (d.fade.speed = 1), /unknown entry fade\.speed/],
    [(d) => delete d.fade.holdS, /missing entry fade\.holdS/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLoading(d), message);
  }
});

test('the respawn preload is data: its radius, or off', () => {
  const d = data();
  d.respawnPreload.radiusChunks = 2;
  assert.match(loadingRust(validateLoading(d)), /RESPAWN_PRELOAD_RADIUS_CHUNKS: Option<i32> = Some\(2\);/);
  assert.match(preloadServerRust(validateLoading(d)), /RESPAWN_PRELOAD_RADIUS_CHUNKS: Option<i32> = Some\(2\);/);
  d.respawnPreload.enabled = false;
  assert.match(preloadServerRust(validateLoading(d)), /RESPAWN_PRELOAD_RADIUS_CHUNKS: Option<i32> = None;/);
});

test('the start stages are data: a stage added by data alone is in the shell', () => {
  const d = data();
  d.stages.list.push({ id: 'fixtureStage', label: "Warming the fixture's cache", progress: 'count' });
  d.stages.slowS = 5;
  const ts = loadingTs(validateLoading(d));
  assert.match(ts, /export type LoadingStageId = [^;]*'fixtureStage';/);
  assert.match(ts, /\{ id: 'fixtureStage', label: "Warming the fixture's cache", progress: 'count' \},/);
  assert.match(ts, /LOADING_STAGE_SLOW_S = 5;/);
  // The order is the data's.
  assert.ok(ts.indexOf("id: 'page'") < ts.indexOf("id: 'terrain'"));
  assert.ok(ts.indexOf("id: 'terrain'") < ts.indexOf("id: 'fixtureStage'"));
});

test('bad stages fail with their entry named', () => {
  const cases = [
    [(d) => delete d.stages, /missing entry stages/],
    [(d) => (d.stages.list = []), /stages\.list must be a non-empty array/],
    [(d) => (d.stages.slowS = 0), /stages\.slowS must be a number in \[0\.5, 60\]/],
    [(d) => (d.stages.speed = 1), /unknown entry stages\.speed/],
    [(d) => (d.stages.list[1].id = 'Game'), /stages\.list\[1\]\.id must be a lowerCamel name/],
    [(d) => (d.stages.list[1].id = 'page'), /stages\.list\[1\]\.id page is listed twice/],
    [(d) => (d.stages.list[0].label = ''), /stages\.list page: label must be 1–32 characters/],
    [(d) => (d.stages.list[0].label = 'x'.repeat(33)), /label must be 1–32 characters/],
    [(d) => (d.stages.list[0].label = 'Loading…'), /label must not end in an ellipsis/],
    [(d) => (d.stages.list[0].progress = 'percent'), /stages\.list page: progress must be one of bytes, count/],
    [(d) => (d.stages.list[0].icon = 'x'), /unknown entry stages\.list\[0\]\.icon/],
  ];
  for (const [edit, message] of cases) {
    const d = data();
    edit(d);
    assert.throws(() => validateLoading(d), message);
  }
});
