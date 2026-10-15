import { execSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';

// The launcher (RELEASES.md §5), built on its own into dist-launcher/ and put at the site's root
// by the Pages workflow. It is served at https://dropkickarcade.com/dwell/ (ADR 0005). It shows the
// main menu (src/ui/menuPage.ts), whose modules read the build constants: the launcher's own.
function sha(): string {
  if (process.env.GITHUB_SHA) return process.env.GITHUB_SHA;
  try {
    return execSync('git rev-parse HEAD', { stdio: ['ignore', 'pipe', 'ignore'] })
      .toString()
      .trim();
  } catch {
    return 'unknown';
  }
}
const version = (JSON.parse(readFileSync('package.json', 'utf8')) as { version: string }).version;

export default defineConfig({
  root: 'launcher',
  base: '/dwell/',
  define: {
    __BUILD_SHA__: JSON.stringify(sha()),
    __BUILD_TIME__: JSON.stringify(new Date().toISOString()),
    __APP_VERSION__: JSON.stringify(version),
  },
  build: {
    outDir: '../dist-launcher',
    emptyOutDir: true,
    target: 'es2022',
  },
});
