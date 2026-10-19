// Renderer configurations for `bench --compare` (docs/DEV_TOOLS.md, client/bench/configs.json,
// docs/RAY_MARCHED_RENDERER.md §5): each scenario runs once per configuration, in a fresh page
// on the same seed and inputs, and the report puts them side by side. A configuration is a
// renderer, a resolution scale and an upscaler, passed to the page as `?renderer=`, `?scale=` and
// `?upscaler=`. A configuration whose renderer or upscaler the build doesn't offer is skipped with
// a note. Pure: no Node or browser imports.

import { ViewError } from './views.ts';

/** Upscalers by name: `bilinear` is the browser's stretch of a scaled canvas (today's `?scale=`);
 * `fsr1` and `bilateral` are the marcher's (its own render scale, the canvas at full size). */
export const UPSCALERS = ['none', 'bilinear', 'fsr1', 'bilateral', 'taa-u'] as const;
export type Upscaler = (typeof UPSCALERS)[number];
/** The upscalers every build has (they need nothing from the renderer). */
const BUILT_IN_UPSCALERS: readonly Upscaler[] = ['none', 'bilinear'];

export interface Config {
  /** Lowercase words joined by dashes; unique. */
  name: string;
  notes: string;
  /** `?renderer=`: a renderer the client knows by name (`raster`, `march`). */
  renderer: string;
  /** `?scale=`, 0.25–2. */
  scale: number;
  upscaler: Upscaler;
  /** More page parameters (e.g. `{ "waterdbg": "caustics" }`, a dev switch), added as given. */
  params: Record<string, string>;
}

export interface ConfigsFile {
  configs: Config[];
  /** The configuration the others are compared with (images and metrics); the first by default. */
  baseline: string;
  /** Colour difference (CIE76 ΔE) over which a pixel counts as different. */
  deltaE: number;
}

/** What a build offers, read from the page (`__dwell.renderers()`, `__dwell.upscalers()`). */
export interface Support {
  renderers: string[];
  upscalers: string[];
}

/** A build without the hooks: today's single renderer (the mesher) and the browser's stretch. */
export const DEFAULT_SUPPORT: Support = {
  renderers: ['raster'],
  upscalers: [...BUILT_IN_UPSCALERS],
};

const NAME = /^[a-z0-9]+(-[a-z0-9]+)*$/;
type Obj = Record<string, unknown>;

function obj(where: string, v: unknown, keys: readonly string[]): Obj {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) {
    throw new ViewError(`${where}: expected an object`);
  }
  for (const k of Object.keys(v)) {
    if (!keys.includes(k)) {
      throw new ViewError(`${where}: unknown field "${k}" (expected one of ${keys.join(', ')})`);
    }
  }
  return v as Obj;
}

function name(where: string, v: unknown): string {
  if (typeof v !== 'string' || !NAME.test(v)) {
    throw new ViewError(`${where}: expected lowercase words joined by dashes`);
  }
  return v;
}

function num(where: string, v: unknown, min: number, max: number, fallback: number): number {
  if (v === undefined) return fallback;
  if (typeof v !== 'number' || !Number.isFinite(v))
    throw new ViewError(`${where}: expected a number`);
  if (v < min || v > max) {
    throw new ViewError(`${where}: ${String(v)} is outside ${String(min)}–${String(max)}`);
  }
  return v;
}

function parseConfig(index: string, raw: unknown): Config {
  const named = typeof (raw as Obj | null)?.name === 'string';
  const where = named ? `configs["${String((raw as Obj).name)}"]` : index;
  const o = obj(where, raw, ['name', 'notes', 'renderer', 'scale', 'upscaler', 'params']);
  const n = name(`${where}.name`, o.name);
  if (o.notes !== undefined && typeof o.notes !== 'string') {
    throw new ViewError(`${where}.notes: expected text`);
  }
  const scale = num(`${where}.scale`, o.scale, 0.25, 2, 1);
  const upscaler = o.upscaler ?? (scale === 1 ? 'none' : 'bilinear');
  if (!UPSCALERS.includes(upscaler as Upscaler)) {
    throw new ViewError(
      `${where}.upscaler: expected one of ${UPSCALERS.join(', ')} (got ${JSON.stringify(upscaler)})`,
    );
  }
  const up = upscaler as Upscaler;
  if (scale === 1 && up !== 'none') {
    throw new ViewError(`${where}.upscaler: "${up}" needs a scale below or above 1`);
  }
  const params = o.params ?? {};
  if (
    typeof params !== 'object' ||
    Array.isArray(params) ||
    !Object.values(params).every((v) => typeof v === 'string')
  ) {
    throw new ViewError(`${where}.params: expected an object of text values`);
  }
  return {
    name: n,
    notes: o.notes ?? '',
    params: params as Record<string, string>,
    renderer: name(`${where}.renderer`, o.renderer),
    scale,
    upscaler: up,
  };
}

