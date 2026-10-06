// The launcher's end-to-end tests (RELEASES.md §9): a locally assembled site with three versions,
// served at /dwell/ as GitHub Pages serves it. No game server or WASM core is needed: the tests
// check which build each address opens and what its menu shows.
// Prerequisite: `npm run site:fixture` (builds dist-site/); `npm run e2e:site` does both.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'e2e',
  testMatch: 'launcher.spec.ts',
  timeout: 30_000,
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  webServer: {
    command: 'npx vite preview --outDir dist-site --base /dwell/ --port 4174 --strictPort',
    url: 'http://localhost:4174/dwell/',
    reuseExistingServer: !process.env.CI,
  },
  use: {
    baseURL: 'http://localhost:4174/dwell/',
    launchOptions: {
      ...(process.env.DWELL_CHROMIUM ? { executablePath: process.env.DWELL_CHROMIUM } : {}),
      args: ['--enable-unsafe-swiftshader', '--no-proxy-server'],
    },
  },
});
