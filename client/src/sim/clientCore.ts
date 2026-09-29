// The client's own instance of the sim core (PLAYER_CONTROLLER.md §8): prediction and
// reconciliation of the local player, remote-player proxies, the streamed chunks (§6.3), and
// terrain faces for rendering. Wraps the dwell_client_* exports of server/wasm/wasm_api.cpp.
import type { GroundKind, PlayerState } from '../protocol/constants.gen';
import { PADDED_VOLUME } from '../mesh/mesher';
import { CHUNK_VOLUME } from '../protocol/chunkVoxels';
import type { ChunkCoord, InputFrame, Vec3 } from '../protocol/messages';
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

/** A targeted block (§6.5): its cell and the face the view ray entered through. */
export interface BlockTarget {
  cell: Vec3;
  /** 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z. */
  face: number;
}

export class ClientCore {
  private constructor(private readonly m: DwellCoreModule) {}

  /** A client sim with an empty streamed world: chunks arrive through setChunk. */
  static async load(factory: DwellCoreFactory): Promise<ClientCore> {
    const m = await factory();
    m._dwell_client_create();
    return new ClientCore(m);
  }

  /** Stores a chunk: CHUNK_VOLUME materials in chunk index order (x | y << 5 | z << 10). */
  setChunk(coord: ChunkCoord, revision: number, voxels: Uint16Array): void {
    if (voxels.length !== CHUNK_VOLUME) throw new RangeError('chunk voxels must be CHUNK_VOLUME');
    const bytes = new Uint8Array(voxels.buffer, voxels.byteOffset, voxels.byteLength);
    withHeapBytes(this.m, bytes, (ptr) => {
      this.m._dwell_client_chunk_set(coord[0], coord[1], coord[2], revision, ptr);
    });
  }

  removeChunk(coord: ChunkCoord): void {
    this.m._dwell_client_chunk_remove(coord[0], coord[1], coord[2]);
  }

  chunkCount(): number {
    return this.m._dwell_client_chunk_count();
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
    const base = this.m._dwell_client_state() >> 3;
    const f = this.m.HEAPF64.subarray(base, base + 64);
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

  /** A chunk's voxels with a one-voxel apron (mesh/mesher.ts layout), for the meshing workers. */
  paddedChunk(cx: number, cy: number, cz: number): Uint16Array<ArrayBuffer> {
    const ptr = this.m._dwell_client_chunk_padded(cx, cy, cz);
    return this.m.HEAPU16.slice(ptr >> 1, (ptr >> 1) + PADDED_VOLUME);
  }

  /** Applies a VoxelModification's changes to one chunk: interleaved (index, material) pairs. */
  editChunk(coord: ChunkCoord, revision: number, changes: Uint16Array): void {
    const bytes = new Uint8Array(changes.buffer, changes.byteOffset, changes.byteLength);
    withHeapBytes(this.m, bytes, (ptr) => {
      this.m._dwell_client_chunk_edit(
        coord[0],
        coord[1],
        coord[2],
        revision,
        ptr,
        changes.length / 2,
      );
    });
  }

  /** The block along a view ray (unit `dir`) within `maxDistance`, if any. */
  target(origin: Vec3, dir: Vec3, maxDistance: number): BlockTarget | null {
    const out = this.m._malloc(16);
    try {
      const hit = this.m._dwell_client_target(
        origin[0],
        origin[1],
        origin[2],
        dir[0],
        dir[1],
        dir[2],
        maxDistance,
        out,
      );
      if (!hit) return null;
      const i = out >> 2;
      const at = (k: number) => this.m.HEAP32[i + k] ?? 0;
      return { cell: [at(0), at(1), at(2)], face: at(3) };
    } finally {
      this.m._free(out);
    }
  }

  /** Material at a world voxel (missing chunks read as air). */
  voxel(x: number, y: number, z: number): number {
    return this.m._dwell_client_voxel(x, y, z);
  }
}
