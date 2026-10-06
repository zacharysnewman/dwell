// A local site with several versions, assembled as the Pages workflow does (RELEASES.md §4), for
// the launcher's end-to-end tests: `npm run site:fixture [VERSION…]` writes dist-site/ with the
// launcher, each version's build in v/<version>/ and versions.json. Default: 0.1.0 0.1.1 0.2.0.
import { execFileSync } from 'node:child_process';
import { rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { buildManifest, readBuilds } from './site.ts';

const SITE = 'dist-site';
const versions = process.argv.length > 2 ? process.argv.slice(2) : ['0.1.0', '0.1.1', '0.2.0'];

function vite(args: string[], env: Record<string, string> = {}): void {
  execFileSync('npx', ['vite', 'build', ...args], {
    stdio: ['ignore', 'ignore', 'inherit'],
    env: { ...process.env, ...env },
  });
}

rmSync(SITE, { recursive: true, force: true });
// The launcher first: it empties its output directory, which the versions then fill.
vite(['-c', 'vite.launcher.config.ts', '--outDir', `../${SITE}`]);
for (const version of versions) {
  vite(['--outDir', join(SITE, 'v', version)], { DWELL_VERSION: version });
  console.log(`built ${version}`);
}
const manifest = buildManifest(readBuilds(SITE), new Date().toISOString());
writeFileSync(join(SITE, 'versions.json'), `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`${SITE}/: ${versions.join(', ')}`);
