import { describe, expect, it } from 'vitest';
import {
  MAP_LAYERS,
  NO_COLOR,
  type LayerGrid,
  type MapLayerDef,
  layerPixels,
  layerSpan,
  nextLayer,
  parseLayerGrid,
  rampColor,
} from './mapLayers';

/** `LayerGrid::encode`'s bytes (map_layers.rs), for a fixture. */
function encode(names: string[], colors: number[], n: number, samples: [number, number, number][]) {
  const head: number[] = [names.length];
  for (const s of names) head.push(s.length, ...new TextEncoder().encode(s));
  const b = new Uint8Array(head.length + 4 * names.length + 4 + n * n * 6);
  b.set(head);
  const v = new DataView(b.buffer);
  let o = head.length;
  for (const c of colors) {
    v.setUint32(o, c, true);
    o += 4;
  }
  v.setUint32(o, n, true);
  o += 4;
  for (const [cat, id, value] of samples) {
    b[o] = cat;
    b[o + 1] = id;
    v.setFloat32(o + 2, value, true);
    o += 6;
  }
  return b;
}

const px = (out: Uint8ClampedArray, k: number) => [...out.subarray(k * 4, k * 4 + 4)];

describe('the big map debug layers (REALISTIC_WORLD.md R6)', () => {
  it('the data has the plates, uplift, rain and rock layers', () => {
    expect(MAP_LAYERS.layers.map((l) => l.field)).toEqual(['plates', 'uplift', 'rain', 'rocks']);
    expect(MAP_LAYERS.key).toBe('KeyL');
  });

  it('decodes the generator grid, and nothing from empty or short bytes', () => {
    const b = encode(['oceanic', 'continental'], [NO_COLOR, NO_COLOR], 2, [
      [0, 1, 0],
      [1, 2, 0],
      [1, 2, 0],
      [0, 1, 1.5],
    ]);
    const g = parseLayerGrid(b);
    expect(g?.n).toBe(2);
    expect(g?.names).toEqual(['oceanic', 'continental']);
    expect([...(g?.cats ?? [])]).toEqual([0, 1, 1, 0]);
    expect([...(g?.ids ?? [])]).toEqual([1, 2, 2, 1]);
    expect(g?.values[3]).toBeCloseTo(1.5);
    expect(parseLayerGrid(new Uint8Array())).toBeNull();
    expect(parseLayerGrid(b.subarray(0, b.length - 1))).toBeNull();
  });

  it('colours plates by kind from the data, outlined where the plate changes', () => {
    const plates = MAP_LAYERS.layers.find((l) => l.field === 'plates') as MapLayerDef;
    const g = parseLayerGrid(
      encode(['oceanic', 'continental'], [NO_COLOR, NO_COLOR], 2, [
        [1, 1, 0],
        [0, 2, 0],
        [1, 1, 0],
        [0, 2, 0],
      ]),
    ) as LayerGrid;
    const out = layerPixels(plates, g);
    const hex = (c: string) => [1, 3, 5].map((i) => parseInt(c.slice(i, i + 2), 16));
    // (0, 0) borders plate 2 to its +x: the outline, opaque.
    expect(px(out, 0)).toEqual([...hex(plates.outlineColor ?? ''), 255]);
    // (1, 0) and (1, 1) are inside plate 2 (oceanic): its kind's colour at the layer's opacity.
    const a = Math.round(255 * plates.opacity);
    expect(px(out, 3)).toEqual([...hex(plates.colors?.oceanic ?? ''), a]);
  });

  it('colours rocks by their own rocks.json colour unless the layer names one', () => {
    const def: MapLayerDef = {
      name: 'r',
      label: 'R',
      field: 'rocks',
      opacity: 1,
      colors: { b: '#00ff00' },
    };
    const g = parseLayerGrid(encode(['a', 'b'], [0x112233, 0x445566], 1, [[0, 0, 0]])) as LayerGrid;
    expect(px(layerPixels(def, g), 0)).toEqual([0x11, 0x22, 0x33, 255]);
    g.cats[0] = 1;
    expect(px(layerPixels(def, g), 0)).toEqual([0, 255, 0, 255]);
  });

  it('colours a value layer by its ramp, clamped at the ends', () => {
    const ramp = [
      { at: 0, color: '#000000' },
      { at: 100, color: '#ffffff' },
    ];
    expect(rampColor(ramp, -5)).toBe(0x000000);
    expect(rampColor(ramp, 50)).toBe(0x808080);
    expect(rampColor(ramp, 500)).toBe(0xffffff);
    const rain = MAP_LAYERS.layers.find((l) => l.field === 'rain') as MapLayerDef;
    const g = parseLayerGrid(encode([], [], 1, [[0, 0, 3000]])) as LayerGrid;
    const last = rain.ramp?.[rain.ramp.length - 1]?.color ?? '';
    expect(px(layerPixels(rain, g), 0).slice(0, 3)).toEqual(
      [1, 3, 5].map((i) => parseInt(last.slice(i, i + 2), 16)),
    );
  });

  it('a layer added by data alone draws (a second rain ramp)', () => {
    const def: MapLayerDef = {
      name: 'wet',
      label: 'Wet',
      field: 'rain',
      opacity: 0.5,
      ramp: [
        { at: 0, color: '#ff0000' },
        { at: 10, color: '#0000ff' },
      ],
    };
    const g = parseLayerGrid(encode([], [], 1, [[0, 0, 10]])) as LayerGrid;
    expect(px(layerPixels(def, g), 0)).toEqual([0, 0, 255, 128]);
  });

  it('draws over the face disc only, not the square around it (R6 browser check)', () => {
    // A 4 × 4 grid over a disc of radius 2 m, 1 m a sample from (−2, −2): the corner samples'
    // centres (±1.5, ±1.5) are 2.12 m out, past the edge; the middle ones (±0.5) are inside.
    const def = MAP_LAYERS.layers.find((l) => l.field === 'rain') as MapLayerDef;
    const g = parseLayerGrid(
      encode(
        [],
        [],
        4,
        Array.from({ length: 16 }, () => [0, 0, 500] as [number, number, number]),
      ),
    ) as LayerGrid;
    const out = layerPixels(def, g, { x0: -2, step: 1, radiusM: 2 });
    expect(px(out, 0)[3]).toBe(0);
    expect(px(out, 15)[3]).toBe(0);
    expect(px(out, 5)[3]).toBe(Math.round(255 * def.opacity));
    expect(px(out, 1)[3]).toBe(Math.round(255 * def.opacity));
  });

  it('covers the face disc and steps through the layers back to none', () => {
    const { x0, step } = layerSpan(50_000, 256);
    expect(x0).toBeLessThanOrEqual(-50_000);
    expect(x0 + step * 256).toBeGreaterThanOrEqual(50_000);
    const n = MAP_LAYERS.layers.length;
    let l = -1;
    const seen: number[] = [];
    for (let k = 0; k <= n; k++) seen.push((l = nextLayer(l)));
    expect(seen).toEqual([...Array.from({ length: n }, (_, i) => i), -1]);
  });
});
