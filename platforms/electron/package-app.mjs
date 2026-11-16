// Packages the desktop shell as an app (owner, 2026-11-15: "The native app should be electron wrapper
// for now"): the client build (client/dist, `npm run build` there first) copied into the app as
// `app/`, which main.js serves from its `dwell://` protocol, so the game runs offline at the build's
// version. macOS on Apple Silicon by default; `--arch=x64` or `--platform=win32|linux` for others.
// Unsigned: macOS asks once (right click → Open).
//
// Usage: node package-app.mjs [--platform=darwin] [--arch=arm64]   → out/Dwell-<platform>-<arch>/

import { cpSync, existsSync, mkdtempSync, rmSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { packager } from '@electron/packager';

const here = dirname(fileURLToPath(import.meta.url));
const arg = (name, fallback) =>
  process.argv.find((a) => a.startsWith(`--${name}=`))?.slice(name.length + 3) ?? fallback;
const platform = arg('platform', 'darwin');
const arch = arg('arch', 'arm64');

const client = join(here, '../../client/dist');
if (!existsSync(join(client, 'index.html'))) {
  console.error(`No client build in ${client}: run \`npm run build\` in client/ first.`);
  process.exit(1);
}

// The app's source: main.js, a package.json naming it, and the client as `app/`.
const stage = mkdtempSync(join(tmpdir(), 'dwell-electron-'));
cpSync(join(here, 'main.js'), join(stage, 'main.js'));
const pkg = JSON.parse(readFileSync(join(here, 'package.json'), 'utf8'));
const version = JSON.parse(readFileSync(join(here, '../../client/package.json'), 'utf8')).version;
writeFileSync(
  join(stage, 'package.json'),
  JSON.stringify({ name: 'dwell', productName: 'Dwell', version, type: pkg.type, main: 'main.js' }, null, 2),
);
cpSync(client, join(stage, 'app'), { recursive: true });

const electronVersion = JSON.parse(
  readFileSync(join(here, 'node_modules/electron/package.json'), 'utf8'),
).version;
const paths = await packager({
  dir: stage,
  name: 'Dwell',
  platform,
  arch,
  electronVersion,
  appVersion: version,
  appBundleId: 'com.dropkickarcade.dwell',
  out: join(here, 'out'),
  overwrite: true,
  asar: false,
});
rmSync(stage, { recursive: true, force: true });
console.log(`Dwell ${version}: ${paths.join(', ')}`);
