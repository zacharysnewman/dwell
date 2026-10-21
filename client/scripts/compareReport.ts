// The math of `bench --compare`'s report (scripts/compare.ts, docs/DEV_TOOLS.md): per-config
// metrics from a `record` step (frame intervals and dropped frames, CPU phases, GPU passes,
// memory, uploads, streaming), image agreement between two screenshots (CIE76 ΔE per pixel, a
// difference heatmap, a side-by-side image) and the printed table. Pure: no Node or browser imports.

import { memoryTotal, type MemoryReport } from '../src/ui/memory.ts';

/** The value at quantile `q` of `values`, as the telemetry computes it (telemetry.rs `Stats`). */
export function percentile(values: readonly number[], q: number): number {
  if (values.length === 0) return 0;
  const v = [...values].sort((a, b) => a - b);
  return v[Math.round((v.length - 1) * q)];
}

/** Frames missed at `hz`: an interval of n refresh periods dropped n − 1 of them. */
export function droppedFrames(intervals: readonly number[], hz = 60): number {
  const period = 1000 / hz;
  return intervals.reduce((n, iv) => n + Math.max(0, Math.round(iv / period) - 1), 0);
}

interface Stats {
  n: number;
  mean: number;
  p50: number;
  p95: number;
  p99: number;
  max: number;
  sum: number;
}

/** What bench.ts saves for a `record` step (the parts the report reads). */
export interface RecordData {
  telemetry: {
    frames: Record<string, Stats | undefined>;
    samples: Record<string, Stats | undefined>;
    frameCount: number;
  };
  frames: { names: string[]; frames: number[][] };
  memory: MemoryReport | null;
  state: { lod?: Record<string, unknown> | null; terrain?: Record<string, number> } | null;
  seconds: number;
}

export interface Summary {
  mean: number;
  p95: number;
  p99: number;
}

export interface Metrics {
  frames: number;
  seconds: number;
  /** Time between frames (ms), from the kept frames' timestamps. */
  interval: { p50: number; p95: number; p99: number; max: number; mean: number };
  /** Refresh periods missed at 60 Hz. */
  dropped60: number;
  /** Per-frame CPU series (ms): every telemetry series but `interval`, counts and `gpu.*`. */
  cpu: Record<string, Summary>;
  /** GPU passes (`gpu.<pass>`, ms), per-frame series or samples; null when the build has none. */
  gpu: Record<string, Summary> | null;
  memory: { totalBytes: number; report: MemoryReport } | null;
  upload: { bytes: number; bytesPerFrame: number; chunks: number; sections: number };
  /** At the record: whether the view had nothing left to load, and what was outstanding. */
  streaming: { complete: boolean; outstanding: Record<string, number> };
}

/** Per-frame series that are counts, not times. */
export const COUNTS = [
  'jobs.done',
  'upload.chunks',
  'upload.sections',
  'upload.bytes',
  'ticks.count',
];

const summary = (s: Stats): Summary => ({ mean: s.mean, p95: s.p95, p99: s.p99 });

