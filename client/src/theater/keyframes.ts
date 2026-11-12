// Theater mode's keyframe editor model (docs/THEATER_MODE.md §2, §3): the keys of a camera path
// and each segment's move, edited by the timeline (add at the camera, select, move in time,
// delete) and written as an ordinary path (`formatVersion` 1) for `__dwell.theater('play')`, the
// path store and export. Pure: the toolbar draws it, tests drive it.

export type Vec3 = [number, number, number];

export interface Keyframe {
  t: number;
  position: Vec3;
  /** [yaw, pitch] degrees, face-local (as placeCamera takes them). */
  look: [number, number];
  fov?: number;
  timeOfDay?: number;
  speed?: number;
}

export interface Segment {
  move: string;
  easing?: string;
}

/** A camera path as JSON (dwell_theater::Path), the fields this editor writes. */
export interface PathJson {
  formatVersion: 1;
  name: string;
  loop?: boolean;
  keyframes: Record<string, unknown>[];
  segments: Record<string, unknown>[];
}

/** The smallest gap between two keys (path seconds): keys stay strictly in order. */
export const MIN_GAP_S = 0.1;

export class KeyframeEditor {
  keys: Keyframe[] = [];
  segments: Segment[] = [];
  selected: number | null = null;

  constructor(
    public name = 'shot',
    private readonly gapS = 5,
    private readonly defaultMove = 'ease',
  ) {}

  /** The path's length (path seconds). */
  get duration(): number {
    return this.keys.at(-1)?.t ?? 0;
  }

  /**
   * Adds a key: after the selected one (halfway to the next, or `gapS` past it when it is the
   * last), or at the end; selects it.
   */
  add(k: Omit<Keyframe, 't'> & { t?: number }): number {
    let t = k.t;
    let at: number;
    if (t === undefined) {
      const s = this.selected;
      if (s !== null && s < this.keys.length - 1) {
        const a = this.keys[s]?.t ?? 0;
        const b = this.keys[s + 1]?.t ?? a;
        t = (a + b) / 2;
      } else t = this.keys.length === 0 ? 0 : this.duration + this.gapS;
    }
    at = this.keys.findIndex((x) => x.t > t);
    if (at < 0) at = this.keys.length;
    this.keys.splice(at, 0, { ...k, t });
    // A key between two splits their segment: both halves take its move. One at either end adds
    // a segment of the default move.
    if (this.keys.length > 1) {
      const inner = at > 0 && at < this.keys.length - 1;
      const seg = inner ? this.segments[at - 1] : undefined;
      this.segments.splice(Math.min(at, this.segments.length), 0, {
        ...(seg ?? { move: this.defaultMove }),
      });
    }
    this.selected = at;
    return at;
  }

  /** Moves key `i` to time `t`, held between its neighbours (the first key stays at 0). */
  move(i: number, t: number): void {
    const k = this.keys[i];
    if (!k) return;
    if (i === 0) {
      k.t = 0;
      return;
    }
    const lo = (this.keys[i - 1]?.t ?? 0) + MIN_GAP_S;
    const next = this.keys[i + 1];
    const hi = next ? next.t - MIN_GAP_S : Infinity;
    k.t = Math.min(hi, Math.max(lo, t));
  }

  /** Deletes key `i`; the segments either side join (the one before it is kept). */
  remove(i: number): void {
    if (!this.keys[i]) return;
    this.keys.splice(i, 1);
    this.segments.splice(Math.min(i, this.segments.length - 1), 1);
    this.segments.length = Math.max(0, this.keys.length - 1);
    const first = this.keys[0];
    if (first && first.t !== 0) {
      const shift = first.t;
      for (const k of this.keys) k.t -= shift;
    }
    this.selected = this.keys.length === 0 ? null : Math.min(i, this.keys.length - 1);
  }

  /** Sets segment `i`'s move (from key i to i + 1). */
  setMove(i: number, move: string): void {
    const s = this.segments[i];
    if (s) s.move = move;
  }

  /** Replaces the selected key's camera (position, look, …), keeping its time. */
  update(i: number, k: Omit<Keyframe, 't'>): void {
    const old = this.keys[i];
    if (old) this.keys[i] = { ...k, t: old.t };
  }

  clear(): void {
    this.keys = [];
    this.segments = [];
    this.selected = null;
  }

  /** The path as JSON (`dwell_theater::Path`); fields a key leaves out carry over. */
  toPath(): PathJson {
    return {
      formatVersion: 1,
      name: this.name,
      keyframes: this.keys.map((k) => {
        const o: Record<string, unknown> = { t: round(k.t), position: k.position, look: k.look };
        if (k.fov !== undefined) o.fov = k.fov;
        if (k.timeOfDay !== undefined) o.timeOfDay = k.timeOfDay;
        if (k.speed !== undefined) o.speed = k.speed;
        return o;
      }),
      segments: this.segments.map((s) => ({ ...s })),
    };
  }

  /**
   * Loads a path written by this editor (or by hand, with looks): keys with a `target` or no
   * position take the key before's, which the editor can't show; throws naming the bad key.
   */
  load(p: PathJson): void {
    const keys: Keyframe[] = [];
    let last: Keyframe | undefined;
    p.keyframes.forEach((raw, i) => {
      const t = raw.t;
      if (typeof t !== 'number') throw new Error(`keyframe ${String(i)}: t is required`);
      const position = (raw.position as Vec3 | undefined) ?? last?.position;
      if (!position) throw new Error(`keyframe ${String(i)}: position is required on the first`);
      const look = (raw.look as [number, number] | undefined) ?? last?.look ?? [0, 0];
      const k: Keyframe = { t, position, look };
      for (const f of ['fov', 'timeOfDay', 'speed'] as const) {
        const v = raw[f];
        if (typeof v === 'number') k[f] = v;
      }
      keys.push(k);
      last = k;
    });
    this.name = p.name;
    this.keys = keys;
    this.segments = keys.slice(1).map((_, i) => {
      const s = p.segments[i];
      return s ? ({ ...s } as unknown as Segment) : { move: this.defaultMove };
    });
    this.selected = keys.length > 0 ? 0 : null;
  }
}

const round = (v: number): number => Math.round(v * 1000) / 1000;
