// `npm run check:<area>` (docs/AGENT_QUICKSTART.md): runs one area's generators (failing when
// their checked-in output was stale), node tests over the data, Rust fmt/clippy/tests for its
// crates and its Vitest files. The areas are `checks.ts`'s table.
//
//   node --experimental-strip-types scripts/check.ts <area|all> [--dry-run]
//   node --experimental-strip-types scripts/check.ts --list
//
// Generated files are checked the way CI does (regenerate, then compare): the work tree's diff
// and untracked files are hashed before and after the generators, so uncommitted work of
// your own is fine; a generator that changes anything fails the run (commit what it wrote).

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { AREAS, describeStep, plan, type Step } from './checks.ts';

const ROOT = fileURLToPath(new URL('../..', import.meta.url));

function git(args: string[]): string {
  const r = spawnSync('git', args, { cwd: ROOT, encoding: 'utf8', maxBuffer: 1 << 30 });
  if (r.status !== 0) throw new Error(`git ${args.join(' ')} failed: ${r.stderr}`);
  return r.stdout;
}

/** Each file that differs from HEAD (changed, deleted or untracked), with its content's hash. */
function snapshot(): Map<string, string> {
  const files = [
    ...git(['diff', 'HEAD', '--name-only']).split('\n'),
    ...git(['ls-files', '--others', '--exclude-standard']).split('\n'),
  ].filter(Boolean);
  const out = new Map<string, string>();
  for (const f of files) {
    const p = `${ROOT}/${f}`;
    out.set(f, existsSync(p) ? createHash('sha256').update(readFileSync(p)).digest('hex') : '-');
  }
  return out;
}

/** The files whose state differs between two snapshots. */
function changed(a: Map<string, string>, b: Map<string, string>): string[] {
  return [...new Set([...a.keys(), ...b.keys()])].filter((f) => a.get(f) !== b.get(f)).sort();
}

function hasNextest(): boolean {
  return spawnSync('cargo', ['nextest', '--version'], { stdio: 'ignore' }).status === 0;
}

function run(s: Step): boolean {
  console.log(`\n▶ ${describeStep(s)}`);
  const r = spawnSync(s.argv[0], s.argv.slice(1), {
    cwd: s.cwd === '.' ? ROOT : `${ROOT}/${s.cwd}`,
    env: { ...process.env, ...s.env },
    stdio: 'inherit',
  });
  return r.status === 0;
}

function main(): number {
  const args = process.argv.slice(2);
  if (args.includes('--list') || args.length === 0) {
    for (const [name, a] of Object.entries(AREAS)) console.log(`${name.padEnd(9)} ${a.notes}`);
    console.log(`${'all'.padEnd(9)} every area, workspace-wide (as CI, minus e2e and WASM tests)`);
    return args.length === 0 ? 1 : 0;
  }
  const area = args.find((a) => !a.startsWith('--')) ?? '';
  const dry = args.includes('--dry-run');
  const steps = plan(area, { nextest: !dry && hasNextest() });
  if (dry) {
    for (const s of steps) console.log(`${s.kind === 'gen' ? '[gen] ' : ''}${describeStep(s)}`);
    return 0;
  }

  const failed: string[] = [];
  const t0 = Date.now();
  const gens = steps.filter((s) => s.kind === 'gen');
  if (gens.length) {
    const before = snapshot();
    for (const s of gens) if (!run(s)) failed.push(describeStep(s));
    const stale = changed(before, snapshot());
    if (stale.length) {
      console.log(
        `\n✗ generated files were stale; the generators rewrote:\n  ${stale.join('\n  ')}`,
      );
      failed.push('generated files up to date (commit what the generators wrote)');
    }
  }
  for (const s of steps.filter((s) => s.kind === 'cmd')) if (!run(s)) failed.push(describeStep(s));

  const secs = ((Date.now() - t0) / 1000).toFixed(0);
  if (failed.length) {
    console.log(`\n✗ check:${area} failed in ${secs} s:\n  ${failed.join('\n  ')}`);
    return 1;
  }
  console.log(`\n✓ check:${area} passed in ${secs} s`);
  return 0;
}

process.exit(main());
