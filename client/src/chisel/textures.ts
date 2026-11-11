// Chisel's Textures tab (ADR 0074 B6, the Painterly Texture Lab folded in): the painterly
// materials and palette (shared/textures/materials.json, palette.json) edited with the build's own
// painter (shared/textures/paint.mjs, the one gen.mjs bakes with), so an unchanged material previews
// bit-identical to the baked atlas; the art direction's palette checks (docs/ART_DIRECTION.md §2);
// the tab's state in the URL; the edit kept in the browser and exported as the files' JSON.
import {
  RAMP_STOPS,
  STROKE_KINDS,
  TILE,
  paintMaterial,
  resolveMaterial,
  validateMaterials,
  validatePalette,
  type Layer,
  type Materials,
  type Palette,
} from '../../../shared/textures/paint.mjs';
import materialsJson from '../../../shared/textures/materials.json';
import paletteJson from '../../../shared/textures/palette.json';

export { TILE };

/** What the Textures tab edits: the whole materials file and the palette. */
export interface TextureEdit {
  materials: Materials;
  palette: Palette;
}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

export const BUILD_MATERIALS = materialsJson as unknown as Materials;
export const BUILD_PALETTE = paletteJson as unknown as Palette;

/** The build's textures (a fresh copy). */
export function buildTextures(): TextureEdit {
  return { materials: clone(BUILD_MATERIALS), palette: clone(BUILD_PALETTE) };
}

/** The edit's problems: the palette's and the materials' validation (gen.mjs's rules), one message. */
export function validateTextures(e: TextureEdit): string[] {
  try {
    validatePalette(e.palette);
    validateMaterials(e.materials, e.palette);
    return [];
  } catch (err) {
    return [(err as Error).message];
  }
}

/** Paints `material` as the bake does: TILE² RGBA, row y = 0 (the tile's bottom) first. */
export function paintTile(e: TextureEdit, material: string): Uint8Array {
  return paintMaterial(material, e.materials, e.palette);
}

/** The block materials (use `block`), in file order: the ones the tab's picker offers. */
export function blockMaterials(m: Materials): string[] {
  return Object.keys(m.materials).filter((n) => {
    try {
      return (resolveMaterial(m, n).use ?? 'block') === 'block';
    } catch {
      return true;
    }
  });
}

/** The atlas tiles `material` paints (materials.json's `tiles`). */
export function tilesOf(m: Materials, material: string): string[] {
  return Object.entries(m.tiles).filter(([, v]) => v === material).map(([k]) => k);
}

const lin = (b: number) => {
  const c = b / 255;
  return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
};
const enc = (v: number) =>
  Math.round(255 * (v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055));

/** A tile's far colour: its texels' mean in linear light, by coverage, as sRGB bytes. */
export function farColour(tile: Uint8Array): [number, number, number] {
  let r = 0;
  let g = 0;
  let b = 0;
  let n = 0;
  for (let k = 0; k < TILE * TILE; k++) {
    const a = (tile[k * 4 + 3] ?? 0) / 255;
    r += lin(tile[k * 4] ?? 0) * a;
    g += lin(tile[k * 4 + 1] ?? 0) * a;
    b += lin(tile[k * 4 + 2] ?? 0) * a;
    n += a;
  }
  n = n || 1;
  return [enc(r / n), enc(g / n), enc(b / n)];
}

export const hex = (c: readonly number[]) =>
  '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('');
