// Starts a local master server (`wrangler dev`, services/master) for friend worlds (Phase 5c) and
// dedicated servers (5d), then a native dwell_server registered with it, and records the server's
// invite query and join code for the tests.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export const INVITE_FILE = fileURLToPath(new URL('../test-results/invite.txt', import.meta.url));
/** The suite server's join code from the master (Phase 5d). */
export const CODE_FILE = fileURLToPath(new URL('../test-results/code.txt', import.meta.url));
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

/** The dwell_server binary the suite runs. */
export function serverBin(): string {
  return (
    process.env.DWELL_SERVER_BIN ??
    fileURLToPath(new URL('../../server/build/dev/app/dwell_server', import.meta.url))
  );
}

export interface StartedServer {
  process: ChildProcess;
  /** The invite link's query ("?join=…"). */
  query: string;
  /** Its join code from the master ("KQ7-XM4"). */
  code: string;
}

/**
 * Starts a dwell_server registered with the local master (2 s heartbeats, Phase 5d) and waits
 * for its invite link and join code.
 */
export async function startServer(args: string[]): Promise<StartedServer> {
  const server = spawn(serverBin(), [...args, '--master', MASTER_URL, '--heartbeat', '2'], {
    stdio: ['ignore', 'pipe', 'inherit'],
  });
  try {
    return await new Promise<StartedServer>((resolve, reject) => {
      let out = '';
      const timer = setTimeout(() => {
        reject(new Error(`dwell_server did not print an invite link and a join code:\n${out}`));
      }, 15_000);
      server.stdout.on('data', (chunk: Buffer) => {
        out += chunk.toString();
        const invite = /invite link: \S+?(\?\S+)/.exec(out);
        const code = /join code: (\S+)/.exec(out);
        if (invite?.[1] && code?.[1]) {
          clearTimeout(timer);
          resolve({ process: server, query: invite[1], code: code[1] });
        }
      });
      server.on('exit', (exit) => {
        reject(new Error(`dwell_server exited (${String(exit)}):\n${out}`));
      });
    });
  } catch (err) {
    server.kill();
    throw err;
  }
}

export default async function globalSetup(): Promise<() => void> {
  mkdirSync(dirname(WORLD_FILE), { recursive: true });
  for (const suffix of ['', '-wal', '-shm']) rmSync(WORLD_FILE + suffix, { force: true });
  // The master first: the suite's server registers with it (Phase 5d).
  const master = await startMaster();
  const stopMaster = () => {
    // npx starts wrangler, which starts workerd: end the whole group.
    if (master.pid) {
      try {
        process.kill(-master.pid);
      } catch {
        // already gone
      }
    }
  };
  let server: StartedServer;
  try {
    server = await startServer(['--port', '0', '--name', 'E2E', '--world', WORLD_FILE]);
  } catch (err) {
    stopMaster();
    throw err;
  }
  writeFileSync(INVITE_FILE, server.query);
  writeFileSync(CODE_FILE, server.code);
  return () => {
    server.process.kill();
    stopMaster();
  };
}
