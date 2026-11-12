// BUGS.md #78 (owner, 2026-11-12: "It seems like we're hiding some performance settings") and
// WORK_QUEUE item 30 ("Ensure all of our graphics have settings in our graphics menu"): every visual
// feature is either driven by a Graphics setting a player can change, or listed here as fixed with
// the reason. The features are found in four places, each a declared list below that a new one fails
// until it is given a setting or a reason:
// - the renderer's tier functions (`dwell-client` render/, `pub fn <name>_for(`: what an adapter or
//   device draws of a feature);
// - the look data (`shared/look/*.json`, one file a visual feature);
// - the mesher's per-feature switches (`dwell_mesh::ChunkFlags`' fields, and the LOD section's trees);
// - the client's particle effects (`dwell-client` src/*.rs with a CPU tier, `cpu: bool`).
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
  // The distant terrain's shadows: their own switch, at the Shadows setting's tier (off with it).
  lod_shadows: { setting: 'features.distantShadows' },
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

const LOOKS: Record<string, Audit> = {
  ambient: { fixed: 'the vertex ambient occlusion baked at mesh time: no per-frame cost to cut' },
  atmosphere: { fixed: "the sky's colour by height, one term of the sky pass: no cost of its own" },
  bloom: { setting: 'bloom' },
  clouds: { setting: 'clouds' },
  daynight: { fixed: 'the world clock, sun, moon and sky keyframes: the look of the day itself' },
  flames: { fixed: "a few billboards a lit torch; their light follows Torch shadows' tiers" },
  fog: {
    fixed:
      "the height fog and the haze (`air`, `veil`): one term of every pixel's shading, fixed look data for every client (owner, 2026-10-21)",
  },
  leaves: { setting: 'leafCards' },
  light: { fixed: 'the flood light baked into each vertex at mesh time: no per-frame cost' },
  lights: { setting: 'torchShadows' },
  loading: { fixed: 'the loading screen, not a drawn feature' },
  lodblend: { fixed: "the LOD swaps' cross-fade: removing it would pop, not save GPU time" },
  lodcolour: { fixed: "the distant risers' colours, chosen at mesh time: no cost" },
  lodmap: { fixed: 'a preview (`?lodmap=on`), off unless asked for' },
  lodshadows: { setting: 'features.distantShadows' },
  motes: { fixed: "a portal shaft's motes at the midplane crossing only: a few dozen points" },
  motion: { setting: 'motion.speedEffects' },
  orbits: { fixed: "the sky's orbit by world seed: the world's look, not a cost tier" },
  outline: { fixed: 'a dev preview (`?outline=`), off unless asked for' },
  pixelart: { fixed: 'a dev preview (`?pixelart=`), off unless asked for' },
  reflections: { setting: 'water' },
  shadows: { setting: 'shadows' },
  shafts: { setting: 'shafts' },
  spyglass: { fixed: 'the zoom tool, drawn only while zoomed' },
  underwater: { fixed: 'drawn only with the eye in water; the water tier sets its cost' },
  waterfalls: { setting: 'water.waterfalls' },
  waterflow: { setting: 'water.flowRipples' },
  watertiers: { setting: 'water' },
};

/** The mesher's per-feature switches: `dwell_mesh::ChunkFlags`' fields, then the section's trees. */
const MESHER: Record<string, Audit> = {
  water_tops: { fixed: "the GPU heightfield's hand-over: follows the Water setting's tier" },
  cards: { setting: 'leafCards' },
  plants: { setting: 'features.tallGrass' },
  section_trees: { setting: 'features.farTrees' },
};

/** The client's particle effects (a module with a CPU tier). */
const EFFECTS: Record<string, Audit> = {
  blasts: { setting: 'explosions.smoke' },
  chips: { setting: 'features.particles' },
  far_herds: { setting: 'features.farHerds' },
  hit_shards: { setting: 'features.hitShards' },
  steam: { setting: 'features.particles' },
};

const ROOT = join(__dirname, '../../..');
const read = (p: string): string => readFileSync(join(ROOT, p), 'utf8');

function lookFiles(): string[] {
  return readdirSync(join(ROOT, 'shared/look'))
    .filter((f) => f.endsWith('.json'))
    .map((f) => f.replace(/\.json$/, ''))
    .sort();
}

function mesherSwitches(): string[] {
  const src = read('crates/dwell-mesh/src/mesher.rs');
  const body = /pub struct ChunkFlags \{([\s\S]*?)\n\}/.exec(src)?.[1] ?? '';
  const fields = [...body.matchAll(/pub ([a-z_]+):/g)].map((m) => m[1] ?? '');
  const lod = read('crates/dwell-mesh/src/lod_mesher.rs');
  return [...fields, ...(/pub fn mesh_section_trees\(/.test(lod) ? ['section_trees'] : [])];
}

function effectModules(): string[] {
  const dir = join(ROOT, 'crates/dwell-client/src');
  return readdirSync(dir)
    .filter((f) => f.endsWith('.rs') && /\bcpu: bool\b/.test(readFileSync(join(dir, f), 'utf8')))
    .map((f) => f.replace(/\.rs$/, ''))
    .sort();
}

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

const SOURCES: [string, () => string[], Record<string, Audit>][] = [
  ['renderer tier functions (render/*_for)', tierFunctions, TIERS],
  ['look data files (shared/look/*.json)', lookFiles, LOOKS],
  ['mesher switches (ChunkFlags, section trees)', mesherSwitches, MESHER],
  ['client particle effects (src/*.rs with a CPU tier)', effectModules, EFFECTS],
];

describe('graphics settings audit', () => {
  for (const [what, find, audit] of SOURCES) {
    it(`every one of the ${what} has a setting or a stated reason it has none`, () => {
      const found = find();
      expect(found.length).toBeGreaterThan(3);
      const missing = found.filter((n) => !(n in audit));
      expect(missing, `${what} with no Graphics setting: ${missing.join(', ')}`).toEqual([]);
      const gone = Object.keys(audit).filter((n) => !found.includes(n));
      expect(gone, `audited ${what} no longer there: ${gone.join(', ')}`).toEqual([]);
    });
  }

  it("each audited setting is one the player's settings hold", () => {
    const s = defaultSettings(false);
    for (const [what, , audit] of SOURCES)
      for (const [name, a] of Object.entries(audit)) {
        if (!('setting' in a)) continue;
        expect(
          hasPath(s, a.setting) || a.setting in PRESET_FIELDS,
          `${what}: ${name} → ${a.setting}`,
        ).toBe(true);
      }
  });

  it('a stated reason is a sentence, not a placeholder', () => {
    for (const [, , audit] of SOURCES)
      for (const a of Object.values(audit))
        if ('fixed' in a) expect(a.fixed.length).toBeGreaterThan(20);
  });

  it('every new feature setting is set by every preset and shown on the Graphics page', () => {
    // WORK_QUEUE item 30: the features with no setting before it each have a preset field.
    for (const k of [
      'tallGrass',
      'farTrees',
      'distantShadows',
      'hitShards',
      'particles',
      'farHerds',
    ])
      expect(k in PRESET_FIELDS, k).toBe(true);
  });
});
