// Loading a saved structure into the world (docs/STRUCTURES.md §4): the structure is stamped as
// ordinary block edits, each validated by the server like a click (reach, line of sight, the
// palette, the rate limit; ARCHITECTURE.md §6.5). Single-block edits are all the protocol has
// until S3, so a stamp is a queue: each step sends at most one edit when the rate allows, places
// a block only against one already in the world and facing the eye, and checks the world for
// the result. Cells out of reach or with nothing beside them wait (the player can move closer);
// cells the server keeps refusing are given up after a few tries.
// TODO(S3): send the structure as region edits (about 32³ per request) instead.
import { Players } from '../protocol/constants.gen';
import { STATE_DEFS, stateId } from '../world/blocks';
import type { ValidStructure, Vec3 } from './structures';

/** What the stamp needs of the game: its world, the eye, and edits by cell (`Client.editAt`). */
export interface StampWorld {
  voxel(x: number, y: number, z: number): number;
  /** The eye the last frame aimed from; null while not playing. */
  eye(): Vec3 | null;
  /** Sends an edit; false when it is too soon after the last one or the game isn't playing. */
  editAt(action: 'break' | 'place', cell: Vec3, face: number, material: number, now: number): boolean;
}

export interface StampProgress {
  /** Cells the structure sets (its kept cells aside). */
  total: number;
  /** Cells that hold what the structure wants. */
  done: number;
  /** Cells waiting for the player: out of reach, or with nothing beside them to build against. */
  waiting: number;
  /** Cells the server kept refusing (a player in the way, an unbreakable block). */
  failed: number;
  /** Cells whose block can't be placed by hand (not in the palette). */
  skipped: number;
  /** Nothing left to send or wait for except cells waiting for the player. */
  settled: boolean;
}

/** Faces as the protocol numbers them (0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z). */
export const FACE_DIRS: readonly Vec3[] = [
  [1, 0, 0],
  [-1, 0, 0],
  [0, 1, 0],
  [0, -1, 0],
  [0, 0, 1],
  [0, 0, -1],
];

const AIR = stateId('dwell:air');

/** Whether an edit can aim at the state (not air or a liquid): `dwell_world::edit::targetable`. */
export function targetable(state: number): boolean {
  const s = STATE_DEFS[state];
  return state !== AIR && s !== undefined && s.look !== 'water';
}

/** Distance from a point to a cell's box, as the server measures reach. */
export function reachTo(eye: Vec3, cell: Vec3): number {
  let g2 = 0;
  for (let i = 0; i < 3; i++) {
    const lo = cell[i] ?? 0;
    const e = eye[i] ?? 0;
    const g = e < lo ? lo - e : e > lo + 1 ? e - lo - 1 : 0;
    g2 += g * g;
  }
  return Math.sqrt(g2);
}

/** How far beyond face `face` of `cell` the eye is (positive: in front of it). */
function inFront(eye: Vec3, cell: Vec3, face: number): number {
  const n = FACE_DIRS[face] ?? [0, 0, 0];
  let d = 0;
  for (let i = 0; i < 3; i++) {
    const plane = (cell[i] ?? 0) + 0.5 + 0.5 * (n[i] ?? 0);
    d += ((eye[i] ?? 0) - plane) * (n[i] ?? 0);
  }
  return d;
}

const add = (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];

interface Job {
  cell: Vec3;
  want: number;
  /** Air wanted: the cell is broken, not placed into. */
  clear: boolean;
  placeable: boolean;
  attempts: number;
  /** When the last edit for it was sent (it is in flight until the world shows it, or it times out). */
  sentAt: number | null;
  state: 'open' | 'done' | 'failed' | 'skipped';
}

export interface StampOptions {
  /** Reach used to pick edits; the server allows a little more (`REACH_SLACK`). */
  reach?: number;
  /** How long an edit may take to show in the world before it counts as refused, ms. */
  confirmMs?: number;
  /** Tries per cell before giving it up. */
  attempts?: number;
}

/** The world cell of each cell of a structure set at `at` (where its anchor goes), in layer order. */
export function stampCells(
  s: ValidStructure,
  at: Vec3,
): { cell: Vec3; state: string }[] {
  const [sx, sy, sz] = s.size;
  const out: { cell: Vec3; state: string }[] = [];
  for (let y = 0; y < sy; y++) {
    for (let z = 0; z < sz; z++) {
      for (let x = 0; x < sx; x++) {
        const state = s.palette.get(s.cells[(y * sz + z) * sx + x] ?? '');
        if (state === null || state === undefined) continue;
        out.push({ cell: [at[0] + x - s.anchor[0], at[1] + y - s.anchor[1], at[2] + z - s.anchor[2]], state });
      }
    }
  }
  return out;
}

/**
 * A structure being stamped at `at`. Call `step(now)` often (every frame or so); it sends at most
 * one edit per call and returns the progress. Placing a structure again fills what is missing.
 * The structure is used as saved (no variants: those are for worldgen).
 */
export class StructureStamp {
  private readonly jobs: Job[];
  private readonly reach: number;
  private readonly confirmMs: number;
  private readonly maxAttempts: number;
  private cursor = 0;
  private cancelled = false;
  /** Open cells waiting for the player, as of the last scan that sent nothing. */
  private waiting = 0;

