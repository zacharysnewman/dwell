// The warm prebuilt client for the bench (docs/DEV_TOOLS.md "The warm client"): builds the WASM
// module and the client once into a cache keyed by the git tree hash of the sources the build
// reads, reuses it while they are unchanged (a docs- or bench-only change doesn't rebuild), and
// keeps a long-lived `vite preview` of it on a fixed port that later runs and agents reuse.
//   node --experimental-strip-types scripts/warmClient.ts           build if stale, serve, print the URL
//   node --experimental-strip-types scripts/warmClient.ts --status  the cache key, built or not, the server
//   node --experimental-strip-types scripts/warmClient.ts --stop    stops the preview server
// The cache is `$DWELL_BENCH_CACHE`, else `<main checkout>/target/bench-client` (shared by every
// worktree, like the warm `CARGO_TARGET_DIR`): `<key>/dist` per build, the newest few kept, and
// `preview.json` the running server's pid, port and key.
import { execFileSync, spawn } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { CACHE_KEEP, SOURCE_PATHSPECS, staleEntries, WARM_PORT } from './warmKey.ts';

const client = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repo = resolve(client, '..');

function git(args: string[], env: NodeJS.ProcessEnv = process.env): string {
  return execFileSync('git', args, { cwd: repo, env, encoding: 'utf8' }).trim();
}

/** The main checkout (a worktree's shares its `.git`): where the shared cache lives. */
function mainCheckout(): string {
  return dirname(resolve(repo, git(['rev-parse', '--git-common-dir'])));
}

export function cacheRoot(): string {
  return process.env.DWELL_BENCH_CACHE ?? join(mainCheckout(), 'target', 'bench-client');
}

/**
 * The git tree hash of the build's sources as they are on disk (committed or not, untracked
 * included, ignored files not): `git add` into a scratch index that starts empty, then
 * `write-tree`. The real index is untouched.
 */
export function sourceKey(): string {
  const index = join(tmpdir(), `dwell-warm-index-${String(process.pid)}`);
  const env = { ...process.env, GIT_INDEX_FILE: index };
  try {
    git(['read-tree', '--empty'], env);
    git(['add', '-A', '--', ...SOURCE_PATHSPECS], env);
    return git(['write-tree'], env);
  } finally {
    rmSync(index, { force: true });
  }
}

function targetDir(): string {
  return process.env.CARGO_TARGET_DIR ?? join(mainCheckout(), 'target');
}

/** Builds the WASM module and the client into `<cache>/<key>/dist` (seconds to minutes). */
function build(key: string): string {
  const out = join(cacheRoot(), key);
  const dist = join(out, 'dist');
  const env = { ...process.env, CARGO_TARGET_DIR: targetDir() };
  const run = (cmd: string, args: string[], cwd: string): void => {
    execFileSync(cmd, args, { cwd, env, stdio: 'inherit' });
  };
  console.log(`warm client: building ${key.slice(0, 12)} → ${dist}`);
  run(
    'cargo',
    ['build', '-p', 'dwell-client', '--target', 'wasm32-unknown-unknown', '--release'],
    repo,
  );
  run(
    'wasm-bindgen',
    [
      '--target',
      'web',
      '--out-dir',
      join(client, 'src', 'pkg'),
      join(targetDir(), 'wasm32-unknown-unknown', 'release', 'dwell_client.wasm'),
    ],
    repo,
  );
  mkdirSync(out, { recursive: true });
  run('npx', ['vite', 'build', '--outDir', dist, '--emptyOutDir'], client);
  writeFileSync(
    join(out, 'built.json'),
    `${JSON.stringify({ key, at: new Date().toISOString(), from: repo })}\n`,
  );
  // The newest few builds stay; older ones go (absolute paths under the cache only).
  const root = cacheRoot();
  const entries = readdirSync(root)
    .filter((n) => existsSync(join(root, n, 'built.json')))
    .map((n) => ({ name: n, mtimeMs: statSync(join(root, n, 'built.json')).mtimeMs }));
  for (const n of staleEntries(entries, CACHE_KEEP, key))
    rmSync(join(root, n), { recursive: true, force: true });
  return dist;
}

