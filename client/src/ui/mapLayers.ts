// The big map's debug layers (docs/proposals/REALISTIC_WORLD.md R6; owner 2026-11-12, §7 q8: the
// plates, uplift, rain and rock overlays "also on the big map"). The layers are data
// (shared/ui/mapLayers.json, checked by shared/ui/gen.mjs): each names a field of the generator's
// fixed vocabulary (`dwell-worldgen` `map_layers.rs`) and how it is coloured. The jobs worker samples
// a field over the face's disc (`requestMapLayer`); this colours the grid and the big map draws it
// over the revealed ground. A world without a field (plates outside a Realistic world) has nothing
// to draw for it.
import data from '../../../shared/ui/mapLayers.json';

export interface RampStop {
  at: number;
  color: string;
}

/** A layer as the data gives it. */
export interface MapLayerDef {
  name: string;
  label: string;
  field: string;
  opacity: number;
  colors?: Record<string, string>;
  outlineColor?: string;
  ramp?: RampStop[];
}

export interface MapLayersData {
  key: string;
  samples: number;
  layers: MapLayerDef[];
}

export const MAP_LAYERS: MapLayersData = data;

/** A category without a colour of its own (`map_layers.rs` `NO_COLOR`). */
export const NO_COLOR = 0xffffffff;

/** A sampled field (`LayerGrid::encode`). */
export interface LayerGrid {
  n: number;
  names: string[];
  colors: number[];
  cats: Uint8Array;
  ids: Uint8Array;
  values: Float32Array;
}

/** Decodes a layer's bytes (`LayerGrid::encode`), or null when empty or short (no such field). */
export function parseLayerGrid(b: Uint8Array): LayerGrid | null {
  if (b.length === 0) return null;
  const view = new DataView(b.buffer, b.byteOffset, b.byteLength);
  let o = 0;
  const count = b[o++] ?? 0;
  const names: string[] = [];
  const dec = new TextDecoder();
  for (let k = 0; k < count; k++) {
    const len = b[o++] ?? 0;
    if (o + len > b.length) return null;
    names.push(dec.decode(b.subarray(o, o + len)));
    o += len;
  }
  if (o + 4 * count + 4 > b.length) return null;
  const colors: number[] = [];
  for (let k = 0; k < count; k++, o += 4) colors.push(view.getUint32(o, true));
  const n = view.getUint32(o, true);
  o += 4;
  if (o + n * n * 6 > b.length) return null;
  const cats = new Uint8Array(n * n);
  const ids = new Uint8Array(n * n);
  const values = new Float32Array(n * n);
  for (let k = 0; k < n * n; k++, o += 6) {
    cats[k] = b[o] ?? 0;
    ids[k] = b[o + 1] ?? 0;
    values[k] = view.getFloat32(o + 2, true);
  }
  return { n, names, colors, cats, ids, values };
}

const hex = (c: string): number => parseInt(c.slice(1), 16);

/** The ramp's colour (0xRRGGBB) at `v`: linear between stops, clamped at the ends. */
export function rampColor(ramp: readonly RampStop[], v: number): number {
  const first = ramp[0];
  const last = ramp[ramp.length - 1];
  if (!first || !last) return 0;
  if (v <= first.at) return hex(first.color);
  if (v >= last.at) return hex(last.color);
  for (let k = 1; k < ramp.length; k++) {
    const a = ramp[k - 1];
    const b = ramp[k];
    if (!a || !b || v > b.at) continue;
    const t = (v - a.at) / (b.at - a.at);
    const [ca, cb] = [hex(a.color), hex(b.color)];
    const ch = (s: number) => Math.round(((ca >> s) & 0xff) * (1 - t) + ((cb >> s) & 0xff) * t);
    return (ch(16) << 16) | (ch(8) << 8) | ch(0);
  }
  return hex(last.color);
}

/** A category's colour: the layer's for its name, else its own (rocks.json's), else grey. */
export function categoryColor(layer: MapLayerDef, grid: LayerGrid, cat: number): number {
  const name = grid.names[cat] ?? '';
  const named = layer.colors?.[name];
  if (named) return hex(named);
  const own = grid.colors[cat];
  return own !== undefined && own !== NO_COLOR ? own : 0x808080;
}

/**
 * The layer's RGBA pixels, row k of the grid row k (face-local +z down), alpha by `opacity`;
 * a category layer outlined (opaque `outlineColor`) where a neighbour's id differs. With `disc`
 * (the grid's `layerSpan` and the face's radius) a sample whose square's centre lies past the disc's
 * edge is clear, so the layer covers the face and not the square around it.
 */
export function layerPixels(
  layer: MapLayerDef,
  grid: LayerGrid,
  disc?: { x0: number; step: number; radiusM: number },
): Uint8ClampedArray<ArrayBuffer> {
  const { n } = grid;
  const at = (i: number) => (disc ? disc.x0 + (i + 0.5) * disc.step : 0);
  const out = new Uint8ClampedArray(n * n * 4);
  const alpha = Math.round(255 * layer.opacity);
  const outline = layer.outlineColor ? hex(layer.outlineColor) : null;
  for (let j = 0; j < n; j++) {
    for (let i = 0; i < n; i++) {
      const k = j * n + i;
      let c: number;
      let a = alpha;
      if (layer.ramp) {
        c = rampColor(layer.ramp, grid.values[k] ?? 0);
      } else {
        c = categoryColor(layer, grid, grid.cats[k] ?? 0);
        const id = grid.ids[k];
        const edge =
          outline !== null &&
          ((i + 1 < n && grid.ids[k + 1] !== id) || (j + 1 < n && grid.ids[k + n] !== id));
        if (edge) {
          c = outline;
          a = 255;
        }
      }
      out[k * 4] = (c >> 16) & 0xff;
      out[k * 4 + 1] = (c >> 8) & 0xff;
      out[k * 4 + 2] = c & 0xff;
      if (disc && Math.hypot(at(i), at(j)) > disc.radiusM) a = 0;
      out[k * 4 + 3] = a;
    }
  }
  return out;
}

/** The face's disc as the layer grid covers it: its first sample (m) and the metres per sample. */
export function layerSpan(radiusM: number, n: number): { x0: number; step: number } {
  const step = Math.ceil((2 * radiusM) / n);
  return { x0: -(step * n) / 2, step };
}

/** The layer after `current` (−1: none) by the data's order, wrapping back to none. */
export function nextLayer(current: number, count: number = MAP_LAYERS.layers.length): number {
  return current + 1 >= count ? -1 : current + 1;
}
