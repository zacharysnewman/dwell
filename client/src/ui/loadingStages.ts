// The loading screen's stages while the game starts (ARCHITECTURE.md §6.3). Owner (2026-11-06):
// "there's often quite a long delay before loading starts counting up on the loading screen, I'd
// like to quantify whatever is loading before that and show it, so it doesn't just look hung".
// The stages are data (shared/look/loading.json `stages` → `LOADING_STAGES`, loading.gen.ts); code
// reports one by its id as it begins, progresses and ends (the page here, the local world's worker
// by message, the Rust client's readout by its `stage`), and the screen lists those begun so far
// with their times. Each is also a performance mark and measure (`dwell:stage:<id>`) for the bench
// and Chrome's Performance panel.
import {
  LOADING_STAGES,
  LOADING_STAGE_SLOW_S,
  type LoadingStageDef,
  type LoadingStageId,
} from './loading.gen';

/** One stage as it ran: when it began and ended (ms on the page's clock) and its progress. */
export interface StageRun {
  id: LoadingStageId;
  label: string;
  progress: LoadingStageDef['progress'];
  beganMs: number;
  endedMs: number | null;
  done: number | null;
  /** When it first counted something (ms): the wait before a count moves. */
  firstMs: number | null;
  total: number | null;
  /** What it is doing now, after its label ("rivers (5/6), face 1 of 2"; BUGS.md #91), or null. */
  detail: string | null;
}

/** A line of the screen's list. */
export interface StageLine {
  id: string;
  /** The label, with an ellipsis while it runs. */
  text: string;
  running: boolean;
  /** "3.1 / 7.8 MB", "1 / 2", or "" (no progress, or none yet). */
  progress: string;
  /** Its time: "1.2 s". */
  time: string;
  /** Running (or ran) longer than `slowS`. */
  slow: boolean;
}

/** Marks and measures on the page's Performance timeline (or nothing, in tests). */
export interface StageMarks {
  begin(id: string, atMs: number): void;
  end(id: string, beganMs: number, endedMs: number): void;
}

/** The browser's `performance` marks: `dwell:stage:<id>`. */
export const PERFORMANCE_MARKS: StageMarks = {
  begin: (id, atMs) => {
    try {
      performance.mark(`dwell:stage:${id}:begin`, { startTime: atMs });
    } catch {
      // no User Timing: nothing to mark
    }
  },
  end: (id, beganMs, endedMs) => {
    try {
      performance.measure(`dwell:stage:${id}`, { start: beganMs, end: endedMs });
    } catch {
      // no User Timing
    }
  },
};

/** "7.8 MB" (one decimal). */
function mb(bytes: number): string {
  return (bytes / 1e6).toFixed(1);
}

/** A stage's progress as the screen shows it. */
export function formatProgress(run: StageRun): string {
  if (!run.progress || run.done === null) return '';
  if (run.progress === 'bytes') {
    return run.total ? `${mb(run.done)} / ${mb(run.total)} MB` : `${mb(run.done)} MB`;
  }
  // A count can move in fractions (the world map's faces by their steps): whole ones are shown.
  const done = Math.floor(run.done);
  return run.total !== null ? `${String(done)} / ${String(run.total)}` : String(done);
}

/** A time in seconds as the screen shows it: "0.4 s", "12.3 s". */
export function formatStageTime(ms: number): string {
  return `${(Math.max(0, ms) / 1000).toFixed(1)} s`;
}

/**
 * The stages begun so far: begun, progressed and ended by id, in any order (some run at once: the
 * world map is shaped while the world's server starts). An id not in the data is ignored (with a
 * warning once), so a stale worker or client cannot break the screen.
 */
export class StageTracker {
  private readonly defs = new Map<string, LoadingStageDef>();
  private runs: StageRun[] = [];
  private readonly warned = new Set<string>();
  /** Every run that has ended, kept across `clear` (the bench reads the start's timeline). */
  readonly history: StageRun[] = [];

  constructor(
    defs: readonly LoadingStageDef[] = LOADING_STAGES,
    private readonly slowS: number = LOADING_STAGE_SLOW_S,
    private readonly marks: StageMarks | null = null,
  ) {
    for (const d of defs) this.defs.set(d.id, d);
  }

  private def(id: string): LoadingStageDef | null {
    const d = this.defs.get(id);
    if (!d && !this.warned.has(id)) {
      this.warned.add(id);
      console.warn(`Dwell: no loading stage "${id}" (shared/look/loading.json stages)`);
    }
    return d ?? null;
  }

  private run(id: string): StageRun | undefined {
    return this.runs.find((r) => r.id === id);
  }

  /** Stage `id` began at `atMs`: once a wait (`clear` starts the next, a respawn's). */
  begin(id: LoadingStageId, atMs: number): void {
    const d = this.def(id);
    if (!d) return;
    // Once a wait: running, or already ended (the client's readout naming a stage the page ended).
    if (this.run(id)) return;
    this.runs.push({
      id,
      label: d.label,
      progress: d.progress,
      beganMs: atMs,
      endedMs: null,
      done: null,
      firstMs: null,
      total: null,
      detail: null,
    });
    this.marks?.begin(id, atMs);
  }

