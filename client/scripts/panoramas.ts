// The main menu's backgrounds, rendered from the real game (shared/ui/menu-backgrounds.json; docs/DEV_TOOLS.md
// "Menu panoramas"). For each entry: the built client opened on its seed at its spot (a local world,
// `?local=1&seed=…&fresh=1&at=…`) at its time of day, the High preset, the HUD and arms hidden;
// the camera placed at its position, level, and turned in `capture.steps` equal yaw steps (a
// square window at `capture.fovDeg`); the shots stitched into one cylindrical 360° strip whose two
// ends meet (each output column taken from the shot looking nearest along it, reprojected from
// that shot's perspective), and encoded as WebP into shared/ui/menu-backgrounds/<image>.
//
//   cd client && npm run build:wasm && npm run build
//   npm run panoramas                 # every background
//   npm run panoramas -- beach        # by name
//   options: --base URL (a served build), --out DIR (the shots; default bench/out/panoramas)
//
// A new background is an entry in the data and one run; then `node shared/ui/gen.mjs`.
import { spawn, type ChildProcess } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium, type Page } from '@playwright/test';

const DATA = fileURLToPath(new URL('../../shared/ui/menu-backgrounds.json', import.meta.url));
const IMAGES = fileURLToPath(new URL('../../shared/ui/menu-backgrounds/', import.meta.url));

export interface Capture {
  steps: number;
  sizePx: number;
  fovDeg: number;
  /** The strip's height either side of the horizon, as a share of the focal length (≤ 1). */
  span: number;
  quality: number;
  settleS: number;
}

export interface Background {
  name: string;
  label: string;
  seed: number;
  position: [number, number, number];
  yaw: number;
  timeOfDay: number;
  image: string;
}

/** The strip's size for a capture: 360° around at the shots' focal length, `span` up and down. */
export function stripSize(c: Capture): { width: number; height: number; focal: number } {
  const focal = c.sizePx / 2 / Math.tan((c.fovDeg * Math.PI) / 360);
  return {
    width: Math.round(2 * Math.PI * focal),
    height: 2 * Math.round(c.span * focal),
    focal,
  };
}

/** The yaw of each shot (degrees), each next one turned left (yaw falls to the right), so each lands to the right of the last in the strip. */
export function shotYaws(b: Background, c: Capture): number[] {
  return Array.from({ length: c.steps }, (_, k) => (b.yaw + 360 - (360 * k) / c.steps) % 360);
}

/**
 * The page's address: the entry's local world, spawned at its spot (the player stands there, so
 * the chunks round it stream in), at its time of day, at the High preset.
 */
export function panoramaUrl(base: string, b: Background): string {
  const [x, y, z] = b.position;
  const q = new URLSearchParams({
    local: '1',
    seed: String(b.seed),
    fresh: '1',
    at: `${String(x)},${String(y)},${String(z)}`,
    time: String(b.timeOfDay),
    preset: 'high',
  });
  return `${base}/dwell/?${q.toString()}`;
}

/** The stored settings for the capture: the FOV, no arms, no speed effects, no music. */
export function panoramaStorage(c: Capture): Record<string, string> {
  return {
    'dwell.view': JSON.stringify({ fovDeg: c.fovDeg, showArms: false }),
    'dwell.motion': JSON.stringify({ speedEffects: false, fovBoost: 0, plantSway: true }),
    'dwell.music': '"off"',
  };
}

