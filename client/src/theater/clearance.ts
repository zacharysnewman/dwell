// The camera clearance check (docs/THEATER_MODE.md §6.2; owner, 2026-11-14: "That flyover video goes
// straight through a wall. Not a big deal since you're just showing something but that shouldn't be
// included in most recordings."): before a take the camera path is sampled finely (every
// `clearance.stepS` of take time) and each eye is probed against the world's solid blocks within
// `clearance.radiusM` (`dwell-client` `camera_clearance`). A path that passes through solid ground fails
// the take with the times and positions and the lift that would clear it; a take with `allowClipping`
// keeps going (a deliberate shot) and still reports them.
//
// On a local world the eyes are probed against the world itself (its server's chunks, generated as
// needed), all at once before the take. On a remote one only the client's loaded chunks are known: the
// eyes are probed as the world near them loads, during a cinematic take's pre-roll (each step's samples
// once it has settled) or, for a real-time take, a quick pass that flies the path in coarse steps
// (`runClearancePass`). An eye whose chunks never loaded is counted as unchecked, never as clear or solid.
//
// Pure but for the world it is given (`ClearanceWorld`): the tests drive it with a fake world.

import type { CinematicHost } from './cinematic';

export type Vec3 = [number, number, number];

export interface ClearanceConfig {
  /** How close (m) the eye may come to a solid block. */
  radiusM: number;
  /** The sampling step (s of take time). */
  stepS: number;
  /** A real-time take's pass: its step along the path (s) and the most it waits a step for the chunks (ms). */
  passStepS: number;
  capMs: number;
  /** The lift suggested out of a clip: probed in steps of `liftStepM`, at most `liftMaxM`. */
  liftStepM: number;
  liftMaxM: number;
}

/**
 * What the check needs from the page: the WASM client's `theaterSamples`, and the local world's
 * `cameraClearance` (or the client's `cameraClearance` and `cameraLift` over its loaded chunks).
 */
export interface ClearanceWorld {
  /** The path's eye every `stepS` s of take time: `[takeS, pathS, x, y, z, …]`, at most `max` samples. */
  samples(json: string, stepS: number, max: number): ArrayLike<number>;
  /**
   * Per eye (`[x, y, z, …]`) its code (1 within `radiusM` of a solid, 0 clear, -1 not loaded) and, for
   * a solid one, the rise (m, along its face's up, in `liftStepM` steps) that clears it (NaN: none
   * within `liftMaxM`).
   */
  probe(
    eyes: Float64Array,
    radiusM: number,
    liftStepM: number,
    liftMaxM: number,
  ): Promise<{ codes: ArrayLike<number>; lifts: ArrayLike<number> }>;
}

/** One stretch of the path inside the radius of solid blocks. */
export interface ClipRun {
  /** Take seconds (the video's time). */
  fromS: number;
  toS: number;
  /** Path seconds (from the first keyframe). */
  pathFromS: number;
  pathToS: number;
  /** The eye where the run starts. */
  at: Vec3;
  /** The keyframes the run lies between (indices: the first segment's start, the last segment's end). */
  keys: [number, number];
  /** The most lift any of its eyes needs (m), or null when one can't be cleared within `liftMaxM`. */
  liftM: number | null;
}

export interface ClearanceResult {
  samples: number;
  /** Eyes whose chunks never loaded. */
  unchecked: number;
  radiusM: number;
  clipping: ClipRun[];
  /** The failure's text (empty when clear). */
  message: string;
}

/** The path's keyframe times (path seconds from the first), from its JSON. */
export function keyTimes(path: unknown): number[] {
  const p = typeof path === 'string' ? (JSON.parse(path) as unknown) : path;
  const keys = (p as { keyframes?: { t?: unknown }[] } | null)?.keyframes;
  if (!Array.isArray(keys)) return [];
  const ts = keys.map((k) => (typeof k.t === 'number' ? k.t : 0));
  const t0 = ts[0] ?? 0;
  return ts.map((t) => t - t0);
}

