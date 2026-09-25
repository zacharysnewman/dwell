// The client's own instance of the sim core (PLAYER_CONTROLLER.md §8): prediction and
// reconciliation of the local player, remote-player proxies, and terrain faces for rendering.
// Wraps the dwell_client_* exports of server/wasm/wasm_api.cpp.
import type { GroundKind, PlayerState } from '../protocol/constants.gen';
import type { InputFrame, Vec3 } from '../protocol/messages';
import { withHeapBytes, type DwellCoreFactory, type DwellCoreModule } from './module';

export interface PredictionStats {
  ticks: number;
  snapshots: number;
  replays: number;
  snaps: number;
  knockbackReplays: number;
  resets: number;
  /** Position jump of the last replay, before smoothing (m). */
  lastCorrection: number;
  /** Prediction error at the last acknowledged input (m). */
  lastError: number;
}

/** Snapshot of the client sim after the latest tick (layout: dwell_client_state). */
export interface ClientState {
  active: boolean;
  /** Predicted capsule centre. */
  position: Vec3;
  /** Correction still being smoothed out: draw the player at position + renderOffset. */
  renderOffset: Vec3;
  velocity: Vec3;
  halfHeight: number;
  state: PlayerState;
  /** ControllerFlags. */
  controllerFlags: number;
  /** Controller events (player::Events bits) since the previous read. */
  events: number;
  landedSpeed: number;
  platformYawDelta: number;
  groundNormal: Vec3;
  groundKind: GroundKind;
  groundGap: number;
  ceilingBlocked: boolean;
  touchingWall: boolean;
  horizontalCurrent: Vec3;
  horizontalExternal: Vec3;
  targetVelocity: Vec3;
  submerged: number;
  stats: PredictionStats;
  nextSeq: number;
  eyeHeight: number;
  crouchEyeHeight: number;
  maxStepHeight: number;
  radius: number;
  standingHeight: number;
}

/** Controller event bits (server/core/include/dwell/player/controller.h). */
export const ControllerEvents = {
  jumped: 1 << 0,
  landed: 1 << 1,
  crouchChanged: 1 << 2,
  climbStarted: 1 << 3,
  climbEnded: 1 << 4,
  swimStarted: 1 << 5,
  swimEnded: 1 << 6,
} as const;

/** Bytes per face returned by chunkFaces: x, y, z, face (u8 each), material (u16), reserved. */
export const RENDER_FACE_BYTES = 8;

export class ClientCore {
  private constructor(private readonly m: DwellCoreModule) {}

  static async load(
    factory: DwellCoreFactory,
    generatorVersion: number,
    worldSeed = 0n,
  ): Promise<ClientCore> {
    const m = await factory();
    m._dwell_client_create(
      generatorVersion,
      Number(BigInt.asUintN(32, worldSeed)),
      Number(BigInt.asUintN(32, worldSeed >> 32n)),
    );
    return new ClientCore(m);
  }

  nextSeq(): number {
    return this.m._dwell_client_next_seq();
  }

  /** Predicts one tick; `frame.seq` must be nextSeq(). */
  tick(frame: InputFrame): void {
    this.m._dwell_client_tick(
      frame.seq,
      frame.moveX,
      frame.moveY,
      frame.buttons,
      frame.yaw,
      frame.pitch,
    );
  }

  /** Feeds this client's PhysicsSnapshot (raw datagram bytes). */
  snapshot(bytes: Uint8Array): boolean {
    let ok = 0;
    withHeapBytes(this.m, bytes, (ptr) => {
      ok = this.m._dwell_client_snapshot(ptr, bytes.length);
    });
    return ok === 1;
  }

  knockback(inputSeq: number, v: Vec3): void {
    this.m._dwell_client_knockback(inputSeq, v[0], v[1], v[2]);
  }

  setRemote(
    playerId: number,
    feet: Vec3,
    velocity: Vec3,
    crouched: boolean,
    leadSeconds = 0,
  ): void {
    this.m._dwell_client_set_remote(
      playerId,
      feet[0],
      feet[1],
      feet[2],
      velocity[0],
      velocity[1],
      velocity[2],
      crouched ? 1 : 0,
      leadSeconds,
    );
  }

  removeRemote(playerId: number): void {
    this.m._dwell_client_remove_remote(playerId);
  }

  state(): ClientState {
    const base = this.m._dwell_client_state() >> 2;
    const f = this.m.HEAPF32.subarray(base, base + 64);
    const at = (i: number): number => f[i] ?? 0;
    const v3 = (i: number): Vec3 => [at(i), at(i + 1), at(i + 2)];
    return {
      active: at(0) !== 0,
      position: v3(1),
      renderOffset: v3(4),
      velocity: v3(7),
      halfHeight: at(10),
      state: at(11) as PlayerState,
      controllerFlags: at(12),
      events: at(13),
      landedSpeed: at(14),
      platformYawDelta: at(15),
      groundNormal: v3(16),
      groundKind: at(19) as GroundKind,
      groundGap: at(20),
      ceilingBlocked: at(21) !== 0,
      touchingWall: at(22) !== 0,
      horizontalCurrent: v3(23),
      horizontalExternal: v3(26),
      targetVelocity: v3(29),
      submerged: at(32),
      stats: {
        ticks: at(33),
        snapshots: at(34),
        replays: at(35),
        snaps: at(36),
        knockbackReplays: at(37),
        lastCorrection: at(38),
        lastError: at(39),
        resets: at(41),
      },
      nextSeq: at(40),
      eyeHeight: at(42),
      crouchEyeHeight: at(43),
      maxStepHeight: at(44),
      radius: at(45),
      standingHeight: at(46),
    };
  }

  /** Visible faces of a chunk (RENDER_FACE_BYTES each), copied out of the heap. */
  chunkFaces(cx: number, cy: number, cz: number): Uint8Array {
    const countPtr = this.m._malloc(4);
    try {
      const ptr = this.m._dwell_client_chunk_faces(cx, cy, cz, countPtr);
      const count = this.m.HEAPU32[countPtr >> 2] ?? 0;
      return this.m.HEAPU8.slice(ptr, ptr + count * RENDER_FACE_BYTES);
    } finally {
      this.m._free(countPtr);
    }
  }
}
