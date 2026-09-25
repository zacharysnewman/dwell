// Dwell desktop shell (ARCHITECTURE.md §2.2). Serves the same client build as GitHub Pages from a
// secure custom protocol, so WebTransport, WebCrypto, and IndexedDB work. COOP/COEP headers make the
// page cross-origin isolated, ready for the multithreaded sim-core build (ADR 0007).
//
// Usage: electron . [--join=<invite query, e.g. "?join=127.0.0.1:4433&cert=...">] [--smoke]
//   --smoke  load, wait until joined (or 20 s), print the status line, and exit (0 = joined).
import { app, BrowserWindow, net, protocol } from 'electron';
import { existsSync } from 'node:fs';
import { dirname, join, normalize, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
// Packaged builds copy the client into ./app; in the repo, use the client's dist directly.
const clientDir = existsSync(join(here, 'app')) ? join(here, 'app') : join(here, '../../client/dist');

const args = process.argv.slice(1);
const smoke = args.includes('--smoke');
const joinArg = args.find((a) => a.startsWith('--join='));
const query = joinArg ? joinArg.slice('--join='.length) : '';

if (smoke) {
  // Headless CI machines have no GPU: render WebGL2 in software.
  app.commandLine.appendSwitch('use-angle', 'swiftshader');
  app.commandLine.appendSwitch('enable-unsafe-swiftshader');
}

protocol.registerSchemesAsPrivileged([
  {
    scheme: 'dwell',
    privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true },
  },
]);

function serveClient() {
  protocol.handle('dwell', async (request) => {
    const url = new URL(request.url);
    // The client is built for the /dwell/ base path (ADR 0005).
    const relative = decodeURIComponent(url.pathname).replace(/^\/dwell\/?/, '') || 'index.html';
    const file = normalize(join(clientDir, relative));
    if (!file.startsWith(clientDir + sep)) return new Response('Forbidden', { status: 403 });
    const response = await net.fetch(pathToFileURL(file).toString());
    const headers = new Headers(response.headers);
    headers.set('Cross-Origin-Opener-Policy', 'same-origin');
    headers.set('Cross-Origin-Embedder-Policy', 'require-corp');
    return new Response(response.body, { status: response.status, headers });
  });
}

async function runSmoke(win) {
  const deadline = Date.now() + 20_000;
  let status = '';
  while (Date.now() < deadline) {
    status = await win.webContents.executeJavaScript(
      "document.getElementById('net-status')?.textContent ?? ''",
    );
    if (/player \d+ .*datagram \d+ ms/.test(status)) break;
    await new Promise((r) => setTimeout(r, 250));
  }
  console.log(`STATUS: ${status}`);
  app.exit(/player \d+/.test(status) ? 0 : 1);
}

app.whenReady().then(async () => {
  if (!existsSync(join(clientDir, 'index.html'))) {
    console.error(`Client build not found in ${clientDir}; run \`npm run build\` in client/.`);
    app.exit(1);
    return;
  }
  serveClient();
  const win = new BrowserWindow({
    width: 1280,
    height: 720,
    show: !smoke,
    title: 'Dwell',
    webPreferences: { contextIsolation: true, nodeIntegration: false, sandbox: true },
  });
  if (smoke) {
    win.webContents.on('console-message', (e) => {
      if (e.level === 'error' || e.level === 'warning') console.log(`[page ${e.level}] ${e.message}`);
    });
    win.webContents.on('did-fail-load', (_e, code, description, url) => {
      console.log(`[load failed] ${String(code)} ${description} ${url}`);
    });
  }
  await win.loadURL(`dwell://app/dwell/index.html${query}`);
  if (smoke) await runSmoke(win);
});

app.on('window-all-closed', () => app.quit());
