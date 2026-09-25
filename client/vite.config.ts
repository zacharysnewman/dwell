import { execSync } from 'node:child_process';
import { defineConfig } from 'vitest/config';

// Commit shown in the build-info overlay: CI provides GITHUB_SHA; locally ask git.
function buildSha(): string {
  if (process.env.GITHUB_SHA) return process.env.GITHUB_SHA;
  try {
    return execSync('git rev-parse HEAD', { stdio: ['ignore', 'pipe', 'ignore'] })
      .toString()
      .trim();
  } catch {
    return 'unknown';
  }
}

export default defineConfig({
  // Served at https://dropkickarcade.com/dwell/ (ADR 0005).
  base: '/dwell/',
  define: {
    __BUILD_SHA__: JSON.stringify(buildSha()),
    __BUILD_TIME__: JSON.stringify(new Date().toISOString()),
  },
  build: {
    target: 'es2022',
    sourcemap: true,
    // Three.js's WebGLRenderer alone is ~500 kB minified (~130 kB gzip).
    chunkSizeWarningLimit: 700,
  },
  test: {
    environment: 'node',
  },
});
