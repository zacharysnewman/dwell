// Anti-aliasing and anisotropic filtering (ARCHITECTURE.md §6.10): Settings → Graphics's
// Anti-aliasing (the scene's multisample count and an FXAA post pass) and Anisotropic filtering (the
// block atlas sampler's maxAnisotropy). The choices are data (shared/render/antialias.json); the
// renderer is the Rust client's (render/mod.rs `samples_for`, `anisotropy_for`; render/fxaa.rs).
// The sample count is fixed when the renderer is made (`Client.create`'s `samples`), so it applies
// on the next load; FXAA and the anisotropy apply live. Kept per browser under `dwell.antialias`;
// the record is append-only (unknown fields kept on rewrite), like every cross-version store.
import data from '../../../shared/render/antialias.json';

export interface AntialiasSettings {
  /** The mode's id: one of the data's modes. */
  mode: string;
  /** The atlas's maxAnisotropy: one of the data's values. */
  anisotropy: number;
}

export interface AntialiasMode {
  id: string;
  label: string;
  samples: number;
  fxaa: boolean;
}
export interface AnisotropyChoice {
  value: number;
  label: string;
}
export interface AntialiasData {
  modes: AntialiasMode[];
  anisotropy: AnisotropyChoice[];
  default: AntialiasSettings;
  hint: string;
  anisotropyHint: string;
}

/** The sample counts WebGPU allows a render target (1 or 4). */
export const WEBGPU_SAMPLES: readonly number[] = [1, 4];
/** WebGPU's largest maxAnisotropy. */
export const MAX_ANISOTROPY = 16;

/** Checks the data: modes with a WebGPU sample count, anisotropy in [1, 16], a listed default. */
export function validateAntialias(raw: unknown): AntialiasData {
  const d = raw as Partial<AntialiasData> | null;
  const fail = (m: string): never => {
    throw new Error(`antialias.json: ${m}`);
  };
  if (!d || !Array.isArray(d.modes) || d.modes.length === 0) return fail('modes must be a list');
  const ids = new Set<string>();
  for (const [i, m] of d.modes.entries()) {
    if (typeof m.id !== 'string' || ids.has(m.id))
      fail(`modes[${String(i)}].id is missing or repeated`);
    ids.add(m.id);
    if (!WEBGPU_SAMPLES.includes(m.samples))
      fail(`modes[${String(i)}].samples must be 1 or 4 (WebGPU), got ${JSON.stringify(m.samples)}`);
    if (typeof m.fxaa !== 'boolean') fail(`modes[${String(i)}].fxaa must be true or false`);
    if (typeof m.label !== 'string') fail(`modes[${String(i)}].label must be text`);
  }
  if (!Array.isArray(d.anisotropy) || d.anisotropy.length === 0)
    return fail('anisotropy must be a list');
  for (const [i, a] of d.anisotropy.entries()) {
    if (!Number.isInteger(a.value) || a.value < 1 || a.value > MAX_ANISOTROPY)
      fail(`anisotropy[${String(i)}].value must be a whole number in [1, 16]`);
  }
  const def = d.default;
  if (!def || !d.modes.some((m) => m.id === def.mode))
    fail('default.mode must be one of the modes');
  if (!def || !d.anisotropy.some((a) => a.value === def.anisotropy))
    fail('default.anisotropy must be one of the anisotropy values');
  if (typeof d.hint !== 'string' || typeof d.anisotropyHint !== 'string')
    fail('hint and anisotropyHint must be text');
  return d as AntialiasData;
}

export const ANTIALIAS: AntialiasData = validateAntialias(data);

export const ANTIALIAS_KEY = 'dwell.antialias';

export function defaultAntialias(d: AntialiasData = ANTIALIAS): AntialiasSettings {
  return { ...d.default };
}

/** A stored record; a mode or anisotropy the data doesn't list (or none stored) takes the fallback. */
export function sanitizeAntialias(
  value: unknown,
  d: AntialiasData = ANTIALIAS,
  fallback: AntialiasSettings = d.default,
): AntialiasSettings {
  const v = typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : {};
  const mode = d.modes.find((m) => m.id === v.mode)?.id ?? fallback.mode;
  const anisotropy =
    d.anisotropy.find((a) => a.value === v.anisotropy)?.value ?? fallback.anisotropy;
  return { mode, anisotropy };
}

/** The record to store: the stored one's other fields kept (newer builds' fields survive). */
export function antialiasRecord(stored: unknown, s: AntialiasSettings): Record<string, unknown> {
  const base =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return { ...base, mode: s.mode, anisotropy: s.anisotropy };
}

/** The renderer's settings for a mode id: its samples and FXAA (an unknown id: the default's). */
export function antialiasMode(id: string, d: AntialiasData = ANTIALIAS): AntialiasMode {
  return (d.modes.find((m) => m.id === id) ??
    d.modes.find((m) => m.id === d.default.mode)) as AntialiasMode;
}

/**
 * `?aa=<mode id>` and `?aniso=<value>` override the stored setting for a visit (the bench's
 * comparisons); a value the data doesn't list is ignored.
 */
export function antialiasFromQuery(
  search: string,
  s: AntialiasSettings,
  d: AntialiasData = ANTIALIAS,
): AntialiasSettings {
  const q = new URLSearchParams(search);
  const mode = d.modes.find((m) => m.id === q.get('aa'))?.id ?? s.mode;
  const aniso = Number(q.get('aniso'));
  const anisotropy = d.anisotropy.find((a) => a.value === aniso)?.value ?? s.anisotropy;
  return { mode, anisotropy };
}