const rgb = (h: string): [number, number, number] => {
  const n = parseInt(h.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};

/** The art direction's albedo range (ART_DIRECTION.md §2: "about 0.06–0.92 sRGB"), with its "about"
 * as a margin: a colour's mean value outside [0.04, 0.96] is flagged. */
export const ALBEDO: readonly [number, number] = [0.04, 0.96];
/** How far (a share of the colour's sum) a shadow may sit warmer, or a light cooler, than its base
 * before it is flagged: ramps desaturate toward their light, so only a clear reversal counts. */
export const HUE_SLACK = 0.12;
/** Ramps and swatches that are not albedo content (the utility tiles' colours, ink, sky accents). */
export const UNCHECKED: readonly string[] = [
  'chalk',
  'cloud',
  'missing_magenta',
  'missing_black',
  'ink',
  'ink_soft',
  'foam',
  'sun',
  'star',
];

/** One art-direction warning: the colour (`ramp.stop` or a swatch) and what is off. */
export interface PaletteWarning {
  ref: string;
  message: string;
}

const sum = (c: readonly number[]) => c.reduce((a, b) => a + b, 0) || 1;

/**
 * The palette's art-direction warnings (ART_DIRECTION.md §2): a ramp stop or swatch whose mean
 * value is outside the albedo range, a shadow clearly warmer than its base (it should shift toward
 * blue-violet), a light clearly cooler (it should shift toward yellow). Warnings, not errors: the
 * bake takes the colour; the tab flags it.
 */
export function paletteWarnings(p: Palette): PaletteWarning[] {
  const out: PaletteWarning[] = [];
  const range = (ref: string, h: string) => {
    const v = sum(rgb(h)) / 765;
    if (v < ALBEDO[0]) out.push({ ref, message: `${h} is darker than the albedo range (${String(ALBEDO[0])})` });
    if (v > ALBEDO[1]) out.push({ ref, message: `${h} is brighter than the albedo range (${String(ALBEDO[1])})` });
  };
  const cool = (h: string) => {
    const c = rgb(h);
    return (c[2] - c[0]) / sum(c);
  };
  const warm = (h: string) => {
    const c = rgb(h);
    return ((c[0] + c[1]) / 2 - c[2]) / sum(c);
  };
  for (const [name, ramp] of Object.entries(p.ramps)) {
    if (UNCHECKED.includes(name)) continue;
    for (const s of RAMP_STOPS) range(`${name}.${s}`, ramp[s]);
    if (cool(ramp.shadow) < cool(ramp.base) - HUE_SLACK) {
      out.push({ ref: `${name}.shadow`, message: 'warmer than its base (shift it toward blue-violet)' });
    }
    if (warm(ramp.light) < warm(ramp.base) - HUE_SLACK) {
      out.push({ ref: `${name}.light`, message: 'cooler than its base (shift it toward yellow)' });
    }
  }
  for (const [name, h] of Object.entries(p.swatches ?? {})) {
    if (!UNCHECKED.includes(name)) range(name, h);
  }
  return out;
}

/** The palette references a layer's colour pickers offer: the material's own stops, then every stop and swatch. */
export function colourRefs(p: Palette): string[] {
  const out = RAMP_STOPS.map((s) => `@${s}`);
  for (const r of Object.keys(p.ramps)) for (const s of RAMP_STOPS) out.push(`${r}.${s}`);
  for (const s of Object.keys(p.swatches ?? {})) out.push(s);
  return out;
}

/**
 * The sliders' ranges for a layer's numbers, as paint.mjs's validation bounds them
 * (`validateLayer`): [min, max, step]. A field absent here is edited as text.
 */
export const LAYER_RANGES: Record<string, readonly [number, number, number]> = {
  opacity: [0, 1, 0.01],
  start: [0, 1, 0.01],
  end: [0, 1, 0.01],
  steps: [2, 16, 1],
  count: [1, 256, 1],
  jitter: [0, 1, 0.01],
  band: [0, 1, 0.01],
  ragged: [0, TILE / 4, 0.25],
  angle: [-360, 360, 1],
  angleJitter: [0, 180, 1],
  bend: [-TILE / 2, TILE / 2, 0.25],
  softness: [0.25, 8, 0.25],
  arc: [1, 360, 1],
  seed: [0, 9999, 1],
};
/** `width` and `length` by layer kind (an edge light's width is to TILE / 2). */
export function rangeOf(layer: Layer, key: string): readonly [number, number, number] | undefined {
  if (key === 'width') return layer.kind === 'edgeLight' ? [0.25, TILE / 2, 0.25] : [0.25, TILE, 0.25];
  if (key === 'length') return [0.25, 2 * TILE, 0.25];
  return LAYER_RANGES[key];
}

/** The number fields a layer of `kind` takes (paint.mjs's keys), in the editor's order. */
export function layerFields(kind: string): string[] {
  if (STROKE_KINDS.includes(kind)) {
    return ['count', 'band', 'jitter', 'width', 'length', 'angle', 'angleJitter', 'bend', 'softness', 'opacity', ...(kind === 'ring' ? ['arc'] : []), 'seed'];
  }
  if (kind === 'gradient') return ['start', 'end', 'steps', 'opacity'];
  if (kind === 'edgeLight') return ['width', 'ragged', 'opacity', 'seed'];
  return ['opacity'];
}

/** The tab's state in the URL: `tab=textures` and the picked material `tex`. */
export interface TextureView {
  on: boolean;
  material: string;
}

export function textureToParams(v: TextureView, p: URLSearchParams) {
  if (v.on) p.set('tab', 'textures');
  else p.delete('tab');
  p.set('tex', v.material);
}

/** Read back from the URL; an unknown material keeps `base`'s. */
export function textureFromParams(p: URLSearchParams, m: Materials, base: TextureView): TextureView {
  const tex = p.get('tex');
  return {
    on: p.get('tab') === 'textures',
    material: tex !== null && tex in m.materials ? tex : base.material,
  };
}

/** The files to drop into shared/textures/ (as the repo writes them: two-space JSON, a newline). */
export function exportMaterials(e: TextureEdit): string {
  return JSON.stringify(e.materials, null, 2) + '\n';
}
export function exportPalette(e: TextureEdit): string {
  return JSON.stringify(e.palette, null, 2) + '\n';
}

export const TEXTURE_KEY = 'dwell.chisel.textures';
/** The build's data the stored edit was made against: a stored edit of older data is set aside. */
const STAMP = JSON.stringify([BUILD_PALETTE, BUILD_MATERIALS]).length;

/** The stored edit, or the build's (also when storage throws or holds an edit of other data). */
export function loadTextures(store: Pick<Storage, 'getItem'> | undefined): TextureEdit {
  try {
    const raw = store?.getItem(TEXTURE_KEY);
    if (raw) {
      const s = JSON.parse(raw) as Partial<TextureEdit> & { stamp?: number };
      if (s.stamp === STAMP && s.materials && s.palette) return { materials: s.materials, palette: s.palette };
    }
  } catch {
    /* storage unavailable */
  }
  return buildTextures();
}

export function saveTextures(store: Pick<Storage, 'setItem'> | undefined, e: TextureEdit) {
  try {
    store?.setItem(TEXTURE_KEY, JSON.stringify({ ...e, stamp: STAMP }));
  } catch {
    /* not saved */
  }
}
