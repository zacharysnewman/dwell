// End-to-end tests (Phase 1 exit criteria): a real native server and the built client in Chromium.
// Prerequisites: `npm run build:wasm && npm run build` here, the server built with the `dev`
// CMake preset (or DWELL_SERVER_BIN pointing at dwell_server), and `npm ci` in services/master
// (the local master for friend worlds).
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'e2e',
  // The launcher's tests have their own config and site (playwright.site.config.ts).
  testIgnore: 'launcher.spec.ts',
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
        // Friend worlds (Phase 5c): two contexts on one machine connect by their real host
        // addresses, not mDNS names that the runner may not resolve.
        '--disable-features=WebRtcHideLocalIpsWithMdns',
        // The suite only talks to local servers: never through a proxy from the environment (one
        // can't carry WebTransport to a LAN address, Phase 5d).
        '--no-proxy-server',
      ],
    },
  },
});
