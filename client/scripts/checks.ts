// Per-area checks (`npm run check:<area>`, docs/AGENT_QUICKSTART.md): the generators, Rust crates
// and client tests that belong to one area of the game, so a change there runs only those locally
// (CI still runs everything). The table is the source of truth; `checks.test.ts` fails when a
// generator CI runs, a node test, a workspace crate or a client test file belongs to no area, or a
// path here no longer exists. Pure: no Node or process imports (the runner is `check.ts`).

export interface Area {
  /** What the area covers, printed by `check.ts --list`. */
  notes: string;
  /** Generator scripts, from the repo root (`node <path>`, or `python3` for `.py`). */
  generators: string[];
  /** Node's test runner over the data (`node --test <path>`), from the repo root. */
  nodeTests: string[];
  /** Workspace crates: fmt, clippy (all targets, `-D warnings`) and tests. */
  crates: string[];
  /** Vitest filters, relative to `client/` (a directory with a trailing `/` or a test file). */
  vitest: string[];
  /** Extra commands, from the repo root (e.g. the controller's mirror suite). */
  extra?: { argv: string[]; env?: Record<string, string> }[];
}

const look = (names: string[]) => names.map((n) => `shared/look/${n}.mjs`);
const lookTests = (names: string[]) => names.map((n) => `shared/look/${n}.test.mjs`);

/** The `shared/look` scripts that are the renderer's alone (not the HUD's). */
const RENDER_LOOKS = [
  'gen',
  'clouds',
  'reflections',
  'shadows',
  'lodshadows',
  'fog',
  'atmosphere',
  'lodblend',
  'lodcolour',
  'leaves',
  'lodmap',
  'outline',
  'pixelart',
  'ambient',
  'light',
  'lights',
  'flames',
  'motes',
  'waterflow',
  'underwater',
  'shafts',
  'waterfalls',
  'daynight',
  'bloom',
];
/** The `shared/look` scripts that also emit HUD or settings code (client/src/ui). */
const UI_LOOKS = ['loading', 'spyglass', 'watertiers', 'orbits'];

