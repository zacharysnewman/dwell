// Phase 40 C1 (docs/CHARACTERS.md): two clients on a native server. B draws A as the full body,
// playing the clip A's controller state calls for while A walks, sprints, crouches and jumps; A sees
// its own first-person arms. With DWELL_SHOTS=<dir>, both pages' views are saved there too (the
// in-game check on a real GPU: a copy of playwright.config.ts without the SwiftShader flags).
//
// The server is the spec's own, on the flat world: on the suite's generated terrain A ran +X for as
// long as the slow page took to see each role, and on CI (from 0.22.3, as the frames slowed) it
// often got as far as the trees ~30 m from the spawn, where a trunk stopped the slide and the leaves
// 1.5 m up kept it crouched and stopped its jump (`Expected "idle", Received "crouch"`, …).
import { join } from 'node:path';
import { expect, test, type BrowserContext, type Page } from '@playwright/test';
import { startServer, type StartedServer } from './global-setup';

type Vec3 = [number, number, number];
interface Body {
  playerId: number;
  role: string;
  action: string | null;
  /** The latest base roles it started, oldest first, and how many it has started in all. */
  played: string[];
  plays: number;
}
interface State {
  playerId: number;
  terrainReady: boolean;
  feet: Vec3;
  remotes: { playerId: number; feet: Vec3 }[];
  bodies: Body[];
  arms: { role: string; action: string | null };
  stats: { ticks: number };
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

const body = async (b: Page, id: number) =>
  (await state(b))?.bodies.find((x) => x.playerId === id) ?? null;

/** How many base roles B's body of A has started: a mark for `seesRole`. */
const mark = async (b: Page, id: number) => (await body(b, id))?.plays ?? 0;

/**
 * Polls until B draws A playing `role`: playing it now, or having started it since `since` (a
 * `mark`). B's body keeps the roles it played, so a short one (a slide, a jump) is seen however
 * seldom the slow page is sampled; `again` runs each poll to start it again (CI's software
 * renderer draws ~5 frames a second, and a body only takes a role in a frame that sees it; a jump
 * that started since B's last frame is the exception, `RemoteView::jumped`, which until it came
 * left B's slow frames seeing only A's fall or landing, `Expected "jump", Received "idle"`).
 */
async function seesRole(
  a: Page,
  b: Page,
  id: number,
  role: string,
  since: number,
  again?: () => Promise<void>,
): Promise<void> {
  // What each poll saw, for the failure message: B's body of A, and A's own controller.
  const seen: string[] = [];
  const t0 = Date.now();
  try {
    await expect
      .poll(
        async () => {
          await face(b, a);
          const s = await body(b, id);
          seen.push(await polled(a, s, t0));
          if (!s) return null;
          const recent = s.played.slice(Math.max(0, s.played.length - (s.plays - since)));
          if (s.role === role || recent.includes(role)) return role;
          await again?.();
          return s.role;
        },
        { timeout: 30_000, intervals: [100] },
      )
      .toBe(role);
  } catch (e) {
    const frames = await Promise.all([a, b].map(frameStats));
    throw new Error(
      `B never drew A's ${role} (frames: A ${frames[0]}, B ${frames[1]}); the last polls, ` +
        `B's body of A (role, latest roles played, plays) | A's arms, ticks and last tick:\n` +
        seen.slice(-40).join('\n'),
      { cause: e },
    );
  }
}

/** One poll's line for `seesRole`'s failure message. */
async function polled(a: Page, s: Body | null, t0: number): Promise<string> {
  const sa = await state(a);
  const tk = await a.evaluate<{ state: string; jump: boolean; grounded: boolean }[]>(
    'globalThis.__dwell ? globalThis.__dwell.ticks() : []',
  );
  const k = tk.at(-1);
  const at = sa ? sa.feet.map((v) => v.toFixed(1)).join(',') : '?';
  return (
    `${String(Date.now() - t0)} ms: ${s ? `${s.role} ${s.played.slice(-6).join(',')} ${String(s.plays)}` : 'no body'}` +
    ` | ${sa?.arms.role ?? '?'} ${String(sa?.stats.ticks ?? '?')} ` +
    `${k ? `${k.state}${k.grounded ? ' grounded' : ''}${k.jump ? ' jump' : ''}` : '-'} at ${at}`
  );
}

/** A page's frame intervals so far: p50 and max (ms). */
async function frameStats(p: Page): Promise<string> {
  const f = await p.evaluate<{ p50: number; max: number } | undefined>(
    'globalThis.__dwell?.telemetry().frames.interval',
  );
  return f ? `p50 ${f.p50.toFixed(0)} ms, max ${f.max.toFixed(0)} ms` : '?';
}

/** A's own first-person arms' role: what A's controller is doing, as A predicts it. */
const armsRole = async (a: Page) => (await state(a))?.arms.role ?? null;

test('another client sees your body walk, sprint, crouch and jump, and you see your arms', async ({
  browser,
}) => {
  test.setTimeout(150_000);
  // A flat world in memory (generator 0): nothing in A's way however far it runs.
  const server: StartedServer = await startServer([
    '--port',
    '0',
    '--name',
    'E2E characters',
    '--world',
    '',
    '--generator',
    '0',
  ]);
  const contexts: BrowserContext[] = [await browser.newContext(), await browser.newContext()];
  const [a, b] = await Promise.all(contexts.map((c) => c.newPage()));
  try {
    for (const p of [a, b]) await p.setViewportSize({ width: 1280, height: 720 });
    await a.goto(`./${server.query}`);
    await b.goto(`./${server.query}`);
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
    await seesRole(a, b, id, 'idle', await mark(b, id));
    await shot(b, 'b-sees-a-idle');
    // Walk away from B, then sprint, crouch, jump.
    await call(a, 'd.look(90, 0)');
    let since = await mark(b, id);
    await press(a, 'KeyW', true);
    await seesRole(a, b, id, 'walk', since);
    await shot(b, 'b-sees-a-walk');
    since = await mark(b, id);
    await press(a, 'ShiftLeft', true);
    await seesRole(a, b, id, 'sprint', since);
    await shot(b, 'b-sees-a-sprint');
    // Crouch at a sprint: the sprint slide. A slide is short: while B hasn't seen it, A stands up
    // once it ends and crouches again once its controller is back at a sprint.
    since = await mark(b, id);
    let crouched = true;
    await press(a, 'KeyC', true);
    await seesRole(a, b, id, 'slide', since, async () => {
      const r = await armsRole(a);
      if (crouched && r !== 'slide' && r !== 'sprint') {
        crouched = false;
        await press(a, 'KeyC', false);
      } else if (!crouched && r === 'sprint') {
        crouched = true;
        await press(a, 'KeyC', true);
      }
    });
    await shot(b, 'b-sees-a-slide');
    await press(a, 'KeyC', false);
    await press(a, 'ShiftLeft', false);
    since = await mark(b, id);
    await press(a, 'KeyW', false);
    await seesRole(a, b, id, 'idle', since);
    since = await mark(b, id);
    await press(a, 'KeyC', true);
    await seesRole(a, b, id, 'crouch', since);
    await shot(b, 'b-sees-a-crouch');
    since = await mark(b, id);
    await press(a, 'KeyC', false);
    await seesRole(a, b, id, 'idle', since);
    // A jump is short too: while B hasn't seen it, A lets go of jump and jumps again once its
    // controller is back on the ground. The key is held for some ticks of A's either way (a key
    // pressed and let go between two of A's frames is never seen).
    since = await mark(b, id);
    let jumping = true;
    const ticks = async () => (await state(a))?.stats.ticks ?? 0;
    let toggled = await ticks();
    await press(a, 'Space', true);
    await seesRole(a, b, id, 'jump', since, async () => {
      if ((await armsRole(a)) !== 'idle' || (await ticks()) < toggled + 10) return;
      jumping = !jumping;
      toggled = await ticks();
      await press(a, 'Space', jumping);
    });
    await shot(b, 'b-sees-a-jump');
    since = await mark(b, id);
    await press(a, 'Space', false);
    await seesRole(a, b, id, 'idle', since);
  } finally {
    for (const c of contexts) await c.close();
    server.process.kill();
  }
});