export function metricsOf(r: RecordData): Metrics {
  const at = r.frames.frames.map((f) => f[0]);
  const intervals = at.slice(1).map((t, i) => t - at[i]);
  const mean = intervals.length ? intervals.reduce((a, b) => a + b, 0) / intervals.length : 0;
  const cpu: Record<string, Summary> = {};
  const gpu: Record<string, Summary> = {};
  for (const [k, s] of Object.entries(r.telemetry.frames)) {
    if (!s || k === 'interval' || COUNTS.includes(k)) continue;
    (k.startsWith('gpu.') ? gpu : cpu)[k] = summary(s);
  }
  for (const [k, s] of Object.entries(r.telemetry.samples)) {
    if (s && k.startsWith('gpu.')) gpu[k] = summary(s);
  }
  const sum = (k: string): number => r.telemetry.frames[k]?.sum ?? 0;
  const outstanding: Record<string, number> = {};
  const lod = r.state?.lod;
  for (const k of ['generating', 'meshing', 'asking']) {
    const v = lod?.[k];
    if (typeof v === 'number' && v > 0) outstanding[`lod.${k}`] = v;
  }
  for (const k of ['generating', 'meshing', 'dirty']) {
    const v = r.state?.terrain?.[k];
    if (typeof v === 'number' && v > 0) outstanding[`terrain.${k}`] = v;
  }
  return {
    frames: r.telemetry.frameCount,
    seconds: r.seconds,
    interval: {
      p50: percentile(intervals, 0.5),
      p95: percentile(intervals, 0.95),
      p99: percentile(intervals, 0.99),
      max: intervals.length ? Math.max(...intervals) : 0,
      mean,
    },
    dropped60: droppedFrames(intervals),
    cpu,
    gpu: Object.keys(gpu).length ? gpu : null,
    memory: r.memory ? { totalBytes: memoryTotal(r.memory), report: r.memory } : null,
    upload: {
      bytes: sum('upload.bytes'),
      bytesPerFrame: r.telemetry.frameCount ? sum('upload.bytes') / r.telemetry.frameCount : 0,
      chunks: sum('upload.chunks'),
      sections: sum('upload.sections'),
    },
    streaming: { complete: Object.keys(outstanding).length === 0, outstanding },
  };
}

// --- Images -------------------------------------------------------------------------------------

/** An RGBA image, 8 bits per channel, rows top to bottom. */
export interface Rgba {
  width: number;
  height: number;
  data: Uint8Array;
}

/** A burst's flicker (`flickerOf`). */
export interface Flicker {
  frames: number;
  /** The share of pixels (0–1) whose brightness changes by more than `threshold`, per frame. */
  changed: number;
  /** The mean brightness change per pixel and frame (0–255). */
  meanDelta: number;
  threshold: number;
}

/**
 * The temporal noise of a burst of frames from a still camera: for each frame after the first, the
 * share of pixels whose brightness (Rec. 709 luma of the stored sRGB values, 0–255) moved more than
 * `threshold` from the frame before, and the mean of that move; averaged over the pairs. A still
 * scene scores 0; shadow edges crawling from frame to frame score the share of the view they cover.
 */
export function flickerOf(frames: readonly Rgba[], threshold = 3): Flicker {
  if (frames.length < 2) throw new Error('flicker: needs two frames or more');
  const { width, height } = frames[0];
  const luma = (im: Rgba): Float32Array => {
    if (im.width !== width || im.height !== height) throw new Error('flicker: frame sizes differ');
    const out = new Float32Array(width * height);
    for (let i = 0; i < out.length; i++) {
      const d = im.data;
      out[i] = 0.2126 * d[i * 4] + 0.7152 * d[i * 4 + 1] + 0.0722 * d[i * 4 + 2];
    }
    return out;
  };
  let prev = luma(frames[0]);
  let changed = 0;
  let delta = 0;
  for (const im of frames.slice(1)) {
    const cur = luma(im);
    for (let i = 0; i < cur.length; i++) {
      const m = Math.abs(cur[i] - prev[i]);
      delta += m;
      if (m > threshold) changed++;
    }
    prev = cur;
  }
  const n = (frames.length - 1) * width * height;
  return { frames: frames.length, changed: changed / n, meanDelta: delta / n, threshold };
}

const LINEAR = Array.from({ length: 256 }, (_, i) => {
  const c = i / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
});

/** CIE L*a*b* (D65) of an sRGB colour. */
export function lab(r: number, g: number, b: number): [number, number, number] {
  const [lr, lg, lb] = [LINEAR[r], LINEAR[g], LINEAR[b]];
  const x = (0.4124564 * lr + 0.3575761 * lg + 0.1804375 * lb) / 0.95047;
  const y = 0.2126729 * lr + 0.7151522 * lg + 0.072175 * lb;
  const z = (0.0193339 * lr + 0.119192 * lg + 0.9503041 * lb) / 1.08883;
  const f = (t: number): number => (t > 216 / 24389 ? Math.cbrt(t) : ((24389 / 27) * t + 16) / 116);
  const [fx, fy, fz] = [f(x), f(y), f(z)];
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}

