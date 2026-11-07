// Render scale and its upscaler (docs/DEV_TOOLS.md, Render scale and upscaling; ARCHITECTURE.md
// §6.10): the scene drawn at a share of the screen's pixels per axis and brought up by one of a
// fixed vocabulary of upscalers. The choices are data (shared/render/upscale.json); the renderer is
// the Rust client's (render/upscale.rs, `Client.setRenderScale`). Kept per browser under its own
// key; the record is append-only (unknown fields kept on rewrite), like every cross-version store.
import data from '../../../shared/render/upscale.json';

export interface RenderScaleSettings {
  /** The share of the screen's pixels drawn, per axis: one of the data's scales. */
  scale: number;
  /** The upscaler's id: one of the data's upscalers. */
  upscaler: string;
}

export interface ScaleChoice {
  value: number;
  label: string;
}
export interface UpscalerChoice {
  id: string;
  label: string;
  hint: string;
}
export interface UpscaleData {
  scales: ScaleChoice[];
  upscalers: UpscalerChoice[];
  default: RenderScaleSettings;
  sharpenStops: number;
}

/** The ids the renderer knows (render/upscale.rs `Upscaler`; `stretch-pass` is for the bench). */
export const UPSCALER_KINDS = ['stretch', 'sharpen', 'stretch-pass'] as const;

/** Checks the data: scales in (0, 1] with 1 among them, known upscaler ids, a listed default. */
export function validateUpscale(raw: unknown): UpscaleData {
  const d = raw as Partial<UpscaleData> | null;
  const fail = (m: string): never => {
    throw new Error(`upscale.json: ${m}`);
  };
  if (!d || !Array.isArray(d.scales) || d.scales.length === 0) return fail('scales must be a list');
  for (const [i, s] of d.scales.entries()) {
    if (typeof s.value !== 'number' || !(s.value >= 0.5 && s.value <= 1))
      fail(`scales[${String(i)}].value must be in [0.5, 1], got ${JSON.stringify(s.value)}`);
    if (typeof s.label !== 'string') fail(`scales[${String(i)}].label must be text`);
  }
  if (!d.scales.some((s) => s.value === 1)) fail('scales must include 1 (full resolution)');
  if (!Array.isArray(d.upscalers) || d.upscalers.length === 0)
    return fail('upscalers must be a list');
  for (const [i, u] of d.upscalers.entries()) {
    if (!(UPSCALER_KINDS as readonly string[]).includes(u.id))
      fail(`upscalers[${String(i)}].id ${JSON.stringify(u.id)} is not one the renderer knows`);
  }
  const def = d.default;
  if (!def || !d.scales.some((s) => s.value === def.scale))
    fail('default.scale must be one of the scales');
  if (!def || !d.upscalers.some((u) => u.id === def.upscaler))
    fail('default.upscaler must be one of the upscalers');
  if (typeof d.sharpenStops !== 'number' || !(d.sharpenStops >= 0 && d.sharpenStops <= 4))
    fail('sharpenStops must be in [0, 4]');
  return d as UpscaleData;
}

export const UPSCALE: UpscaleData = validateUpscale(data);

export const RENDER_SCALE_KEY = 'dwell.renderScale';

export function defaultRenderScale(d: UpscaleData = UPSCALE): RenderScaleSettings {
  return { ...d.default };
}

/** A stored record; a scale or upscaler the data doesn't list takes the default. */
export function sanitizeRenderScale(value: unknown, d: UpscaleData = UPSCALE): RenderScaleSettings {
  const v = typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : {};
  const scale = d.scales.find((s) => s.value === v.scale)?.value ?? d.default.scale;
  const upscaler = d.upscalers.find((u) => u.id === v.upscaler)?.id ?? d.default.upscaler;
  return { scale, upscaler };
}

/** The record to store: the stored one's other fields kept (newer builds' fields survive). */
export function renderScaleRecord(
  stored: unknown,
  s: RenderScaleSettings,
): Record<string, unknown> {
  const base =
    typeof stored === 'object' && stored !== null && !Array.isArray(stored)
      ? (stored as Record<string, unknown>)
      : {};
  return { ...base, scale: s.scale, upscaler: s.upscaler };
}

/**
 * `?renderscale=0.5` and `?upscaler=sharpen` override the stored setting for a visit (the bench's
 * configurations): any scale in [0.5, 1] and any kind the renderer knows (`stretch-pass` too).
 */
export function renderScaleFromQuery(search: string, s: RenderScaleSettings): RenderScaleSettings {
  const p = new URLSearchParams(search);
  const scale = Number(p.get('renderscale'));
  const upscaler = p.get('upscaler');
  return {
    scale: p.has('renderscale') && scale >= 0.5 && scale <= 1 ? scale : s.scale,
    upscaler:
      upscaler !== null && (UPSCALER_KINDS as readonly string[]).includes(upscaler)
        ? upscaler
        : s.upscaler,
  };
}