export const AREAS: Record<string, Area> = {
  protocol: {
    notes: 'wire constants, golden vectors, the tables the protocol carries (modes, tools, …)',
    generators: [
      'shared/protocol/gen.mjs',
      'shared/inventory/gen.mjs',
      'shared/modes/gen.mjs',
      'shared/tools/gen.mjs',
      'shared/vitals/gen.mjs',
      'shared/protocol/make_vectors.py',
      'shared/master/make_vectors.mjs',
    ],
    nodeTests: [
      'shared/protocol/capabilities.test.mjs',
      'shared/inventory/gen.test.mjs',
      'shared/modes/gen.test.mjs',
      'shared/tools/gen.test.mjs',
      'shared/vitals/gen.test.mjs',
    ],
    crates: ['dwell-protocol', 'dwell-net'],
    vitest: ['src/net/', 'src/ui/modes.test.ts'],
  },
  data: {
    notes: 'the block registry, items, structures, explosives, fluids and ground tables',
    generators: [
      'shared/blocks/gen.mjs',
      'shared/items/gen.mjs',
      'shared/structures/gen.mjs',
      'shared/explosives/gen.mjs',
      'shared/fluids/gen.mjs',
      'shared/fluids/falls.mjs',
      'shared/ground/gen.mjs',
      'shared/brushes/gen.mjs',
    ],
    nodeTests: [
      'shared/blocks/faces.test.mjs',
      'shared/blocks/lights.test.mjs',
      'shared/blocks/parts.test.mjs',
      'shared/items/gen.test.mjs',
      'shared/structures/gen.test.mjs',
      'shared/explosives/gen.test.mjs',
      'shared/fluids/gen.test.mjs',
      'shared/fluids/falls.test.mjs',
      'shared/ground/gen.test.mjs',
    ],
    crates: ['dwell-blocks'],
    vitest: [
      'src/world/',
      'src/local/structure',
      'src/ui/creativeInventory.test.ts',
      'src/ui/hotbar.test.ts',
      'src/ui/brush',
      'src/ui/blueprints.test.ts',
    ],
  },
  worldgen: {
    notes: 'terrain, biomes, trees and the world map; the voxel world and its face frames',
    generators: [
      'shared/worldgen/gen.mjs',
      'shared/trees/gen.mjs',
      'shared/structures/gen.mjs',
      'shared/ground/gen.mjs',
    ],
    nodeTests: [
      'shared/worldgen/gen.test.mjs',
      'shared/trees/gen.test.mjs',
      'shared/structures/gen.test.mjs',
      'shared/ground/gen.test.mjs',
    ],
    crates: ['dwell-worldgen', 'dwell-world'],
    vitest: ['src/world/worldMaps.test.ts'],
  },
  mesh: {
    notes: 'chunk meshing, the LOD and the texture atlas',
    generators: ['shared/textures/gen.mjs', 'shared/blocks/gen.mjs'],
    nodeTests: ['shared/textures/gen.test.mjs'],
    crates: ['dwell-mesh', 'dwell-lod'],
    vitest: ['src/chisel/'],
  },
  render: {
    notes: 'the looks (shaders and their tunables), models, characters, the renderer',
    generators: [
      ...look(RENDER_LOOKS),
      'shared/textures/gen.mjs',
      'shared/models/gen.mjs',
      'shared/characters/gen.mjs',
    ],
    nodeTests: [
      ...lookTests(RENDER_LOOKS),
      'shared/textures/gen.test.mjs',
      'shared/models/gen.test.mjs',
      'shared/models/painter.test.mjs',
      'shared/characters/gen.test.mjs',
    ],
    crates: ['dwell-look', 'dwell-models', 'dwell-characters', 'dwell-client', 'dwell-native'],
    vitest: [
      'src/engine/',
      'src/ui/graphicsPresets.test.ts',
      'src/ui/shadowSettings.test.ts',
      'src/ui/antialiasSettings.test.ts',
      'src/ui/renderScaleSettings.test.ts',
      'src/ui/modelDetailSettings.test.ts',
    ],
    extra: [
      {
        argv: [
          'cargo',
          'clippy',
          '-p',
          'dwell-client',
          '--target',
          'wasm32-unknown-unknown',
          '--',
          '-D',
          'warnings',
        ],
      },
    ],
  },
  sim: {
    notes: 'the server, physics and the player controller (both faces), creatures, storage',
    generators: [
      'shared/physics/gen.mjs',
      'shared/creatures/gen.mjs',
      'shared/blueprints/gen.mjs',
      'shared/explosives/gen.mjs',
      'shared/fluids/gen.mjs',
      'shared/fluids/falls.mjs',
      'shared/fluids/lava.mjs',
      'shared/ecology/gen.mjs',
    ],
    nodeTests: [
      'shared/physics/gen.test.mjs',
      'shared/creatures/gen.test.mjs',
      'shared/blueprints/gen.test.mjs',
      'shared/explosives/gen.test.mjs',
      'shared/fluids/gen.test.mjs',
      'shared/fluids/falls.test.mjs',
      'shared/fluids/lava.test.mjs',
      'shared/ecology/gen.test.mjs',
    ],
    crates: [
      'dwell-sim',
      'dwell-physics',
      'dwell-world',
      'dwell-storage',
      'dwell-server',
      'dwell-lava',
      'dwell-ecology',
    ],
    vitest: ['src/local/', 'src/predict/'],
    // CLAUDE.md "both faces": controller changes pass the mirror suite too.
    extra: [{ argv: ['cargo', 'test', '-p', 'dwell-physics'], env: { DWELL_FACE: 'b' } }],
  },
  ui: {
    notes: 'the DOM shell: HUD, menus, settings, input and gamepad, the client game logic',
    generators: [
      'shared/ui/gen.mjs',
      'shared/play/gen.mjs',
      'shared/input/gen.mjs',
      'shared/modes/gen.mjs',
      'shared/inventory/gen.mjs',
      'shared/vitals/gen.mjs',
      'shared/tools/gen.mjs',
      'shared/items/gen.mjs',
      'shared/brushes/gen.mjs',
      ...look(UI_LOOKS),
    ],
    nodeTests: [
      'shared/ui/gen.test.mjs',
      'shared/play/gen.test.mjs',
      'shared/input/gen.test.mjs',
      'shared/modes/gen.test.mjs',
      'shared/inventory/gen.test.mjs',
      'shared/vitals/gen.test.mjs',
      'shared/tools/gen.test.mjs',
      'shared/items/gen.test.mjs',
      ...lookTests(UI_LOOKS),
    ],
    crates: ['dwell-game', 'dwell-theater'],
    vitest: ['src/ui/', 'src/input/', 'src/debug/', 'src/identity/', 'src/theater/'],
  },
  audio: {
    notes: 'sounds and music (data, validation, the synth and mixer)',
    generators: ['shared/sounds/gen.mjs', 'shared/music/gen.mjs'],
    nodeTests: [],
    crates: ['dwell-game'],
    vitest: ['src/audio/'],
  },
  tools: {
    notes: 'release, site, launcher and bench scripts (no crates or generators)',
    generators: [],
    nodeTests: [],
    crates: [],
    vitest: [
      'scripts/',
      'launcher/',
      'src/version/',
      'src/buildInfo.test.ts',
      'src/workerCount.test.ts',
    ],
  },
};

