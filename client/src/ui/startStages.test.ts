import { describe, expect, it } from 'vitest';
import { StageTracker } from './loadingStages';
import { paintedFrame, runStartStages } from './startStages';

describe("the start's stages", () => {
  it('paints each stage before its work, so a long graphics start is not shown as the game', async () => {
    // Owner (2026-11-11): "the loading game (xMB) step still takes like 10+ seconds sometimes and
    // shows as 0.0s". A cached module loads at once; the graphics then block the thread for 9 s.
    const stages = new StageTracker();
    let clock = 0;
    let onScreen = 'nothing';
    const seen: string[] = [];
    await runStartStages(stages, {
      now: () => clock,
      painted: () => {
        // What the browser paints: the screen's headline, the stage running latest.
        onScreen = stages.current()?.id ?? 'nothing';
        return Promise.resolve();
      },
      load: (onBytes) => {
        seen.push(`load under ${onScreen}`);
        clock += 40; // from the cache
        onBytes(15_900_000, 15_900_000);
        return Promise.resolve();
      },
      create: () => {
        seen.push(`create under ${onScreen}`);
        clock += 9000; // one long task: the screen shows what was painted last
        return Promise.resolve('client');
      },
    });
    expect(seen).toEqual(['load under game', 'create under graphics']);
    const times = Object.fromEntries(
      stages.timeline().map((r) => [r.id, (r.endedMs ?? 0) - r.beganMs]),
    );
    expect(times).toEqual({ game: 40, graphics: 9000 });
  });

  it("counts a slow download and compile in the game's time", async () => {
    const stages = new StageTracker();
    let clock = 1000;
    await runStartStages(stages, {
      now: () => clock,
      painted: () => Promise.resolve(),
      load: async (onBytes) => {
        for (let i = 1; i <= 4; i++) {
          await Promise.resolve();
          clock += 2500; // a slow network
          onBytes(i * 4e6, 16e6);
        }
        clock += 1500; // the compile
      },
      create: () => Promise.resolve(null),
    });
    const game = stages.timeline().find((r) => r.id === 'game');
    expect(game && (game.endedMs ?? 0) - game.beganMs).toBe(11_500);
    expect(game?.done).toBe(16e6);
  });

  it('resolves after a frame and the task after it, or on the timeout without frames', async () => {
    const order: string[] = [];
    const tasks: { cb: () => void; ms: number }[] = [];
    let frame: (() => void) | null = null;
    const done = paintedFrame(
      (cb) => (frame = cb),
      (cb, ms) => tasks.push({ cb, ms }),
    ).then(() => order.push('painted'));
    expect(tasks.map((t) => t.ms)).toEqual([100]); // the timeout, armed
    (frame as (() => void) | null)?.();
    expect(tasks.map((t) => t.ms)).toEqual([100, 0]);
    tasks[1]?.cb();
    await done;
    expect(order).toEqual(['painted']);

    // A background tab: no frame, the timeout.
    const slow: (() => void)[] = [];
    const late = paintedFrame(
      () => {},
      (cb) => slow.push(cb),
    );
    slow[0]?.();
    await expect(late).resolves.toBeUndefined();
  });
});
