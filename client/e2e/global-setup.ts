// Starts a native dwell_server for the suite and records its invite query for the tests.
import { spawn } from 'node:child_process';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export const INVITE_FILE = fileURLToPath(new URL('../test-results/invite.txt', import.meta.url));
/** The server's world file, fresh for every run (§6.4). */
export const WORLD_FILE = fileURLToPath(new URL('../test-results/e2e.dwellworld', import.meta.url));

export default async function globalSetup(): Promise<() => void> {
  const bin =
    process.env.DWELL_SERVER_BIN ??
    fileURLToPath(new URL('../../server/build/dev/app/dwell_server', import.meta.url));
  mkdirSync(dirname(WORLD_FILE), { recursive: true });
  for (const suffix of ['', '-wal', '-shm']) rmSync(WORLD_FILE + suffix, { force: true });
  const server = spawn(bin, ['--port', '0', '--name', 'E2E', '--world', WORLD_FILE], {
    stdio: ['ignore', 'pipe', 'inherit'],
  });

  const stop = () => {
    server.kill();
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
  return stop;
}
