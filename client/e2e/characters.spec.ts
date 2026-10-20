// Phase 40 C1 (docs/CHARACTERS.md): two clients on a native server. B draws A as the full body,
// playing the clip A's controller state calls for while A walks, sprints, crouches and jumps; A sees
// its own first-person arms. With DWELL_SHOTS=<dir>, both pages' views are saved there too (the
// in-game check on a real GPU: a copy of playwright.config.ts without the SwiftShader flags).
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test, type BrowserContext, type Page } from '@playwright/test';
import { INVITE_FILE } from './global-setup';

type Vec3 = [number, number, number];
interface Body {
  playerId: number;
  role: string;
  action: string | null;
}
interface State {
  playerId: number;
  terrainReady: boolean;
  feet: Vec3;
  remotes: { playerId: number; feet: Vec3 }[];
  bodies: Body[];
  arms: { role: string; action: string | null };
}
interface Hooks {
  state(): State | null;
  press(code: string, down: boolean): void;
  look(yaw: number, pitch: number): void;
  select(slot: number): void;
  renderStats(): { characters: number; arms: boolean };
}
const hooks = () => (globalThis as unknown as { __dwell?: Hooks }).__dwell;
const call = <T>(page: Page, body: string): Promise<T> =>
  page.evaluate(`(() => { const d = (${hooks.toString()})(); return ${body}; })()`);
const state = (page: Page) => call<State | null>(page, 'd?.state() ?? null');
const press = (page: Page, code: string, down: boolean) =>
  call(page, `d.press(${JSON.stringify(code)}, ${String(down)})`);

const shots = process.env.DWELL_SHOTS;
async function shot(page: Page, name: string): Promise<void> {
  if (shots) await page.screenshot({ path: join(shots, `${name}.png`) });
}

/** B turns to look at A (yaw 0 is +Z) from where it stands. */
async function face(b: Page, a: Page): Promise<void> {
  const [sa, sb] = [await state(a), await state(b)];
  if (!sa || !sb) return;
  const [dx, dz] = [sa.feet[0] - sb.feet[0], sa.feet[2] - sb.feet[2]];
  const yaw = (Math.atan2(dx, dz) * 180) / Math.PI;
  const pitch =
    (-Math.atan2(sb.feet[1] + 1.2 - (sa.feet[1] + 1), Math.hypot(dx, dz)) * 180) / Math.PI;
  await call(b, `d.look(${String(yaw)}, ${String(pitch)})`);
}

/** Polls until B draws A playing `role` (keeping B's look on A). */
async function seesRole(a: Page, b: Page, id: number, role: string): Promise<void> {
  await expect
    .poll(
      async () => {
        await face(b, a);
        return (await state(b))?.bodies.find((x) => x.playerId === id)?.role ?? null;
      },
      { timeout: 20_000, intervals: [100] },
    )
    .toBe(role);
}

test('another client sees your body walk, sprint, crouch and jump, and you see your arms', async ({
  browser,
}) => {
  test.setTimeout(150_000);
  const contexts: BrowserContext[] = [await browser.newContext(), await browser.newContext()];
  const [a, b] = await Promise.all(contexts.map((c) => c.newPage()));
  try {
    for (const p of [a, b]) await p.setViewportSize({ width: 1280, height: 720 });
    const invite = readFileSync(INVITE_FILE, 'utf8');
    const textures = process.env.DWELL_TEXTURES ? `&textures=${process.env.DWELL_TEXTURES}` : '';
    await a.goto(`./${invite}${textures}`);
    await b.goto(`./${invite}${textures}`);
    for (const p of [a, b]) {
      await expect
        .poll(async () => (await state(p))?.terrainReady ?? false, { timeout: 60_000 })
        .toBe(true);
    }
    const id = (await state(a))?.playerId ?? -1;
    await expect
      .poll(async () => (await state(b))?.bodies.some((x) => x.playerId === id) ?? false, {
        timeout: 20_000,
      })
      .toBe(true);
    await call(a, 'd.select(0)');
    // A's arms, holding the hotbar's block.
    await expect
      .poll(async () => (await call<{ arms: boolean }>(a, 'd.renderStats()')).arms)
      .toBe(true);
    await call(a, 'd.look(0, -20)');
    await shot(a, 'a-first-person');
    await seesRole(a, b, id, 'idle');
    await shot(b, 'b-sees-a-idle');
    // Walk away from B, then sprint, crouch, jump.
    await call(a, 'd.look(90, 0)');
    await press(a, 'KeyW', true);
    await seesRole(a, b, id, 'walk');
    await shot(b, 'b-sees-a-walk');
    await press(a, 'ShiftLeft', true);
    await seesRole(a, b, id, 'sprint');
    await shot(b, 'b-sees-a-sprint');
    // Crouch at a sprint: the sprint slide.
    await press(a, 'KeyC', true);
    await seesRole(a, b, id, 'slide');
    await shot(b, 'b-sees-a-slide');
    await press(a, 'KeyC', false);
    await press(a, 'ShiftLeft', false);
    await press(a, 'KeyW', false);
    await seesRole(a, b, id, 'idle');
    await press(a, 'KeyC', true);
    await seesRole(a, b, id, 'crouch');
    await shot(b, 'b-sees-a-crouch');
    await press(a, 'KeyC', false);
    await seesRole(a, b, id, 'idle');
    await press(a, 'Space', true);
    await seesRole(a, b, id, 'jump');
    await shot(b, 'b-sees-a-jump');
    await press(a, 'Space', false);
    await seesRole(a, b, id, 'idle');
  } finally {
    for (const c of contexts) await c.close();
  }
});
