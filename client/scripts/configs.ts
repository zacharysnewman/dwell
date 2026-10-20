// Configurations for `bench --compare` (docs/DEV_TOOLS.md, client/bench/configs.json): each
// scenario runs once per configuration, in a fresh page on the same seed and inputs, and the
// report puts them side by side. A configuration is a resolution scale (`?scale=`, the browser
// stretching the canvas) and page parameters (dev switches such as `?outline=`). Pure: no Node or
// browser imports.

import { ViewError } from './views.ts';

export interface Config {
  /** Lowercase words joined by dashes; unique. */
  name: string;
  notes: string;
  /** `?scale=`, 0.25–2. */
  scale: number;
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
  const o = obj(where, raw, ['name', 'notes', 'scale', 'params']);
  const n = name(`${where}.name`, o.name);
  if (o.notes !== undefined && typeof o.notes !== 'string') {
    throw new ViewError(`${where}.notes: expected text`);
  }
  const scale = num(`${where}.scale`, o.scale, 0.25, 2, 1);
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
    scale,
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

/** The page parameters for `c` (scenarios.ts `scenarioUrl`). */
export function configParams(c: Config): Record<string, string> {
  return { ...c.params, scale: String(c.scale) };
}