/** The segment holding path time `t` (its first keyframe's index). */
function segmentAt(keys: readonly number[], t: number): number {
  let i = 0;
  while (i + 1 < keys.length - 1 && (keys[i + 1] ?? Infinity) <= t) i++;
  return i;
}

const MAX_SAMPLES = 200_000;
const fmt = (n: number, d = 2): string => n.toFixed(d);

export class ClearanceCheck {
  private readonly take: Float64Array;
  private readonly path: Float64Array;
  private readonly eyes: Float64Array;
  /** Per sample: 1 solid, 0 clear, -1 not loaded (or not probed yet). */
  private readonly status: Int8Array;
  private readonly lifts: Float64Array;
  private readonly keys: number[];

  constructor(
    private readonly world: ClearanceWorld,
    json: string,
    private readonly cfg: ClearanceConfig,
  ) {
    const raw = world.samples(json, cfg.stepS, MAX_SAMPLES);
    const n = Math.floor(raw.length / 5);
    this.take = new Float64Array(n);
    this.path = new Float64Array(n);
    this.eyes = new Float64Array(n * 3);
    for (let i = 0; i < n; i++) {
      this.take[i] = raw[i * 5] ?? 0;
      this.path[i] = raw[i * 5 + 1] ?? 0;
      for (let k = 0; k < 3; k++) this.eyes[i * 3 + k] = raw[i * 5 + 2 + k] ?? 0;
    }
    this.status = new Int8Array(n).fill(-1);
    this.lifts = new Float64Array(n).fill(NaN);
    this.keys = keyTimes(json);
  }

  get count(): number {
    return this.take.length;
  }

  /** The last sample's take time (s). */
  get lastS(): number {
    return this.take[this.count - 1] ?? 0;
  }

  /** The eye of sample `i`. */
  eye(i: number): Vec3 {
    return [this.eyes[i * 3] ?? 0, this.eyes[i * 3 + 1] ?? 0, this.eyes[i * 3 + 2] ?? 0];
  }

  /**
   * Probes the samples up to take time `tS` (all with Infinity) not yet found clear or solid; a solid
   * one's lift is measured with it, while its chunks are loaded. Resolves with how many of them are
   * still unloaded.
   */
  async probeUpTo(tS: number): Promise<number> {
    const idx: number[] = [];
    for (let i = 0; i < this.count && (this.take[i] ?? 0) <= tS + 1e-9; i++)
      if (this.status[i] === -1) idx.push(i);
    if (!idx.length) return 0;
    const eyes = new Float64Array(idx.length * 3);
    idx.forEach((s, j) => {
      eyes.set(this.eye(s), j * 3);
    });
    const { codes, lifts } = await this.world.probe(
      eyes,
      this.cfg.radiusM,
      this.cfg.liftStepM,
      this.cfg.liftMaxM,
    );
    let unknown = 0;
    idx.forEach((s, j) => {
      const c = codes[j] ?? -1;
      this.status[s] = c;
      if (c === -1) unknown++;
      if (c === 1) this.lifts[s] = lifts[j] ?? NaN;
    });
    return unknown;
  }

  /** The runs of solid samples, the unchecked count and the message. */
  result(): ClearanceResult {
    const runs: ClipRun[] = [];
    let unchecked = 0;
    let run: { a: number; b: number } | null = null;
    const close = (): void => {
      if (!run) return;
      const { a, b } = run;
      let lift: number | null = 0;
      for (let i = a; i <= b; i++) {
        const l = this.lifts[i] ?? NaN;
        lift = Number.isNaN(l) || lift === null ? null : Math.max(lift, l);
      }
      const pa = this.path[a] ?? 0;
      const pb = this.path[b] ?? 0;
      runs.push({
        fromS: this.take[a] ?? 0,
        toS: this.take[b] ?? 0,
        pathFromS: pa,
        pathToS: pb,
        at: this.eye(a),
        keys: [
          segmentAt(this.keys, pa),
          Math.min(segmentAt(this.keys, pb) + 1, this.keys.length - 1),
        ],
        liftM: lift,
      });
      run = null;
    };
    for (let i = 0; i < this.count; i++) {
      const s = this.status[i];
      if (s === -1) unchecked++;
      if (s === 1) {
        if (run) run.b = i;
        else run = { a: i, b: i };
      } else close();
    }
    close();
    return {
      samples: this.count,
      unchecked,
      radiusM: this.cfg.radiusM,
      clipping: runs,
      message: clearanceMessage(runs, unchecked, this.count, this.cfg),
    };
  }
}

