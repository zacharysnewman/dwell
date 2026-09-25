#!/usr/bin/env node
// Native↔WASM divergence of the player controller (PLAYER_CONTROLLER.md §8.3): runs the scenario
// trace natively and under Node (WASM), and reports the largest position and velocity differences.
// The velocity divergence *introduced per tick* must stay below externalAbsorbThreshold, so
// numerical differences between the builds are never absorbed as external forces.
//
// usage: node tools/divergence.mjs <native dwell_scenario_trace> <wasm dwell_scenario_trace.js>
import { execFileSync } from 'node:child_process';

const EXTERNAL_ABSORB_THRESHOLD = 0.01; // m/s, PlayerControllerConfig::advanced

const [nativeBin, wasmJs] = process.argv.slice(2);
if (!nativeBin || !wasmJs) {
  console.error('usage: divergence.mjs <native trace binary> <wasm trace .js>');
  process.exit(2);
}

function trace(cmd, args) {
  const out = execFileSync(cmd, args, { maxBuffer: 64 << 20 }).toString().trim().split('\n');
  return out.map((line) => line.split(' ').map(Number));
}

const native = trace(nativeBin, []);
const wasm = trace(process.execPath, [wasmJs]);
if (native.length !== wasm.length) {
  console.error(`trace lengths differ: native ${native.length}, wasm ${wasm.length}`);
  process.exit(1);
}

let maxPos = 0;
let maxVel = 0;
let maxVelStep = 0; // velocity divergence added in one tick (difference of consecutive divergences)
let firstDiff = -1;
let stateMismatches = 0;
const prevVelDiff = new Map();
for (let i = 0; i < native.length; i++) {
  const a = native[i];
  const b = wasm[i];
  const [tick, player] = a;
  const dp = Math.hypot(a[2] - b[2], a[3] - b[3], a[4] - b[4]);
  const dv = Math.hypot(a[5] - b[5], a[6] - b[6], a[7] - b[7]);
  if ((dp > 0 || dv > 0) && firstDiff < 0) firstDiff = tick;
  if (a[8] !== b[8]) stateMismatches++;
  maxPos = Math.max(maxPos, dp);
  maxVel = Math.max(maxVel, dv);
  const prev = prevVelDiff.get(player) ?? 0;
  maxVelStep = Math.max(maxVelStep, Math.abs(dv - prev));
  prevVelDiff.set(player, dv);
}

console.log(`native↔WASM over ${native.length} player-ticks:`);
console.log(`  first difference at tick ${firstDiff < 0 ? '— (bit-identical)' : firstDiff}`);
console.log(`  max position divergence ${maxPos.toExponential(3)} m`);
console.log(`  max velocity divergence ${maxVel.toExponential(3)} m/s`);
console.log(`  max velocity divergence added in one tick ${maxVelStep.toExponential(3)} m/s`);
console.log(`  state mismatches ${stateMismatches}`);
console.log(`  externalAbsorbThreshold ${EXTERNAL_ABSORB_THRESHOLD} m/s`);
if (maxVelStep >= EXTERNAL_ABSORB_THRESHOLD) {
  console.error('FAIL: per-tick divergence reaches externalAbsorbThreshold');
  process.exit(1);
}
