// Remote players: snapshot buffer and interpolation (ARCHITECTURE.md §9.5). Remote players are drawn
// INTERP_DELAY_MS in the past, interpolated between the snapshots around that time.
import { Players, PlayerFlags, SIM_HZ, type PlayerState } from '../protocol/constants.gen';
import type { RemotePlayerState, Vec3 } from '../protocol/messages';
import { dequantizePitch, dequantizeYaw } from '../predict/input';

export interface RemoteView {
  playerId: number;
  feet: Vec3;
  velocity: Vec3;
  yaw: number;
  pitch: number;
  state: PlayerState;
  flags: number;
}

interface Sample {
  serverMs: number;
  state: RemotePlayerState;
}

const TICK_MS = 1000 / SIM_HZ;
/** Players not in any snapshot for this long are removed. */
const FORGET_MS = 1000;

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/** Shortest-path interpolation of angles in degrees. */
function lerpAngle(a: number, b: number, t: number): number {
  const d = ((((b - a) % 360) + 540) % 360) - 180;
  return a + d * t;
}

function view(s: RemotePlayerState): RemoteView {
  return {
    playerId: s.playerId,
    feet: [...s.position],
    velocity: [...s.velocity],
    yaw: dequantizeYaw(s.yaw),
    pitch: dequantizePitch(s.pitch),
    state: s.state,
    flags: s.flags,
  };
}

export class RemotePlayers {
  private readonly samples = new Map<number, Sample[]>();
  private readonly lastSeen = new Map<number, number>();
  /** Estimated (local clock − server clock) in ms; the smallest seen, i.e. the fastest delivery. */
  private clockOffset: number | null = null;

  constructor(private readonly delayMs: number = Players.interpDelayMs) {}

  /** Adds one snapshot's remote players, received at local time `nowMs`. */
  push(serverTick: number, remotes: readonly RemotePlayerState[], nowMs: number): void {
    const serverMs = serverTick * TICK_MS;
    const offset = nowMs - serverMs;
    // Track the minimum one-way delay, drifting up slowly so clock skew can't pin it forever.
    this.clockOffset =
      this.clockOffset === null ? offset : Math.min(offset, this.clockOffset + 0.05);
    for (const r of remotes) {
      let list = this.samples.get(r.playerId);
      if (!list) {
        list = [];
        this.samples.set(r.playerId, list);
      }
      if (list.length > 0 && (list.at(-1)?.serverMs ?? 0) >= serverMs) continue; // reordered
      list.push({ serverMs, state: r });
      while (list.length > 32) list.shift();
      this.lastSeen.set(r.playerId, nowMs);
    }
    for (const [id, seen] of this.lastSeen) {
      if (nowMs - seen > FORGET_MS) {
        this.lastSeen.delete(id);
        this.samples.delete(id);
      }
    }
  }

  ids(): number[] {
    return [...this.samples.keys()];
  }

  /** Latest received state of each remote player (for the prediction world's proxies). */
  latest(): RemoteView[] {
    const out: RemoteView[] = [];
    for (const list of this.samples.values()) {
      const last = list.at(-1);
      if (last) out.push(view(last.state));
    }
    return out;
  }

  /** Interpolated views at local time `nowMs` (INTERP_DELAY_MS in the past). */
  sample(nowMs: number): RemoteView[] {
    if (this.clockOffset === null) return [];
    const t = nowMs - this.clockOffset - this.delayMs;
    const out: RemoteView[] = [];
    for (const list of this.samples.values()) {
      let i = list.length - 1;
      while (i > 0 && (list[i]?.serverMs ?? 0) > t) i--;
      const a = list[i];
      const b = list[i + 1];
      if (!a) continue;
      if (!b || t <= a.serverMs) {
        out.push(view(a.state)); // before the buffer or no newer sample: hold
        continue;
      }
      const f = Math.min(1, (t - a.serverMs) / (b.serverMs - a.serverMs));
      const va = view(a.state);
      const vb = view(b.state);
      out.push({
        ...vb,
        feet: [
          lerp(va.feet[0], vb.feet[0], f),
          lerp(va.feet[1], vb.feet[1], f),
          lerp(va.feet[2], vb.feet[2], f),
        ],
        velocity: [
          lerp(va.velocity[0], vb.velocity[0], f),
          lerp(va.velocity[1], vb.velocity[1], f),
          lerp(va.velocity[2], vb.velocity[2], f),
        ],
        yaw: lerpAngle(va.yaw, vb.yaw, f),
        pitch: lerp(va.pitch, vb.pitch, f),
        // Discrete state switches at the midpoint.
        state: f < 0.5 ? va.state : vb.state,
        flags: f < 0.5 ? va.flags : vb.flags,
      });
    }
    return out;
  }
}

export const isDead = (v: RemoteView): boolean => (v.flags & PlayerFlags.dead) !== 0;
export const isCrouched = (v: RemoteView): boolean => (v.flags & PlayerFlags.crouched) !== 0;
