// End-to-end tests (Phase 1 exit criteria): a real native server and the built client in Chromium.
// Prerequisites: `npm run build:wasm && npm run build` here, `cargo build --release -p dwell-server`
// (or DWELL_SERVER_BIN pointing at a dwell-server), and `npm ci` in services/master (the local
// master for friend worlds). WebGPU: headless Chromium's software adapter loses its device when a
// canvas presents, so the browser runs headed (under Xvfb on CI: `xvfb-run npx playwright test`).
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
    // No clouds: on the software adapter their per-pixel noise slows every frame (to about one a
    // second with two players), and the suite's timings assume the frame rate it had without them.
    // The clouds have their own checks (render tests, the bench's screenshots).
    // No loading screen on joining (ARCHITECTURE.md §6.3): the suite waits for the terrain itself
    // (`terrainReady`), and on the software adapter the near ring takes far longer to mesh than a
    // test needs; `join.spec.ts` turns it back on for its own check.
    storageState: {
      cookies: [],
      origins: [
        {
          origin: 'http://localhost:4173',
          localStorage: [
            { name: 'dwell.clouds', value: '"off"' },
            { name: 'dwell.loading', value: '"off"' },
            // No music (docs/AUDIO.md §6a): no streaming, decoding or audio noise in the suite.
            { name: 'dwell.music', value: '"off"' },
          ],
        },
      ],
    },
    // Headed: see above. The software adapter (SwiftShader's Vulkan) draws WebGPU without a GPU.
    headless: false,
    launchOptions: {
      // Local sandboxes may provide a preinstalled Chromium; CI uses `playwright install`.
      ...(process.env.DWELL_CHROMIUM ? { executablePath: process.env.DWELL_CHROMIUM } : {}),
      // Two players run side by side: keep background pages' timers and frames running.
      args: [
        '--enable-unsafe-webgpu',
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