// The stitch, in the page (no image library here): each output column's angle picks the shot
// looking nearest along it; its pixel is that shot's along the same ray (bilinear).
const STITCH = `async (shots, size, w, h, focal, quality) => {
    const n = shots.length, step = (2 * Math.PI) / n, c = size / 2;
    const px = [];
    for (const b64 of shots) {
      const bytes = Uint8Array.from(atob(b64), (ch) => ch.charCodeAt(0));
      const bmp = await createImageBitmap(new Blob([bytes], { type: 'image/png' }));
      const cv = new OffscreenCanvas(size, size);
      const cx = cv.getContext('2d');
      cx.drawImage(bmp, 0, 0, size, size);
      px.push(cx.getImageData(0, 0, size, size).data);
    }
    const out = new ImageData(w, h);
    const o = out.data;
    for (let u = 0; u < w; u++) {
      const theta = ((u + 0.5) / w) * 2 * Math.PI;
      const k = Math.round(theta / step) % n;
      const phi = theta - Math.round(theta / step) * step;
      const xs = c + focal * Math.tan(phi) - 0.5;
      const cos = Math.cos(phi);
      const src = px[k];
      for (let v = 0; v < h; v++) {
        const t = (h / 2 - (v + 0.5)) / focal;
        const ys = c - (focal * t) / cos - 0.5;
        const x0 = Math.max(0, Math.min(size - 2, Math.floor(xs)));
        const y0 = Math.max(0, Math.min(size - 2, Math.floor(ys)));
        const fx = Math.min(1, Math.max(0, xs - x0));
        const fy = Math.min(1, Math.max(0, ys - y0));
        const i00 = (y0 * size + x0) * 4, i10 = i00 + 4, i01 = i00 + size * 4, i11 = i01 + 4;
        const j = (v * w + u) * 4;
        for (let ch = 0; ch < 3; ch++) {
          const top = src[i00 + ch] * (1 - fx) + src[i10 + ch] * fx;
          const bot = src[i01 + ch] * (1 - fx) + src[i11 + ch] * fx;
          o[j + ch] = top * (1 - fy) + bot * fy;
        }
        o[j + 3] = 255;
      }
    }
    const cv = new OffscreenCanvas(w, h);
    cv.getContext('2d').putImageData(out, 0, 0);
    const buf = new Uint8Array(await (await cv.convertToBlob({ type: 'image/webp', quality })).arrayBuffer());
    let s = '';
    for (let i = 0; i < buf.length; i += 0x8000) s += String.fromCharCode(...buf.subarray(i, i + 0x8000));
    return btoa(s);
  }`;

function call<T>(page: Page, expression: string): Promise<T> {
  return page.evaluate<T>(
    `(() => { const d = globalThis.__dwell; return d ? (${expression}) : null; })()`,
  );
}

async function poll(page: Page, expression: string, ms: number): Promise<boolean> {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (await call<boolean>(page, expression).catch(() => false)) return true;
    await page.waitForTimeout(200);
  }
  return false;
}

/** Waits until the LOD has had nothing to generate, mesh or ask for, for 1 s (bench `settle`). */
async function settle(page: Page, maxS: number): Promise<void> {
  const end = Date.now() + maxS * 1000;
  let quiet: number | null = null;
  while (Date.now() < end) {
    const idle = await call<boolean>(
      page,
      '(() => { const l = d.state()?.lod; return !!l && l.generating === 0 && l.meshing === 0 && l.asking === 0; })()',
    );
    quiet = idle ? (quiet ?? Date.now()) : null;
    if (quiet !== null && Date.now() - quiet >= 1000) return;
    await page.waitForTimeout(100);
  }
}

async function reachable(url: string): Promise<boolean> {
  try {
    return (await fetch(url)).ok;
  } catch {
    return false;
  }
}

async function serve(base: string | null): Promise<{ url: string; server: ChildProcess | null }> {
  if (base) return { url: base.replace(/\/$/, '').replace(/\/dwell$/, ''), server: null };
  const url = 'http://localhost:4173';
  if (await reachable(`${url}/dwell/`)) return { url, server: null };
  const server = spawn('npx', ['vite', 'preview', '--port', '4173', '--strictPort'], {
    stdio: 'ignore',
  });
  for (let i = 0; i < 100 && !(await reachable(`${url}/dwell/`)); i++) {
    await new Promise((r) => setTimeout(r, 200));
  }
  return { url, server };
}

