// Starts a native dwell_server for the suite and records its invite query for the tests, and a
// local master server (`wrangler dev`, services/master) for friend worlds (Phase 5c).
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export const INVITE_FILE = fileURLToPath(new URL('../test-results/invite.txt', import.meta.url));
/** The server's world file, fresh for every run (§6.4). */
export const WORLD_FILE = fileURLToPath(new URL('../test-results/e2e.dwellworld', import.meta.url));

/** The local master's URL; pages use it through `?master=`. */
export const MASTER_URL = 'http://localhost:8787';

/** Starts `wrangler dev` for the master and waits until it answers. */
async function startMaster(): Promise<ChildProcess> {
  const dir = fileURLToPath(new URL('../../services/master/', import.meta.url));
  const master = spawn('npx', ['wrangler', 'dev', '--port', '8787', '--ip', '127.0.0.1'], {
    cwd: dir,
    stdio: ['ignore', 'ignore', 'inherit'],
    env: { ...process.env, WRANGLER_SEND_METRICS: 'false' },
    detached: true,
  });
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    if (master.exitCode !== null) throw new Error('wrangler dev exited');
    try {
      if ((await fetch(`${MASTER_URL}/v1/health`)).ok) return master;
    } catch {
      // not up yet
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error('the local master did not start within 60 s');
}

export default async function globalSetup(): Promise<() => void> {
  const bin =
    process.env.DWELL_SERVER_BIN ??
    fileURLToPath(new URL('../../server/build/dev/app/dwell_server', import.meta.url));
  mkdirSync(dirname(WORLD_FILE), { recursive: true });
  for (const suffix of ['', '-wal', '-shm']) rmSync(WORLD_FILE + suffix, { force: true });
  const server = spawn(bin, ['--port', '0', '--name', 'E2E', '--world', WORLD_FILE], {
    stdio: ['ignore', 'pipe', 'inherit'],
  });

  let master: ChildProcess | null = null;
  const stop = () => {
    server.kill();
    // npx starts wrangler, which starts workerd: end the whole group.
    if (master?.pid) {
      try {
        process.kill(-master.pid);
      } catch {
        // already gone
      }
    }
  };
  const query = await new Promise<string>((resolve, reject) => {
    let out = '';
    const timer = setTimeout(() => {
      reject(new Error(`dwell_server did not print an invite link:\n${out}`));
    }, 10_000);
    server.stdout.on('data', (chunk: Buffer) => {
      out += chunk.toString();
      const m = /invite link: \S+?(\?\S+)/.exec(out);
      if (m?.[1]) {
        clearTimeout(timer);
        resolve(m[1]);
      }
    });
    server.on('exit', (code) => {
      reject(new Error(`dwell_server exited (${String(code)}):\n${out}`));
    });
  }).catch((err: unknown) => {
    stop();
    throw err;
  });
  mkdirSync(dirname(INVITE_FILE), { recursive: true });
  writeFileSync(INVITE_FILE, query);
  try {
    master = await startMaster();
  } catch (err) {
    stop();
    throw err;
  }
  return stop;
}