/** The failure's text: each run's times, eye and the lift that clears it. */
export function clearanceMessage(
  runs: readonly ClipRun[],
  unchecked: number,
  samples: number,
  cfg: Pick<ClearanceConfig, 'radiusM' | 'liftMaxM'>,
): string {
  if (!runs.length) return '';
  const lines = runs.map((r) => {
    const at = r.at.map((n) => fmt(n, 1)).join(', ');
    const lift =
      r.liftM === null
        ? `no lift up to ${fmt(cfg.liftMaxM, 0)} m clears it: move the keys`
        : `lift the camera ≥ ${fmt(r.liftM, 1)} m there`;
    return (
      `  ${fmt(r.fromS)}–${fmt(r.toS)} s (path ${fmt(r.pathFromS)}–${fmt(r.pathToS)} s, keyframes ` +
      `${String(r.keys[0])}–${String(r.keys[1])}) at (${at}): ${lift}`
    );
  });
  return [
    `the camera path passes through solid ground (within ${fmt(cfg.radiusM)} m) in ${String(runs.length)} place${runs.length === 1 ? '' : 's'}:`,
    ...lines,
    unchecked
      ? `  (${String(unchecked)} of ${String(samples)} samples weren't loaded and went unchecked)`
      : '',
    'Raise or move those keyframes, or set allowClipping: true on the shot when the clip is deliberate.',
  ]
    .filter(Boolean)
    .join('\n');
}

type PassHost = Pick<
  CinematicHost,
  | 'theaterExact'
  | 'theaterAdvance'
  | 'theaterPlay'
  | 'theaterStop'
  | 'theaterState'
  | 'nextFrame'
  | 'wallMs'
> & { clearance: ClearanceWorld };

/**
 * A real-time take's check: every eye probed at once, and when some aren't loaded (a remote world), a
 * quick pass: the path flown in `passStepS` steps (frame-exact, nothing captured), each step's samples
 * probed as their chunks load (at most `capMs` a step), then the path stopped.
 */
export async function runClearancePass(
  host: PassHost,
  path: unknown,
  cfg: ClearanceConfig,
): Promise<ClearanceResult> {
  const json = typeof path === 'string' ? path : JSON.stringify(path);
  const check = new ClearanceCheck(host.clearance, json, cfg);
  // A local world knows every eye at once: no pass.
  if ((await check.probeUpTo(Infinity)) === 0) return check.result();
  host.theaterExact(true);
  try {
    host.theaterPlay(json);
    const duration = host.theaterState()?.duration ?? 0;
    const settle = async (t: number): Promise<void> => {
      const t0 = host.wallMs();
      for (;;) {
        await host.nextFrame(null);
        if ((await check.probeUpTo(t)) === 0 || host.wallMs() - t0 >= cfg.capMs) return;
      }
    };
    await settle(0);
    // Steps of take time until the path ends (its speed may differ from 1), as far as it was sampled.
    const step = Math.max(cfg.passStepS, 1e-3);
    const end = Math.max(check.lastS, duration);
    for (let t = step; t < end + step; t += step) {
      host.theaterAdvance(step);
      await settle(t);
      if (host.theaterState()?.finished ?? true) break;
    }
    await check.probeUpTo(Infinity);
  } finally {
    host.theaterStop();
    host.theaterExact(false);
  }
  return check.result();
}