async function main(): Promise<void> {
  const args = process.argv.slice(2);
  const opt = (name: string): string | null => {
    const i = args.indexOf(name);
    if (i < 0) return null;
    const v = args[i + 1] ?? null;
    args.splice(i, 2);
    return v;
  };
  const base = opt('--base');
  const out = opt('--out') ?? join('bench', 'out', 'panoramas');
  const data = JSON.parse(readFileSync(DATA, 'utf8')) as {
    capture: Capture;
    backgrounds: Background[];
  };
  const chosen = args.length
    ? data.backgrounds.filter((b) => args.includes(b.name))
    : data.backgrounds;
  const unknown = args.filter((a) => !data.backgrounds.some((b) => b.name === a));
  if (unknown.length) throw new Error(`no such background: ${unknown.join(', ')}`);
  const c = data.capture;
  const strip = stripSize(c);
  const { url, server } = await serve(base);
  const browser = await chromium.launch({
    headless: false,
    ...(process.env.DWELL_CHROMIUM
      ? { executablePath: process.env.DWELL_CHROMIUM }
      : { channel: 'chrome' }),
    args: [
      '--disable-background-timer-throttling',
      '--disable-renderer-backgrounding',
      '--disable-backgrounding-occluded-windows',
    ],
  });
  try {
    for (const b of chosen) {
      console.log(`${b.name}: ${b.label} (seed ${String(b.seed)} at ${b.position.join(', ')})`);
      const dir = join(out, b.name);
      mkdirSync(dir, { recursive: true });
      const context = await browser.newContext({
        viewport: { width: c.sizePx, height: c.sizePx },
      });
      const page = await context.newPage();
      await page.addInitScript((entries: Record<string, string>) => {
        for (const [k, v] of Object.entries(entries)) localStorage.setItem(k, v);
      }, panoramaStorage(c));
      await page.goto(panoramaUrl(url, b));
      if (!(await poll(page, 'd.state()?.terrainReady === true', 120_000)))
        throw new Error(`${b.name}: the game did not start`);
      await call(page, `d.setTime(${String(b.timeOfDay)})`);
      await call(page, 'd.fly(true)');
      await poll(page, `d.placeCamera(${JSON.stringify(b.position)}, ${String(b.yaw)}, 0)`, 30_000);
      await settle(page, c.settleS);
      // The scene only (set from script: the page's CSP allows no inline style sheet).
      await page.evaluate(`(() => {
        for (const e of document.querySelectorAll('body *'))
          e.style.visibility = e.id === 'view' ? 'visible' : 'hidden';
      })()`);
      const shots: string[] = [];
      for (const [k, yaw] of shotYaws(b, c).entries()) {
        await call(page, `d.placeCamera(${JSON.stringify(b.position)}, ${String(yaw)}, 0)`);
        // What this way shows streamed in and drawn.
        await poll(page, 'd.state()?.lod?.waitingInView === 0', 20_000);
        await settle(page, 10);
        await page.waitForTimeout(300);
        const png = await page.screenshot({
          path: join(dir, `shot-${String(k).padStart(2, '0')}.png`),
        });
        shots.push(png.toString('base64'));
      }
      const webp = await page.evaluate(
        `(${STITCH})(${JSON.stringify(shots)}, ${String(c.sizePx)}, ${String(strip.width)}, ${String(strip.height)}, ${String(strip.focal)}, ${String(c.quality)})`,
      );
      const bytes = Buffer.from(webp as string, 'base64');
      mkdirSync(IMAGES, { recursive: true });
      writeFileSync(join(IMAGES, b.image), bytes);
      writeFileSync(join(dir, b.image), bytes);
      console.log(
        `  ${b.image}: ${String(strip.width)}×${String(strip.height)}, ${String(Math.round(bytes.length / 1024))} KB`,
      );
      await context.close();
    }
  } finally {
    await browser.close();
    server?.kill();
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((e: unknown) => {
    console.error(e);
    process.exit(1);
  });
}