  constructor(
    structure: ValidStructure,
    at: Vec3,
    private readonly world: StampWorld,
    options: StampOptions = {},
  ) {
    this.reach = options.reach ?? Players.reachDistance;
    this.confirmMs = options.confirmMs ?? 1500;
    this.maxAttempts = options.attempts ?? 3;
    // Bottom up: a layer is built on the one below (the order only helps; support decides).
    this.jobs = stampCells(structure, at).map(({ cell, state }) => {
      const want = stateId(state);
      return {
        cell,
        want,
        clear: !targetable(want),
        placeable: STATE_DEFS[want]?.placeable ?? false,
        attempts: 0,
        sentAt: null,
        state: 'open',
      };
    });
  }

  cancel(): void {
    this.cancelled = true;
  }

  get isCancelled(): boolean {
    return this.cancelled;
  }

  /** Checks edits in flight, sends the next edit if one can go, and reports. */
  step(now: number): StampProgress {
    const eye = this.cancelled ? null : this.world.eye();
    for (const job of this.jobs) {
      if (job.state !== 'open') continue;
      if (this.satisfied(job)) {
        job.state = 'done';
        continue;
      }
      if (job.sentAt !== null && now - job.sentAt >= this.confirmMs) {
        job.sentAt = null;
        if (job.attempts >= this.maxAttempts) job.state = 'failed';
      }
    }
    // One edit per step: the next open cell from a moving cursor that can go now. A scan that
    // sends nothing has looked at every open cell, so it counts those waiting for the player.
    const n = this.jobs.length;
    let waiting = 0;
    let full = eye !== null;
    for (let k = 0; k < n && eye; k++) {
      const i = (this.cursor + k) % n;
      const job = this.jobs[i];
      if (!job || job.state !== 'open' || job.sentAt !== null) continue;
      const edit = this.editFor(job, eye);
      if (edit === 'skip') {
        job.state = 'skipped';
        continue;
      }
      if (!edit) {
        waiting++;
        continue;
      }
      full = false;
      if (this.world.editAt(edit.action, edit.cell, edit.face, job.want, now)) {
        job.sentAt = now;
        job.attempts++;
        this.cursor = i;
      }
      break;
    }
    if (full) this.waiting = waiting;
    else if (!eye) this.waiting = this.open();
    return this.progress();
  }

  private open(): number {
    return this.jobs.filter((j) => j.state === 'open').length;
  }

  private progress(): StampProgress {
    const waiting = Math.min(this.waiting, this.open());
    const count = (s: Job['state']) => this.jobs.filter((j) => j.state === s).length;
    const inFlight = this.jobs.some((j) => j.state === 'open' && j.sentAt !== null);
    return {
      total: this.jobs.length,
      done: count('done'),
      waiting,
      failed: count('failed'),
      skipped: count('skipped'),
      settled: !inFlight && this.open() === waiting,
    };
  }

  private satisfied(job: Job): boolean {
    const now = this.world.voxel(...job.cell);
    return job.clear ? !targetable(now) : now === job.want;
  }

  /** The edit that moves the cell toward what is wanted now, null to wait, or 'skip'. */
  private editFor(
    job: Job,
    eye: Vec3,
  ): { action: 'break' | 'place'; cell: Vec3; face: number } | null | 'skip' {
    const current = this.world.voxel(...job.cell);
    if (job.clear || targetable(current)) {
      // Clear the cell first: aim at the face that looks most toward the eye.
      if (!job.clear && !job.placeable) return 'skip';
      if (reachTo(eye, job.cell) > this.reach) return null;
      let face = 0;
      let best = -Infinity;
      for (let f = 0; f < 6; f++) {
        const d = inFront(eye, job.cell, f);
        if (d > best) [best, face] = [d, f];
      }
      return best > 0 ? { action: 'break', cell: job.cell, face } : null;
    }
    if (!job.placeable) return 'skip';
    // Place against a block beside the cell whose face toward it the eye can see, nearest first.
    let pick: { cell: Vec3; face: number; d: number } | null = null;
    for (let f = 0; f < 6; f++) {
      const dir = FACE_DIRS[f] ?? [0, 0, 0];
      const support = add(job.cell, [-dir[0], -dir[1], -dir[2]]);
      if (!targetable(this.world.voxel(...support))) continue;
      if (inFront(eye, support, f) <= 0) continue;
      const d = reachTo(eye, support);
      if (d > this.reach) continue;
      if (!pick || d < pick.d) pick = { cell: support, face: f, d };
    }
    return pick ? { action: 'place', cell: pick.cell, face: pick.face } : null;
  }
}

/** One line for the progress: "Placing: 40 of 120 · 80 waiting — move closer". */
export function progressText(p: StampProgress): string {
  const parts = [`${String(p.done)} of ${String(p.total)} blocks`];
  if (p.waiting) parts.push(`${String(p.waiting)} out of reach or with nothing to build on`);
  if (p.failed) parts.push(`${String(p.failed)} refused`);
  if (p.skipped) parts.push(`${String(p.skipped)} can't be placed by hand`);
  const finished = p.done + p.failed + p.skipped === p.total;
  const head = finished ? 'Loaded' : p.settled && p.waiting ? 'Waiting' : 'Loading';
  return `${head}: ${parts.join(' · ')}${p.settled && p.waiting ? ' — move closer' : ''}`;
}