/** CIE76 colour difference between two sRGB colours (≈ 2.3: just noticeable). */
export function deltaE(a: readonly number[], b: readonly number[]): number {
  const [l1, a1, b1] = lab(a[0], a[1], a[2]);
  const [l2, a2, b2] = lab(b[0], b[1], b[2]);
  return Math.hypot(l1 - l2, a1 - a2, b1 - b2);
}

export interface DiffStats {
  pixels: number;
  /** Pixels exactly equal (RGB). */
  identicalPct: number;
  /** Pixels whose ΔE is over the threshold. */
  overPct: number;
  threshold: number;
  meanDeltaE: number;
  p99DeltaE: number;
  maxDeltaE: number;
}

/**
 * Per-pixel ΔE between `a` and `b` (same size) and a heatmap: matching pixels as a dim grey of
 * `a`, differences under the threshold in blue, over it from red (just over) to yellow and white
 * (four times the threshold and more).
 */
export function diffImages(
  a: Rgba,
  b: Rgba,
  threshold: number,
): { stats: DiffStats; heatmap: Rgba } {
  if (a.width !== b.width || a.height !== b.height) {
    throw new Error(
      `images differ in size: ${String(a.width)}×${String(a.height)} and ${String(b.width)}×${String(b.height)}`,
    );
  }
  const n = a.width * a.height;
  const out = new Uint8Array(n * 4);
  const de = new Float64Array(n);
  let identical = 0;
  let over = 0;
  let sum = 0;
  let max = 0;
  for (let i = 0; i < n; i++) {
    const o = i * 4;
    const pa = [a.data[o], a.data[o + 1], a.data[o + 2]];
    const pb = [b.data[o], b.data[o + 1], b.data[o + 2]];
    const same = pa[0] === pb[0] && pa[1] === pb[1] && pa[2] === pb[2];
    const d = same ? 0 : deltaE(pa, pb);
    de[i] = d;
    sum += d;
    if (d > max) max = d;
    if (same) identical++;
    let rgb: [number, number, number];
    if (d === 0) {
      const g = Math.round((pa[0] + pa[1] + pa[2]) / 12);
      rgb = [g, g, g];
    } else if (d <= threshold) {
      rgb = [0, 40, Math.round(80 + (175 * d) / threshold)];
    } else {
      over++;
      const t = Math.min(1, (d - threshold) / (3 * threshold));
      rgb = t < 0.5 ? [255, Math.round(510 * t), 0] : [255, 255, Math.round(510 * (t - 0.5))];
    }
    out.set([...rgb, 255], o);
  }
  const pct = (k: number): number => (n ? (100 * k) / n : 0);
  return {
    stats: {
      pixels: n,
      identicalPct: pct(identical),
      overPct: pct(over),
      threshold,
      meanDeltaE: n ? sum / n : 0,
      p99DeltaE: percentile(Array.from(de), 0.99),
      maxDeltaE: max,
    },
    heatmap: { width: a.width, height: a.height, data: out },
  };
}

/** `images` left to right, `gap` pixels of mid grey between them, top-aligned. */
export function sideBySide(images: readonly Rgba[], gap = 4): Rgba {
  const width = images.reduce((w, im) => w + im.width, 0) + gap * Math.max(0, images.length - 1);
  const height = Math.max(...images.map((im) => im.height));
  const data = new Uint8Array(width * height * 4).fill(128);
  let x0 = 0;
  for (const im of images) {
    for (let y = 0; y < im.height; y++) {
      data.set(im.data.subarray(y * im.width * 4, (y + 1) * im.width * 4), (y * width + x0) * 4);
    }
    x0 += im.width + gap;
  }
  return { width, height, data };
}

// --- The table -----------------------------------------------------------------------------------

/** A plain-text table: the first column left-aligned, the rest right-aligned. */
export function formatTable(
  header: readonly string[],
  rows: readonly (readonly string[])[],
): string {
  const widths = header.map((h, i) => Math.max(h.length, ...rows.map((r) => (r[i] ?? '').length)));
  const line = (r: readonly string[]): string =>
    r
      .map((c, i) => (i === 0 ? c.padEnd(widths[i]) : c.padStart(widths[i])))
      .join('  ')
      .trimEnd();
  return [line(header), ...rows.map(line)].join('\n');
}

