// The capture (debug tooling): the debug overlay's button copies the frame as a PNG data URL and
// the game's state as JSON.
import { expect, test, type Page } from '@playwright/test';

const call = <T>(page: Page, expr: string): Promise<T> =>
  page.evaluate<T>(`(() => { const d = globalThis.__dwell; return d ? ${expr} : null; })()`);

test('the debug overlay copies the frame and the game state as JSON', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('./?world=flat&debug=1');
  await expect
    .poll(() => call<boolean>(page, 'd.state()?.terrainReady ?? false'), { timeout: 20_000 })
    .toBe(true);
  await call(page, 'd.look(30, -20)');

  await page.locator('#debug-capture').click();
  await expect(page.locator('#hud-note')).toHaveText('Capture copied');
  const capture = JSON.parse(await page.evaluate<string>('navigator.clipboard.readText()')) as {
    kind: string;
    format: number;
    sections: {
      build: { version: string };
      world: { seed: string; generatorVersion: number };
      view: { yaw: number; pitch: number };
      game: { feet: number[] };
      render: { calls: number };
    };
    image: string;
  };
  expect(capture.kind).toBe('dwell-capture');
  expect(capture.format).toBe(1);
  expect(capture.sections.world.generatorVersion).toBe(0); // the flat world
  expect(capture.sections.view).toEqual({ yaw: 30, pitch: -20 });
  expect(capture.sections.game.feet).toHaveLength(3);
  expect(typeof capture.sections.build.version).toBe('string');
  // The frame, as drawn: a PNG of the canvas's size, not a blank buffer.
  expect(capture.image.startsWith('data:image/png;base64,')).toBe(true);
  const size = await page.evaluate(async (url) => {
    const img = new Image();
    img.src = url;
    await img.decode();
    const c = document.createElement('canvas');
    c.width = img.width;
    c.height = img.height;
    const g = c.getContext('2d');
    if (!g) return null;
    g.drawImage(img, 0, 0);
    const px = g.getImageData(0, 0, img.width, img.height).data;
    let lit = 0;
    for (let i = 0; i < px.length; i += 4) if ((px[i] ?? 0) + (px[i + 1] ?? 0) > 0) lit++;
    const view = document.getElementById('view') as HTMLCanvasElement;
    return { w: img.width, h: img.height, cw: view.width, ch: view.height, lit };
  }, capture.image);
  expect(size).not.toBeNull();
  expect([size?.w, size?.h]).toEqual([size?.cw, size?.ch]);
  expect(size?.lit ?? 0).toBeGreaterThan((size?.w ?? 0) * (size?.h ?? 0) * 0.5);
});