/** The configurations file (`{ "configs": [...], "baseline"?, "deltaE"? }`), checked. */
export function parseConfigs(json: unknown): ConfigsFile {
  const file = obj('configs file', json, ['$comment', 'configs', 'baseline', 'deltaE']);
  if (!Array.isArray(file.configs) || file.configs.length === 0) {
    throw new ViewError('configs file: expected "configs": [...] with at least one entry');
  }
  const configs = file.configs.map((c, i) => parseConfig(`configs[${String(i)}]`, c));
  const seen = new Set<string>();
  for (const c of configs) {
    if (seen.has(c.name)) throw new ViewError(`configs["${c.name}"]: the name is used twice`);
    seen.add(c.name);
  }
  const baseline =
    file.baseline === undefined ? configs[0].name : name('configs file.baseline', file.baseline);
  if (!seen.has(baseline)) {
    throw new ViewError(`configs file.baseline: no configuration "${baseline}"`);
  }
  return { configs, baseline, deltaE: num('configs file.deltaE', file.deltaE, 0.1, 100, 2.3) };
}

/** The configurations named (all when `names` is empty), the baseline first; throws on unknown. */
export function selectConfigs(file: ConfigsFile, names: readonly string[]): Config[] {
  const unknown = names.filter((n) => !file.configs.some((c) => c.name === n));
  if (unknown.length) {
    throw new ViewError(
      `no configuration ${unknown.join(', ')} (have ${file.configs.map((c) => c.name).join(', ')})`,
    );
  }
  const chosen = file.configs.filter(
    (c) => c.name === file.baseline || names.length === 0 || names.includes(c.name),
  );
  return [
    ...chosen.filter((c) => c.name === file.baseline),
    ...chosen.filter((c) => c.name !== file.baseline),
  ];
}

/** Why the build can't run `c`, or null when it can. */
export function unsupported(c: Config, s: Support): string | null {
  if (!s.renderers.includes(c.renderer)) {
    return `this build has no "${c.renderer}" renderer (it offers ${s.renderers.join(', ')})`;
  }
  if (!BUILT_IN_UPSCALERS.includes(c.upscaler) && !s.upscalers.includes(c.upscaler)) {
    return `this build has no "${c.upscaler}" upscaler (it offers ${s.upscalers.join(', ')})`;
  }
  return null;
}

/** The configurations a build can run, and the others with the reason. */
export function partition(
  configs: readonly Config[],
  s: Support,
): { run: Config[]; skipped: { name: string; reason: string }[] } {
  const run: Config[] = [];
  const skipped: { name: string; reason: string }[] = [];
  for (const c of configs) {
    const why = unsupported(c, s);
    if (why === null) run.push(c);
    else skipped.push({ name: c.name, reason: why });
  }
  return { run, skipped };
}

/** The page's answer to the support hooks (anything malformed or absent: the defaults). */
export function supportOf(raw: unknown): Support {
  const r = raw as { renderers?: unknown; upscalers?: unknown } | null;
  const list = (v: unknown, fallback: readonly string[]): string[] =>
    Array.isArray(v) && v.length > 0 && v.every((x) => typeof x === 'string') ? v : [...fallback];
  return {
    renderers: list(r?.renderers, DEFAULT_SUPPORT.renderers),
    upscalers: list(r?.upscalers, DEFAULT_SUPPORT.upscalers),
  };
}

/** The page parameters for `c` (scenarios.ts `scenarioUrl`). */
export function configParams(c: Config): Record<string, string> {
  const p: Record<string, string> = {
    ...c.params,
    renderer: c.renderer,
    scale: String(c.scale),
  };
  if (!BUILT_IN_UPSCALERS.includes(c.upscaler)) p.upscaler = c.upscaler;
  return p;
}
