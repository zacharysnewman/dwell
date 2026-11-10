// The Model detail setting (ADR 0070 §10; owner, 2026-11-10: "make this a slider in the graphics and
// then later we'll set an opinionated value"): which of shared/models/models.json's `detail.levels`
// every skinned model (the Peasant and the animals) draws, live. The levels are data: the import
// bakes each one into every GLB, decimated and flat shaded, and the slider runs through them in
// order, full detail first. Kept in this browser under `dwell.modelDetail` as the level's id (a bare
// value); an id the data no longer lists reads as the data's default. Not part of a graphics preset:
// it is a look, not a cost the presets trade (each level is a few thousand triangles at most).
import models from '../../../shared/models/models.json';

export const MODEL_DETAIL_KEY = 'dwell.modelDetail';

export interface DetailLevel {
  id: string;
  label: string;
  ratio: number;
}

export interface DetailData {
  levels: DetailLevel[];
  default: string;
}

/** The levels, validated: at least one, unique ids, the default one of them (else it throws). */
export function detailData(raw: unknown): DetailData {
  const d = raw as Partial<DetailData> | undefined;
  const levels = d?.levels;
  if (!Array.isArray(levels) || levels.length === 0) throw new Error('models.json: detail: no levels');
  const ids = new Set<string>();
  for (const l of levels) {
    if (typeof l.id !== 'string' || ids.has(l.id))
      throw new Error(`models.json: detail: bad or repeated level ${String(l.id)}`);
    ids.add(l.id);
  }
  if (typeof d.default !== 'string' || !ids.has(d.default))
    throw new Error(`models.json: detail: default ${String(d.default)} is not a level`);
  return { levels, default: d.default };
}

export const MODEL_DETAIL: DetailData = detailData((models as { detail?: unknown }).detail);

/** A stored value as a level id: one the data lists, else the data's default. */
export function sanitizeModelDetail(value: unknown, data: DetailData = MODEL_DETAIL): string {
  return data.levels.some((l) => l.id === value) ? (value as string) : data.default;
}

/** The slider's position for a level id (0: full detail). */
export function detailIndex(id: string, data: DetailData = MODEL_DETAIL): number {
  return Math.max(
    0,
    data.levels.findIndex((l) => l.id === sanitizeModelDetail(id, data)),
  );
}

/** The level at a slider position (rounded to the nearest level). */
export function detailAt(v: number, data: DetailData = MODEL_DETAIL): DetailLevel {
  const k = Math.min(data.levels.length - 1, Math.max(0, Math.round(v)));
  return data.levels[k] as DetailLevel;
}
