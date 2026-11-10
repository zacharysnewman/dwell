// The loading bar as a per-device time estimate (ARCHITECTURE.md §6.3). Owner (2026-11-10): "The
// map loading progress bar speed should be a best guess for how long it will take to load the game
// on the current device … When it reaches the end, it keeps visually showing it's doing stuff … I'd
// rather we estimate this somehow without a previous run if possible." No history is stored: the
// time left is the work still to do (the stages not yet ended) at the rates measured as it runs
// (bytes/s from the download, time per item from a count's first items), and before anything is
// measured an opening guess from the stages' base costs (shared/look/loading.json `estimate`)
// scaled by a quick read of the device (cores, memory, phone, a CPU adapter). The bar fills by
// elapsed / (elapsed + left), eased, never backward, holding short of full until the screen opens.
import {
  LOADING_ESTIMATE,
  LOADING_STAGES,
  type LoadingStageCost,
  type LoadingStageDef,
  type LoadingStageId,
} from './loading.gen';
import type { StageRun } from './loadingStages';

/** The estimate's tunables: `LOADING_ESTIMATE`'s shape. */
export type EstimateTunables = typeof LOADING_ESTIMATE;

/** What the page can read of the device before measuring anything. */
export interface DeviceRead {
  /** `navigator.hardwareConcurrency` (undefined: not said). */
  cores?: number;
  /** `navigator.deviceMemory` in GB, where the browser says (Chromium). */
  memoryGB?: number;
  /** A phone or tablet (touch first). */
  phone: boolean;
  /** The GPU adapter is a CPU (software) one. */
  cpuAdapter: boolean;
  /** The generation and meshing pools' workers (`workerCount`). */
  workers: number;
}

/** The device's read from the browser (`cpuAdapter` false until the graphics say). */
export function readDevice(
  nav: { hardwareConcurrency?: number; deviceMemory?: number },
  phone: boolean,
  workers: number,
): DeviceRead {
  const cores = nav.hardwareConcurrency;
  const memoryGB = nav.deviceMemory;
  return {
    ...(typeof cores === 'number' && cores > 0 ? { cores } : {}),
    ...(typeof memoryGB === 'number' && memoryGB > 0 ? { memoryGB } : {}),
    phone,
    cpuAdapter: false,
    workers: Math.max(1, workers),
  };
}

/** How much slower than the reference desktop this device runs a 'device' stage (× its base). */
export function deviceFactor(d: DeviceRead, t: EstimateTunables = LOADING_ESTIMATE): number {
  const v = t.device;
  const cores = d.cores ?? v.referenceCores;
  let f = Math.min(v.maxFactor, Math.max(v.minFactor, v.referenceCores / cores));
  if (d.phone) f *= v.phoneFactor;
  if (d.memoryGB !== undefined && d.memoryGB < v.lowMemoryGB) f *= v.lowMemoryFactor;
  if (d.cpuAdapter) f *= v.cpuFactor;
  return f;
}

/** A count stage's items: the terrain's near ring, or the stage's guess. */
function itemsGuess(id: LoadingStageId, c: LoadingStageCost, t: EstimateTunables): number {
  return id === 'terrain' ? t.ringChunks : (c.totalGuess ?? 0);
}

/** A whole stage's opening guess (s): base + its items (bytes at the guessed rate). */
export function stageGuessS(
  id: LoadingStageId,
  c: LoadingStageCost,
  scale: number,
  workers: number,
  items: number | null = null,
  t: EstimateTunables = LOADING_ESTIMATE,
): number {
  const f = c.scales === 'device' ? scale : 1;
  const n = items ?? itemsGuess(id, c, t);
  if (c.bytesPerS !== undefined) return c.baseS + n / c.bytesPerS;
  const per = ((c.perItemS ?? 0) * f) / (c.perWorker ? workers : 1);
  return c.baseS * f + n * per;
}

/**
 * The device's speed as measured so far: each ended 'device' stage's time against its guess, as one
 * ratio (1 with nothing ended), within [0.25, 8]. It rescales the guesses still to come.
 */
export function calibration(
  ended: readonly StageRun[],
  device: DeviceRead,
  t: EstimateTunables = LOADING_ESTIMATE,
): number {
  const scale = deviceFactor(device, t);
  let guessed = 0;
  let took = 0;
  for (const r of ended) {
    const c = t.stages[r.id];
    if (!c || c.scales !== 'device' || r.endedMs === null) continue;
    guessed += stageGuessS(r.id, c, scale, device.workers, r.total, t);
    took += (r.endedMs - r.beganMs) / 1000;
  }
  if (guessed < 0.05) return 1;
  return Math.min(8, Math.max(0.25, took / guessed));
}

