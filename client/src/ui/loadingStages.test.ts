import { describe, expect, it, vi } from 'vitest';
import { LOADING_STAGES, type LoadingStageDef, type LoadingStageId } from './loading.gen';
import {
  StageTracker,
  formatProgress,
  formatStageTime,
  pageStartMs,
  stageFraction,
  stagesFromHud,
  type StageMarks,
} from './loadingStages';

// A fixture stage set, as loading.json's `stages.list` could be: the tracker takes any.
const DEFS = [
  { id: 'game', label: 'Loading the game', progress: 'bytes' },
  { id: 'worldMap', label: 'Shaping the world map', progress: 'count' },
  { id: 'spawn', label: 'Finding a place to stand' },
  { id: 'join', label: 'Joining the world' },
  { id: 'terrain', label: 'Loading the terrain', progress: 'count' },
] as unknown as LoadingStageDef[];

describe('the loading stages', () => {
  it('lists the stages begun, in the order they began, the running one with an ellipsis', () => {
    const t = new StageTracker(DEFS, 3);
    t.begin('game', 0);
    t.progress('game', 2_500_000, 7_800_000, 50);
    expect(t.lines(100)).toEqual([
      {
        id: 'game',
        text: 'Loading the game…',
        running: true,
        progress: '2.5 / 7.8 MB',
        time: '0.1 s',
        slow: false,
      },
    ]);
    t.end('game', 400);
    t.begin('worldMap', 450);
    t.begin('spawn', 500); // the world map and the spawn at once
    t.progress('worldMap', 1, 2, 1500);
    const lines = t.lines(2000);
    expect(lines.map((l) => [l.text, l.progress, l.time])).toEqual([
      ['Loading the game', '2.5 / 7.8 MB', '0.4 s'],
      ['Shaping the world map…', '1 / 2', '1.6 s'],
      ['Finding a place to stand…', '', '1.5 s'],
    ]);
    // The headline and the bar follow the latest begun of those running.
    expect(t.current()?.id).toBe('spawn');
    t.end('spawn', 2100);
    expect(t.current()?.id).toBe('worldMap');
    expect(stageFraction(t.current())).toBe(0.5);
  });

  it('marks a stage slow past slowS (owner: so a slow one is obvious)', () => {
    const t = new StageTracker(DEFS, 3);
    t.begin('spawn', 0);
    expect(t.lines(2999)[0]?.slow).toBe(false);
    expect(t.lines(3001)[0]?.slow).toBe(true);
    t.end('spawn', 3500);
    expect(t.lines(9000)[0]).toMatchObject({ running: false, time: '3.5 s', slow: true });
  });

  it('runs a stage once a wait: a begin after its end, or a late progress, changes nothing', () => {
    const t = new StageTracker(DEFS, 3);
    t.progress('worldMap', 2, 2, 100);
    t.end('worldMap', 2000);
    // The client's readout naming the stage the page already ended (its maps came from the page).
    t.begin('worldMap', 2500);
    t.progress('worldMap', 0, 1, 2600);
    expect(t.lines(3000)).toHaveLength(1);
    expect(t.lines(3000)[0]).toMatchObject({ running: false, time: '1.9 s', progress: '2 / 2' });
    expect(t.history).toHaveLength(1);
  });

  it('ignores an id not in the data, warning once', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const t = new StageTracker(DEFS, 3);
    t.begin('nope' as LoadingStageId, 0);
    t.progress('nope' as LoadingStageId, 1, 2, 1);
    t.end('nope' as LoadingStageId, 2);
    expect(t.lines(3)).toEqual([]);
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });

  it('follows the client readout: join until placed, then the terrain counting', () => {
    const t = new StageTracker(DEFS, 3);
    t.begin('join', 1000);
    stagesFromHud(t, { stage: 'join', ready: 0, needed: 0 }, 1100);
    stagesFromHud(t, { stage: 'terrain', ready: 0, needed: 179 }, 1500);
    stagesFromHud(t, { stage: 'terrain', ready: 37, needed: 179 }, 1800);
    const [join, terrain] = t.lines(2000);
    expect(join).toMatchObject({ id: 'join', running: false, time: '0.5 s' });
    expect(terrain).toMatchObject({ id: 'terrain', running: true, progress: '37 / 179' });
    // The wait before the count moved: the bench's "first count".
    expect(t.timeline().find((r) => r.id === 'terrain')?.firstMs).toBe(1800);
  });

  it('ends everything when the screen opens, and a respawn lists only its own wait', () => {
    const marks: string[] = [];
    const m: StageMarks = {
      begin: (id, at) => marks.push(`begin ${id} ${String(at)}`),
      end: (id, b, e) => marks.push(`end ${id} ${String(b)}-${String(e)}`),
    };
    const t = new StageTracker(DEFS, 3, m);
    t.begin('join', 0);
    stagesFromHud(t, { stage: 'terrain', ready: 10, needed: 20 }, 100);
    t.clear(900); // opened
    expect(t.lines(1000)).toEqual([]);
    expect(t.timeline().map((r) => [r.id, r.endedMs])).toEqual([
      ['join', 100],
      ['terrain', 900],
    ]);
    // Respawned: the terrain again, a new run.
    stagesFromHud(t, { stage: 'terrain', ready: 5, needed: 20 }, 5000);
    expect(t.lines(5100).map((l) => l.text)).toEqual(['Loading the terrain…']);
    expect(marks).toEqual([
      'begin join 0',
      'end join 0-100',
      'begin terrain 100',
      'end terrain 100-900',
      'begin terrain 5000',
    ]);
  });

  it('formats progress and times', () => {
    const run = {
      id: 'game' as const,
      label: '',
      progress: 'bytes' as const,
      beganMs: 0,
      endedMs: null,
      done: 3_400_000,
      firstMs: 1,
      total: null,
      detail: null,
    };
    expect(formatProgress(run)).toBe('3.4 MB'); // compressed: no total
    expect(stageFraction(run)).toBeNull();
    expect(formatProgress({ ...run, progress: 'count', done: 3, total: 8 })).toBe('3 / 8');
    expect(formatProgress({ ...run, progress: undefined })).toBe('');
    expect(formatProgress({ ...run, done: null })).toBe('');
    expect(formatStageTime(12_345)).toBe('12.3 s');
    expect(formatStageTime(-5)).toBe('0.0 s');
  });

  it('knows every stage the code reports, from loading.json', () => {
    // The ids reported by code (main.ts, local/worker.ts, the Rust client's `loading_stage` and its
    // shaping readout): a stage renamed in the data fails here, not silently on the screen.
    const ids = LOADING_STAGES.map((s) => s.id);
    for (const id of [
      'page',
      'game',
      'graphics',
      'worldFile',
      'worldMap',
      'server',
      'spawn',
      'connect',
      'join',
      'terrain',
    ]) {
      expect(ids).toContain(id);
    }
    expect(LOADING_STAGES.find((s) => s.id === 'terrain')?.progress).toBe('count');
    expect(LOADING_STAGES.find((s) => s.id === 'game')?.progress).toBe('bytes');
  });

  it('a stage added by data alone is listed when reported', () => {
    const defs = [...DEFS, { id: 'fixtureStage', label: 'Warming the fixture', progress: 'count' }];
    const t = new StageTracker(defs as unknown as LoadingStageDef[], 3);
    t.progress('fixtureStage' as LoadingStageId, 1, 4, 0);
    expect(t.lines(250)).toEqual([
      {
        id: 'fixtureStage',
        text: 'Warming the fixture…',
        running: true,
        progress: '1 / 4',
        time: '0.3 s',
        slow: false,
      },
    ]);
  });
});

describe('the page stage from the launcher', () => {
  // Measured 2026-11-10 on dropkickarcade.com (warm): the launcher at /dwell/ ran 378 ms (its
  // versions.json and build.json fetches) before going to v/<version>/, whose clock starts at 0,
  // so that time was on no stage. The launcher stamps when it opened; the page stage begins there.
  it('begins the page stage when the launcher opened, on this page clock', () => {
    const origin = 1_000_000;
    const t = new StageTracker([{ id: 'page', label: 'Loading the page' }] as LoadingStageDef[]);
    t.begin('page', pageStartMs(String(origin - 378), origin, origin + 50));
    t.end('page', 50);
    // The whole span from opening the link: no gap over 100 ms before the first stage.
    expect(t.timeline()[0]?.beganMs).toBe(-378);
  });
  it('ignores a missing, bad, future or stale stamp (0: this page opened)', () => {
    const origin = 1_000_000;
    expect(pageStartMs(null, origin, origin + 50)).toBe(0);
    expect(pageStartMs('nope', origin, origin + 50)).toBe(0);
    expect(pageStartMs(String(origin + 10), origin, origin + 50)).toBe(0);
    expect(pageStartMs(String(origin - 120_000), origin, origin + 50)).toBe(0);
  });
});
