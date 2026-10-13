// The WebGPU probe (Phase 24a, ADR 0028): the Rust client's wgpu probe in headless Chromium.
// Prerequisite: the probe built into crates/dwell-client/www/pkg (`npm run build:probe`).
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: 'e2e-webgpu',
  timeout: 30_000,
  workers: 1,
  reporter: [['list']],
  webServer: {
    command: 'python3 -m http.server 4190 --bind 127.0.0.1 --directory ../crates/dwell-client/www',
    url: 'http://127.0.0.1:4190/',
    reuseExistingServer: !process.env.CI,
  },
  use: {
    baseURL: 'http://127.0.0.1:4190/',
    launchOptions: {
      ...(process.env.DWELL_CHROMIUM ? { executablePath: process.env.DWELL_CHROMIUM } : {}),
      // WebGPU in headless Chromium without a GPU: SwiftShader's Vulkan as the adapter.
      args: [
        '--enable-unsafe-webgpu',
        '--enable-features=Vulkan',
        '--use-vulkan=swiftshader',
        '--use-webgpu-adapter=swiftshader',
        '--no-proxy-server',
      ],
    },
  },
});