/** A running stage's time left (s) at `nowMs`, from its progress where it has some. */
export function runningLeftS(
  r: StageRun,
  c: LoadingStageCost,
  scale: number,
  workers: number,
  nowMs: number,
  t: EstimateTunables = LOADING_ESTIMATE,
): number {
  const elapsed = Math.max(0, (nowMs - r.beganMs) / 1000);
  const done = r.done ?? 0;
  const whole = stageGuessS(r.id, c, scale, workers, r.total, t);
  if (r.progress === 'bytes' && c.bytesPerS !== undefined) {
    const total = r.total ?? c.totalGuess ?? 0;
    const rate = done > 0 && elapsed > 0.2 ? done / elapsed : c.bytesPerS;
    return Math.max(0, total - done) / rate;
  }
  if (r.progress === 'count' && done > 0) {
    const total = r.total ?? itemsGuess(r.id, c, t);
    const f = c.scales === 'device' ? scale : 1;
    const guessPer = ((c.perItemS ?? 0) * f) / (c.perWorker ? workers : 1);
    const measuredPer = elapsed / done;
    const w = Math.min(1, done / t.minSamples);
    return Math.max(0, total - done) * (w * measuredPer + (1 - w) * guessPer);
  }
  // Nothing counted yet: the guess, less what has run; past it, a fraction of its time again.
  return Math.max(whole - elapsed, t.overrunFraction * elapsed);
}

/** What the estimate reads: this wait's runs, those ended before (any wait), the device, the route. */
export interface EstimateInput {
  runs: readonly StageRun[];
  history: readonly StageRun[];
  device: DeviceRead;
  /** Joining a server (its 'remote' stages run, not the local world's). */
  remote: boolean;
  nowMs: number;
}

/** The estimate at a moment: time since the wait began and the time left (s). */
export interface Estimate {
  elapsedS: number;
  leftS: number;
}

/**
 * The time left: the running stages' at their measured rates, plus the guesses for the stages after
 * the latest begun (in the list's order) that this route runs; stages before it that never began
 * are taken as not run. With nothing begun, every stage of the route is to come.
 */
export function estimate(
  input: EstimateInput,
  t: EstimateTunables = LOADING_ESTIMATE,
  order: readonly LoadingStageDef[] = LOADING_STAGES,
): Estimate {
  const { runs, device, nowMs } = input;
  const scale = deviceFactor(device, t) * calibration(input.history, device, t);
  const index = new Map(order.map((d, i) => [d.id, i]));
  let latest = -1;
  let startMs = nowMs;
  for (const r of runs) {
    latest = Math.max(latest, index.get(r.id) ?? -1);
    startMs = Math.min(startMs, r.beganMs);
  }
  let left = 0;
  for (const r of runs) {
    const c = t.stages[r.id];
    if (c && r.endedMs === null) left += runningLeftS(r, c, scale, device.workers, nowMs, t);
  }
  const begun = new Set(runs.map((r) => r.id));
  order.forEach((d, i) => {
    const c = t.stages[d.id];
    if (!c || i <= latest || begun.has(d.id)) return;
    if ((c.local && input.remote) || (c.remote && !input.remote)) return;
    left += stageGuessS(d.id, c, scale, device.workers, null, t);
  });
  return { elapsedS: Math.max(0, (nowMs - startMs) / 1000), leftS: left };
}

/**
 * The bar's fill: elapsed / (elapsed + left), eased over `easeS`, never backward, and held at
 * `holdFraction` until the screen opens (`reset` for the next wait).
 */
export class BarFill {
  shown = 0;

  constructor(private readonly t: EstimateTunables = LOADING_ESTIMATE) {}

  /** The fill after `dtS` seconds more of the estimate `e`. */
  update(e: Estimate, dtS: number): number {
    const total = e.elapsedS + e.leftS;
    const target = Math.min(this.t.holdFraction, total > 0 ? e.elapsedS / total : 0);
    const k = this.t.easeS > 0 ? 1 - Math.exp(-Math.max(0, dtS) / this.t.easeS) : 1;
    const next = this.shown + (target - this.shown) * k;
    this.shown = Math.min(this.t.holdFraction, Math.max(this.shown, next));
    return this.shown;
  }

  /** At the hold point (within 1 %): the game is not ready yet, the shimmer shows it working. */
  get holding(): boolean {
    return this.shown >= this.t.holdFraction - 0.01;
  }

  reset(): void {
    this.shown = 0;
  }
}

/** Seconds as the label rounds them: whole ones, in steps of 5 above `roundAboveS`. */
export function roundSeconds(s: number, t: EstimateTunables = LOADING_ESTIMATE): number {
  if (s >= t.roundAboveS) return Math.max(t.roundAboveS, Math.round(s / 5) * 5);
  return Math.max(1, Math.round(s));
}

/**
 * The "About N s" label, damped so it does not flicker: looked at most every `labelEveryS`, it
 * changes only when the estimate moved by `labelHysteresisS`, and hides under `hideBelowS`.
 */
export class SecondsLabel {
  private value: number | null = null;
  private atMs = -Infinity;

  constructor(private readonly t: EstimateTunables = LOADING_ESTIMATE) {}

  update(leftS: number, nowMs: number): string {
    if (nowMs - this.atMs >= this.t.labelEveryS * 1000) {
      this.atMs = nowMs;
      if (leftS < this.t.hideBelowS) this.value = null;
      else if (this.value === null || Math.abs(leftS - this.value) >= this.t.labelHysteresisS) {
        this.value = leftS;
      }
    }
    return this.value === null ? '' : `About ${String(roundSeconds(this.value, this.t))} s`;
  }

  reset(): void {
    this.value = null;
    this.atMs = -Infinity;
  }
}
