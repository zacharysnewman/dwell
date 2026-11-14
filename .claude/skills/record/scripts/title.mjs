// A video's title card (the record skill): the game's own wordmark — the main menu's `.wordmark` rule from
// client/src/style.css (the rounded display font, white with the grass-and-earth extrusion) — rendered by Chrome
// to a transparent PNG that splice.py's titleCard fades in over the last shot.
//
//   node title.mjs "DWELL" out.png [widthPx]
//
// Owner (2026-11-13, the trailer): "Title and game sound, no music." Showcases stay text-free (SKILL.md); only a
// manifest with a `title` gets one.
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const SKILL = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const REPO = resolve(SKILL, '../../..');
const STYLE = join(REPO, 'client/src/style.css');

/** The page that draws the wordmark: the client's stylesheet inlined (its fonts, colours and `.wordmark`; a file:
 * link doesn't load into a set page), a transparent body, the text at the rule's own largest size (152 px), escaped. */
export function titleHtml(text, css = readFileSync(STYLE, 'utf8')) {
  const esc = String(text).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  return `<!doctype html><html><head><meta charset="utf-8"><style>${css}</style>
<style>html,body{margin:0;background:transparent!important}body::before,body::after{display:none!important}
.wrap{display:inline-block;padding:12px 28px 40px}.wordmark{font-size:152px!important;white-space:nowrap}</style></head>
<body><div class="wrap"><h1 class="wordmark">${esc}</h1></div></body></html>`;
}

/** Renders `text` to `out` (a PNG with an alpha channel), `widthPx` wide: the wordmark is laid out at 152 px and
 * scaled by the device pixel ratio, so the extrusion scales with it. */
export async function renderTitle(text, out, widthPx = 800) {
  if (!readFileSync(STYLE, 'utf8').includes('.wordmark')) throw new Error(`${STYLE} has no .wordmark rule`);
  const require = createRequire(join(REPO, 'client/package.json'));
  const { chromium } = require('playwright');
  const browser = await chromium.launch({ channel: 'chrome' });
  try {
    const probe = await browser.newPage();
    await probe.setContent(titleHtml(text), { waitUntil: 'load' });
    const box = await probe.locator('.wrap').boundingBox();
    await probe.close();
    const scale = widthPx / box.width;
    const page = await browser.newPage({ deviceScaleFactor: scale, viewport: { width: Math.ceil(box.width), height: Math.ceil(box.height) } });
    await page.setContent(titleHtml(text), { waitUntil: 'load' });
    await page.locator('.wrap').screenshot({ path: out, omitBackground: true });
    return { out, widthPx, scale: +scale.toFixed(3) };
  } finally { await browser.close(); }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [text, out, w] = process.argv.slice(2);
  if (!text || !out) { console.error('usage: title.mjs "TEXT" out.png [widthPx]'); process.exit(1); }
  renderTitle(text, out, w ? +w : undefined).then((r) => console.log(JSON.stringify(r)), (e) => { console.error(e.message); process.exit(1); });
}
