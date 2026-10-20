// Opens a named view (scripts/views.ts) in a page and waits until the distant view has finished
// loading, for specs and for scripts/shots.ts. Specs: `const result = await openView(page, view)`,
// then screenshot or read `result`. The camera is placed (`__dwell.placeCamera`), never flown.
import { readFileSync } from 'node:fs';
import type { Page } from '@playwright/test';
import { parseViews, selectViews, viewStorage, viewUrl, type View } from '../scripts/views.ts';

/** The checked-in views (client/views/views.json), or another file's. */
export function loadViews(file = new URL('../views/views.json', import.meta.url)): View[] {
  return parseViews(JSON.parse(readFileSync(file, 'utf8')) as unknown);
}

/** One view by name from the checked-in views (throws on an unknown name). */
export function view(name: string): View {
  const [v] = selectViews(loadViews(), [name]);
  return v;
}

export interface LodStats {
  drawn: number[];
  generating: number;
  meshing: number;
  asking: number;
  nodes: number;
  cacheBytes: number;
  errorScale: number;
}

export interface ViewResult {
  /** The LOD had nothing left to generate, mesh or ask for (for a second) within `settleS`. */
  settled: boolean;
  /** Seconds from placing the camera to settled (or to giving up). */
  seconds: number;
  /** The player's feet when the terrain was ready, before the camera was placed. */
  spawnFeet: [number, number, number];
  /** The player's feet (where it stayed). */
  feet: [number, number, number];
  lod: LodStats | null;
  render: { calls: number; triangles: number };
}

/** Evaluates `expression` with `d` bound to the page's debug hooks (main.ts `__dwell`). */
function call<T>(page: Page, expression: string): Promise<T> {
  return page.evaluate<T>(
    `(() => { const d = globalThis.__dwell; return d ? (${expression}) : null; })()`,
  );
}

/** Polls `expression` (see `call`) until it is true. String evaluation, as `waitForFunction`'s
 * would need `unsafe-eval`, which the page's CSP refuses. */
async function until(page: Page, expression: string, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while ((await call<boolean | null>(page, expression)) !== true) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${expression}`);
    await page.waitForTimeout(100);
  }
}

/**
 * Loads `view` in `page` (its window, settings, seed and switches; `base` is the client's URL,
 * './' under the e2e config), places the camera and waits for the LOD to settle. The HUD is
 * hidden unless the view shows the F3 overlay.
 */
export async function openView(page: Page, view: View, base = './'): Promise<ViewResult> {
  await page.setViewportSize(view.window);
  const storage = viewStorage(view);
  await page.addInitScript((entries: Record<string, string>) => {
    for (const [k, v] of Object.entries(entries)) localStorage.setItem(k, v);
  }, storage);
  await page.goto(viewUrl(base, view));
  await until(page, 'd.state()?.terrainReady === true', 120_000);
  const spawnFeet = (await call<[number, number, number] | undefined>(page, 'd.state()?.feet')) ?? [
    0, 0, 0,
  ];
  const { position, yaw, pitch } = view.camera;
  const look = `${String(yaw)}, ${String(pitch)}`;
  if (position) {
    await until(page, `d.placeCamera(${JSON.stringify(position)}, ${look})`, 30_000);
  } else {
    await call(page, `d.look(${look})`);
  }
  if (!view.debug) {
    // Only the picture. Through the CSSOM: the page's CSP refuses injected style sheets.
    await page.evaluate(() => {
      for (const el of Array.from(document.body.children)) {
        if (el.tagName !== 'CANVAS' && el instanceof HTMLElement) el.style.display = 'none';
      }
    });
  }
  const start = Date.now();
  let quietSince: number | null = null;
  let settled = false;
  while (Date.now() - start < view.settleS * 1000) {
    const lod = await call<LodStats | null>(page, 'd.state()?.lod ?? null');
    const quiet = lod !== null && lod.generating === 0 && lod.meshing === 0 && lod.asking === 0;
    const now = Date.now();
    quietSince = quiet ? (quietSince ?? now) : null;
    if (quietSince !== null && now - quietSince >= 1000) {
      settled = true;
      break;
    }
    await page.waitForTimeout(100);
  }
  const seconds = (Date.now() - start) / 1000;
  // Two more frames so the last meshes are drawn.
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        requestAnimationFrame(() => requestAnimationFrame(resolve));
      }),
  );
  const state = await call<{
    feet: [number, number, number];
    lod: LodStats | null;
    render: { calls: number; triangles: number };
  } | null>(
    page,
    '{ feet: d.state()?.feet, lod: d.state()?.lod ?? null, render: d.renderStats() }',
  );
  return {
    settled,
    seconds,
    spawnFeet,
    feet: state?.feet ?? [0, 0, 0],
    lod: state?.lod ?? null,
    render: state?.render ?? { calls: 0, triangles: 0 },
  };
}