  /**
   * Its progress: `done` of `total` (null: not known), and what it is doing (`detail`, shown after
   * its label; null: nothing); begins it if it had not (not once ended).
   */
  progress(
    id: LoadingStageId,
    done: number,
    total: number | null,
    atMs: number,
    detail: string | null = null,
  ): void {
    this.begin(id, atMs);
    const r = this.run(id);
    if (!r || r.endedMs !== null) return;
    if (done > 0 && r.firstMs === null) r.firstMs = atMs;
    r.done = done;
    r.total = total;
    r.detail = detail;
  }

  /** Stage `id` ended at `atMs` (nothing if it was not running). */
  end(id: LoadingStageId, atMs: number): void {
    const r = this.run(id);
    if (!r || r.endedMs !== null) return;
    r.endedMs = Math.max(r.beganMs, atMs);
    this.history.push({ ...r });
    this.marks?.end(id, r.beganMs, r.endedMs);
  }

  /** Every running stage ends (the screen opened, or the start failed). */
  endAll(atMs: number): void {
    for (const r of this.runs) this.end(r.id, atMs);
  }

  /** Forgets the list (kept in `history`): the next wait, a respawn's, starts a new one. */
  clear(atMs: number): void {
    this.endAll(atMs);
    this.runs = [];
  }

  /** Every run so far, ended ones first (the bench's and the console's `__dwellStages()`). */
  timeline(): StageRun[] {
    return [...this.history, ...this.runs.filter((r) => r.endedMs === null).map((r) => ({ ...r }))];
  }

  /** This wait's runs (since the last `clear`), in the order they began: the estimate reads them. */
  runsNow(): readonly StageRun[] {
    return this.runs;
  }

  /** Whether `id` is running. */
  running(id: string): boolean {
    return this.run(id)?.endedMs === null;
  }

  /** The stages begun so far in the order they began: the screen's list at `nowMs`. */
  lines(nowMs: number): StageLine[] {
    return this.runs.map((r) => {
      const ms = (r.endedMs ?? nowMs) - r.beganMs;
      const running = r.endedMs === null;
      return {
        id: r.id,
        text: running ? `${r.label}${r.detail ? ` — ${r.detail}` : ''}…` : r.label,
        running,
        progress: formatProgress(r),
        time: formatStageTime(ms),
        slow: ms > this.slowS * 1000,
      };
    });
  }

  /** The stage the screen's headline and bar follow: the latest begun of those running. */
  current(): StageRun | null {
    for (let i = this.runs.length - 1; i >= 0; i--) {
      const r = this.runs[i];
      if (r && r.endedMs === null) return r;
    }
    return null;
  }
}

/** The session store key the launcher stamps with when it opened (epoch ms, `performance.timeOrigin`). */
export const LAUNCH_AT_KEY = 'dwell.launchAt';

/**
 * When the page stage begins, on this page's clock (ms, ≤ 0): the launcher's stamp (`launchAt`,
 * epoch ms) when it opened before this page, so its time (its manifest and build checks, then
 * the redirect) lands on 'page' rather than on no stage; 0 (this page opened) without a stamp,
 * or one that is unreadable, after this page opened, or over a minute old (a stale tab's).
 */
export function pageStartMs(
  launchAt: string | null,
  timeOrigin: number,
  nowEpochMs: number,
): number {
  const at = Number(launchAt);
  if (!launchAt || !Number.isFinite(at) || at > timeOrigin) return 0;
  if (nowEpochMs - at > 60_000) return 0;
  return at - timeOrigin;
}

/** The bar's fill for a stage:its count's or bytes' fraction, or null (no known total). */
export function stageFraction(run: StageRun | null): number | null {
  if (!run || run.done === null || !run.total) return null;
  return Math.min(1, Math.max(0, run.done / run.total));
}

/** The Rust client's readout (`Client.hud().loading`): its stage and counts. */
export interface StageHud {
  stage?: string;
  ready: number;
  needed: number;
}

/**
 * The client's readout into the stages: `join` until the join places the player, then `terrain`
 * counting the near ring (the join ends); `worldMap` while a joined world's maps are on the way.
 */
export function stagesFromHud(t: StageTracker, h: StageHud, nowMs: number): void {
  switch (h.stage) {
    case 'terrain':
      t.end('join', nowMs);
      t.progress('terrain', h.ready, h.needed, nowMs);
      break;
    case 'join':
      t.begin('join', nowMs);
      break;
    case 'worldMap':
      t.begin('worldMap', nowMs);
      break;
    case undefined:
      break;
    default:
      t.begin(h.stage as LoadingStageId, nowMs); // checked against the data at run time
  }
}
