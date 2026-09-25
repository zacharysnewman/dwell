// End-to-end tests (Phase 1 exit criteria): a real native server and the built client in Chromium.
// Prerequisites: `npm run build:wasm && npm run build` here, and the server built with the `dev`
// CMake preset (or DWELL_SERVER_BIN pointing at dwell_server).
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'e2e',
  timeout: 30_000,
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  globalSetup: './e2e/global-setup.ts',
  webServer: {
    command: 'npx vite preview --port 4173 --strictPort',
    url: 'http://localhost:4173/dwell/',
    reuseExistingServer: !process.env.CI,
  },
  use: {
    baseURL: 'http://localhost:4173/dwell/',
    launchOptions: {
      // Local sandboxes may provide a preinstalled Chromium; CI uses `playwright install`.
      ...(process.env.DWELL_CHROMIUM ? { executablePath: process.env.DWELL_CHROMIUM } : {}),
      // Two players run side by side: keep background pages' timers and frames running.
      args: [
        '--enable-unsafe-swiftshader',
        '--disable-background-timer-throttling',
        '--disable-renderer-backgrounding',
        '--disable-backgrounding-occluded-windows',
      ],
    },
  },
});