export interface Step {
  /** `gen` steps run between a snapshot of the work tree and a comparison with it. */
  kind: 'gen' | 'cmd';
  /** Directory, relative to the repo root. */
  cwd: '.' | 'client';
  argv: string[];
  env?: Record<string, string>;
}

const uniq = (xs: string[]) => [...new Set(xs)];

function genArgv(path: string): string[] {
  return path.endsWith('.py') ? ['python3', path] : ['node', path];
}

/**
 * The steps for `area`, or for every area with `all` (the workspace-wide forms: `--workspace`,
 * the whole Vitest suite). Generators first (cheap, and the crates read what they emit), then
 * node tests, Rust, Vitest.
 */
export function plan(area: string, opts: { nextest?: boolean } = {}): Step[] {
  const all = area === 'all';
  const areas = all ? Object.values(AREAS) : [AREAS[area]];
  if (!areas[0]) {
    throw new Error(`unknown area "${area}"; one of: ${[...Object.keys(AREAS), 'all'].join(', ')}`);
  }
  const generators = uniq(areas.flatMap((a) => a.generators));
  const nodeTests = uniq(areas.flatMap((a) => a.nodeTests));
  const crates = uniq(areas.flatMap((a) => a.crates));
  const vitest = uniq(areas.flatMap((a) => a.vitest));
  // `--all-features` for a few crates: CI's `--workspace` build unifies features (dwell-blocks'
  // `fixture`, which other crates' dev-dependencies enable), so `-p` alone would skip tests CI runs.
  const pkgs = all ? ['--workspace'] : [...crates.flatMap((c) => ['-p', c]), '--all-features'];

  const steps: Step[] = generators.map((g) => ({ kind: 'gen', cwd: '.', argv: genArgv(g) }));
  if (nodeTests.length) {
    steps.push({ kind: 'cmd', cwd: '.', argv: ['node', '--test', ...nodeTests] });
  }
  if (crates.length) {
    const fmtPkgs = all ? ['--all'] : crates.flatMap((c) => ['-p', c]);
    steps.push({ kind: 'cmd', cwd: '.', argv: ['cargo', 'fmt', ...fmtPkgs, '--check'] });
    steps.push({
      kind: 'cmd',
      cwd: '.',
      argv: ['cargo', 'clippy', ...pkgs, '--all-targets', '--', '-D', 'warnings'],
    });
    steps.push({
      kind: 'cmd',
      cwd: '.',
      argv: opts.nextest ? ['cargo', 'nextest', 'run', ...pkgs] : ['cargo', 'test', ...pkgs],
    });
  }
  for (const a of areas) {
    for (const e of a.extra ?? []) {
      const step: Step = { kind: 'cmd', cwd: '.', argv: e.argv };
      if (e.env) step.env = e.env;
      if (!steps.some((s) => JSON.stringify(s) === JSON.stringify(step))) steps.push(step);
    }
  }
  if (vitest.length) {
    steps.push({
      kind: 'cmd',
      cwd: 'client',
      argv: ['npx', 'vitest', 'run', ...(all ? [] : vitest)],
    });
  }
  return steps;
}

/** A step as a shell line, for `--dry-run` and the log. */
export function describeStep(s: Step): string {
  const env = Object.entries(s.env ?? {}).map(([k, v]) => `${k}=${v} `);
  const dir = s.cwd === '.' ? '' : `(cd ${s.cwd}) `;
  return `${dir}${env.join('')}${s.argv.join(' ')}`;
}
