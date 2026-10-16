import { execSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitest/config';
import { GENERATORS } from './src/local/world.ts';
import { isStable, parseVersion, withoutBuild, type Version } from './src/version/semver.ts';

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

/**
 * The app version this build carries (RELEASES.md §3). `package.json` holds the version the next
 * release will have; the release workflow passes the exact version as DWELL_VERSION (`0.1.0` for a
 * stable release, `0.1.0-dev.42+ab12cd3` for a dev build). Without it this is a local build, a
 * pre-release of the next version: `<version>-dev.local+<sha>`.
 */
function appVersion(sha: string): string {
  const fromEnv = process.env.DWELL_VERSION;
  const version =
    fromEnv ??
    `${(JSON.parse(readFileSync('package.json', 'utf8')) as { version: string }).version}-dev.local+${sha.slice(0, 7)}`;
  if (!parseVersion(version)) throw new Error(`not a valid app version: ${version}`);
  return version;
}

const sha = buildSha();
const buildTime = new Date().toISOString();
const version = appVersion(sha);
const parsedVersion = parseVersion(version);
if (!parsedVersion) throw new Error(`not a valid app version: ${version}`);
const parsed: Version = parsedVersion;

// A release build is served from its own directory (`/dwell/v/<version>/`, RELEASES.md §3); a
// local build, the preview and the e2e suite from `/dwell/`.
/**
 * The renderer switch (RAY_MARCHED_RENDERER.md §3): on in local builds, off in the release
 * workflow's (DWELL_VERSION set) until the owner's decision (A7). DWELL_RENDERER_SWITCH=0/1 forces it.
 */
const rendererSwitch =
  process.env.DWELL_RENDERER_SWITCH !== undefined
    ? process.env.DWELL_RENDERER_SWITCH === '1'
    : process.env.DWELL_VERSION === undefined;

const base =
  process.env.DWELL_BASE ??
  (process.env.DWELL_VERSION ? `/dwell/v/${withoutBuild(parsed)}/` : '/dwell/');

/** `build.json` (what the site's `versions.json` lists) and the licence notices, in every build. */
function releaseFiles(): Plugin {
  return {
    name: 'dwell-release-files',
    generateBundle() {
      const protocol = JSON.parse(readFileSync('../shared/protocol/constants.json', 'utf8')) as {
        protocolVersion: number;
      };
      const buildJson = {
        // As the tag and the version directory name it: the commit is its own field.
        version: withoutBuild(parsed),
        channel: isStable(parsed) ? 'stable' : 'dev',
        date: buildTime,
        commit: sha,
        protocolVersion: protocol.protocolVersion,
        generators: GENERATORS,
        // The launcher (RELEASES.md §5) this build needs; bump when its contract changes.
        minLauncher: 1,
      };
      this.emitFile({
        type: 'asset',
        fileName: 'build.json',
        source: `${JSON.stringify(buildJson, null, 2)}\n`,
      });
      // Shipped with every build and linked from the menu's About screen (RELEASES.md §2).
      this.emitFile({
        type: 'asset',
        fileName: 'THIRD_PARTY_NOTICES.txt',
        source: readFileSync('../THIRD_PARTY_NOTICES', 'utf8'),
      });
    },
  };
}

export default defineConfig({
  // Served at https://dropkickarcade.com/dwell/ (ADR 0005), or from its own version directory.
  base,
  define: {
    __BUILD_SHA__: JSON.stringify(sha),
    __BUILD_TIME__: JSON.stringify(buildTime),
    __APP_VERSION__: JSON.stringify(version),
    __RENDERER_SWITCH__: JSON.stringify(rendererSwitch),
  },
  plugins: [releaseFiles()],
  // Module workers: the job and local-mode workers load the Rust client's WASM module.
  worker: {
    format: 'es',
  },
  build: {
    target: 'es2022',
    sourcemap: true,
    rollupOptions: {
      // The game's page, and `tools.js` at a fixed name for the launcher (src/tools.ts).
      input: { index: 'index.html', tools: 'src/tools.ts' },
      // Keep `tools.js`'s exports: nothing in the build imports them.
      preserveEntrySignatures: 'exports-only',
      output: {
        entryFileNames: (chunk) =>
          chunk.name === 'tools' ? 'tools.js' : 'assets/[name]-[hash].js',
      },
    },
  },
  test: {
    environment: 'node',
    // Unit tests only: e2e/*.spec.ts are Playwright's (`npm run e2e`), not Vitest's.
    include: ['src/**/*.test.ts', 'launcher/**/*.test.ts', 'scripts/**/*.test.ts'],
  },
});