/** The cached build for the sources as they are, built first if there is none. */
export function warmDist(): { key: string; dist: string; built: boolean } {
  const key = sourceKey();
  const dist = join(cacheRoot(), key, 'dist');
  if (existsSync(join(cacheRoot(), key, 'built.json'))) return { key, dist, built: false };
  return { key, dist: build(key), built: true };
}

interface PreviewState {
  pid: number;
  port: number;
  key: string;
}

function previewState(): PreviewState | null {
  try {
    return JSON.parse(readFileSync(join(cacheRoot(), 'preview.json'), 'utf8')) as PreviewState;
  } catch {
    return null;
  }
}

function alive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function reachable(url: string): Promise<boolean> {
  try {
    return (await fetch(url)).ok;
  } catch {
    return false;
  }
}

export function stopPreview(): boolean {
  const s = previewState();
  rmSync(join(cacheRoot(), 'preview.json'), { force: true });
  if (!s || !alive(s.pid)) return false;
  // The server was started in its own process group: stop npx and vite together.
  try {
    process.kill(-s.pid, 'SIGTERM');
  } catch {
    process.kill(s.pid, 'SIGTERM');
  }
  return true;
}

/**
 * The warm client served: the cached build (built if stale) behind the long-lived preview on
 * `WARM_PORT`, reused when it already serves this key, restarted on a new one. Returns the
 * server's root URL for the bench (`server` null: the bench must not stop it).
 */
export async function warmServe(): Promise<{ url: string; server: null }> {
  const t0 = Date.now();
  const { key, dist, built } = warmDist();
  const url = `http://localhost:${String(WARM_PORT)}`;
  const s = previewState();
  if (s && s.key === key && alive(s.pid) && (await reachable(`${url}/dwell/`))) {
    console.log(
      `warm client: ${key.slice(0, 12)} reused (${built ? 'built' : 'cached'}, served on :${String(WARM_PORT)}) in ${((Date.now() - t0) / 1000).toFixed(1)} s`,
    );
    return { url, server: null };
  }
  stopPreview();
  const child = spawn(
    'npx',
    ['vite', 'preview', '--outDir', dist, '--port', String(WARM_PORT), '--strictPort'],
    {
      cwd: client,
      detached: true,
      stdio: 'ignore',
    },
  );
  child.unref();
  mkdirSync(cacheRoot(), { recursive: true });
  writeFileSync(
    join(cacheRoot(), 'preview.json'),
    `${JSON.stringify({ pid: child.pid, port: WARM_PORT, key })}\n`,
  );
  for (let i = 0; i < 150 && !(await reachable(`${url}/dwell/`)); i++) {
    await new Promise((r) => setTimeout(r, 200));
  }
  if (!(await reachable(`${url}/dwell/`)))
    throw new Error(`warm client: the preview on :${String(WARM_PORT)} did not start`);
  console.log(
    `warm client: ${key.slice(0, 12)} ${built ? 'built' : 'cached'}, preview started on :${String(WARM_PORT)} in ${((Date.now() - t0) / 1000).toFixed(1)} s (stop: npm run bench:warm -- --stop)`,
  );
  return { url, server: null };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const arg = process.argv[2];
  if (arg === '--stop') {
    console.log(stopPreview() ? 'warm client: preview stopped' : 'warm client: no preview running');
  } else if (arg === '--status') {
    const key = sourceKey();
    const s = previewState();
    console.log(
      JSON.stringify(
        {
          key,
          cache: cacheRoot(),
          built: existsSync(join(cacheRoot(), key, 'built.json')),
          preview: s ? { ...s, alive: alive(s.pid), current: s.key === key } : null,
        },
        null,
        2,
      ),
    );
  } else {
    const { url } = await warmServe();
    console.log(`${url}/dwell/`);
  }
}