const ms = (v: number): string => v.toFixed(2);
const mib = (v: number): string => (v / 2 ** 20).toFixed(1);

/** The table rows for one record across configurations (`null`: that config has no such record). */
export function metricRows(per: readonly (Metrics | null)[]): string[][] {
  const row = (label: string, get: (m: Metrics) => string | undefined): string[] => [
    label,
    ...per.map((m) => (m ? (get(m) ?? '—') : '—')),
  ];
  const keys = (pick: (m: Metrics) => Record<string, Summary> | null): string[] =>
    [...new Set(per.flatMap((m) => (m ? Object.keys(pick(m) ?? {}) : [])))].sort();
  const cpuKeys = keys((m) => m.cpu).filter((k) =>
    ['frame.cpu', 'draw.render', 'lod', 'game.draw', 'jobs.apply'].includes(k),
  );
  return [
    row('interval p50 ms', (m) => ms(m.interval.p50)),
    row('interval p95 ms', (m) => ms(m.interval.p95)),
    row('interval p99 ms', (m) => ms(m.interval.p99)),
    row('dropped @60 Hz', (m) => String(m.dropped60)),
    ...cpuKeys.flatMap((k) => [
      row(`${k} p95 ms`, (m) => (k in m.cpu ? ms(m.cpu[k].p95) : undefined)),
    ]),
    ...keys((m) => m.gpu).map((k) =>
      row(`${k} p95 ms`, (m) => (m.gpu?.[k] ? ms(m.gpu[k].p95) : undefined)),
    ),
    row('memory MiB', (m) => (m.memory ? mib(m.memory.totalBytes) : undefined)),
    row('upload MiB', (m) => mib(m.upload.bytes)),
    row('streamed', (m) => (m.streaming.complete ? 'complete' : 'loading')),
  ];
}

// --- A comparison's results ---------------------------------------------------------------------

/** One shot of one configuration against the baseline (paths relative to the output directory). */
export interface ShotDiff {
  shot: string;
  config: string;
  stats: DiffStats | null;
  error?: string;
  side?: string;
  diff?: string;
}

export interface ScenarioResult {
  name: string;
  notes: string;
  split: 'near' | 'far' | null;
  /** The scenario's `shot` names, in order. */
  shotNames: string[];
  settles: Record<string, number[]>;
  errors: Record<string, string[]>;
  records: Record<string, Record<string, Metrics>>;
  shots: ShotDiff[];
}

const pct = (v: number): string => `${v.toFixed(2)} %`;

/** A scenario's comparison table: the configurations (`ran`, baseline first) as columns. */
export function scenarioTable(
  r: ScenarioResult,
  ran: readonly { name: string }[],
  deltaE: number,
): { header: string[]; rows: string[][] } {
  const header = [`${r.name}${r.split ? ` (${r.split})` : ''}`, ...ran.map((c) => c.name)];
  const rows: string[][] = [
    [
      'settle s',
      ...ran.map((c) => (r.settles[c.name] ?? []).map((s) => s.toFixed(1)).join(', ') || '—'),
    ],
  ];
  for (const [name, per] of Object.entries(r.records)) {
    rows.push([`[${name}]`, ...ran.map(() => '')]);
    rows.push(...metricRows(ran.map((c) => (c.name in per ? per[c.name] : null))));
  }
  for (const shot of r.shotNames) {
    const cell = (i: number, f: (s: DiffStats) => string): string => {
      if (i === 0) return 'baseline';
      const d = r.shots.find((s) => s.shot === shot && s.config === ran[i].name);
      return d?.stats ? f(d.stats) : (d?.error ?? '—');
    };
    rows.push([
      `shot ${shot}: ΔE>${String(deltaE)}`,
      ...ran.map((_, i) => cell(i, (s) => pct(s.overPct))),
    ]);
    rows.push([
      '  mean / max ΔE',
      ...ran.map((_, i) =>
        cell(i, (s) => `${s.meanDeltaE.toFixed(2)} / ${s.maxDeltaE.toFixed(1)}`),
      ),
    ]);
  }
  return { header, rows };
}
