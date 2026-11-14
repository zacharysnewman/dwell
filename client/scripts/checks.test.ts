import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { AREAS, describeStep, plan } from './checks.ts';

const root = (p: string) => new URL(`../../${p}`, import.meta.url);
const areas = Object.values(AREAS);
const ci = readFileSync(root('.github/workflows/ci.yml'), 'utf8');

/** Every `*.test.ts` Vitest runs (vite.config.ts `include`), relative to client/. */
function testFiles(dir: string): string[] {
  const out: string[] = [];
  for (const e of readdirSync(root(`client/${dir}`), { withFileTypes: true })) {
    const p = `${dir}/${e.name}`;
    if (e.isDirectory()) out.push(...testFiles(p));
    else if (e.name.endsWith('.test.ts')) out.push(p);
  }
  return out;
}

describe('per-area checks (check:<area>)', () => {
  it('puts every shared/*/gen.mjs in at least one area', () => {
    const gens = readdirSync(root('shared'))
      .map((d) => `shared/${d}/gen.mjs`)
      .filter((p) => existsSync(root(p)));
    expect(gens.length).toBeGreaterThan(20);
    const owned = new Set(areas.flatMap((a) => a.generators));
    expect(gens.filter((g) => !owned.has(g))).toEqual([]);
  });

  it('puts every generator and node test that CI runs in at least one area', () => {
    // Read from the workflow, so a generator added to CI without an area fails here.
    const gens = [...ci.matchAll(/^\s+(?:node|python3) (shared\/\S+\.(?:mjs|py))$/gm)].map(
      (m) => m[1],
    );
    const tests = [...ci.matchAll(/node --test (shared\/\S+\.test\.mjs)/g)].map((m) => m[1]);
    expect(gens.length).toBeGreaterThan(40);
    expect(tests.length).toBeGreaterThan(40);
    const ownedGens = new Set(areas.flatMap((a) => a.generators));
    const ownedTests = new Set(areas.flatMap((a) => a.nodeTests));
    expect(gens.filter((g) => !ownedGens.has(g))).toEqual([]);
    expect(tests.filter((t) => !ownedTests.has(t))).toEqual([]);
  });

  it('puts every workspace crate in at least one area', () => {
    const crates = readdirSync(root('crates'), { withFileTypes: true })
      .filter((e) => e.isDirectory() && existsSync(root(`crates/${e.name}/Cargo.toml`)))
      .map((e) => e.name);
    expect(crates.length).toBeGreaterThan(10);
    const owned = new Set(areas.flatMap((a) => a.crates));
    expect(crates.filter((c) => !owned.has(c))).toEqual([]);
    // And names no crate that does not exist.
    expect([...owned].filter((c) => !crates.includes(c))).toEqual([]);
  });

  it('puts every client test file in at least one area', () => {
    const files = [...testFiles('src'), ...testFiles('launcher'), ...testFiles('scripts')];
    const filters = areas.flatMap((a) => a.vitest);
    expect(files.filter((f) => !filters.some((v) => f.includes(v)))).toEqual([]);
    // Every filter matches something (a renamed test does not leave a dead filter behind).
    expect(filters.filter((v) => !files.some((f) => f.includes(v)))).toEqual([]);
  });

  it('names only paths that exist', () => {
    const paths = areas.flatMap((a) => [...a.generators, ...a.nodeTests]);
    expect(paths.filter((p) => !existsSync(root(p)))).toEqual([]);
  });

  it('has an npm script for every area and for all', () => {
    const pkg = JSON.parse(readFileSync(root('client/package.json'), 'utf8')) as {
      scripts: Record<string, string>;
    };
    for (const name of [...Object.keys(AREAS), 'all']) {
      expect(pkg.scripts[`check:${name}`]).toBe(
        `node --experimental-strip-types scripts/check.ts ${name}`,
      );
    }
  });

  it("plans an area's generators first, then its tests, crates and client files", () => {
    const steps = plan('data').map(describeStep);
    expect(steps[0]).toBe('node shared/blocks/gen.mjs');
    expect(steps).toContain(
      'cargo clippy -p dwell-blocks --all-features --all-targets -- -D warnings',
    );
    expect(steps).toContain('cargo test -p dwell-blocks --all-features');
    expect(steps.at(-1)).toMatch(/^\(cd client\) npx vitest run src\/world\//);
    const nodeTest = steps.find((s) => s.startsWith('node --test'));
    expect(nodeTest).toContain('shared/blocks/faces.test.mjs');
  });

  it('runs the controller mirror suite with sim, and nextest when asked', () => {
    const steps = plan('sim', { nextest: true }).map(describeStep);
    expect(steps).toContain('DWELL_FACE=b cargo test -p dwell-physics');
    expect(steps.some((s) => s.startsWith('cargo nextest run -p dwell-sim'))).toBe(true);
  });

  it('plans all workspace-wide, each generator once', () => {
    const steps = plan('all');
    const gens = steps.filter((s) => s.kind === 'gen').map((s) => s.argv[1]);
    expect(new Set(gens).size).toBe(gens.length);
    const lines = steps.map(describeStep);
    expect(lines).toContain('cargo test --workspace');
    expect(lines).toContain('cargo fmt --all --check');
    expect(lines.at(-1)).toBe('(cd client) npx vitest run');
  });

  it('rejects an unknown area by name', () => {
    expect(() => plan('nope')).toThrow(/unknown area "nope"; one of: protocol, data/);
  });
});
