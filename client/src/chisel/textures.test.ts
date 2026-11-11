// Chisel's Textures tab (ADR 0074 B6): the preview paints what the build bakes, an export goes
// back through gen.mjs to the same pixels, a bad recipe fails, the tab's state rides in the URL.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { bake } from '../../../shared/textures/gen.mjs';
import blocksDwell from '../../../shared/blocks/dwell.json';
import {
  TILE,
  blockMaterials,
  blockTints,
  buildTextures,
  exportMaterials,
  exportPalette,
  loadTextures,
  paintTile,
  paletteWarnings,
  saveTextures,
  textureFromParams,
  textureToParams,
  validateTextures,
} from './textures';

const baked = readFileSync(
  new URL('../../../crates/dwell-mesh/src/painterly.rgba', import.meta.url),
);

describe('the Textures tab', () => {
  it('previews every atlas tile bit-identical to the baked atlas at the build data', () => {
    const e = buildTextures();
    const names = Object.keys(e.materials.tiles);
    const size = TILE * TILE * 4;
    expect(baked.length).toBe(names.length * size);
    names.forEach((tile, i) => {
      const material = e.materials.tiles[tile] ?? '';
      const ours = Buffer.from(paintTile(e, material));
      expect(ours.equals(baked.subarray(i * size, (i + 1) * size)), tile).toBe(true);
    });
  });

  it('round-trips an edit through the exported JSON and gen.mjs to the same pixels', () => {
    const e = buildTextures();
    const stone = e.materials.materials.stone;
    expect(stone?.layers?.length).toBeGreaterThan(0);
    const layer = stone?.layers?.find((l) => typeof l.opacity === 'number') ?? stone?.layers?.[0];
    if (layer) layer.opacity = 0.37;
    e.palette.ramps.meadow = { shadow: '#3f7a50', base: '#86c04a', light: '#d0e070' };
    expect(validateTextures(e)).toEqual([]);
    const materials = JSON.parse(exportMaterials(e)) as typeof e.materials;
    const palette = JSON.parse(exportPalette(e)) as typeof e.palette;
    const { names, texels } = bake(materials, palette, Object.keys(materials.tiles));
    const size = TILE * TILE * 4;
    for (const t of ['stone', 'grass']) {
      const i = names.indexOf(t);
      const ours = Buffer.from(paintTile(e, e.materials.tiles[t] ?? ''));
      expect(ours.equals(Buffer.from(texels.subarray(i * size, (i + 1) * size))), t).toBe(true);
      expect(ours.equals(baked.subarray(i * size, (i + 1) * size)), `${t} changed`).toBe(false);
    }
  });

  it('fails a bad recipe with a message naming it', () => {
    const e = buildTextures();
    const layer = e.materials.materials.stone?.layers?.[0];
    if (layer) layer.opacity = 4;
    expect(validateTextures(e).join()).toMatch(/materials\.stone.*opacity must be/);
    const f = buildTextures();
    f.materials.materials.stone?.layers?.push({ kind: 'fill', color: 'no_such.base' });
    expect(validateTextures(f).join()).toMatch(/unknown palette colour "no_such.base"/);
    const g = buildTextures();
    g.palette.ramps.meadow = { shadow: 'green', base: '#82bd45', light: '#c8e06a' };
    expect(validateTextures(g).join()).toMatch(/ramps\.meadow\.shadow must be a colour/);
  });

  it('flags colours off the art direction; the build palette only its known deviations', () => {
    const e = buildTextures();
    // Snow's top and the deer's cream belly are near white (the brightest content there is).
    expect(paletteWarnings(e.palette).map((w) => w.ref)).toEqual([
      'snow.light',
      'deer_cream.light',
    ]);
    e.palette.ramps.meadow = { shadow: '#020202', base: '#82bd45', light: '#4a6aff' };
    const refs = paletteWarnings(e.palette).map((w) => w.ref);
    expect(refs).toContain('meadow.shadow');
    expect(refs).toContain('meadow.light');
  });

  it('keeps its state in the URL', () => {
    const p = new URLSearchParams('seed=3');
    textureToParams({ on: true, material: 'stone' }, p);
    expect(p.toString()).toBe('seed=3&tab=textures&tex=stone');
    const m = buildTextures().materials;
    expect(textureFromParams(p, m, { on: false, material: 'grass_top' })).toEqual({
      on: true,
      material: 'stone',
    });
    expect(
      textureFromParams(new URLSearchParams('tex=nope'), m, { on: false, material: 'grass_top' }),
    ).toEqual({
      on: false,
      material: 'grass_top',
    });
  });

  it('keeps the edit in the browser and starts from the build when storage fails', () => {
    const map = new Map<string, string>();
    const store = {
      getItem: (k: string) => map.get(k) ?? null,
      setItem: (k: string, v: string) => map.set(k, v),
    };
    const e = buildTextures();
    e.palette.ramps.meadow = { shadow: '#4a8a45', base: '#80b040', light: '#c8e06a' };
    saveTextures(store, e);
    expect(loadTextures(store).palette.ramps.meadow?.base).toBe('#80b040');
    const broken = {
      getItem: () => {
        throw new Error('denied');
      },
    };
    expect(loadTextures(broken)).toEqual(buildTextures());
    expect(blockMaterials(e.materials)).toContain('stone');
  });

  it("tints the 3D section's blocks only where an edit changes their far colour", () => {
    const e = buildTextures();
    expect(blockTints(e, blocksDwell)).toEqual({});
    e.palette.ramps.meadow = { shadow: '#30508a', base: '#4060c0', light: '#90a0e0' };
    const t = blockTints(e, blocksDwell);
    const grass = t['dwell:grass'];
    expect(grass, 'grass tops paint with meadow').toBeDefined();
    expect(grass?.[2] ?? 0).toBeGreaterThan(1);
    expect(grass?.[1] ?? 9).toBeLessThan(1);
    expect(t['dwell:stone'], 'stone untouched').toBeUndefined();
  });
});
