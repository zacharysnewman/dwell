// BUGS.md #78 (owner, 2026-11-12: "It seems like we're hiding some performance settings"): every
// renderer tier function (`dwell-client` render/, `pub fn <name>_for(`: what an adapter or device
// draws of a feature) is either driven by a Graphics setting a player can change, or listed here as
// fixed with the reason. A new tier function fails this test until it is given one or the other.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { PRESET_FIELDS } from './graphicsPresets';
import { defaultSettings } from './settingsMenu';

const RENDER = join(__dirname, '../../../crates/dwell-client/src/render');

/** A setting by its `Settings` path (a preset field or a stored setting), or fixed with a reason. */
type Audit = { setting: string } | { fixed: string };

const TIERS: Record<string, Audit> = {
  anisotropy: { setting: 'antialias.anisotropy' },
  bloom: { setting: 'bloom' },
  cards: { setting: 'leafCards' },
  clouds: { setting: 'clouds' },
  fxaa: { setting: 'antialias.mode' },
  // The distant terrain's shadows follow the Shadows setting (off with it).
  lod_shadows: { setting: 'shadows' },
  samples: { setting: 'antialias.mode' },
  shadows: { setting: 'shadows' },
  shafts: { setting: 'shafts' },
  torch_shadows: { setting: 'torchShadows' },
  water: { setting: 'water' },
  pixel_ratio: { setting: 'renderScale' },
  bake: { fixed: "the moon's terrain bake for a world seed, not a cost tier" },
  player: { fixed: 'a clip player lookup (characters), not a cost tier' },
  pixel_art: { fixed: 'a dev preview (`?pixelart=`), off unless asked for' },
  moon_normals: { fixed: "the CPU adapter's cheapest tier only; a GPU always draws it (one disc)" },
  stars: { fixed: "the CPU adapter's cheapest tier only; a GPU always draws them (night sky)" },
  motion_blur: {
    fixed: "follows Gameplay's Speed effects (motion.speedEffects); off on a CPU adapter",
  },
};

function tierFunctions(): string[] {
  const names = new Set<string>();
  for (const f of readdirSync(RENDER)) {
    if (!f.endsWith('.rs')) continue;
    const src = readFileSync(join(RENDER, f), 'utf8');
    for (const m of src.matchAll(/pub(?:\(crate\))? fn ([a-z_]+)_for\(/g))
      if (m[1]) names.add(m[1]);
  }
  return [...names].sort();
}

function hasPath(o: unknown, path: string): boolean {
  let at: unknown = o;
  for (const k of path.split('.')) {
    if (typeof at !== 'object' || at === null || !(k in at)) return false;
    at = (at as Record<string, unknown>)[k];
  }
  return true;
}

describe('graphics settings audit', () => {
  it('every renderer tier function has a setting or a stated reason it has none', () => {
    const found = tierFunctions();
    expect(found.length).toBeGreaterThan(10);
    const missing = found.filter((n) => !(n in TIERS));
    expect(missing, `tier functions with no Graphics setting: ${missing.join(', ')}`).toEqual([]);
    const gone = Object.keys(TIERS).filter((n) => !found.includes(n));
    expect(gone, `audited tier functions no longer in render/: ${gone.join(', ')}`).toEqual([]);
  });

  it("each audited setting is one the player's settings hold", () => {
    const s = defaultSettings(false);
    for (const [name, a] of Object.entries(TIERS)) {
      if (!('setting' in a)) continue;
      expect(
        hasPath(s, a.setting) || a.setting in PRESET_FIELDS,
        `${name}_for → ${a.setting}`,
      ).toBe(true);
    }
  });
});
